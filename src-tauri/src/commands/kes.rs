use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::db::{audit_log_insert, machine_get, machine_list as repo_machine_list, DbState};
use crate::error::AppError;

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

fn kes_staging_dir(app_handle: &AppHandle, machine_id: i64) -> Result<PathBuf, AppError> {
    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|err| AppError::Internal(format!("app_data_dir error: {err}")))?;
    Ok(app_dir.join("kes").join(machine_id.to_string()))
}

fn severity_for_remaining_days(remaining_days: Option<i64>) -> String {
    match remaining_days {
        Some(days) if days > 10 => "healthy".into(),
        Some(days) if days >= 3 => "warning".into(),
        Some(_) => "critical".into(),
        None => "warning".into(),
    }
}

fn remaining_days_from_expiry(conn: &Connection, expiry_date: Option<&str>) -> Result<Option<i64>, AppError> {
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

fn ensure_bp_machine(conn: &Connection, machine_id: i64) -> Result<crate::db::MachineRow, AppError> {
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

fn run_command_checked(program: &str, args: &[&str]) -> Result<(), AppError> {
    let output = Command::new(program).args(args).output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(AppError::Internal(format!("{program} failed: {stderr}")))
}

fn kes_generate_with_runner<F>(
    staging_dir: &Path,
    counter_value: i64,
    run: F,
) -> Result<KesSignRequest, AppError>
where
    F: Fn(&str, &[&str]) -> Result<(), AppError>,
{
    fs::create_dir_all(staging_dir)?;
    let skey_path = staging_dir.join("kes.skey");
    let vkey_path = staging_dir.join("kes.vkey");
    let cert_path = staging_dir.join("node.cert");
    if skey_path.exists() {
        fs::remove_file(&skey_path)?;
    }
    if vkey_path.exists() {
        fs::remove_file(&vkey_path)?;
    }
    if cert_path.exists() {
        fs::remove_file(&cert_path)?;
    }

    let vkey = vkey_path
        .to_str()
        .ok_or_else(|| AppError::Internal("invalid kes.vkey path".into()))?;
    let skey = skey_path
        .to_str()
        .ok_or_else(|| AppError::Internal("invalid kes.skey path".into()))?;
    run(
        "cardano-cli",
        &[
            "node",
            "key-gen-KES",
            "--verification-key-file",
            vkey,
            "--signing-key-file",
            skey,
        ],
    )?;

    Ok(KesSignRequest {
        machine_id: 0,
        kes_vkey_path: vkey_path.display().to_string(),
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
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    ensure_bp_machine(&conn, machine_id)?;
    let counter_value = current_op_cert_counter(&conn, machine_id)?;
    let staging_dir = kes_staging_dir(&app_handle, machine_id)?;
    let mut sign_request = kes_generate_with_runner(staging_dir.as_path(), counter_value, run_command_checked)?;
    sign_request.machine_id = machine_id;
    audit_log_insert(
        &conn,
        "kes_generate",
        &json!({
            "machine_id": machine_id,
            "kes_vkey_path": sign_request.kes_vkey_path,
            "counter_value": sign_request.counter_value
        }),
    )?;
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
    fn tc_kes_002_generate_returns_sign_request_with_staging_paths() {
        let base = std::env::temp_dir().join(format!("ouro-kes-generate-{}", uuid::Uuid::new_v4()));
        let request = kes_generate_with_runner(base.as_path(), 7, |_, args| {
            let vkey = PathBuf::from(args[3]);
            let skey = PathBuf::from(args[5]);
            fs::create_dir_all(vkey.parent().expect("parent"))?;
            fs::write(&vkey, "vkey")?;
            fs::write(&skey, "skey")?;
            Ok(())
        })
        .expect("generate");
        assert!(Path::new(&request.kes_vkey_path).exists());
        assert_eq!(request.counter_value, 7);
        assert!(request.instructions.contains("counter=7"));
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
}
