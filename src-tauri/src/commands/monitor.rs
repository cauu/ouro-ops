use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde_json::Value;
use tauri::State;

use crate::db::{machine_list as repo_machine_list, DbState, MachineRow};
use crate::error::AppError;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MonitorSnapshot {
    pub machine_id: i64,
    pub machine_name: String,
    pub role: String,
    pub network: String,
    pub block_height: Option<i64>,
    pub sync_progress: Option<f64>,
    pub blocks_per_minute: Option<f64>,
    pub status: String,
    pub stalled: bool,
    pub collected_at: String,
    pub note: Option<String>,
}

#[derive(Debug)]
struct PreviousHealthSample {
    block_height: Option<i64>,
    collected_at_epoch: i64,
}

fn classify_ssh_error(target: &str, stderr: &str) -> AppError {
    if stderr.contains("Permission denied") || stderr.contains("Authentication failed") {
        return AppError::SshAuthFailed(target.to_string());
    }
    if stderr.contains("Connection timed out")
        || stderr.contains("Operation timed out")
        || stderr.contains("No route to host")
        || stderr.contains("Connection refused")
        || stderr.contains("Could not resolve hostname")
    {
        return AppError::SshTimeout(target.to_string());
    }
    AppError::Internal(format!("ssh command failed: {stderr}"))
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn wrap_remote_command(remote_cmd: &str) -> String {
    if !remote_cmd.trim_start().starts_with("docker ") {
        return remote_cmd.to_string();
    }
    let wrapped = format!(
        "if [ \"$(id -u)\" -eq 0 ]; then {cmd}; else {cmd} || sudo -n {cmd}; fi",
        cmd = remote_cmd
    );
    format!("sh -lc {}", shell_single_quote(wrapped.as_str()))
}

fn ssh_exec(machine: &MachineRow, remote_cmd: &str) -> Result<String, AppError> {
    let target = format!("{}@{}", machine.ssh_user, machine.ip);
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=8",
            "-p",
        ])
        .arg(machine.ssh_port.to_string())
        .arg(target.as_str())
        .arg(wrap_remote_command(remote_cmd))
        .output()?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(classify_ssh_error(
        format!("{}@{}:{}", machine.ssh_user, machine.ip, machine.ssh_port).as_str(),
        stderr.as_str(),
    ))
}

fn parse_sync_progress(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::String(raw)) => raw.trim().parse::<f64>().ok(),
        Some(Value::Number(raw)) => raw.as_f64(),
        _ => None,
    }
}

fn parse_tip(raw: &str) -> Result<(Option<i64>, Option<f64>), AppError> {
    let tip: Value =
        serde_json::from_str(raw).map_err(|err| AppError::Internal(format!("invalid tip json: {err}")))?;
    let block_height = tip.get("block").and_then(Value::as_i64);
    let sync_progress = parse_sync_progress(tip.get("syncProgress"));
    Ok((block_height, sync_progress))
}

fn current_epoch_seconds() -> Result<i64, AppError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| AppError::Internal(format!("time error: {err}")))?;
    Ok(now.as_secs() as i64)
}

fn sqlite_datetime(conn: &Connection, epoch_seconds: i64) -> Result<String, AppError> {
    conn.query_row(
        "SELECT datetime(?1, 'unixepoch')",
        params![epoch_seconds],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

fn get_previous_health_sample(
    conn: &Connection,
    machine_id: i64,
) -> Result<Option<PreviousHealthSample>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT block_height, CAST(strftime('%s', collected_at) AS INTEGER)
         FROM machine_health
         WHERE machine_id = ?1
         ORDER BY collected_at DESC, id DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![machine_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(PreviousHealthSample {
            block_height: row.get(0)?,
            collected_at_epoch: row.get(1)?,
        }))
    } else {
        Ok(None)
    }
}

fn insert_health_sample(
    conn: &Connection,
    machine_id: i64,
    block_height: Option<i64>,
    sync_progress: Option<f64>,
    collected_at_epoch: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO machine_health (machine_id, block_height, sync_progress, collected_at)
         VALUES (?1, ?2, ?3, datetime(?4, 'unixepoch'))",
        params![machine_id, block_height, sync_progress, collected_at_epoch],
    )?;
    Ok(())
}

fn compute_blocks_per_minute(
    previous: Option<&PreviousHealthSample>,
    current_block_height: Option<i64>,
    current_epoch_seconds: i64,
) -> Option<f64> {
    let previous = previous?;
    let previous_height = previous.block_height?;
    let current_height = current_block_height?;
    let elapsed_seconds = current_epoch_seconds - previous.collected_at_epoch;
    if elapsed_seconds <= 0 {
        return None;
    }
    Some(((current_height - previous_height) as f64) * 60.0 / (elapsed_seconds as f64))
}

fn determine_stalled(
    previous: Option<&PreviousHealthSample>,
    current_block_height: Option<i64>,
    current_sync_progress: Option<f64>,
    current_epoch_seconds: i64,
) -> bool {
    let Some(progress) = current_sync_progress else {
        return false;
    };
    if progress >= 100.0 {
        return false;
    }
    let Some(previous) = previous else {
        return false;
    };
    let Some(previous_height) = previous.block_height else {
        return false;
    };
    let Some(current_height) = current_block_height else {
        return false;
    };
    current_height == previous_height && (current_epoch_seconds - previous.collected_at_epoch) >= 300
}

fn determine_status(sync_progress: Option<f64>, stalled: bool, note: Option<&str>) -> String {
    if note.is_some() {
        return "unreachable".into();
    }
    if stalled {
        return "stalled".into();
    }
    match sync_progress {
        Some(progress) if progress >= 100.0 => "synced".into(),
        Some(_) => "syncing".into(),
        None => "unknown".into(),
    }
}

fn collect_machine_snapshot(
    conn: &Connection,
    machine: &MachineRow,
) -> Result<MonitorSnapshot, AppError> {
    let collected_at_epoch = current_epoch_seconds()?;
    let collected_at = sqlite_datetime(conn, collected_at_epoch)?;
    let previous = get_previous_health_sample(conn, machine.id)?;
    let remote_cmd = format!(
        "docker exec cardano-node cardano-cli query tip --socket-path /ipc/node.socket --{}",
        machine.network
    );

    let tip_raw = match ssh_exec(machine, remote_cmd.as_str()) {
        Ok(output) => output,
        Err(err) => {
            let note = err.to_string();
            return Ok(MonitorSnapshot {
                machine_id: machine.id,
                machine_name: machine.name.clone(),
                role: machine.role.clone(),
                network: machine.network.clone(),
                block_height: None,
                sync_progress: None,
                blocks_per_minute: None,
                status: determine_status(None, false, Some(note.as_str())),
                stalled: false,
                collected_at,
                note: Some(note),
            });
        }
    };

    let (block_height, sync_progress) = match parse_tip(tip_raw.as_str()) {
        Ok(parsed) => parsed,
        Err(err) => {
            let note = err.to_string();
            return Ok(MonitorSnapshot {
                machine_id: machine.id,
                machine_name: machine.name.clone(),
                role: machine.role.clone(),
                network: machine.network.clone(),
                block_height: None,
                sync_progress: None,
                blocks_per_minute: None,
                status: determine_status(None, false, Some(note.as_str())),
                stalled: false,
                collected_at,
                note: Some(note),
            });
        }
    };

    insert_health_sample(
        conn,
        machine.id,
        block_height,
        sync_progress,
        collected_at_epoch,
    )?;

    let blocks_per_minute =
        compute_blocks_per_minute(previous.as_ref(), block_height, collected_at_epoch);
    let stalled = determine_stalled(
        previous.as_ref(),
        block_height,
        sync_progress,
        collected_at_epoch,
    );
    let status = determine_status(sync_progress, stalled, None);

    Ok(MonitorSnapshot {
        machine_id: machine.id,
        machine_name: machine.name.clone(),
        role: machine.role.clone(),
        network: machine.network.clone(),
        block_height,
        sync_progress,
        blocks_per_minute,
        status,
        stalled,
        collected_at,
        note: None,
    })
}

#[tauri::command]
pub async fn monitor_snapshot(
    machine_ids: Option<Vec<i64>>,
    db: State<'_, DbState>,
) -> Result<Vec<MonitorSnapshot>, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let selected = repo_machine_list(&conn, None, None)?
        .into_iter()
        .filter(|machine| {
            machine_ids
                .as_ref()
                .map(|ids| ids.contains(&machine.id))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    let mut snapshots = Vec::with_capacity(selected.len());
    for machine in selected {
        snapshots.push(collect_machine_snapshot(&conn, &machine)?);
    }
    Ok(snapshots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_mon_001_parse_tip_supports_string_progress() {
        let raw = r#"{"block":123,"syncProgress":"42.50"}"#;
        let (block_height, sync_progress) = parse_tip(raw).expect("parse tip");
        assert_eq!(block_height, Some(123));
        assert_eq!(sync_progress, Some(42.5));
    }

    #[test]
    fn tc_mon_002_compute_blocks_per_minute() {
        let previous = PreviousHealthSample {
            block_height: Some(1_000),
            collected_at_epoch: 1_000,
        };
        let blocks_per_minute =
            compute_blocks_per_minute(Some(&previous), Some(1_120), 1_600).expect("speed");
        assert!((blocks_per_minute - 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tc_mon_003_stalled_when_height_not_moving_for_five_minutes() {
        let previous = PreviousHealthSample {
            block_height: Some(1_000),
            collected_at_epoch: 1_000,
        };
        assert!(determine_stalled(Some(&previous), Some(1_000), Some(12.5), 1_301));
        assert!(!determine_stalled(Some(&previous), Some(1_001), Some(12.5), 1_301));
    }

    #[test]
    fn tc_mon_004_wrap_remote_command_uses_sudo_for_docker() {
        let wrapped = wrap_remote_command("docker exec cardano-node cardano-cli query tip");
        assert!(wrapped.contains("sudo -n docker exec cardano-node cardano-cli query tip"));
        assert!(wrapped.starts_with("sh -lc "));
    }
}
