//! Sidecar 进程管理：启动 Python runner_bridge，JSON-RPC 请求/响应，事件转发

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use tauri::Emitter;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::error::AppError;

/// Sidecar 子进程 + 请求/响应通道
pub struct SidecarRunner {
    child_stdin: Option<ChildStdin>,
    tx_response: Receiver<(String, Value)>,
}

impl SidecarRunner {
    /// 发送 JSON-RPC 请求，等待同 id 的 result 或 error
    pub fn request(&mut self, id: &str, method: &str, params: Value) -> Result<Value, AppError> {
        let req = serde_json::json!({
            "id": id,
            "method": method,
            "params": params
        });
        let line = serde_json::to_string(&req).map_err(|e| AppError::Internal(e.to_string()))?;
        let stdin = self
            .child_stdin
            .as_mut()
            .ok_or(AppError::SidecarCrash)?;
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;

        // 等待匹配 id 的响应（5s 超时）
        loop {
            let (rid, val) = self.tx_response.recv_timeout(Duration::from_secs(5)).map_err(|_| {
                AppError::Internal("sidecar response timeout".into())
            })?;
            if rid == id {
                if let Some(err) = val.get("error") {
                    let msg = err.get("message").and_then(Value::as_str).unwrap_or("unknown");
                    return Err(AppError::PlaybookFailed(msg.to_string()));
                }
                return Ok(val.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    pub fn ping(&mut self) -> Result<(), AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.request(&id, "ping", Value::Object(serde_json::Map::new()))?;
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        let _ = self.request(&id, "shutdown", Value::Object(serde_json::Map::new()));
        Ok(())
    }
}

/// 全局 Sidecar 状态：子进程 + 通道
pub struct SidecarState {
    pub runner: Mutex<Option<SidecarRunner>>,
    _child: Mutex<Option<Child>>,
}

fn sidecar_script_path() -> Result<PathBuf, AppError> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| AppError::Internal("CARGO_MANIFEST_DIR not set".into()))?;
    let path = PathBuf::from(manifest_dir)
        .parent()
        .ok_or_else(|| AppError::Internal("no parent dir".into()))?
        .join("sidecar")
        .join("src")
        .join("runner_bridge.py");
    if path.exists() {
        Ok(path)
    } else {
        Err(AppError::Internal(format!("sidecar script not found: {}", path.display())))
    }
}

/// 启动 Sidecar 进程，返回 State（需传入 app_handle 用于事件发射）
pub fn spawn_sidecar(app_handle: tauri::AppHandle) -> Result<SidecarState, AppError> {
    let script = sidecar_script_path()?;
    let mut child = Command::new("python3")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Internal(format!("failed to spawn sidecar: {}", e)))?;

    let stdin = child.stdin.take().ok_or(AppError::SidecarCrash)?;
    let stdout = child.stdout.take().ok_or(AppError::SidecarCrash)?;

    let (tx_response, rx_response) = std::sync::mpsc::channel();

    thread::spawn(move || {
        read_sidecar_stdout(stdout, app_handle, tx_response);
    });

    let runner = SidecarRunner {
        child_stdin: Some(stdin),
        tx_response: rx_response,
    };

    let state = SidecarState {
        runner: Mutex::new(Some(runner)),
        _child: Mutex::new(Some(child)),
    };

    Ok(state)
}

fn read_sidecar_stdout(
    stdout: ChildStdout,
    app_handle: tauri::AppHandle,
    tx_response: Sender<(String, Value)>,
) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let obj: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(id) = obj.get("id").and_then(Value::as_str) {
            if obj.get("result").is_some() || obj.get("error").is_some() {
                let _ = tx_response.send((id.to_string(), obj.clone()));
            }
        }
        if let Some(event) = obj.get("event").and_then(Value::as_str) {
            let run_id = obj.get("id").and_then(Value::as_str).map(String::from);
            let data = obj.get("data").cloned().unwrap_or(Value::Null);
            let payload = serde_json::json!({ "event": event, "id": run_id, "data": data });
            if event == "runner_on_ok" || event == "runner_on_failed" {
                let _ = app_handle.emit("task:progress", payload.clone());
            }
            if event == "playbook_complete" {
                let status = data.get("status").and_then(Value::as_str).unwrap_or("unknown");
                if status == "successful" {
                    let _ = app_handle.emit("task:completed", payload.clone());
                } else {
                    let _ = app_handle.emit("task:failed", payload.clone());
                }
                if let Some(ref id) = run_id {
                    let _ = tx_response.send((id.clone(), obj));
                }
            }
        }
    }
}

/// 调用 Sidecar run_playbook，并向前端发射 task:progress / task:log / task:completed|task:failed
pub fn run_playbook(
    state: &SidecarState,
    _app_handle: &tauri::AppHandle,
    task_id: &str,
    playbook: &str,
    inventory: Value,
    extra_vars: Value,
) -> Result<(), AppError> {
    let mut runner = state.runner.lock().map_err(|_| AppError::Internal("lock poisoned".into()))?;
    let runner = runner.as_mut().ok_or(AppError::SidecarCrash)?;
    let params = serde_json::json!({
        "run_id": task_id,
        "playbook": playbook,
        "inventory": inventory,
        "extra_vars": extra_vars
    });
    runner.request(task_id, "run_playbook", params)?;
    Ok(())
}
