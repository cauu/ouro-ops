//! Tauri IPC commands.

pub mod deploy;
pub mod machine;
pub mod pool;

use crate::db::{get_user_version, table_exists, DbState};
use crate::error::AppError;
use crate::sidecar::{run_playbook, SidecarState};
use std::sync::{Arc, Mutex};
use tauri::State;

/// 健康检查：Sidecar ping
#[tauri::command]
pub async fn ping(sidecar: State<'_, Mutex<Option<Arc<SidecarState>>>>) -> Result<(), AppError> {
    let state = {
        let guard = sidecar
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        guard.as_ref().cloned().ok_or(AppError::SidecarCrash)?
    };
    let mut runner = state
        .runner
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    let runner = runner.as_mut().ok_or(AppError::SidecarCrash)?;
    runner.ping()
}

/// 返回 DB user_version 与表是否存在（用于 TC-DB-001）
#[tauri::command]
pub async fn db_version(db: State<'_, DbState>) -> Result<serde_json::Value, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let version = get_user_version(&conn)?;
    let tables = [
        "pool",
        "machine",
        "task",
        "task_machine",
        "machine_health",
        "audit_log",
        "kes_state",
    ];
    let mut exists = serde_json::Map::new();
    for t in tables {
        exists.insert(t.to_string(), serde_json::json!(table_exists(&conn, t)?));
    }
    Ok(serde_json::json!({ "user_version": version, "tables": exists }))
}

/// 触发一次 run_playbook（mock），用于验证事件流 TC-SC-002 / TC-EVT-*
#[tauri::command]
pub async fn run_playbook_test(
    sidecar: State<'_, Mutex<Option<Arc<SidecarState>>>>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    let playbook = std::env::var("CARGO_MANIFEST_DIR")
        .ok()
        .and_then(|manifest_dir| {
            let p = std::path::PathBuf::from(manifest_dir)
                .parent()?
                .join("ansible")
                .join("playbooks")
                .join("deploy.yml");
            Some(p)
        })
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "deploy.yml".to_string());

    let task_id = uuid::Uuid::new_v4().to_string();
    let state = {
        let guard = sidecar
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        guard.as_ref().cloned().ok_or(AppError::SidecarCrash)?
    };
    run_playbook(
        state.as_ref(),
        &app_handle,
        &task_id,
        playbook.as_str(),
        serde_json::json!({ "_meta": { "hostvars": {} } }),
        serde_json::json!({ "cardano_version": "10.2.1" }),
    )?;
    Ok(task_id)
}
