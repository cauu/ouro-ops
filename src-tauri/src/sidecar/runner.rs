//! Sidecar 进程管理：启动 Python runner_bridge，JSON-RPC 请求/响应，事件转发

use serde_json::Value;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::Emitter;
use tauri::Manager;

#[cfg(test)]
use std::sync::mpsc;

use crate::error::AppError;

/// Sidecar 子进程 + 请求/响应通道
pub struct SidecarRunner {
    child_stdin: Option<ChildStdin>,
    tx_response: Receiver<(String, Value)>,
}

impl SidecarRunner {
    /// 发送 JSON-RPC 请求，等待同 id 的 result 或 error
    /// timeout = None 表示不超时
    pub fn request(
        &mut self,
        id: &str,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, AppError> {
        let req = serde_json::json!({
            "id": id,
            "method": method,
            "params": params
        });
        let line = serde_json::to_string(&req).map_err(|e| AppError::Internal(e.to_string()))?;
        let stdin = self.child_stdin.as_mut().ok_or(AppError::SidecarCrash)?;
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;

        // 等待匹配 id 的响应（可选超时）
        loop {
            let (rid, val) = match timeout {
                Some(t) => self
                    .tx_response
                    .recv_timeout(t)
                    .map_err(|_| AppError::Internal("sidecar response timeout".into()))?,
                None => self
                    .tx_response
                    .recv()
                    .map_err(|_| AppError::Internal("sidecar response channel closed".into()))?,
            };
            if rid == id {
                if let Some(err) = val.get("error") {
                    let msg = err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    return Err(AppError::PlaybookFailed(msg.to_string()));
                }
                return Ok(val.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    pub fn ping(&mut self) -> Result<(), AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.request(
            &id,
            "ping",
            Value::Object(serde_json::Map::new()),
            Some(Duration::from_secs(5)),
        )?;
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        let _ = self.request(
            &id,
            "shutdown",
            Value::Object(serde_json::Map::new()),
            Some(Duration::from_secs(5)),
        );
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
        Err(AppError::Internal(format!(
            "sidecar script not found: {}",
            path.display()
        )))
    }
}

/// 启动 Sidecar 进程，返回 State（需传入 app_handle 用于事件发射）
pub fn spawn_sidecar(app_handle: tauri::AppHandle) -> Result<SidecarState, AppError> {
    let script = sidecar_script_path()?;
    let mut child = Command::new("python3")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Internal(format!("failed to spawn sidecar: {}", e)))?;

    let stdin = child.stdin.take().ok_or(AppError::SidecarCrash)?;
    let stdout = child.stdout.take().ok_or(AppError::SidecarCrash)?;
    let stderr = child.stderr.take().ok_or(AppError::SidecarCrash)?;

    let (tx_response, rx_response) = std::sync::mpsc::channel();

    let app_handle_for_stdout = app_handle.clone();
    thread::spawn(move || {
        read_sidecar_stdout(stdout, app_handle_for_stdout, tx_response);
    });

    // best-effort stderr logging
    let stderr_log_path = app_handle
        .path()
        .app_data_dir()
        .ok()
        .map(|p| p.join("sidecar.stderr.log"));
    thread::spawn(move || {
        read_sidecar_stderr(stderr, stderr_log_path);
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
        handle_sidecar_object(
            obj,
            |event, payload| {
                let _ = app_handle.emit(event, payload);
            },
            &tx_response,
        );
    }
}

fn handle_sidecar_object<F>(obj: Value, mut emit: F, tx_response: &Sender<(String, Value)>)
where
    F: FnMut(&str, Value),
{
    if let Some(id) = obj.get("id").and_then(Value::as_str) {
        if obj.get("result").is_some() || obj.get("error").is_some() {
            let _ = tx_response.send((id.to_string(), obj.clone()));
            return;
        }
    }

    let Some(event) = obj.get("event").and_then(Value::as_str) else {
        return;
    };
    let run_id = obj.get("id").and_then(Value::as_str).map(String::from);
    let data = obj.get("data").cloned().unwrap_or(Value::Null);

    if let Some(payload) = build_progress_payload(event, run_id.as_deref(), &data) {
        emit("task:progress", payload);
    }
    if let Some(payload) = build_log_payload(event, run_id.as_deref(), &data) {
        emit("task:log", payload);
    }
    if event == "playbook_complete" {
        if let Some(id) = run_id {
            let status = data
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if status == "successful" {
                emit(
                    "task:completed",
                    serde_json::json!({
                        "task_id": id,
                        "rc": data.get("rc").cloned().unwrap_or(Value::Null),
                        "status": "successful"
                    }),
                );
                let _ = tx_response.send((
                    id.clone(),
                    serde_json::json!({
                        "id": id,
                        "result": data
                    }),
                ));
            } else {
                let message = extract_playbook_error(&data);
                emit(
                    "task:failed",
                    serde_json::json!({
                        "task_id": id,
                        "error": message,
                        "status": status
                    }),
                );
                let _ = tx_response.send((
                    id.clone(),
                    serde_json::json!({
                        "id": id,
                        "error": { "message": message }
                    }),
                ));
            }
        }
    }
}

fn build_progress_payload(event: &str, run_id: Option<&str>, data: &Value) -> Option<Value> {
    let task_id = run_id?;
    if !matches!(
        event,
        "runner_on_ok" | "runner_on_failed" | "runner_on_skipped" | "runner_on_changed"
    ) {
        return None;
    }
    let status = match event {
        "runner_on_failed" => "failed",
        "runner_on_skipped" => "skipped",
        "runner_on_changed" => "changed",
        _ => {
            if data
                .get("result")
                .and_then(Value::as_str)
                .map(|v| v == "changed")
                .unwrap_or(false)
            {
                "changed"
            } else {
                "ok"
            }
        }
    };
    let machine_name = data
        .get("host")
        .and_then(Value::as_str)
        .or_else(|| {
            data.get("event_data")
                .and_then(|v| v.get("host"))
                .and_then(Value::as_str)
        })
        .unwrap_or("unknown");
    let ansible_task_name = data
        .get("task")
        .and_then(Value::as_str)
        .or_else(|| {
            data.get("event_data")
                .and_then(|v| v.get("task"))
                .and_then(Value::as_str)
        })
        .unwrap_or("unknown");
    let progress_percent = data
        .get("progress_percent")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);

    Some(serde_json::json!({
        "task_id": task_id,
        "machine_name": machine_name,
        "ansible_task_name": ansible_task_name,
        "status": status,
        "progress_percent": progress_percent
    }))
}

fn build_log_payload(event: &str, run_id: Option<&str>, data: &Value) -> Option<Value> {
    let task_id = run_id?;
    if event == "log" {
        let stream = data
            .get("stream")
            .and_then(Value::as_str)
            .unwrap_or("stdout");
        let line = data
            .get("line")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if line.is_empty() {
            return None;
        }
        let timestamp = data
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        return Some(serde_json::json!({
            "task_id": task_id,
            "stream": stream,
            "line": line,
            "timestamp": timestamp
        }));
    }

    if event == "runner_on_failed" {
        let line = data
            .get("msg")
            .and_then(Value::as_str)
            .or_else(|| data.get("stderr").and_then(Value::as_str))
            .unwrap_or("");
        if line.is_empty() {
            return None;
        }
        return Some(serde_json::json!({
            "task_id": task_id,
            "stream": "stderr",
            "line": line,
            "timestamp": ""
        }));
    }

    if event == "runner_on_ok" || event == "runner_on_changed" {
        let line = data.get("stdout").and_then(Value::as_str).unwrap_or("");
        if line.is_empty() {
            return None;
        }
        return Some(serde_json::json!({
            "task_id": task_id,
            "stream": "stdout",
            "line": line,
            "timestamp": ""
        }));
    }

    None
}

fn extract_playbook_error(data: &Value) -> String {
    if let Some(msg) = data.get("message").and_then(Value::as_str) {
        return msg.to_string();
    }
    let host = data
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("unknown-host");
    let task = data
        .get("task")
        .and_then(Value::as_str)
        .unwrap_or("unknown-task");
    let msg = data
        .get("stderr")
        .and_then(Value::as_str)
        .or_else(|| data.get("msg").and_then(Value::as_str))
        .unwrap_or("playbook failed");
    format!("{host}/{task}: {msg}")
}

fn read_sidecar_stderr(stderr: ChildStderr, log_path: Option<PathBuf>) {
    let mut log_file =
        log_path.and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok());
    let reader = BufReader::new(stderr);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if let Some(ref mut f) = log_file {
            let _ = writeln!(f, "{}", line);
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
    let mut runner = state
        .runner
        .lock()
        .map_err(|_| AppError::Internal("lock poisoned".into()))?;
    let runner = runner.as_mut().ok_or(AppError::SidecarCrash)?;
    let params = serde_json::json!({
        "run_id": task_id,
        "playbook": playbook,
        "inventory": inventory,
        "extra_vars": extra_vars
    });
    // 长任务不设超时，依赖 playbook_complete 事件完成
    runner.request(task_id, "run_playbook", params, None)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Child;
    use std::time::{Duration, Instant};

    fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Ok(Some(_)) = child.try_wait() {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn spawn_test_runner() -> Result<(SidecarRunner, Child), AppError> {
        let script = sidecar_script_path()?;
        let mut child = Command::new("python3")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::Internal(format!("spawn test sidecar failed: {e}")))?;
        let stdin = child.stdin.take().ok_or(AppError::SidecarCrash)?;
        let stdout = child.stdout.take().ok_or(AppError::SidecarCrash)?;
        let (tx_response, rx_response) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(obj) = serde_json::from_str::<Value>(trimmed) {
                    handle_sidecar_object(obj, |_ev, _payload| {}, &tx_response);
                }
            }
        });
        Ok((
            SidecarRunner {
                child_stdin: Some(stdin),
                tx_response: rx_response,
            },
            child,
        ))
    }

    #[test]
    fn tc_evt_payloads_and_error_mapping() {
        let (tx_response, rx_response) = mpsc::channel();
        let mut emitted: Vec<(String, Value)> = Vec::new();

        handle_sidecar_object(
            serde_json::json!({
                "event": "runner_on_ok",
                "id": "task-1",
                "data": {"host": "relay-1", "task": "Install Chrony", "result": "changed", "progress_percent": 40.0}
            }),
            |ev, payload| emitted.push((ev.to_string(), payload)),
            &tx_response,
        );
        handle_sidecar_object(
            serde_json::json!({
                "event": "log",
                "id": "task-1",
                "data": {"stream": "stdout", "line": "ok: [relay-1]", "timestamp": "2026-03-03T00:00:00Z"}
            }),
            |ev, payload| emitted.push((ev.to_string(), payload)),
            &tx_response,
        );
        handle_sidecar_object(
            serde_json::json!({
                "event": "playbook_complete",
                "id": "task-1",
                "data": {"status": "failed", "rc": 2, "message": "relay-1/Install Chrony: docker pull failed"}
            }),
            |ev, payload| emitted.push((ev.to_string(), payload)),
            &tx_response,
        );

        let progress = emitted
            .iter()
            .find(|(ev, _)| ev == "task:progress")
            .expect("progress emitted")
            .1
            .clone();
        assert_eq!(progress["task_id"], "task-1");
        assert_eq!(progress["machine_name"], "relay-1");
        assert_eq!(progress["ansible_task_name"], "Install Chrony");
        assert_eq!(progress["status"], "changed");

        let log = emitted
            .iter()
            .find(|(ev, _)| ev == "task:log")
            .expect("log emitted")
            .1
            .clone();
        assert_eq!(log["task_id"], "task-1");
        assert_eq!(log["stream"], "stdout");
        assert_eq!(log["line"], "ok: [relay-1]");

        let failed = emitted
            .iter()
            .find(|(ev, _)| ev == "task:failed")
            .expect("failed emitted")
            .1
            .clone();
        assert_eq!(failed["task_id"], "task-1");
        assert!(failed["error"]
            .as_str()
            .unwrap_or_default()
            .contains("docker pull failed"));

        let (id, rpc) = rx_response
            .recv_timeout(Duration::from_secs(2))
            .expect("response forwarded");
        assert_eq!(id, "task-1");
        assert_eq!(
            rpc["error"]["message"],
            "relay-1/Install Chrony: docker pull failed"
        );
    }

    #[test]
    fn tc_sc_ping_run_and_shutdown() {
        let (mut runner, mut child) = spawn_test_runner().expect("spawn runner");
        runner.ping().expect("ping ok");

        let run_id = uuid::Uuid::new_v4().to_string();
        let result = runner
            .request(
                &run_id,
                "run_playbook",
                serde_json::json!({
                    "run_id": run_id.clone(),
                    "playbook": "deploy.yml",
                    "inventory": {"_meta": {"hostvars": {}}},
                    "extra_vars": {}
                }),
                Some(Duration::from_secs(20)),
            )
            .expect("run playbook ok");
        assert_eq!(result["status"], "successful");

        runner.shutdown().expect("shutdown ok");
        assert!(
            wait_for_exit(&mut child, Duration::from_secs(5)),
            "sidecar should exit within 5s"
        );
    }

    #[test]
    fn tc_err_playbook_failed_has_readable_message() {
        let (mut runner, mut child) = spawn_test_runner().expect("spawn runner");
        let run_id = uuid::Uuid::new_v4().to_string();
        let err = runner
            .request(
                &run_id,
                "run_playbook",
                serde_json::json!({
                    "run_id": run_id.clone(),
                    "playbook": "deploy.yml",
                    "inventory": {"_meta": {"hostvars": {}}},
                    "extra_vars": {"_mock_fail": true}
                }),
                Some(Duration::from_secs(20)),
            )
            .expect_err("should fail");

        match err {
            AppError::PlaybookFailed(msg) => {
                assert!(msg.contains("local/mock"));
                assert!(msg.contains("mock failure"));
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let _ = runner.shutdown();
        let _ = wait_for_exit(&mut child, Duration::from_secs(5));
    }
}
