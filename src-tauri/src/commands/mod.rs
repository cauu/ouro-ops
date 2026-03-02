//! Tauri IPC 命令，Phase 1 仅暴露 ping、run_playbook_test、db_version

mod deploy;

use crate::db::{get_user_version, table_exists, DbState};
use crate::error::AppError;
use crate::sidecar::{run_playbook, SidecarState};
use std::sync::Mutex;
use tauri::State;

/// 健康检查：Sidecar ping
#[tauri::command]
pub async fn ping(sidecar: State<'_, Mutex<Option<SidecarState>>>) -> Result<(), AppError> {
    let guard = sidecar.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let state = guard.as_ref().ok_or(AppError::SidecarCrash)?;
    let mut runner = state.runner.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let runner = runner.as_mut().ok_or(AppError::SidecarCrash)?;
    runner.ping()
}

/// 返回 DB user_version 与表是否存在（用于 TC-DB-001）
#[tauri::command]
pub async fn db_version(db: State<'_, DbState>) -> Result<serde_json::Value, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let version = get_user_version(&conn)?;
    let tables = ["pool", "machine", "task", "task_machine", "machine_health", "audit_log", "kes_state"];
    let mut exists = serde_json::Map::new();
    for t in tables {
        exists.insert(t.to_string(), serde_json::json!(table_exists(&conn, t)?));
    }
    Ok(serde_json::json!({ "user_version": version, "tables": exists }))
}

/// 触发一次 run_playbook（mock），用于验证事件流 TC-SC-002 / TC-EVT-*
#[tauri::command]
pub async fn run_playbook_test(
    sidecar: State<'_, Mutex<Option<SidecarState>>>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    let task_id = uuid::Uuid::new_v4().to_string();
    let guard = sidecar.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let state = guard.as_ref().ok_or(AppError::SidecarCrash)?;
    run_playbook(
        state,
        &app_handle,
        &task_id,
        "deploy.yml",
        serde_json::json!({ "_meta": { "hostvars": {} } }),
        serde_json::json!({ "cardano_version": "10.2.1" }),
    )?;
    Ok(task_id)
}
