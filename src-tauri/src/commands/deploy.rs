use std::collections::HashSet;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use rusqlite::Connection;
use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use crate::db::{
    audit_log_insert, machine_get, machine_list as repo_machine_list, pool_get_single, DbState,
    MachineRow,
};
use crate::error::AppError;
use crate::sidecar::{run_playbook, spawn_sidecar, SidecarState};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DeployPayload {
    pub machine_ids: Vec<i64>,
    pub cardano_version: String,
    pub image_registry: String,
    pub image_digest: Option<String>,
    pub network: String,
    pub enable_swap: bool,
    pub swap_size_gb: i64,
    pub enable_chrony: bool,
    pub enable_hardening: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskMachineStatus {
    pub machine_id: i64,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeployTaskStatus {
    pub task_id: String,
    pub task_type: String,
    pub status: String,
    pub payload: Value,
    pub error_msg: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
    pub machine_statuses: Vec<TaskMachineStatus>,
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

type SshExecFn = dyn Fn(&MachineRow, &str) -> Result<String, AppError>;

fn validate_deploy_payload(payload: &DeployPayload) -> Result<(), AppError> {
    if payload.machine_ids.is_empty() {
        return Err(AppError::Internal("machine_ids must not be empty".into()));
    }
    if payload.cardano_version.trim().is_empty() {
        return Err(AppError::Internal(
            "cardano_version must not be empty".into(),
        ));
    }
    if payload.image_registry.trim().is_empty() {
        return Err(AppError::Internal(
            "image_registry must not be empty".into(),
        ));
    }
    if !matches!(payload.network.as_str(), "mainnet" | "preprod" | "preview") {
        return Err(AppError::Internal(format!(
            "invalid network: {}",
            payload.network
        )));
    }
    if !(8..=16).contains(&payload.swap_size_gb) {
        return Err(AppError::Internal(
            "swap_size_gb must be between 8 and 16".into(),
        ));
    }
    Ok(())
}

fn parse_i64(value: &str) -> i64 {
    value.trim().parse::<i64>().unwrap_or_default()
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

fn preflight_machine_with_exec(machine: &MachineRow, exec: &SshExecFn) -> Result<(), AppError> {
    exec(machine, "echo deploy-preflight-ok >/dev/null")?;

    let disk_available_gb = parse_i64(&exec(
        machine,
        "df -BG / | awk 'NR==2 {gsub(/G/, \"\", $4); print $4}'",
    )?);
    if disk_available_gb < 50 {
        return Err(AppError::DiskInsufficient(disk_available_gb.max(0) as u64));
    }

    Ok(())
}

fn fetch_selected_machines(
    conn: &Connection,
    machine_ids: &[i64],
) -> Result<Vec<MachineRow>, AppError> {
    if machine_ids.is_empty() {
        return Err(AppError::Internal("machine_ids must not be empty".into()));
    }
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for machine_id in machine_ids {
        if !seen.insert(*machine_id) {
            continue;
        }
        let machine = machine_get(conn, *machine_id)?
            .ok_or_else(|| AppError::Internal(format!("machine not found: {machine_id}")))?;
        rows.push(machine);
    }
    if rows.is_empty() {
        return Err(AppError::Internal("no machine selected".into()));
    }
    Ok(rows)
}

fn ensure_minimum_topology(machines: &[MachineRow]) -> Result<(), AppError> {
    let relay_count = machines.iter().filter(|m| m.role == "relay").count();
    let bp_count = machines.iter().filter(|m| m.role == "bp").count();
    if relay_count < 1 || bp_count < 1 {
        return Err(AppError::Internal(
            "deploy requires at least 1 relay and 1 bp".into(),
        ));
    }
    Ok(())
}

fn build_inventory(conn: &Connection, machine_ids: &[i64]) -> Result<Value, AppError> {
    let selected = fetch_selected_machines(conn, machine_ids)?;
    let pool_id = selected[0].pool_id;
    if selected.iter().any(|m| m.pool_id != pool_id) {
        return Err(AppError::Internal(
            "all selected machines must belong to one pool".into(),
        ));
    }

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
        "_meta": {
            "hostvars": hostvars
        },
        "relay": {
            "hosts": relay_hosts
        },
        "bp": {
            "hosts": bp_hosts
        }
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
) -> Result<Vec<TaskMachineStatus>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT machine_id, status
         FROM task_machine
         WHERE task_id = ?1
         ORDER BY machine_id ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![task_id], |row| {
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

fn insert_task_with_machines(
    conn: &Connection,
    task_id: &str,
    payload: &DeployPayload,
) -> Result<(), AppError> {
    let payload_json =
        serde_json::to_string(payload).map_err(|e| AppError::Internal(e.to_string()))?;
    conn.execute(
        "INSERT INTO task (id, task_type, status, payload)
         VALUES (?1, 'deploy', 'pending', ?2)",
        rusqlite::params![task_id, payload_json],
    )?;
    for machine_id in &payload.machine_ids {
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status)
             VALUES (?1, ?2, 'pending')",
            rusqlite::params![task_id, machine_id],
        )?;
    }
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
    Ok(())
}

fn mark_task_machines(conn: &Connection, task_id: &str, status: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE task_machine SET status = ?1 WHERE task_id = ?2",
        rusqlite::params![status, task_id],
    )?;
    Ok(())
}

fn cancel_task_with_conn(conn: &Connection, task_id: &str) -> Result<bool, AppError> {
    let Some(task) = get_task_row(conn, task_id)? else {
        return Err(AppError::Internal(format!("task not found: {task_id}")));
    };
    if task.task_type != "deploy" {
        return Err(AppError::Internal(format!("task is not deploy: {task_id}")));
    }
    if !matches!(task.status.as_str(), "pending" | "running") {
        return Ok(false);
    }
    mark_task_terminal(conn, task_id, "cancelled", Some("cancelled by user"))?;
    conn.execute(
        "UPDATE task_machine
         SET status = 'cancelled'
         WHERE task_id = ?1 AND status IN ('pending', 'running')",
        rusqlite::params![task_id],
    )?;
    Ok(true)
}

fn deploy_status_with_conn(conn: &Connection, task_id: &str) -> Result<DeployTaskStatus, AppError> {
    let task = get_task_row(conn, task_id)?
        .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
    if task.task_type != "deploy" {
        return Err(AppError::Internal(format!("task is not deploy: {task_id}")));
    }
    let payload = task
        .payload
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|e| AppError::Internal(format!("task payload parse failed: {e}")))?
        .unwrap_or(Value::Null);
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

fn task_is_cancelled(conn: &Connection, task_id: &str) -> Result<bool, AppError> {
    let Some(task) = get_task_row(conn, task_id)? else {
        return Ok(false);
    };
    Ok(task.status == "cancelled")
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

fn deploy_playbook_path() -> Result<String, AppError> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| AppError::Internal("CARGO_MANIFEST_DIR not set".into()))?;
    let path = std::path::PathBuf::from(manifest_dir)
        .parent()
        .ok_or_else(|| AppError::Internal("no parent dir".into()))?
        .join("ansible")
        .join("playbooks")
        .join("deploy.yml");
    Ok(path.display().to_string())
}

fn build_extra_vars(payload: &DeployPayload) -> Value {
    json!({
        "cardano_version": payload.cardano_version,
        "image_registry": payload.image_registry,
        "image_digest": payload.image_digest,
        "network": payload.network,
        "enable_swap": payload.enable_swap,
        "swap_size": format!("{}G", payload.swap_size_gb),
        "swap_size_gb": payload.swap_size_gb,
        "enable_chrony": payload.enable_chrony,
        "enable_hardening": payload.enable_hardening
    })
}

fn run_deploy_worker(
    app_handle: &tauri::AppHandle,
    task_id: &str,
    payload: &DeployPayload,
) -> Result<(), AppError> {
    {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        mark_task_running(&conn, task_id)?;
    }

    let selected = {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        let selected = fetch_selected_machines(&conn, &payload.machine_ids)?;
        ensure_minimum_topology(&selected)?;
        let pool = pool_get_single(&conn)?
            .ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
        if pool.network != payload.network {
            return Err(AppError::Internal(
                "payload network must match current pool network".into(),
            ));
        }
        selected
    };

    for machine in &selected {
        preflight_machine_with_exec(machine, &ssh_exec)?;
    }

    let inventory = {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        build_inventory(&conn, &payload.machine_ids)?
    };

    let playbook = deploy_playbook_path()?;
    let extra_vars = build_extra_vars(payload);
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
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    if !task_is_cancelled(&conn, task_id)? {
        mark_task_terminal(&conn, task_id, "success", None)?;
        mark_task_machines(&conn, task_id, "success")?;
    }
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
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    if task_is_cancelled(&conn, task_id)? {
        return Ok(());
    }
    mark_task_terminal(&conn, task_id, "failed", Some(message))?;
    mark_task_machines(&conn, task_id, "failed")?;
    emit_task_failed(app_handle, task_id, message);
    Ok(())
}

#[tauri::command]
pub async fn deploy_start(
    payload: DeployPayload,
    db: State<'_, DbState>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    validate_deploy_payload(&payload)?;

    {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        let selected = fetch_selected_machines(&conn, &payload.machine_ids)?;
        ensure_minimum_topology(&selected)?;
        let pool = pool_get_single(&conn)?
            .ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
        if pool.network != payload.network {
            return Err(AppError::Internal(
                "payload network must match current pool network".into(),
            ));
        }
    }

    let task_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        insert_task_with_machines(&conn, &task_id, &payload)?;
        audit_log_insert(
            &conn,
            "deploy_start",
            &json!({
                "task_id": task_id,
                "machine_ids": payload.machine_ids,
                "cardano_version": payload.cardano_version,
                "network": payload.network
            }),
        )?;
    }

    let task_id_for_worker = task_id.clone();
    let payload_for_worker = payload.clone();
    let app_for_worker = app_handle.clone();
    thread::spawn(move || {
        if let Err(err) =
            run_deploy_worker(&app_for_worker, &task_id_for_worker, &payload_for_worker)
        {
            let _ =
                mark_task_failed_if_needed(&app_for_worker, &task_id_for_worker, &err.to_string());
        }
    });

    Ok(task_id)
}

#[tauri::command]
pub async fn deploy_status(
    task_id: String,
    db: State<'_, DbState>,
) -> Result<DeployTaskStatus, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    deploy_status_with_conn(&conn, task_id.as_str())
}

#[tauri::command]
pub async fn deploy_cancel(
    task_id: String,
    db: State<'_, DbState>,
    sidecar: State<'_, Mutex<Option<Arc<SidecarState>>>>,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    let should_interrupt = {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        cancel_task_with_conn(&conn, task_id.as_str())?
    };

    if !should_interrupt {
        return Ok(());
    }

    let _ = app_handle.emit(
        "task:failed",
        json!({
            "task_id": task_id,
            "status": "cancelled",
            "error": "cancelled by user"
        }),
    );

    let previous = {
        let mut guard = sidecar
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        guard.take()
    };
    if let Some(old_state) = previous {
        let _ = old_state.terminate_process();
    }

    let new_state = Arc::new(spawn_sidecar(app_handle.clone())?);
    {
        let mut runner = new_state
            .runner
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        let r = runner.as_mut().ok_or(AppError::SidecarCrash)?;
        r.ping()?;
    }
    let mut guard = sidecar
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    *guard = Some(new_state);

    Ok(())
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

    fn seed_pool_with_nodes(conn: &Connection) -> (i64, i64, i64) {
        let pool_id =
            pool_insert(conn, "OURO", "preprod", Some(0.02), Some(340_000_000)).expect("pool");
        let relay_1 = machine_insert(
            conn,
            pool_id,
            "relay-1",
            "203.0.113.10",
            22,
            "root",
            "relay",
            Some("SHA256:relay1"),
        )
        .expect("relay1");
        let relay_2 = machine_insert(
            conn,
            pool_id,
            "relay-2",
            "203.0.113.11",
            22,
            "root",
            "relay",
            Some("SHA256:relay2"),
        )
        .expect("relay2");
        let bp_1 = machine_insert(
            conn,
            pool_id,
            "bp-1",
            "10.0.0.5",
            22,
            "root",
            "bp",
            Some("SHA256:bp1"),
        )
        .expect("bp1");
        (relay_1, relay_2, bp_1)
    }

    #[test]
    fn tc_dep_001_payload_validation() {
        let base = DeployPayload {
            machine_ids: vec![1],
            cardano_version: "10.2.1".into(),
            image_registry: "ghcr.io/intersectmbo/cardano-node".into(),
            image_digest: None,
            network: "preprod".into(),
            enable_swap: true,
            swap_size_gb: 8,
            enable_chrony: true,
            enable_hardening: true,
        };
        assert!(validate_deploy_payload(&base).is_ok());

        let mut empty_ids = base.clone();
        empty_ids.machine_ids = Vec::new();
        assert!(validate_deploy_payload(&empty_ids).is_err());

        let mut bad_swap = base.clone();
        bad_swap.swap_size_gb = 4;
        assert!(validate_deploy_payload(&bad_swap).is_err());
    }

    #[test]
    fn tc_inv_001_inventory_contains_groups_and_hostvars() {
        let conn = new_db();
        let (relay_1, relay_2, bp_1) = seed_pool_with_nodes(&conn);
        let inventory = build_inventory(&conn, &[relay_1, relay_2, bp_1]).expect("inventory");

        let hostvars = inventory["_meta"]["hostvars"]
            .as_object()
            .expect("hostvars object");
        assert!(hostvars.contains_key("relay-1"));
        assert!(hostvars.contains_key("relay-2"));
        assert!(hostvars.contains_key("bp-1"));

        let relay_hosts = inventory["relay"]["hosts"].as_array().expect("relay hosts");
        let bp_hosts = inventory["bp"]["hosts"].as_array().expect("bp hosts");
        assert_eq!(relay_hosts.len(), 2);
        assert_eq!(bp_hosts.len(), 1);

        let bp_vars = hostvars.get("bp-1").expect("bp vars");
        assert_eq!(bp_vars["role"], "bp");
        assert_eq!(bp_vars["network"], "preprod");
        assert!(
            bp_vars["trusted_relay_ips"]
                .as_array()
                .expect("trusted ips")
                .len()
                >= 2
        );
    }

    #[test]
    fn tc_dep_002_insert_task_and_task_machine_rows() {
        let conn = new_db();
        let (relay_1, _, bp_1) = seed_pool_with_nodes(&conn);
        let payload = DeployPayload {
            machine_ids: vec![relay_1, bp_1],
            cardano_version: "10.2.1".into(),
            image_registry: "ghcr.io/intersectmbo/cardano-node".into(),
            image_digest: None,
            network: "preprod".into(),
            enable_swap: true,
            swap_size_gb: 8,
            enable_chrony: true,
            enable_hardening: true,
        };
        insert_task_with_machines(&conn, "task-1", &payload).expect("insert task");

        let task_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM task WHERE id='task-1'", [], |r| {
                r.get(0)
            })
            .expect("count task");
        let tm_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_machine WHERE task_id='task-1'",
                [],
                |r| r.get(0),
            )
            .expect("count task_machine");
        assert_eq!(task_count, 1);
        assert_eq!(tm_count, 2);
    }

    #[test]
    fn tc_dep_003_deploy_status_reads_db() {
        let conn = new_db();
        let (relay_1, _, bp_1) = seed_pool_with_nodes(&conn);
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload, started_at)
             VALUES ('task-2', 'deploy', 'running', '{\"network\":\"preprod\"}', datetime('now'))",
            [],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status) VALUES ('task-2', ?1, 'running')",
            rusqlite::params![relay_1],
        )
        .expect("insert tm relay");
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status) VALUES ('task-2', ?1, 'running')",
            rusqlite::params![bp_1],
        )
        .expect("insert tm bp");

        let status = deploy_status_with_conn(&conn, "task-2").expect("status");
        assert_eq!(status.task_id, "task-2");
        assert_eq!(status.status, "running");
        assert_eq!(status.machine_statuses.len(), 2);
    }

    #[test]
    fn tc_dep_004_cancel_running_task() {
        let conn = new_db();
        let (relay_1, _, _) = seed_pool_with_nodes(&conn);
        conn.execute(
            "INSERT INTO task (id, task_type, status) VALUES ('task-3', 'deploy', 'running')",
            [],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status) VALUES ('task-3', ?1, 'running')",
            rusqlite::params![relay_1],
        )
        .expect("insert task_machine");

        let cancelled = cancel_task_with_conn(&conn, "task-3").expect("cancel");
        assert!(cancelled);
        let task = get_task_row(&conn, "task-3")
            .expect("task")
            .expect("exists");
        assert_eq!(task.status, "cancelled");
    }

    #[test]
    fn tc_dep_005_preflight_disk_insufficient() {
        let conn = new_db();
        let (relay_1, _, _) = seed_pool_with_nodes(&conn);
        let machine = machine_get(&conn, relay_1)
            .expect("query machine")
            .expect("machine exists");

        let err = preflight_machine_with_exec(&machine, &|_m, cmd| {
            if cmd.contains("df -BG /") {
                return Ok("40".into());
            }
            Ok("ok".into())
        })
        .expect_err("preflight should fail");
        match err {
            AppError::DiskInsufficient(v) => assert_eq!(v, 40),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn tc_dep_006_mark_failed_and_success_transitions() {
        let conn = new_db();
        let (relay_1, _, _) = seed_pool_with_nodes(&conn);
        conn.execute(
            "INSERT INTO task (id, task_type, status) VALUES ('task-4', 'deploy', 'running')",
            [],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status) VALUES ('task-4', ?1, 'running')",
            rusqlite::params![relay_1],
        )
        .expect("insert task_machine");

        mark_task_terminal(&conn, "task-4", "failed", Some("sync timeout")).expect("failed");
        let failed = get_task_row(&conn, "task-4").expect("row").expect("exists");
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.error_msg.as_deref(), Some("sync timeout"));

        mark_task_terminal(&conn, "task-4", "success", None).expect("success");
        let success = get_task_row(&conn, "task-4").expect("row").expect("exists");
        assert_eq!(success.status, "success");
    }

    #[test]
    fn tc_dep_007_minimum_topology_requires_relay_and_bp() {
        let conn = new_db();
        let (relay_1, _, bp_1) = seed_pool_with_nodes(&conn);
        let relay_only = fetch_selected_machines(&conn, &[relay_1]).expect("relay only");
        let err = ensure_minimum_topology(&relay_only).expect_err("should require bp");
        assert!(err.to_string().contains("1 relay and 1 bp"));

        let bp_only = fetch_selected_machines(&conn, &[bp_1]).expect("bp only");
        let err = ensure_minimum_topology(&bp_only).expect_err("should require relay");
        assert!(err.to_string().contains("1 relay and 1 bp"));

        let both = fetch_selected_machines(&conn, &[relay_1, bp_1]).expect("both");
        assert!(ensure_minimum_topology(&both).is_ok());
    }
}
