use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use crate::commands::deploy::{DeployTaskStatus, TaskMachineStatus};
use crate::db::{
    audit_log_insert, machine_get, machine_list as repo_machine_list, pool_get_single, DbState,
    MachineRow,
};
use crate::error::AppError;
use crate::sidecar::{run_playbook, SidecarState};

const DEFAULT_IMAGE_REGISTRY: &str = "ghcr.io/blinklabs-io/cardano-node";
const DEFAULT_IMAGE_TAG: &str = "10.5.4-1";

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct UpgradePayload {
    pub target_version: String,
    pub image_registry: String,
    pub image_digest: Option<String>,
    pub machine_ids: Vec<i64>,
    pub auto_continue: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpgradeGateEvent {
    pub task_id: String,
    pub completed_machine: String,
    pub next_machine: String,
    pub is_bp: bool,
    pub message: String,
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

fn normalize_upgrade_payload(payload: &UpgradePayload) -> UpgradePayload {
    let mut next = payload.clone();
    let target_version = next.target_version.trim();
    next.target_version = if target_version.is_empty() {
        DEFAULT_IMAGE_TAG.to_string()
    } else {
        target_version.to_string()
    };
    let image_registry = next.image_registry.trim();
    next.image_registry = if image_registry.is_empty() {
        DEFAULT_IMAGE_REGISTRY.to_string()
    } else {
        image_registry.to_string()
    };
    next.image_digest = next
        .image_digest
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    next
}

fn validate_upgrade_payload(payload: &UpgradePayload) -> Result<(), AppError> {
    if payload.machine_ids.is_empty() {
        return Err(AppError::Internal("machine_ids must not be empty".into()));
    }
    if payload.target_version.trim().is_empty() && payload.image_digest.is_none() {
        return Err(AppError::Internal(
            "target_version must not be empty when image_digest is absent".into(),
        ));
    }
    if payload.image_registry.trim().is_empty() {
        return Err(AppError::Internal(
            "image_registry must not be empty".into(),
        ));
    }
    Ok(())
}

fn upgrade_playbook_path() -> Result<String, AppError> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| AppError::Internal("CARGO_MANIFEST_DIR not set".into()))?;
    let path = std::path::PathBuf::from(manifest_dir)
        .parent()
        .ok_or_else(|| AppError::Internal("no parent dir".into()))?
        .join("ansible")
        .join("playbooks")
        .join("upgrade.yml");
    Ok(path.display().to_string())
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

fn fetch_upgrade_machines(
    conn: &Connection,
    machine_ids: &[i64],
) -> Result<Vec<MachineRow>, AppError> {
    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for machine_id in machine_ids {
        if !seen.insert(*machine_id) {
            continue;
        }
        let machine = machine_get(conn, *machine_id)?
            .ok_or_else(|| AppError::Internal(format!("machine not found: {machine_id}")))?;
        if machine.pool_id != pool.id {
            return Err(AppError::Internal(format!(
                "machine {} does not belong to current pool",
                machine.name
            )));
        }
        if !matches!(machine.role.as_str(), "relay" | "bp") {
            return Err(AppError::Internal(format!(
                "upgrade supports relay/bp only, got {}",
                machine.role
            )));
        }
        rows.push(machine);
    }
    Ok(rows)
}

fn partition_upgrade_order(
    machines: &[MachineRow],
    original_ids: &[i64],
) -> (Vec<i64>, Vec<i64>, Vec<i64>) {
    let by_id: std::collections::HashMap<i64, &MachineRow> = machines
        .iter()
        .map(|machine| (machine.id, machine))
        .collect();
    let mut relay_ids = Vec::new();
    let mut bp_ids = Vec::new();
    for machine_id in original_ids {
        if let Some(machine) = by_id.get(machine_id) {
            if machine.role == "relay" {
                relay_ids.push(*machine_id);
            } else if machine.role == "bp" {
                bp_ids.push(*machine_id);
            }
        }
    }
    let mut planned = relay_ids.clone();
    planned.extend(bp_ids.iter().copied());
    (relay_ids, bp_ids, planned)
}

fn build_upgrade_inventory(conn: &Connection, machine_ids: &[i64]) -> Result<Value, AppError> {
    let selected = fetch_upgrade_machines(conn, machine_ids)?;
    if selected.is_empty() {
        return Err(AppError::Internal("no machine selected".into()));
    }
    let pool_id = selected[0].pool_id;
    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let all_pool_machines: Vec<MachineRow> =
        repo_machine_list(conn, None, Some(pool.network.as_str()))?
            .into_iter()
            .filter(|m| m.pool_id == pool_id)
            .collect();

    let relay_nodes: Vec<Value> = all_pool_machines
        .iter()
        .filter(|m| m.role == "relay")
        .map(|m| json!({ "ip": m.ip, "name": m.name }))
        .collect();
    let bp_nodes: Vec<Value> = all_pool_machines
        .iter()
        .filter(|m| m.role == "bp")
        .map(|m| json!({ "ip": m.ip, "name": m.name }))
        .collect();
    let trusted_relay_ips: Vec<String> = relay_nodes
        .iter()
        .filter_map(|r| r.get("ip").and_then(Value::as_str).map(ToString::to_string))
        .collect();

    let mut hostvars = serde_json::Map::new();
    for machine in &selected {
        hostvars.insert(
            machine.name.clone(),
            json!({
                "ansible_host": machine.ip,
                "ansible_port": machine.ssh_port,
                "ansible_user": machine.ssh_user,
                "role": machine.role,
                "network": machine.network,
                "relay_nodes": relay_nodes.clone(),
                "bp_nodes": bp_nodes.clone(),
                "trusted_relay_ips": trusted_relay_ips.clone(),
            }),
        );
    }

    let relay_hosts: Vec<String> = selected
        .iter()
        .filter(|m| m.role == "relay")
        .map(|m| m.name.clone())
        .collect();
    let bp_hosts: Vec<String> = selected
        .iter()
        .filter(|m| m.role == "bp")
        .map(|m| m.name.clone())
        .collect();

    Ok(json!({
        "_meta": { "hostvars": hostvars },
        "relay": { "hosts": relay_hosts },
        "bp": { "hosts": bp_hosts }
    }))
}

fn insert_upgrade_task(
    conn: &Connection,
    task_id: &str,
    payload_json: &str,
    machine_ids: &[i64],
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO task (id, task_type, status, payload)
         VALUES (?1, 'upgrade', 'pending', ?2)",
        params![task_id, payload_json],
    )?;
    for machine_id in machine_ids {
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status)
             VALUES (?1, ?2, 'pending')",
            params![task_id, machine_id],
        )?;
    }
    Ok(())
}

fn parse_task_payload(task: &TaskRow) -> Result<Value, AppError> {
    task.payload
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|e| AppError::Internal(format!("task payload parse failed: {e}")))
        .map(|value| value.unwrap_or(Value::Null))
}

fn update_task_payload_and_status(
    conn: &Connection,
    task_id: &str,
    payload: &Value,
    status: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE task
         SET payload = ?1,
             status = ?2,
             error_msg = NULL,
             started_at = COALESCE(started_at, datetime('now'))
         WHERE id = ?3",
        params![payload.to_string(), status, task_id],
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
    Ok(())
}

fn mark_task_machine_status(
    conn: &Connection,
    task_id: &str,
    machine_id: i64,
    status: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE task_machine SET status = ?1 WHERE task_id = ?2 AND machine_id = ?3",
        params![status, task_id, machine_id],
    )?;
    Ok(())
}

fn mark_pending_and_running_task_machines(
    conn: &Connection,
    task_id: &str,
    status: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE task_machine
         SET status = ?1
         WHERE task_id = ?2 AND status IN ('pending', 'running', 'paused')",
        params![status, task_id],
    )?;
    Ok(())
}

fn upgrade_status_with_conn(
    conn: &Connection,
    task_id: &str,
) -> Result<DeployTaskStatus, AppError> {
    let task = get_task_row(conn, task_id)?
        .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
    if task.task_type != "upgrade" {
        return Err(AppError::Internal(format!(
            "task is not upgrade: {task_id}"
        )));
    }
    let payload = parse_task_payload(&task)?;
    let machine_statuses = get_task_machine_statuses(conn, task_id)?;
    Ok(DeployTaskStatus {
        task_id: task.id,
        task_type: task.task_type,
        status: task.status,
        payload,
        error_msg: task.error_msg,
        started_at: task.started_at,
        finished_at: task.finished_at,
        created_at: task.created_at,
        machine_statuses,
    })
}

fn update_machine_version(
    conn: &Connection,
    machine_id: i64,
    payload: &UpgradePayload,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE machine
         SET cardano_version = ?1,
             image_registry = ?2,
             image_digest = ?3,
             updated_at = datetime('now')
         WHERE id = ?4",
        params![
            payload.target_version,
            payload.image_registry,
            payload.image_digest,
            machine_id
        ],
    )?;
    Ok(())
}

fn emit_task_failed(app_handle: &tauri::AppHandle, task_id: &str, message: &str) {
    let _ = app_handle.emit(
        "task:failed",
        json!({
            "task_id": task_id,
            "status": "failed",
            "error": message
        }),
    );
}

fn mark_upgrade_task_failed_if_needed(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    message: &str,
    failed_machine_id: Option<i64>,
) -> Result<(), AppError> {
    let db_state = app_handle.state::<DbState>();
    let conn = db_state
        .0
        .get()
        .map_err(|_| AppError::Internal("lock".into()))?;
    let Some(task) = get_task_row(&conn, task_id)? else {
        return Ok(());
    };
    if task.status == "cancelled" {
        return Ok(());
    }
    mark_task_terminal(&conn, task_id, "failed", Some(message))?;
    if let Some(machine_id) = failed_machine_id {
        mark_task_machine_status(&conn, task_id, machine_id, "failed")?;
    }
    mark_pending_and_running_task_machines(&conn, task_id, "failed")?;
    emit_task_failed(app_handle, task_id, message);
    Ok(())
}

fn backup_archive_path() -> String {
    format!(
        "/opt/cardano/backup/config-{}.tar.gz",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    )
}

fn build_upgrade_extra_vars(
    payload: &UpgradePayload,
    network: &str,
    phase: &str,
    backup_archive: Option<&str>,
) -> Value {
    json!({
        "target_version": payload.target_version,
        "image_registry": payload.image_registry,
        "image_digest": payload.image_digest,
        "network": network,
        "upgrade_phase": phase,
        "backup_archive": backup_archive,
    })
}

fn run_upgrade_playbook(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    inventory: Value,
    extra_vars: Value,
) -> Result<(), AppError> {
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
        upgrade_playbook_path()?.as_str(),
        inventory,
        extra_vars,
    )
}

fn emit_upgrade_gate(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    completed_machine: &str,
    next_machine: &str,
    is_bp: bool,
) {
    let message = if is_bp {
        format!(
            "Relay upgrade complete on {completed_machine}; confirm before upgrading BP {next_machine}"
        )
    } else {
        format!(
            "Relay upgrade complete on {completed_machine}; confirm before upgrading {next_machine}"
        )
    };
    let _ = app_handle.emit(
        "upgrade:gate",
        json!(UpgradeGateEvent {
            task_id: task_id.to_string(),
            completed_machine: completed_machine.to_string(),
            next_machine: next_machine.to_string(),
            is_bp,
            message,
        }),
    );
}

fn wait_for_gate_release(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    expected_phase: &str,
) -> Result<(), AppError> {
    loop {
        thread::sleep(Duration::from_millis(500));
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .get()
            .map_err(|_| AppError::Internal("lock".into()))?;
        let task = get_task_row(&conn, task_id)?
            .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
        if task.status == "cancelled" {
            return Err(AppError::Internal(
                "upgrade cancelled while awaiting confirmation".into(),
            ));
        }
        if task.status == "failed" {
            return Err(AppError::Internal(task.error_msg.unwrap_or_else(|| {
                "upgrade failed while awaiting confirmation".into()
            })));
        }
        let payload = parse_task_payload(&task)?;
        let phase = payload.get("phase").and_then(Value::as_str).unwrap_or("");
        if task.status == "running" && phase == expected_phase {
            return Ok(());
        }
    }
}

fn run_upgrade_worker(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    payload: &UpgradePayload,
) -> Result<(), AppError> {
    let selected = {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .get()
            .map_err(|_| AppError::Internal("lock".into()))?;
        fetch_upgrade_machines(&conn, &payload.machine_ids)?
    };
    let (relay_ids, bp_ids, planned_machine_ids) =
        partition_upgrade_order(&selected, &payload.machine_ids);
    let network = selected
        .first()
        .map(|machine| machine.network.clone())
        .ok_or_else(|| AppError::Internal("upgrade requires selected machines".into()))?;
    let name_by_id: std::collections::HashMap<i64, String> = selected
        .iter()
        .map(|machine| (machine.id, machine.name.clone()))
        .collect();
    let previous_version = selected
        .iter()
        .find_map(|machine| machine.cardano_version.clone())
        .unwrap_or_else(|| payload.target_version.clone());
    let backup_archive = backup_archive_path();

    {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .get()
            .map_err(|_| AppError::Internal("lock".into()))?;
        let task = get_task_row(&conn, task_id)?
            .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
        let mut task_payload = parse_task_payload(&task)?;
        task_payload["phase"] = Value::String("BACKUP_CONFIG".into());
        task_payload["previous_version"] = Value::String(previous_version);
        task_payload["backup_archive"] = Value::String(backup_archive.clone());
        update_task_payload_and_status(&conn, task_id, &task_payload, "running")?;
    }

    let backup_inventory = {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .get()
            .map_err(|_| AppError::Internal("lock".into()))?;
        build_upgrade_inventory(&conn, &planned_machine_ids)?
    };
    run_upgrade_playbook(
        app_handle,
        task_id,
        backup_inventory,
        build_upgrade_extra_vars(payload, &network, "BACKUP_CONFIG", Some(&backup_archive)),
    )?;

    for (relay_index, relay_id) in relay_ids.iter().enumerate() {
        {
            let db_state = app_handle.state::<DbState>();
            let conn = db_state
                .0
                .get()
                .map_err(|_| AppError::Internal("lock".into()))?;
            let task = get_task_row(&conn, task_id)?
                .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
            let mut task_payload = parse_task_payload(&task)?;
            task_payload["phase"] = Value::String("UPGRADING_RELAY_N".into());
            task_payload["current_index"] = Value::from(relay_index as i64);
            task_payload["current_machine_id"] = Value::from(*relay_id);
            task_payload["current_machine_name"] = Value::String(
                name_by_id
                    .get(relay_id)
                    .cloned()
                    .unwrap_or_else(|| format!("machine-{relay_id}")),
            );
            update_task_payload_and_status(&conn, task_id, &task_payload, "running")?;
            mark_task_machine_status(&conn, task_id, *relay_id, "running")?;
        }

        let relay_inventory = {
            let db_state = app_handle.state::<DbState>();
            let conn = db_state
                .0
                .get()
                .map_err(|_| AppError::Internal("lock".into()))?;
            build_upgrade_inventory(&conn, &[*relay_id])?
        };
        run_upgrade_playbook(
            app_handle,
            task_id,
            relay_inventory,
            build_upgrade_extra_vars(
                payload,
                &network,
                "UPGRADING_RELAY_N",
                Some(&backup_archive),
            ),
        )?;

        {
            let db_state = app_handle.state::<DbState>();
            let conn = db_state
                .0
                .get()
                .map_err(|_| AppError::Internal("lock".into()))?;
            let task = get_task_row(&conn, task_id)?
                .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
            let mut task_payload = parse_task_payload(&task)?;
            task_payload["phase"] = Value::String("HEALTH_GATE_RELAY".into());
            update_task_payload_and_status(&conn, task_id, &task_payload, "running")?;
            mark_task_machine_status(&conn, task_id, *relay_id, "success")?;
            update_machine_version(&conn, *relay_id, payload)?;
        }

        if relay_index + 1 < relay_ids.len() {
            if payload.auto_continue {
                continue;
            }
            let next_machine = name_by_id
                .get(&relay_ids[relay_index + 1])
                .cloned()
                .unwrap_or_else(|| format!("machine-{}", relay_ids[relay_index + 1]));
            {
                let db_state = app_handle.state::<DbState>();
                let conn = db_state
                    .0
                    .get()
                    .map_err(|_| AppError::Internal("lock".into()))?;
                let task = get_task_row(&conn, task_id)?
                    .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
                let mut task_payload = parse_task_payload(&task)?;
                task_payload["phase"] = Value::String("AWAIT_NEXT_RELAY".into());
                task_payload["next_machine_id"] = Value::from(relay_ids[relay_index + 1]);
                task_payload["next_machine_name"] = Value::String(next_machine.clone());
                update_task_payload_and_status(&conn, task_id, &task_payload, "paused")?;
            }
            emit_upgrade_gate(
                app_handle,
                task_id,
                name_by_id
                    .get(relay_id)
                    .map(String::as_str)
                    .unwrap_or("relay"),
                next_machine.as_str(),
                false,
            );
            wait_for_gate_release(app_handle, task_id, "UPGRADING_RELAY_N")?;
        }
    }

    if let Some(bp_id) = bp_ids.first() {
        let bp_name = name_by_id
            .get(bp_id)
            .cloned()
            .unwrap_or_else(|| format!("machine-{bp_id}"));
        let completed_machine = relay_ids
            .last()
            .and_then(|id| name_by_id.get(id).cloned())
            .unwrap_or_else(|| "relay".to_string());
        {
            let db_state = app_handle.state::<DbState>();
            let conn = db_state
                .0
                .get()
                .map_err(|_| AppError::Internal("lock".into()))?;
            let task = get_task_row(&conn, task_id)?
                .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
            let mut task_payload = parse_task_payload(&task)?;
            task_payload["phase"] = Value::String("AWAIT_BP_CONFIRM".into());
            task_payload["next_machine_id"] = Value::from(*bp_id);
            task_payload["next_machine_name"] = Value::String(bp_name.clone());
            update_task_payload_and_status(&conn, task_id, &task_payload, "paused")?;
        }
        emit_upgrade_gate(
            app_handle,
            task_id,
            completed_machine.as_str(),
            bp_name.as_str(),
            true,
        );
        wait_for_gate_release(app_handle, task_id, "UPGRADING_BP")?;

        {
            let db_state = app_handle.state::<DbState>();
            let conn = db_state
                .0
                .get()
                .map_err(|_| AppError::Internal("lock".into()))?;
            mark_task_machine_status(&conn, task_id, *bp_id, "running")?;
        }
        let bp_inventory = {
            let db_state = app_handle.state::<DbState>();
            let conn = db_state
                .0
                .get()
                .map_err(|_| AppError::Internal("lock".into()))?;
            build_upgrade_inventory(&conn, &[*bp_id])?
        };
        run_upgrade_playbook(
            app_handle,
            task_id,
            bp_inventory,
            build_upgrade_extra_vars(payload, &network, "UPGRADING_BP", Some(&backup_archive)),
        )?;
        {
            let db_state = app_handle.state::<DbState>();
            let conn = db_state
                .0
                .get()
                .map_err(|_| AppError::Internal("lock".into()))?;
            let task = get_task_row(&conn, task_id)?
                .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
            let mut task_payload = parse_task_payload(&task)?;
            task_payload["phase"] = Value::String("HEALTH_GATE_BP".into());
            update_task_payload_and_status(&conn, task_id, &task_payload, "running")?;
            mark_task_machine_status(&conn, task_id, *bp_id, "success")?;
            update_machine_version(&conn, *bp_id, payload)?;
        }
    }

    {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .get()
            .map_err(|_| AppError::Internal("lock".into()))?;
        let task = get_task_row(&conn, task_id)?
            .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
        let mut task_payload = parse_task_payload(&task)?;
        task_payload["phase"] = Value::String("SUCCESS".into());
        update_task_payload_and_status(&conn, task_id, &task_payload, "success")?;
        conn.execute(
            "UPDATE task SET finished_at = datetime('now') WHERE id = ?1",
            params![task_id],
        )?;
        mark_pending_and_running_task_machines(&conn, task_id, "success")?;
    }
    Ok(())
}

#[tauri::command]
pub async fn upgrade_start(
    payload: UpgradePayload,
    db: State<'_, DbState>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    let payload = normalize_upgrade_payload(&payload);
    validate_upgrade_payload(&payload)?;

    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    let machines = fetch_upgrade_machines(&conn, &payload.machine_ids)?;
    let (relay_ids, bp_ids, planned_machine_ids) =
        partition_upgrade_order(&machines, &payload.machine_ids);
    if relay_ids.is_empty() {
        return Err(AppError::Internal(
            "upgrade requires at least one relay machine".into(),
        ));
    }
    let network = pool_get_single(&conn)?
        .ok_or_else(|| AppError::Internal("pool not initialized".into()))?
        .network;
    let task_id = uuid::Uuid::new_v4().to_string();
    let task_payload = json!({
        "target_version": payload.target_version,
        "image_registry": payload.image_registry,
        "image_digest": payload.image_digest,
        "network": network,
        "auto_continue": payload.auto_continue,
        "relay_machine_ids": relay_ids,
        "bp_machine_ids": bp_ids,
        "planned_machine_ids": planned_machine_ids,
        "phase": "BACKUP_CONFIG",
        "current_index": 0,
        "previous_version": Value::Null,
        "backup_archive": Value::Null
    });
    let payload_json = serde_json::to_string(&task_payload)
        .map_err(|e| AppError::Internal(format!("upgrade payload serialize failed: {e}")))?;
    insert_upgrade_task(
        &conn,
        task_id.as_str(),
        payload_json.as_str(),
        &payload.machine_ids,
    )?;
    audit_log_insert(
        &conn,
        "upgrade_start",
        &json!({
            "task_id": task_id,
            "target_version": task_payload["target_version"],
            "image_registry": task_payload["image_registry"],
            "machine_ids": payload.machine_ids,
            "relay_machine_ids": task_payload["relay_machine_ids"],
            "bp_machine_ids": task_payload["bp_machine_ids"],
            "auto_continue": payload.auto_continue
        }),
    )?;
    drop(conn);

    let task_id_for_worker = task_id.clone();
    let payload_for_worker = payload.clone();
    let app_for_worker = app_handle.clone();
    thread::spawn(move || {
        if let Err(err) =
            run_upgrade_worker(&app_for_worker, &task_id_for_worker, &payload_for_worker)
        {
            let failed_machine_id = {
                let db_state = app_for_worker.state::<DbState>();
                let conn = match db_state.0.get() {
                    Ok(conn) => conn,
                    Err(_) => return,
                };
                get_task_row(&conn, &task_id_for_worker)
                    .ok()
                    .flatten()
                    .and_then(|task| parse_task_payload(&task).ok())
                    .and_then(|payload| payload.get("current_machine_id").and_then(Value::as_i64))
            };
            let _ = mark_upgrade_task_failed_if_needed(
                &app_for_worker,
                &task_id_for_worker,
                &err.to_string(),
                failed_machine_id,
            );
        }
    });

    Ok(task_id)
}

#[tauri::command]
pub async fn upgrade_status(
    task_id: String,
    db: State<'_, DbState>,
) -> Result<DeployTaskStatus, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    upgrade_status_with_conn(&conn, task_id.as_str())
}

#[tauri::command]
pub async fn upgrade_confirm_next(task_id: String, db: State<'_, DbState>) -> Result<(), AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    let task = get_task_row(&conn, task_id.as_str())?
        .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
    if task.task_type != "upgrade" {
        return Err(AppError::Internal(format!(
            "task is not upgrade: {task_id}"
        )));
    }
    let payload = parse_task_payload(&task)?;
    let current_phase = payload
        .get("phase")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal("upgrade phase missing".into()))?;
    let next_phase = match current_phase {
        "AWAIT_NEXT_RELAY" => "UPGRADING_RELAY_N",
        "AWAIT_BP_CONFIRM" => "UPGRADING_BP",
        _ => {
            return Err(AppError::Internal(format!(
                "upgrade task is not waiting for confirmation: {current_phase}"
            )))
        }
    };
    let mut next_payload = payload.clone();
    next_payload["phase"] = Value::String(next_phase.to_string());
    next_payload["last_confirmed_at"] = Value::String(chrono::Utc::now().to_rfc3339());
    conn.execute(
        "UPDATE task SET status = 'running', payload = ?1, error_msg = NULL WHERE id = ?2",
        params![next_payload.to_string(), task_id],
    )?;
    audit_log_insert(
        &conn,
        "upgrade_confirm_next",
        &json!({
            "task_id": task.id,
            "from_phase": current_phase,
            "to_phase": next_phase,
            "previous_status": task.status
        }),
    )?;
    Ok(())
}

#[tauri::command]
pub async fn upgrade_rollback(
    task_id: String,
    machine_id: i64,
    db: State<'_, DbState>,
) -> Result<String, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    let task = get_task_row(&conn, task_id.as_str())?
        .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
    if task.task_type != "upgrade" {
        return Err(AppError::Internal(format!(
            "task is not upgrade: {task_id}"
        )));
    }
    let payload = parse_task_payload(&task)?;
    let planned_machine_ids = payload
        .get("planned_machine_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Internal("planned_machine_ids missing".into()))?;
    if !planned_machine_ids
        .iter()
        .filter_map(Value::as_i64)
        .any(|id| id == machine_id)
    {
        return Err(AppError::Internal(format!(
            "machine {machine_id} is not part of upgrade task {task_id}"
        )));
    }
    let machine = machine_get(&conn, machine_id)?
        .ok_or_else(|| AppError::Internal(format!("machine not found: {machine_id}")))?;
    let rollback_task_id = uuid::Uuid::new_v4().to_string();
    let rollback_payload = json!({
        "source_task_id": task_id,
        "machine_id": machine_id,
        "machine_name": machine.name,
        "previous_version": payload.get("previous_version").cloned().unwrap_or(Value::Null),
        "backup_archive": payload.get("backup_archive").cloned().unwrap_or(Value::Null)
    });
    conn.execute(
        "INSERT INTO task (id, task_type, status, payload)
         VALUES (?1, 'upgrade_rollback', 'pending', ?2)",
        params![rollback_task_id, rollback_payload.to_string()],
    )?;
    conn.execute(
        "INSERT INTO task_machine (task_id, machine_id, status)
         VALUES (?1, ?2, 'pending')",
        params![rollback_task_id, machine_id],
    )?;
    audit_log_insert(
        &conn,
        "upgrade_rollback",
        &json!({
            "task_id": rollback_task_id,
            "source_task_id": task.id,
            "machine_id": machine_id,
            "machine_name": machine.name,
            "previous_version": rollback_payload["previous_version"],
            "backup_archive": rollback_payload["backup_archive"]
        }),
    )?;
    Ok(rollback_task_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{machine_insert, open_and_migrate, pool_insert};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn new_db() -> Connection {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ouro-ops-upgrade-{unique}.sqlite"));
        open_and_migrate(&path).expect("open migrated db")
    }

    fn seed_pool(conn: &Connection) -> (i64, i64, i64) {
        let pool_id =
            pool_insert(conn, "OURO", "mainnet", Some(0.02), Some(340000000)).expect("pool");
        let relay_1 = machine_insert(
            conn,
            pool_id,
            "relay-1",
            "10.0.0.10",
            22,
            "root",
            "relay",
            Some("SHA256:relay-1"),
        )
        .expect("relay-1");
        let relay_2 = machine_insert(
            conn,
            pool_id,
            "relay-2",
            "10.0.0.11",
            22,
            "root",
            "relay",
            Some("SHA256:relay-2"),
        )
        .expect("relay-2");
        let bp = machine_insert(
            conn,
            pool_id,
            "bp-1",
            "10.0.0.12",
            22,
            "root",
            "bp",
            Some("SHA256:bp"),
        )
        .expect("bp-1");
        (relay_1, relay_2, bp)
    }

    #[test]
    fn tc_upg_001_start_reorders_relays_before_bp() {
        let conn = new_db();
        let (relay_1, relay_2, bp) = seed_pool(&conn);
        let machines = fetch_upgrade_machines(&conn, &[bp, relay_2, relay_1]).expect("machines");
        let (relay_ids, bp_ids, planned) =
            partition_upgrade_order(&machines, &[bp, relay_2, relay_1]);
        assert_eq!(relay_ids, vec![relay_2, relay_1]);
        assert_eq!(bp_ids, vec![bp]);
        assert_eq!(planned, vec![relay_2, relay_1, bp]);
    }

    #[test]
    fn tc_upg_002_start_requires_relay() {
        let conn = new_db();
        let (_relay_1, _relay_2, bp) = seed_pool(&conn);
        let payload = UpgradePayload {
            target_version: "10.5.4-1".into(),
            image_registry: DEFAULT_IMAGE_REGISTRY.into(),
            image_digest: None,
            machine_ids: vec![bp],
            auto_continue: false,
        };
        let err = (|| {
            let machines = fetch_upgrade_machines(&conn, &payload.machine_ids)?;
            let (relay_ids, _, _) = partition_upgrade_order(&machines, &payload.machine_ids);
            if relay_ids.is_empty() {
                return Err(AppError::Internal(
                    "upgrade requires at least one relay machine".into(),
                ));
            }
            Ok::<(), AppError>(())
        })()
        .expect_err("relay required");
        assert!(err.to_string().contains("at least one relay"));
    }

    #[test]
    fn tc_upg_003_confirm_next_only_accepts_gate_states() {
        let conn = new_db();
        let (relay_1, _relay_2, bp) = seed_pool(&conn);
        let task_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload) VALUES (?1, 'upgrade', 'pending', ?2)",
            params![
                task_id,
                json!({
                    "phase": "AWAIT_BP_CONFIRM",
                    "planned_machine_ids": [relay_1, bp]
                })
                .to_string()
            ],
        )
        .expect("insert task");
        let task = get_task_row(&conn, task_id.as_str())
            .expect("task")
            .expect("row");
        let payload = parse_task_payload(&task).expect("payload parse");
        let mut next = payload.clone();
        next["phase"] = Value::String("UPGRADING_BP".into());
        next["last_confirmed_at"] = Value::String(chrono::Utc::now().to_rfc3339());
        assert_eq!(
            next.get("phase").and_then(Value::as_str),
            Some("UPGRADING_BP")
        );
        assert!(next
            .get("last_confirmed_at")
            .and_then(Value::as_str)
            .is_some());
    }

    #[test]
    fn tc_upg_004_rollback_payload_carries_previous_version_and_backup() {
        let conn = new_db();
        let (relay_1, _relay_2, bp) = seed_pool(&conn);
        let task_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload) VALUES (?1, 'upgrade', 'failed', ?2)",
            params![
                task_id,
                json!({
                    "planned_machine_ids": [relay_1, bp],
                    "previous_version": "10.5.3-1",
                    "backup_archive": "/opt/cardano/backup/config-20260308.tar.gz"
                })
                .to_string()
            ],
        )
        .expect("insert task");
        let task = get_task_row(&conn, task_id.as_str())
            .expect("task")
            .expect("row");
        let payload = parse_task_payload(&task).expect("payload parse");
        assert_eq!(
            payload.get("previous_version").and_then(Value::as_str),
            Some("10.5.3-1")
        );
        assert_eq!(
            payload.get("backup_archive").and_then(Value::as_str),
            Some("/opt/cardano/backup/config-20260308.tar.gz")
        );
    }

    #[test]
    fn tc_upg_005_gate_payload_marks_bp() {
        let payload = UpgradeGateEvent {
            task_id: "task-1".into(),
            completed_machine: "relay-1".into(),
            next_machine: "bp-1".into(),
            is_bp: true,
            message: "relay done".into(),
        };
        let value = serde_json::to_value(payload).expect("serialize");
        assert_eq!(value["task_id"], "task-1");
        assert_eq!(value["completed_machine"], "relay-1");
        assert_eq!(value["next_machine"], "bp-1");
        assert_eq!(value["is_bp"], true);
    }

    #[test]
    fn tc_upg_006_status_reads_upgrade_task() {
        let conn = new_db();
        let (_relay_1, _relay_2, _bp) = seed_pool(&conn);
        let task_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload) VALUES (?1, 'upgrade', 'paused', ?2)",
            params![task_id, json!({"phase": "AWAIT_NEXT_RELAY"}).to_string()],
        )
        .expect("insert task");
        let status = upgrade_status_with_conn(&conn, &task_id).expect("status");
        assert_eq!(status.status, "paused");
        assert_eq!(
            status.payload.get("phase").and_then(Value::as_str),
            Some("AWAIT_NEXT_RELAY")
        );
    }
}
