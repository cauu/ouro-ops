use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tauri::State;

use crate::db::{audit_log_insert, machine_get, pool_get_single, DbState, MachineRow};
use crate::error::AppError;

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

#[derive(Debug, Clone)]
struct TaskRow {
    id: String,
    task_type: String,
    status: String,
    payload: Option<String>,
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

fn get_task_row(conn: &Connection, task_id: &str) -> Result<Option<TaskRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_type, status, payload
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
        }))
    } else {
        Ok(None)
    }
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

fn upgrade_payload_with_phase(payload: &Value, phase: &str) -> Value {
    let mut next = payload.clone();
    next["phase"] = Value::String(phase.to_string());
    next["last_confirmed_at"] = Value::String(chrono::Utc::now().to_rfc3339());
    next
}

#[tauri::command]
pub async fn upgrade_start(
    payload: UpgradePayload,
    db: State<'_, DbState>,
) -> Result<String, AppError> {
    let payload = normalize_upgrade_payload(&payload);
    validate_upgrade_payload(&payload)?;

    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
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
    Ok(task_id)
}

#[tauri::command]
pub async fn upgrade_confirm_next(task_id: String, db: State<'_, DbState>) -> Result<(), AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let task = get_task_row(&conn, task_id.as_str())?
        .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
    if task.task_type != "upgrade" {
        return Err(AppError::Internal(format!(
            "task is not upgrade: {task_id}"
        )));
    }
    let payload = task
        .payload
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|e| AppError::Internal(format!("task payload parse failed: {e}")))?
        .unwrap_or(Value::Null);
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
    let next_payload = upgrade_payload_with_phase(&payload, next_phase);
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
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let task = get_task_row(&conn, task_id.as_str())?
        .ok_or_else(|| AppError::Internal(format!("task not found: {task_id}")))?;
    if task.task_type != "upgrade" {
        return Err(AppError::Internal(format!(
            "task is not upgrade: {task_id}"
        )));
    }
    let payload = task
        .payload
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|e| AppError::Internal(format!("task payload parse failed: {e}")))?
        .unwrap_or(Value::Null);
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
        let payload: Value =
            serde_json::from_str(task.payload.as_deref().expect("payload")).expect("payload parse");
        let next = upgrade_payload_with_phase(&payload, "UPGRADING_BP");
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
        let payload: Value =
            serde_json::from_str(task.payload.as_deref().expect("payload")).expect("payload parse");
        assert_eq!(
            payload.get("previous_version").and_then(Value::as_str),
            Some("10.5.3-1")
        );
        assert_eq!(
            payload.get("backup_archive").and_then(Value::as_str),
            Some("/opt/cardano/backup/config-20260308.tar.gz")
        );
    }
}
