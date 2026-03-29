use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use crate::commands::deploy::{DeployTaskStatus, TaskMachineStatus};
use crate::db::{
    app_config_get, app_config_set, audit_log_insert, machine_get,
    machine_list as repo_machine_list, pool_get_single, DbState, MachineRow,
};
use crate::error::AppError;
use crate::sidecar::{run_playbook, SidecarState};

const TASK_TYPE_BOOTSTRAP: &str = "observability_bootstrap";
const TASK_TYPE_ROLLBACK: &str = "observability_rollback";
const RELAY_TELEMETRY_CFG_USERNAME: &str = "relay.telemetry.username";
const RELAY_TELEMETRY_CFG_PASSWORD: &str = "relay.telemetry.password";
const RELAY_TELEMETRY_CFG_SCHEME: &str = "relay.telemetry.scheme";
const RELAY_TELEMETRY_CFG_PORT: &str = "relay.telemetry.port";
const RELAY_TELEMETRY_CFG_INSECURE: &str = "relay.telemetry.insecure";
const RELAY_TELEMETRY_CFG_TIMEOUT_SECONDS: &str = "relay.telemetry.timeout_seconds";
const RELAY_TELEMETRY_CFG_BACKOFF_SECONDS: &str = "relay.telemetry.backoff_seconds";
const RELAY_TELEMETRY_DEFAULT_USERNAME: &str = "ouro_app";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ObservabilityTaskPayload {
    pub machine_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ObservabilityLastTask {
    pub task_id: String,
    pub status: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ObservabilityRelayProbe {
    pub machine_id: i64,
    pub machine_name: String,
    pub ip: String,
    pub configured: bool,
    pub gateway_conf_present: bool,
    pub htpasswd_present: bool,
    pub nginx_running: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ObservabilityGatewayStatus {
    pub relay_total: usize,
    pub configured_relays: usize,
    pub playbook_executed: bool,
    pub last_bootstrap: Option<ObservabilityLastTask>,
    pub last_rollback: Option<ObservabilityLastTask>,
    pub relays: Vec<ObservabilityRelayProbe>,
}

#[derive(Debug, Clone)]
struct TaskRow {
    id: String,
    task_type: String,
    status: String,
    payload: Option<String>,
    error_msg: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    created_at: String,
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
        .arg(remote_cmd)
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

fn observability_playbook_path(filename: &str) -> Result<String, AppError> {
    if let Ok(workspace_root) = std::env::var("OURO_OPS_WORKSPACE_ROOT") {
        let path = std::path::PathBuf::from(workspace_root)
            .join("ansible")
            .join("playbooks")
            .join(filename);
        if path.is_file() {
            return Ok(path.display().to_string());
        }
    }

    if let Ok(current_dir) = std::env::current_dir() {
        let path = current_dir.join("ansible").join("playbooks").join(filename);
        if path.is_file() {
            return Ok(path.display().to_string());
        }
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| AppError::Internal("CARGO_MANIFEST_DIR not set".into()))?;
    let path = std::path::PathBuf::from(manifest_dir)
        .parent()
        .ok_or_else(|| AppError::Internal("no parent dir".into()))?
        .join("ansible")
        .join("playbooks")
        .join(filename);
    if path.is_file() {
        Ok(path.display().to_string())
    } else {
        Err(AppError::Internal(format!(
            "observability playbook not found: {}",
            path.display()
        )))
    }
}

fn observability_bootstrap_playbook_path() -> Result<String, AppError> {
    observability_playbook_path("observability-bootstrap.yml")
}

fn observability_rollback_playbook_path() -> Result<String, AppError> {
    observability_playbook_path("observability-rollback.yml")
}

fn generate_relay_api_key() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn upsert_default_if_missing(
    conn: &Connection,
    key: &str,
    default_value: &str,
) -> Result<(), AppError> {
    let current = app_config_get(conn, key)?
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if current.is_none() {
        app_config_set(conn, key, default_value)?;
    }
    Ok(())
}

fn ensure_relay_telemetry_credentials(conn: &Connection) -> Result<(String, String), AppError> {
    let username = app_config_get(conn, RELAY_TELEMETRY_CFG_USERNAME)?
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| RELAY_TELEMETRY_DEFAULT_USERNAME.to_string());
    app_config_set(conn, RELAY_TELEMETRY_CFG_USERNAME, username.as_str())?;

    let password = app_config_get(conn, RELAY_TELEMETRY_CFG_PASSWORD)?
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(generate_relay_api_key);
    app_config_set(conn, RELAY_TELEMETRY_CFG_PASSWORD, password.as_str())?;

    // Defaults align with current relay gateway deployment behavior.
    upsert_default_if_missing(conn, RELAY_TELEMETRY_CFG_SCHEME, "https")?;
    upsert_default_if_missing(conn, RELAY_TELEMETRY_CFG_PORT, "443")?;
    upsert_default_if_missing(conn, RELAY_TELEMETRY_CFG_INSECURE, "true")?;
    upsert_default_if_missing(conn, RELAY_TELEMETRY_CFG_TIMEOUT_SECONDS, "3")?;
    upsert_default_if_missing(conn, RELAY_TELEMETRY_CFG_BACKOFF_SECONDS, "30")?;

    Ok((username, password))
}

fn resolve_target_relays(
    conn: &Connection,
    machine_ids: Option<&Vec<i64>>,
) -> Result<Vec<MachineRow>, AppError> {
    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let all_relays: Vec<MachineRow> =
        repo_machine_list(conn, Some("relay"), Some(pool.network.as_str()))?
            .into_iter()
            .filter(|machine| machine.pool_id == pool.id)
            .collect();

    if all_relays.is_empty() {
        return Err(AppError::Internal(
            "observability operation requires at least one relay in current pool".into(),
        ));
    }

    let selected = match machine_ids {
        Some(ids) if !ids.is_empty() => {
            let mut selected = Vec::new();
            for machine_id in ids {
                let machine = machine_get(conn, *machine_id)?.ok_or_else(|| {
                    AppError::Internal(format!("machine not found: {machine_id}"))
                })?;
                if machine.role != "relay" {
                    return Err(AppError::Internal(format!(
                        "observability operation supports relay only, got {}",
                        machine.role
                    )));
                }
                if machine.pool_id != pool.id {
                    return Err(AppError::Internal(format!(
                        "machine {} does not belong to current pool",
                        machine.id
                    )));
                }
                selected.push(machine);
            }
            selected
        }
        _ => all_relays,
    };

    if selected.is_empty() {
        return Err(AppError::Internal(
            "no relay target selected for observability operation".into(),
        ));
    }
    Ok(selected)
}

fn build_pool_scrape_targets(conn: &Connection) -> Result<Vec<Value>, AppError> {
    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let mut machines: Vec<MachineRow> = repo_machine_list(conn, None, Some(pool.network.as_str()))?
        .into_iter()
        .filter(|machine| machine.pool_id == pool.id)
        .filter(|machine| machine.role == "relay" || machine.role == "bp")
        .collect();
    if machines.is_empty() {
        return Err(AppError::Internal(
            "observability operation requires at least one relay/bp in current pool".into(),
        ));
    }
    machines.sort_by_key(|machine| machine.sort_order);
    Ok(machines
        .into_iter()
        .map(|machine| {
            json!({
                "target": format!("{}:12798", machine.ip),
                "node": machine.name,
                "role": machine.role,
                "host_ip": machine.ip
            })
        })
        .collect())
}

fn build_relay_inventory(relays: &[MachineRow]) -> Value {
    let mut hostvars = serde_json::Map::new();
    let mut relay_hosts = Vec::new();
    for relay in relays {
        relay_hosts.push(relay.name.clone());
        hostvars.insert(
            relay.name.clone(),
            json!({
                "ansible_host": relay.ip,
                "ansible_port": relay.ssh_port,
                "ansible_user": relay.ssh_user,
                "role": relay.role,
                "network": relay.network
            }),
        );
    }
    json!({
        "_meta": { "hostvars": hostvars },
        "relay": { "hosts": relay_hosts },
        "bp": { "hosts": Vec::<String>::new() }
    })
}

fn insert_task_with_relays(
    conn: &Connection,
    task_id: &str,
    task_type: &str,
    payload_json: &str,
    relays: &[MachineRow],
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO task (id, task_type, status, payload)
         VALUES (?1, ?2, 'pending', ?3)",
        params![task_id, task_type, payload_json],
    )?;
    for relay in relays {
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status)
             VALUES (?1, ?2, 'pending')",
            params![task_id, relay.id],
        )?;
    }
    Ok(())
}

fn mark_task_running(conn: &Connection, task_id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE task
         SET status = 'running', started_at = COALESCE(started_at, datetime('now')), error_msg = NULL
         WHERE id = ?1",
        params![task_id],
    )?;
    conn.execute(
        "UPDATE task_machine SET status = 'running' WHERE task_id = ?1 AND status = 'pending'",
        params![task_id],
    )?;
    Ok(())
}

fn mark_task_terminal(
    conn: &Connection,
    task_id: &str,
    status: &str,
    error_msg: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE task
         SET status = ?1, error_msg = ?2, finished_at = datetime('now')
         WHERE id = ?3",
        params![status, error_msg, task_id],
    )?;
    conn.execute(
        "UPDATE task_machine SET status = ?1 WHERE task_id = ?2",
        params![status, task_id],
    )?;
    Ok(())
}

fn get_task_row(conn: &Connection, task_id: &str) -> Result<Option<TaskRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_type, status, payload, error_msg, started_at, finished_at, created_at
         FROM task
         WHERE id = ?1
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![task_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(TaskRow {
            id: row.get(0)?,
            task_type: row.get(1)?,
            status: row.get(2)?,
            payload: row.get(3)?,
            error_msg: row.get(4)?,
            started_at: row.get(5)?,
            finished_at: row.get(6)?,
            created_at: row.get(7)?,
        }))
    } else {
        Ok(None)
    }
}

fn get_task_machine_statuses(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<TaskMachineStatus>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT machine_id, status
         FROM task_machine
         WHERE task_id = ?1
         ORDER BY machine_id ASC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        Ok(TaskMachineStatus {
            machine_id: row.get(0)?,
            status: row.get(1)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

fn observability_task_status_with_conn(
    conn: &Connection,
    task_id: &str,
    expected_task_type: &str,
) -> Result<DeployTaskStatus, AppError> {
    let task = get_task_row(conn, task_id)?
        .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
    if task.task_type != expected_task_type {
        return Err(AppError::Internal(format!(
            "task is not {expected_task_type}: {task_id}"
        )));
    }
    let payload = task
        .payload
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|e| AppError::Internal(format!("task payload parse failed: {e}")))?
        .unwrap_or(Value::Null);
    Ok(DeployTaskStatus {
        task_id: task.id,
        task_type: task.task_type,
        status: task.status,
        payload,
        error_msg: task.error_msg,
        started_at: task.started_at,
        finished_at: task.finished_at,
        created_at: task.created_at,
        machine_statuses: get_task_machine_statuses(conn, task_id)?,
    })
}

fn latest_task_by_type(
    conn: &Connection,
    task_type: &str,
) -> Result<Option<ObservabilityLastTask>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, status, finished_at
         FROM task
         WHERE task_type = ?1
         ORDER BY COALESCE(started_at, created_at) DESC, created_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![task_type])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ObservabilityLastTask {
            task_id: row.get(0)?,
            status: row.get(1)?,
            finished_at: row.get(2)?,
        }))
    } else {
        Ok(None)
    }
}

fn remote_file_exists(machine: &MachineRow, path: &str) -> Result<bool, AppError> {
    let command = format!("sh -lc 'test -f {} && echo yes || echo no'", path);
    Ok(matches!(ssh_exec(machine, command.as_str())?.trim(), "yes"))
}

fn remote_nginx_running(machine: &MachineRow) -> Result<bool, AppError> {
    let command = "sh -lc 'if command -v systemctl >/dev/null 2>&1; then systemctl is-active nginx >/dev/null 2>&1 && echo yes || echo no; else ps -ef | grep [n]ginx >/dev/null 2>&1 && echo yes || echo no; fi'";
    Ok(matches!(ssh_exec(machine, command)?.trim(), "yes"))
}

fn probe_relay_gateway(machine: &MachineRow) -> ObservabilityRelayProbe {
    let conf = remote_file_exists(machine, "/etc/nginx/conf.d/ouro-ops-metrics.conf");
    let htpasswd = remote_file_exists(machine, "/etc/ouro-ops/.htpasswd");
    let nginx = remote_nginx_running(machine);

    match (conf, htpasswd, nginx) {
        (Ok(conf), Ok(htpasswd), Ok(nginx_running)) => {
            let configured = conf && htpasswd && nginx_running;
            ObservabilityRelayProbe {
                machine_id: machine.id,
                machine_name: machine.name.clone(),
                ip: machine.ip.clone(),
                configured,
                gateway_conf_present: conf,
                htpasswd_present: htpasswd,
                nginx_running,
                note: None,
            }
        }
        (conf, htpasswd, nginx) => {
            let note = [conf.err(), htpasswd.err(), nginx.err()]
                .into_iter()
                .flatten()
                .map(|err| err.to_string())
                .collect::<Vec<_>>()
                .join(" | ");
            ObservabilityRelayProbe {
                machine_id: machine.id,
                machine_name: machine.name.clone(),
                ip: machine.ip.clone(),
                configured: false,
                gateway_conf_present: false,
                htpasswd_present: false,
                nginx_running: false,
                note: if note.is_empty() { None } else { Some(note) },
            }
        }
    }
}

fn run_observability_worker(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    task_type: &str,
    machine_ids: Option<Vec<i64>>,
) -> Result<(), AppError> {
    let relays = {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .get()
            .map_err(|_| AppError::Internal("lock".into()))?;
        resolve_target_relays(&conn, machine_ids.as_ref())?
    };

    {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .get()
            .map_err(|_| AppError::Internal("lock".into()))?;
        mark_task_running(&conn, task_id)?;
    }

    let playbook = match task_type {
        TASK_TYPE_BOOTSTRAP => observability_bootstrap_playbook_path()?,
        TASK_TYPE_ROLLBACK => observability_rollback_playbook_path()?,
        _ => {
            return Err(AppError::Internal(format!(
                "unsupported task type: {task_type}"
            )))
        }
    };
    let inventory = build_relay_inventory(&relays);
    let extra_vars = if task_type == TASK_TYPE_BOOTSTRAP {
        let (username, password, scrape_targets) = {
            let db_state = app_handle.state::<DbState>();
            let conn = db_state
                .0
                .get()
                .map_err(|_| AppError::Internal("lock".into()))?;
            let (username, password) = ensure_relay_telemetry_credentials(&conn)?;
            let scrape_targets = build_pool_scrape_targets(&conn)?;
            (username, password, scrape_targets)
        };
        json!({
            "ops_metrics_basic_auth_username": username,
            "ops_metrics_basic_auth_password": password,
            "ops_metrics_scrape_targets": scrape_targets
        })
    } else {
        json!({})
    };
    let sidecar_state = {
        let managed = app_handle.state::<Mutex<Option<Arc<SidecarState>>>>();
        let guard = managed
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        guard.as_ref().cloned().ok_or(AppError::SidecarCrash)?
    };

    run_playbook(
        sidecar_state.as_ref(),
        app_handle,
        task_id,
        playbook.as_str(),
        inventory,
        extra_vars,
    )?;

    let db_state = app_handle.state::<DbState>();
    let conn = db_state
        .0
        .get()
        .map_err(|_| AppError::Internal("lock".into()))?;
    mark_task_terminal(&conn, task_id, "success", None)?;
    Ok(())
}

fn mark_task_failed_if_needed(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    message: &str,
) -> Result<(), AppError> {
    let db_state = app_handle.state::<DbState>();
    let conn = db_state
        .0
        .get()
        .map_err(|_| AppError::Internal("lock".into()))?;
    mark_task_terminal(&conn, task_id, "failed", Some(message))?;
    let _ = app_handle.emit(
        "task:failed",
        json!({
            "task_id": task_id,
            "status": "failed",
            "error": message
        }),
    );
    Ok(())
}

fn start_observability_task(
    payload: Option<ObservabilityTaskPayload>,
    task_type: &str,
    action: &str,
    db: State<'_, DbState>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    let machine_ids = payload
        .as_ref()
        .and_then(|p| p.machine_ids.clone())
        .filter(|ids| !ids.is_empty());
    let task_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
        let relays = resolve_target_relays(&conn, machine_ids.as_ref())?;
        let payload_value = json!({
            "machine_ids": relays.iter().map(|relay| relay.id).collect::<Vec<_>>(),
            "phase": action
        });
        insert_task_with_relays(
            &conn,
            task_id.as_str(),
            task_type,
            payload_value.to_string().as_str(),
            &relays,
        )?;
        audit_log_insert(
            &conn,
            action,
            &json!({
                "task_id": task_id,
                "machine_ids": relays.iter().map(|relay| relay.id).collect::<Vec<_>>()
            }),
        )?;
    }

    let task_id_for_worker = task_id.clone();
    let app_for_worker = app_handle.clone();
    let machine_ids_for_worker = machine_ids.clone();
    let task_type_for_worker = task_type.to_string();
    thread::spawn(move || {
        if let Err(err) = run_observability_worker(
            &app_for_worker,
            &task_id_for_worker,
            task_type_for_worker.as_str(),
            machine_ids_for_worker,
        ) {
            let _ =
                mark_task_failed_if_needed(&app_for_worker, &task_id_for_worker, &err.to_string());
        }
    });

    Ok(task_id)
}

#[tauri::command]
pub async fn observability_bootstrap_start(
    payload: Option<ObservabilityTaskPayload>,
    db: State<'_, DbState>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    start_observability_task(
        payload,
        TASK_TYPE_BOOTSTRAP,
        "observability_bootstrap_start",
        db,
        app_handle,
    )
}

#[tauri::command]
pub async fn observability_rollback_start(
    payload: Option<ObservabilityTaskPayload>,
    db: State<'_, DbState>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    start_observability_task(
        payload,
        TASK_TYPE_ROLLBACK,
        "observability_rollback_start",
        db,
        app_handle,
    )
}

#[tauri::command]
pub async fn observability_bootstrap_status(
    task_id: String,
    db: State<'_, DbState>,
) -> Result<DeployTaskStatus, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    observability_task_status_with_conn(&conn, task_id.as_str(), TASK_TYPE_BOOTSTRAP)
}

#[tauri::command]
pub async fn observability_rollback_status(
    task_id: String,
    db: State<'_, DbState>,
) -> Result<DeployTaskStatus, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    observability_task_status_with_conn(&conn, task_id.as_str(), TASK_TYPE_ROLLBACK)
}

#[tauri::command]
pub async fn observability_gateway_status(
    db: State<'_, DbState>,
) -> Result<ObservabilityGatewayStatus, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    let relays = resolve_target_relays(&conn, None)?;
    let probes = relays.iter().map(probe_relay_gateway).collect::<Vec<_>>();
    let configured_relays = probes.iter().filter(|probe| probe.configured).count();
    let last_bootstrap = latest_task_by_type(&conn, TASK_TYPE_BOOTSTRAP)?;
    let last_rollback = latest_task_by_type(&conn, TASK_TYPE_ROLLBACK)?;
    Ok(ObservabilityGatewayStatus {
        relay_total: probes.len(),
        configured_relays,
        playbook_executed: last_bootstrap.is_some() || last_rollback.is_some(),
        last_bootstrap,
        last_rollback,
        relays: probes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{machine_insert, pool_insert, run_migrations};
    use rusqlite::Connection;

    #[test]
    fn tc_obs_001_build_relay_inventory_contains_relay_group() {
        let relay = MachineRow {
            id: 1,
            pool_id: 1,
            name: "relay-1".into(),
            ip: "10.0.0.10".into(),
            ssh_port: 22,
            ssh_user: "root".into(),
            role: "relay".into(),
            network: "mainnet".into(),
            ssh_key_fingerprint: None,
            os_version: None,
            cardano_version: None,
            image_registry: "ghcr.io/blinklabs-io/cardano-node".into(),
            image_digest: None,
            sort_order: 0,
            created_at: "2026-03-13 00:00:00".into(),
            updated_at: "2026-03-13 00:00:00".into(),
        };
        let inventory = build_relay_inventory(&[relay]);
        assert_eq!(inventory["relay"]["hosts"][0], "relay-1");
        assert!(inventory["_meta"]["hostvars"]["relay-1"]["ansible_host"].is_string());
    }

    #[test]
    fn tc_obs_003_ensure_relay_telemetry_credentials_persists_defaults() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");

        let (username, password) =
            ensure_relay_telemetry_credentials(&conn).expect("ensure credentials");
        assert_eq!(username, "ouro_app");
        assert!(!password.trim().is_empty());

        let saved_password = app_config_get(&conn, RELAY_TELEMETRY_CFG_PASSWORD)
            .expect("get password")
            .expect("password exists");
        let saved_insecure = app_config_get(&conn, RELAY_TELEMETRY_CFG_INSECURE)
            .expect("get insecure")
            .expect("insecure exists");
        assert_eq!(saved_password, password);
        assert_eq!(saved_insecure, "true");
    }

    #[test]
    fn tc_obs_004_build_pool_scrape_targets_contains_bp_and_relays() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "TICK", "mainnet", None, None).expect("insert pool");
        machine_insert(
            &conn,
            pool_id,
            "relay-1",
            "10.0.0.10",
            22,
            "ubuntu",
            "relay",
            None,
        )
        .expect("insert relay");
        machine_insert(
            &conn,
            pool_id,
            "bp-1",
            "10.0.0.20",
            22,
            "ubuntu",
            "bp",
            None,
        )
        .expect("insert bp");

        let targets = build_pool_scrape_targets(&conn).expect("targets");
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0]["target"], "10.0.0.10:12798");
        assert_eq!(targets[0]["node"], "relay-1");
        assert_eq!(targets[1]["target"], "10.0.0.20:12798");
        assert_eq!(targets[1]["node"], "bp-1");
        assert_eq!(targets[1]["role"], "bp");
    }
}
