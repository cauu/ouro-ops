use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::commands::deploy::{DeployTaskStatus, TaskMachineStatus};
use crate::db::{
    audit_log_insert, machine_get, machine_list as repo_machine_list, pool_get_single, DbState,
    MachineRow,
};
use crate::error::AppError;
use crate::sidecar::{run_playbook, SidecarState};

#[derive(Debug, Clone, serde::Serialize)]
pub struct KesStatus {
    pub machine_id: i64,
    pub machine_name: String,
    pub kes_period_current: Option<i64>,
    pub kes_period_max: Option<i64>,
    pub remaining_days: Option<i64>,
    pub severity: String,
    pub expiry_date: Option<String>,
    pub op_cert_counter: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct KesSignRequest {
    pub machine_id: i64,
    pub kes_vkey_path: String,
    pub counter_value: i64,
    pub instructions: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct KesRotationPayload {
    machine_id: i64,
    cert_path: String,
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

fn kes_staging_dir(app_handle: &AppHandle, machine_id: i64) -> Result<PathBuf, AppError> {
    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|err| AppError::Internal(format!("app_data_dir error: {err}")))?;
    Ok(app_dir.join("kes").join(machine_id.to_string()))
}

fn kes_generate_playbook_path() -> Result<String, AppError> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| AppError::Internal("CARGO_MANIFEST_DIR not set".into()))?;
    let path = PathBuf::from(manifest_dir)
        .parent()
        .ok_or_else(|| AppError::Internal("no parent dir".into()))?
        .join("ansible")
        .join("playbooks")
        .join("kes-generate.yml");
    Ok(path.display().to_string())
}

fn severity_for_remaining_days(remaining_days: Option<i64>) -> String {
    match remaining_days {
        Some(days) if days > 10 => "healthy".into(),
        Some(days) if days >= 3 => "warning".into(),
        Some(_) => "critical".into(),
        None => "warning".into(),
    }
}

fn remaining_days_from_expiry(
    conn: &Connection,
    expiry_date: Option<&str>,
) -> Result<Option<i64>, AppError> {
    let Some(expiry_date) = expiry_date else {
        return Ok(None);
    };
    let remaining = conn.query_row(
        "SELECT CAST(julianday(?1) - julianday('now') AS INTEGER)",
        params![expiry_date],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    Ok(remaining)
}

fn read_kes_statuses(conn: &Connection) -> Result<Vec<KesStatus>, AppError> {
    let machines = repo_machine_list(conn, None, None)?
        .into_iter()
        .filter(|machine| machine.role == "bp")
        .collect::<Vec<_>>();
    let mut results = Vec::with_capacity(machines.len());

    for machine in machines {
        let row = conn.query_row(
            "SELECT kes_period_current, kes_period_max, op_cert_counter, expiry_date
             FROM kes_state
             WHERE machine_id = ?1",
            params![machine.id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        );

        let (kes_period_current, kes_period_max, op_cert_counter, expiry_date) = match row {
            Ok(values) => values,
            Err(rusqlite::Error::QueryReturnedNoRows) => (None, None, None, None),
            Err(err) => return Err(AppError::from(err)),
        };
        let remaining_days = remaining_days_from_expiry(conn, expiry_date.as_deref())?;
        results.push(KesStatus {
            machine_id: machine.id,
            machine_name: machine.name,
            kes_period_current,
            kes_period_max,
            remaining_days,
            severity: severity_for_remaining_days(remaining_days),
            expiry_date,
            op_cert_counter,
        });
    }

    Ok(results)
}

fn ensure_bp_machine(conn: &Connection, machine_id: i64) -> Result<MachineRow, AppError> {
    let machine = machine_get(conn, machine_id)?
        .ok_or_else(|| AppError::Internal(format!("machine {machine_id} not found")))?;
    if machine.role != "bp" {
        return Err(AppError::Internal(format!(
            "machine {} is role {}, expected bp",
            machine_id, machine.role
        )));
    }
    Ok(machine)
}

fn current_op_cert_counter(conn: &Connection, machine_id: i64) -> Result<i64, AppError> {
    let counter = conn.query_row(
        "SELECT COALESCE(op_cert_counter, 0) FROM kes_state WHERE machine_id = ?1",
        params![machine_id],
        |row| row.get::<_, i64>(0),
    );
    match counter {
        Ok(value) => Ok(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(err) => Err(AppError::from(err)),
    }
}

fn run_kes_generate_remote(
    app_handle: &AppHandle,
    machine_id: i64,
    staging_dir: &Path,
    counter_value: i64,
) -> Result<KesSignRequest, AppError> {
    fs::create_dir_all(staging_dir)?;
    let vkey_path = staging_dir.join("kes.vkey");
    let cert_path = staging_dir.join("node.cert");
    if vkey_path.exists() {
        fs::remove_file(&vkey_path)?;
    }
    if cert_path.exists() {
        fs::remove_file(&cert_path)?;
    }

    let inventory = {
        let db_state = app_handle.state::<DbState>();
        let conn = db_state
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        build_kes_inventory(&conn, machine_id)?
    };

    let sidecar_state = {
        let managed = app_handle.state::<Mutex<Option<Arc<SidecarState>>>>();
        let guard = managed
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        guard.as_ref().cloned().ok_or(AppError::SidecarCrash)?
    };

    let vkey_dest = vkey_path
        .to_str()
        .ok_or_else(|| AppError::Internal("invalid kes.vkey path".into()))?;

    run_playbook(
        sidecar_state.as_ref(),
        app_handle,
        &format!("kes-gen-{machine_id}"),
        kes_generate_playbook_path()?.as_str(),
        inventory,
        json!({ "kes_vkey_fetch_dest": vkey_dest }),
    )?;

    if !vkey_path.exists() {
        return Err(AppError::Internal(
            "KES vkey 未能从 BP 节点取回，请检查 BP 连接状态。".into(),
        ));
    }

    let vkey = vkey_path.display().to_string();
    Ok(KesSignRequest {
        machine_id,
        kes_vkey_path: vkey.clone(),
        counter_value,
        instructions: format!(
            "1. 将 {vkey} 拷贝到离线冷环境。\n2. 使用当前 operational certificate counter={counter_value} 生成新的 node.cert。\n3. 返回到控制平面后调用 kes_import_cert(machine_id, cert_path) 导入证书。"
        ),
    })
}

fn validate_operational_cert(cert_path: &Path) -> Result<Value, AppError> {
    let raw = fs::read_to_string(cert_path)?;
    let parsed: Value = serde_json::from_str(raw.as_str()).map_err(|_| AppError::InvalidKesCert)?;
    let cert_type = parsed
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !cert_type.contains("OperationalCertificate") {
        return Err(AppError::InvalidKesCert);
    }
    Ok(parsed)
}

fn kes_push_playbook_path() -> Result<String, AppError> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| AppError::Internal("CARGO_MANIFEST_DIR not set".into()))?;
    let path = PathBuf::from(manifest_dir)
        .parent()
        .ok_or_else(|| AppError::Internal("no parent dir".into()))?
        .join("ansible")
        .join("playbooks")
        .join("kes-push.yml");
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

fn build_kes_inventory(conn: &Connection, machine_id: i64) -> Result<Value, AppError> {
    let machine = ensure_bp_machine(conn, machine_id)?;
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
        .map(|m| json!({ "ip": m.ip, "name": m.name }))
        .collect();
    let bp_nodes: Vec<Value> = vec![json!({ "ip": machine.ip, "name": machine.name })];

    if relay_nodes.is_empty() {
        return Err(AppError::Internal(
            "KES push for bp requires at least one relay in the pool".into(),
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
        "bp": { "hosts": [machine.name] },
        "relay": { "hosts": [] }
    }))
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

fn kes_rotation_status_with_conn(
    conn: &Connection,
    task_id: &str,
) -> Result<DeployTaskStatus, AppError> {
    let task = get_task_row(conn, task_id)?
        .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
    if task.task_type != "kes_rotation" {
        return Err(AppError::Internal(format!(
            "task is not kes_rotation: {task_id}"
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

fn run_kes_push_worker(
    app_handle: &AppHandle,
    task_id: &str,
    payload: &KesRotationPayload,
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
        build_kes_inventory(&conn, payload.machine_id)?
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
        kes_push_playbook_path()?.as_str(),
        inventory,
        json!({ "kes_cert_path": payload.cert_path }),
    )?;

    let db_state = app_handle.state::<DbState>();
    let conn = db_state
        .0
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    conn.execute(
        "INSERT INTO kes_state (machine_id, last_checked_at)
         VALUES (?1, datetime('now'))
         ON CONFLICT(machine_id) DO UPDATE SET last_checked_at=datetime('now')",
        params![payload.machine_id],
    )?;
    mark_task_terminal(&conn, task_id, "success", None)?;
    Ok(())
}

fn mark_kes_task_failed_if_needed(
    app_handle: &AppHandle,
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
pub async fn kes_status_all(db: State<'_, DbState>) -> Result<Vec<KesStatus>, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    read_kes_statuses(&conn)
}

#[tauri::command]
pub async fn kes_generate(
    machine_id: i64,
    db: State<'_, DbState>,
    app_handle: AppHandle,
) -> Result<KesSignRequest, AppError> {
    let counter_value = {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        ensure_bp_machine(&conn, machine_id)?;
        current_op_cert_counter(&conn, machine_id)?
    };
    let staging_dir = kes_staging_dir(&app_handle, machine_id)?;

    let sign_request =
        run_kes_generate_remote(&app_handle, machine_id, &staging_dir, counter_value)?;

    {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        audit_log_insert(
            &conn,
            "kes_generate",
            &json!({
                "machine_id": machine_id,
                "kes_vkey_path": sign_request.kes_vkey_path,
                "counter_value": sign_request.counter_value,
                "mode": "remote"
            }),
        )?;
    }
    Ok(sign_request)
}

#[tauri::command]
pub async fn kes_import_cert(
    machine_id: i64,
    cert_path: String,
    db: State<'_, DbState>,
    app_handle: AppHandle,
) -> Result<String, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    ensure_bp_machine(&conn, machine_id)?;
    let source_path = PathBuf::from(cert_path.as_str());
    let cert_json = validate_operational_cert(source_path.as_path())?;

    let staging_dir = kes_staging_dir(&app_handle, machine_id)?;
    fs::create_dir_all(&staging_dir)?;
    let staged_cert = staging_dir.join("node.cert");
    fs::copy(source_path, &staged_cert)?;

    let task_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO task (id, task_type, status, payload, started_at)
         VALUES (?1, 'kes_rotation', 'pending', ?2, datetime('now'))",
        params![
            task_id,
            json!({
                "machine_id": machine_id,
                "cert_path": staged_cert.display().to_string()
            })
            .to_string()
        ],
    )?;
    conn.execute(
        "INSERT INTO task_machine (task_id, machine_id, status)
         VALUES (?1, ?2, 'pending')",
        params![task_id, machine_id],
    )?;
    conn.execute(
        "INSERT INTO kes_state (machine_id, last_checked_at)
         VALUES (?1, datetime('now'))
         ON CONFLICT(machine_id) DO UPDATE SET last_checked_at=datetime('now')",
        params![machine_id],
    )?;
    audit_log_insert(
        &conn,
        "kes_import_cert",
        &json!({
            "machine_id": machine_id,
            "task_id": task_id,
            "cert_type": cert_json.get("type").and_then(Value::as_str),
            "staged_cert_path": staged_cert.display().to_string()
        }),
    )?;
    Ok(task_id)
}

#[tauri::command]
pub async fn kes_push_start(
    task_id: String,
    db: State<'_, DbState>,
    app_handle: AppHandle,
) -> Result<String, AppError> {
    let payload = {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        let task = get_task_row(&conn, task_id.as_str())?
            .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
        if task.task_type != "kes_rotation" {
            return Err(AppError::Internal(format!(
                "task is not kes_rotation: {task_id}"
            )));
        }
        if task.status != "pending" {
            return Err(AppError::Internal(format!(
                "task is not pending: {task_id}"
            )));
        }
        let payload: KesRotationPayload = serde_json::from_str(
            task.payload
                .as_deref()
                .ok_or_else(|| AppError::Internal("task payload missing".into()))?,
        )
        .map_err(|e| AppError::Internal(format!("task payload parse failed: {e}")))?;
        if !Path::new(payload.cert_path.as_str()).exists() {
            return Err(AppError::Internal(format!(
                "staged cert not found: {}",
                payload.cert_path
            )));
        }
        let machine = ensure_bp_machine(&conn, payload.machine_id)?;
        audit_log_insert(
            &conn,
            "kes_push_start",
            &json!({
                "task_id": task_id,
                "machine_id": payload.machine_id,
                "machine_name": machine.name,
                "cert_path": payload.cert_path
            }),
        )?;
        payload
    };

    let task_id_for_worker = task_id.clone();
    let payload_for_worker = payload.clone();
    let app_for_worker = app_handle.clone();
    thread::spawn(move || {
        if let Err(err) =
            run_kes_push_worker(&app_for_worker, &task_id_for_worker, &payload_for_worker)
        {
            let _ = mark_kes_task_failed_if_needed(
                &app_for_worker,
                &task_id_for_worker,
                &err.to_string(),
            );
        }
    });

    Ok(task_id)
}

#[tauri::command]
pub async fn kes_rotation_status(
    task_id: String,
    db: State<'_, DbState>,
) -> Result<DeployTaskStatus, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    kes_rotation_status_with_conn(&conn, task_id.as_str())
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
        let path = std::env::temp_dir().join(format!("ouro-ops-kes-{unique}.sqlite"));
        open_and_migrate(&path).expect("open migrated db")
    }

    fn create_bp_machine(conn: &Connection) -> i64 {
        let pool_id =
            pool_insert(conn, "OURO", "mainnet", Some(0.02), Some(340000000)).expect("pool");
        machine_insert(
            conn,
            pool_id,
            "bp-1",
            "10.0.0.2",
            22,
            "root",
            "bp",
            Some("SHA256:bp"),
        )
        .expect("bp machine")
    }

    #[test]
    fn tc_kes_001_status_severity_uses_remaining_days_thresholds() {
        assert_eq!(severity_for_remaining_days(Some(20)), "healthy");
        assert_eq!(severity_for_remaining_days(Some(5)), "warning");
        assert_eq!(severity_for_remaining_days(Some(2)), "critical");
        assert_eq!(severity_for_remaining_days(None), "warning");
    }

    #[test]
    fn tc_kes_002_generate_playbook_path_resolves() {
        let path = kes_generate_playbook_path().expect("playbook path");
        assert!(path.ends_with("ansible/playbooks/kes-generate.yml"));
    }

    #[test]
    fn tc_kes_003_import_cert_requires_operational_certificate_type() {
        let dir = std::env::temp_dir().join(format!("ouro-kes-cert-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir");
        let invalid = dir.join("node.cert");
        fs::write(&invalid, r#"{"type":"NotACert"}"#).expect("write cert");
        let err = validate_operational_cert(invalid.as_path()).expect_err("invalid cert");
        assert!(matches!(err, AppError::InvalidKesCert));
    }

    #[test]
    fn tc_kes_004_status_all_reads_bp_rows_from_kes_state() {
        let conn = new_db();
        let machine_id = create_bp_machine(&conn);
        conn.execute(
            "INSERT INTO kes_state (machine_id, kes_period_current, kes_period_max, op_cert_counter, expiry_date)
             VALUES (?1, 10, 62, 3, datetime('now', '+5 day'))",
            params![machine_id],
        )
        .expect("insert kes_state");
        let statuses = read_kes_statuses(&conn).expect("kes statuses");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].machine_id, machine_id);
        assert_eq!(statuses[0].severity, "warning");
        assert_eq!(statuses[0].op_cert_counter, Some(3));
    }

    #[test]
    fn tc_kes_005_rotation_status_roundtrip() {
        let conn = new_db();
        let machine_id = create_bp_machine(&conn);
        let task_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload) VALUES (?1, 'kes_rotation', 'running', ?2)",
            params![
                task_id,
                json!({"machine_id": machine_id, "cert_path": "/tmp/node.cert"}).to_string()
            ],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status) VALUES (?1, ?2, 'running')",
            params![task_id, machine_id],
        )
        .expect("insert task_machine");
        let status = kes_rotation_status_with_conn(&conn, task_id.as_str()).expect("status");
        assert_eq!(status.task_type, "kes_rotation");
        assert_eq!(status.status, "running");
        assert_eq!(status.machine_statuses.len(), 1);
        assert_eq!(status.machine_statuses[0].machine_id, machine_id);
    }
}
