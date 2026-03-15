use std::sync::{Arc, Mutex};
use std::thread;

use rusqlite::Connection;
use serde_json::{json, Value};
use tauri::{Manager, State};

use crate::commands::deploy::DeployTaskStatus;
use crate::db::{
    audit_log_insert, machine_get, machine_list as repo_machine_list, pool_get_single, DbState,
    MachineRow,
};
use crate::error::AppError;
use crate::sidecar::{run_playbook, SidecarState};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeApplyConfigPayload {
    pub machine_id: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeRestartPayload {
    pub machine_id: i64,
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

fn runtime_config_playbook_path() -> Result<String, AppError> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| AppError::Internal("CARGO_MANIFEST_DIR not set".into()))?;
    let path = std::path::PathBuf::from(manifest_dir)
        .parent()
        .ok_or_else(|| AppError::Internal("no parent dir".into()))?
        .join("ansible")
        .join("playbooks")
        .join("runtime-config.yml");
    Ok(path.display().to_string())
}

fn runtime_restart_playbook_path() -> Result<String, AppError> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| AppError::Internal("CARGO_MANIFEST_DIR not set".into()))?;
    let path = std::path::PathBuf::from(manifest_dir)
        .parent()
        .ok_or_else(|| AppError::Internal("no parent dir".into()))?
        .join("ansible")
        .join("playbooks")
        .join("runtime-restart.yml");
    Ok(path.display().to_string())
}

fn selected_machine(conn: &Connection, machine_id: i64) -> Result<MachineRow, AppError> {
    let machine = machine_get(conn, machine_id)?
        .ok_or_else(|| AppError::Internal(format!("machine not found: {machine_id}")))?;
    if !matches!(machine.role.as_str(), "relay" | "bp") {
        return Err(AppError::Internal(format!(
            "runtime config apply supports relay/bp only, got {}",
            machine.role
        )));
    }
    Ok(machine)
}

fn build_runtime_inventory(conn: &Connection, machine_id: i64) -> Result<Value, AppError> {
    let machine = selected_machine(conn, machine_id)?;
    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let all_pool_machines: Vec<MachineRow> =
        repo_machine_list(conn, None, Some(pool.network.as_str()))?
            .into_iter()
            .filter(|m| m.pool_id == machine.pool_id)
            .collect();
    let relay_nodes: Vec<Value> = all_pool_machines
        .iter()
        .filter(|m| m.role == "relay")
        .map(|m| {
            json!({
                "ip": m.ip,
                "name": m.name
            })
        })
        .collect();
    let bp_nodes: Vec<Value> = all_pool_machines
        .iter()
        .filter(|m| m.role == "bp")
        .map(|m| {
            json!({
                "ip": m.ip,
                "name": m.name
            })
        })
        .collect();

    if machine.role == "bp" && relay_nodes.is_empty() {
        return Err(AppError::Internal(
            "runtime config apply for bp requires at least one relay in the pool".into(),
        ));
    }

    let mut hostvars = serde_json::Map::new();
    hostvars.insert(
        machine.name.clone(),
        json!({
            "ansible_host": machine.ip,
            "ansible_port": machine.ssh_port,
            "ansible_user": machine.ssh_user,
            "role": machine.role,
            "network": machine.network,
            "relay_nodes": relay_nodes,
            "bp_nodes": bp_nodes,
        }),
    );

    Ok(json!({
        "_meta": { "hostvars": hostvars },
        "relay": { "hosts": if machine.role == "relay" { vec![machine.name.clone()] } else { Vec::<String>::new() } },
        "bp": { "hosts": if machine.role == "bp" { vec![machine.name.clone()] } else { Vec::<String>::new() } }
    }))
}

fn get_task_row(conn: &Connection, task_id: &str) -> Result<Option<TaskRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_type, status, payload, error_msg, started_at, finished_at, created_at
         FROM task
         WHERE id = ?1
         LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![task_id])?;
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
) -> Result<Vec<crate::commands::deploy::TaskMachineStatus>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT machine_id, status
         FROM task_machine
         WHERE task_id = ?1
         ORDER BY machine_id ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![task_id], |row| {
        Ok(crate::commands::deploy::TaskMachineStatus {
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

fn insert_runtime_task(
    conn: &Connection,
    task_id: &str,
    task_type: &str,
    machine_id: i64,
    payload_json: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO task (id, task_type, status, payload)
         VALUES (?1, ?2, 'pending', ?3)",
        rusqlite::params![task_id, task_type, payload_json],
    )?;
    conn.execute(
        "INSERT INTO task_machine (task_id, machine_id, status)
         VALUES (?1, ?2, 'pending')",
        rusqlite::params![task_id, machine_id],
    )?;
    Ok(())
}

fn mark_task_running(conn: &Connection, task_id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE task
         SET status = 'running', started_at = COALESCE(started_at, datetime('now')), error_msg = NULL
         WHERE id = ?1",
        rusqlite::params![task_id],
    )?;
    conn.execute(
        "UPDATE task_machine SET status = 'running' WHERE task_id = ?1 AND status = 'pending'",
        rusqlite::params![task_id],
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
        rusqlite::params![status, error_msg, task_id],
    )?;
    conn.execute(
        "UPDATE task_machine SET status = ?1 WHERE task_id = ?2",
        rusqlite::params![status, task_id],
    )?;
    Ok(())
}

fn runtime_config_status_with_conn(
    conn: &Connection,
    task_id: &str,
) -> Result<DeployTaskStatus, AppError> {
    runtime_task_status_with_conn(conn, task_id, "runtime_config")
}

fn runtime_restart_status_with_conn(
    conn: &Connection,
    task_id: &str,
) -> Result<DeployTaskStatus, AppError> {
    runtime_task_status_with_conn(conn, task_id, "runtime_restart")
}

fn runtime_task_status_with_conn(
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

fn run_runtime_config_worker(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    payload: &RuntimeApplyConfigPayload,
) -> Result<(), AppError> {
    {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        mark_task_running(&conn, task_id)?;
    }

    let inventory = {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        build_runtime_inventory(&conn, payload.machine_id)?
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
        runtime_config_playbook_path()?.as_str(),
        inventory,
        json!({}),
    )?;

    let db_state = app_handle.state::<DbState>();
    let conn = db_state
        .0
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    mark_task_terminal(&conn, task_id, "success", None)?;
    Ok(())
}

fn run_runtime_restart_worker(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    payload: &RuntimeRestartPayload,
) -> Result<(), AppError> {
    {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        mark_task_running(&conn, task_id)?;
    }

    let inventory = {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        build_runtime_inventory(&conn, payload.machine_id)?
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
        runtime_restart_playbook_path()?.as_str(),
        inventory,
        json!({}),
    )?;

    let db_state = app_handle.state::<DbState>();
    let conn = db_state
        .0
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    mark_task_terminal(&conn, task_id, "success", None)?;
    Ok(())
}

fn mark_runtime_task_failed_if_needed(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    message: &str,
) -> Result<(), AppError> {
    let db_state = app_handle.state::<DbState>();
    let conn = db_state
        .0
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    mark_task_terminal(&conn, task_id, "failed", Some(message))?;
    Ok(())
}

#[tauri::command]
pub async fn runtime_apply_config(
    machine_id: i64,
    db: State<'_, DbState>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        let machine = selected_machine(&conn, machine_id)?;
        audit_log_insert(
            &conn,
            "runtime_apply_config_start",
            &json!({
                "machine_id": machine_id,
                "machine_name": machine.name,
                "role": machine.role
            }),
        )?;
    }

    let task_id = uuid::Uuid::new_v4().to_string();
    let payload = RuntimeApplyConfigPayload { machine_id };
    {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        let payload_json =
            serde_json::to_string(&payload).map_err(|e| AppError::Internal(e.to_string()))?;
        insert_runtime_task(
            &conn,
            &task_id,
            "runtime_config",
            payload.machine_id,
            &payload_json,
        )?;
    }

    let task_id_for_worker = task_id.clone();
    let payload_for_worker = payload.clone();
    let app_for_worker = app_handle.clone();
    thread::spawn(move || {
        if let Err(err) =
            run_runtime_config_worker(&app_for_worker, &task_id_for_worker, &payload_for_worker)
        {
            let _ = mark_runtime_task_failed_if_needed(
                &app_for_worker,
                &task_id_for_worker,
                &err.to_string(),
            );
        }
    });

    Ok(task_id)
}

#[tauri::command]
pub async fn runtime_restart(
    machine_id: i64,
    db: State<'_, DbState>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        let machine = selected_machine(&conn, machine_id)?;
        audit_log_insert(
            &conn,
            "runtime_restart_start",
            &json!({
                "machine_id": machine_id,
                "machine_name": machine.name,
                "role": machine.role
            }),
        )?;
    }

    let task_id = uuid::Uuid::new_v4().to_string();
    let payload = RuntimeRestartPayload { machine_id };
    {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        let payload_json =
            serde_json::to_string(&payload).map_err(|e| AppError::Internal(e.to_string()))?;
        insert_runtime_task(
            &conn,
            &task_id,
            "runtime_restart",
            payload.machine_id,
            &payload_json,
        )?;
    }

    let task_id_for_worker = task_id.clone();
    let payload_for_worker = payload.clone();
    let app_for_worker = app_handle.clone();
    thread::spawn(move || {
        if let Err(err) =
            run_runtime_restart_worker(&app_for_worker, &task_id_for_worker, &payload_for_worker)
        {
            let _ = mark_runtime_task_failed_if_needed(
                &app_for_worker,
                &task_id_for_worker,
                &err.to_string(),
            );
        }
    });

    Ok(task_id)
}

#[tauri::command]
pub async fn runtime_config_status(
    task_id: String,
    db: State<'_, DbState>,
) -> Result<DeployTaskStatus, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    runtime_config_status_with_conn(&conn, task_id.as_str())
}

#[tauri::command]
pub async fn runtime_restart_status(
    task_id: String,
    db: State<'_, DbState>,
) -> Result<DeployTaskStatus, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    runtime_restart_status_with_conn(&conn, task_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{machine_insert, pool_insert, run_migrations};

    fn new_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        conn
    }

    fn seed_pool(conn: &Connection) -> (i64, i64) {
        let pool_id =
            pool_insert(conn, "OURO", "mainnet", Some(0.02), Some(340000000)).expect("pool");
        let relay_id = machine_insert(
            conn,
            pool_id,
            "relay-1",
            "10.0.0.10",
            22,
            "root",
            "relay",
            Some("SHA256:relay"),
        )
        .expect("relay");
        let bp_id = machine_insert(
            conn,
            pool_id,
            "bp-1",
            "10.0.0.11",
            22,
            "root",
            "bp",
            Some("SHA256:bp"),
        )
        .expect("bp");
        (relay_id, bp_id)
    }

    #[test]
    fn tc_cfg_001_runtime_inventory_includes_pool_topology_context() {
        let conn = new_db();
        let (relay_id, bp_id) = seed_pool(&conn);
        let inventory = build_runtime_inventory(&conn, bp_id).expect("inventory");
        let hostvars = inventory
            .get("_meta")
            .and_then(|v| v.get("hostvars"))
            .and_then(|v| v.get("bp-1"))
            .cloned()
            .expect("bp hostvars");
        assert_eq!(hostvars.get("role").and_then(Value::as_str), Some("bp"));
        assert_eq!(
            hostvars
                .get("relay_nodes")
                .and_then(Value::as_array)
                .map(|v| v.len()),
            Some(1)
        );
        let relay_group = inventory
            .get("relay")
            .and_then(|v| v.get("hosts"))
            .and_then(Value::as_array)
            .expect("relay group");
        assert!(relay_group.is_empty());
        let bp_group = inventory
            .get("bp")
            .and_then(|v| v.get("hosts"))
            .and_then(Value::as_array)
            .expect("bp group");
        assert_eq!(bp_group.len(), 1);
        assert_eq!(relay_id, relay_id);
    }

    #[test]
    fn tc_cfg_002_runtime_apply_rejects_non_runtime_roles() {
        let conn = new_db();
        let pool_id =
            pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000)).expect("pool");
        let archive_id = machine_insert(
            &conn,
            pool_id,
            "archive-1",
            "10.0.0.12",
            22,
            "root",
            "archive",
            Some("SHA256:archive"),
        )
        .expect("archive");
        let err = selected_machine(&conn, archive_id).expect_err("reject archive");
        assert!(err.to_string().contains("relay/bp only"));
    }

    #[test]
    fn tc_cfg_003_runtime_config_task_roundtrip_status() {
        let conn = new_db();
        let (_relay_id, bp_id) = seed_pool(&conn);
        let task_id = uuid::Uuid::new_v4().to_string();
        let payload = RuntimeApplyConfigPayload { machine_id: bp_id };
        let payload_json = serde_json::to_string(&payload).expect("payload");
        insert_runtime_task(
            &conn,
            &task_id,
            "runtime_config",
            payload.machine_id,
            &payload_json,
        )
        .expect("insert task");
        mark_task_running(&conn, &task_id).expect("running");
        let status = runtime_config_status_with_conn(&conn, &task_id).expect("status");
        assert_eq!(status.task_type, "runtime_config");
        assert_eq!(status.status, "running");
        assert_eq!(status.machine_statuses.len(), 1);
        assert_eq!(status.machine_statuses[0].machine_id, bp_id);
    }

    #[test]
    fn tc_cfg_004_runtime_restart_task_roundtrip_status() {
        let conn = new_db();
        let (relay_id, _bp_id) = seed_pool(&conn);
        let task_id = uuid::Uuid::new_v4().to_string();
        let payload = RuntimeRestartPayload {
            machine_id: relay_id,
        };
        let payload_json = serde_json::to_string(&payload).expect("payload");
        insert_runtime_task(
            &conn,
            &task_id,
            "runtime_restart",
            payload.machine_id,
            &payload_json,
        )
        .expect("insert task");
        mark_task_running(&conn, &task_id).expect("running");
        let status = runtime_restart_status_with_conn(&conn, &task_id).expect("status");
        assert_eq!(status.task_type, "runtime_restart");
        assert_eq!(status.status, "running");
        assert_eq!(status.machine_statuses.len(), 1);
        assert_eq!(status.machine_statuses[0].machine_id, relay_id);
    }
}
