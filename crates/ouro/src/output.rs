use serde::Serialize;
use serde_json::{json, Value};

use crate::Result;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub pass: bool,
    pub severity: String,
    pub exit_class: u8,
    pub rollback_safe: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolOutput {
    pub tool: String,
    pub machine: Option<String>,
    pub status: String,
    pub changed: bool,
    pub checks: Vec<Check>,
    pub duration_s: f64,
    pub audit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolError {
    pub code: String,
    pub detail: String,
    pub hint: String,
}

impl ToolOutput {
    pub fn ok(tool: impl Into<String>, changed: bool) -> Self {
        Self {
            tool: tool.into(),
            machine: None,
            status: "ok".to_string(),
            changed,
            checks: Vec::new(),
            duration_s: 0.0,
            audit_id: None,
            data: None,
            error: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn failure(
        tool: impl Into<String>,
        code: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            tool: tool.into(),
            machine: None,
            status: "error".to_string(),
            changed: false,
            checks: Vec::new(),
            duration_s: 0.0,
            audit_id: None,
            data: None,
            error: Some(ToolError {
                code: code.into(),
                detail: detail.into(),
                hint: "inspect the failed precondition and rerun through ouro".to_string(),
            }),
        }
    }
}

/// Emit a tool result. MACHINE CONTRACT: single-line JSON on stdout whenever the output is
/// captured (piped, redirected, or over SSH dispatch) — this is what the agent, the dispatch
/// layer, and the tests parse. Only when stdout is an interactive TTY (and `--json`/`OURO_JSON`
/// is not set) is it rendered as a human-readable summary instead. The JSON contract is never
/// changed for a non-TTY consumer.
pub fn print_json(output: &ToolOutput) -> Result<()> {
    let value = serde_json::to_value(output)?;
    emit_value(&value)
}

/// Forward a child L2 script's single-line-JSON stdout (from `tool run`). Same rule: render human
/// for an interactive TTY, otherwise pass the JSON through verbatim (byte-for-byte, so a captured
/// dispatch result is unchanged). If the bytes are not the expected JSON shape, pass them through.
pub fn forward_tool_stdout(raw: &[u8]) -> Result<()> {
    if want_json() {
        use std::io::Write;
        std::io::stdout().write_all(raw)?;
        return Ok(());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(v) if v.is_object() && v.get("tool").is_some() => {
            print!("{}", render_human(&v));
            Ok(())
        }
        _ => {
            use std::io::Write;
            std::io::stdout().write_all(raw)?;
            Ok(())
        }
    }
}

fn emit_value(value: &Value) -> Result<()> {
    if want_json() {
        println!("{}", serde_json::to_string(value)?);
    } else {
        print!("{}", render_human(value));
    }
    Ok(())
}

/// JSON when captured or explicitly requested; human only for an interactive terminal.
fn want_json() -> bool {
    force_json(
        std::env::args().any(|a| a == "--json"),
        std::env::var_os("OURO_JSON").is_some(),
        std::io::IsTerminal::is_terminal(&std::io::stdout()),
    )
}

/// Pure decision (testable): emit JSON unless a real TTY is present and JSON was not forced.
fn force_json(arg_json: bool, env_json: bool, is_tty: bool) -> bool {
    arg_json || env_json || !is_tty
}

/// Render a ToolOutput-shaped JSON value as a compact, readable summary (no ANSI — 克制/极简).
fn render_human(v: &Value) -> String {
    let get_str = |k: &str| v.get(k).and_then(Value::as_str);
    let status = get_str("status").unwrap_or("");
    let mark = match status {
        "ok" => "✓",
        "error" => "✗",
        _ => "•",
    };
    let mut s = String::new();
    let mut head = format!("{mark} {}", get_str("tool").unwrap_or("ouro"));
    if !status.is_empty() {
        head.push_str(&format!("  {status}"));
    }
    if let Some(m) = get_str("machine") {
        head.push_str(&format!("  [{m}]"));
    }
    if v.get("changed").and_then(Value::as_bool) == Some(true) {
        head.push_str("  (changed)");
    }
    s.push_str(&head);
    s.push('\n');

    if let Some(checks) = v.get("checks").and_then(Value::as_array) {
        for c in checks {
            let cm = if c.get("pass").and_then(Value::as_bool) == Some(true) { "✓" } else { "✗" };
            let name = c.get("name").and_then(Value::as_str).unwrap_or("");
            let detail = c.get("detail").and_then(Value::as_str).unwrap_or("");
            s.push_str(&format!("  {cm} {name}"));
            if !detail.is_empty() {
                s.push_str(&format!(": {detail}"));
            }
            s.push('\n');
        }
    }
    if let Some(data) = v.get("data") {
        if !data.is_null() {
            render_value_into(&mut s, data, 1);
        }
    }
    if let Some(err) = v.get("error") {
        if !err.is_null() {
            let code = err.get("code").and_then(Value::as_str).unwrap_or("");
            let detail = err.get("detail").and_then(Value::as_str).unwrap_or("");
            s.push_str(&format!("  error: {code} — {detail}\n"));
            if let Some(h) = err.get("hint").and_then(Value::as_str) {
                if !h.is_empty() {
                    s.push_str(&format!("  hint: {h}\n"));
                }
            }
        }
    }
    if let Some(a) = get_str("audit_id") {
        s.push_str(&format!("  audit: {a}\n"));
    }
    s
}

fn render_value_into(s: &mut String, v: &Value, indent: usize) {
    let pad = "  ".repeat(indent);
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                match val {
                    Value::Object(_) => {
                        s.push_str(&format!("{pad}{k}:\n"));
                        render_value_into(s, val, indent + 1);
                    }
                    Value::Array(arr) if arr.iter().all(|x| !x.is_object() && !x.is_array()) => {
                        let joined = arr.iter().map(scalar_str).collect::<Vec<_>>().join(", ");
                        s.push_str(&format!("{pad}{k}: {joined}\n"));
                    }
                    Value::Array(_) => {
                        s.push_str(&format!("{pad}{k}:\n"));
                        render_value_into(s, val, indent + 1);
                    }
                    _ => s.push_str(&format!("{pad}{k}: {}\n", scalar_str(val))),
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                if item.is_object() || item.is_array() {
                    render_value_into(s, item, indent);
                } else {
                    s.push_str(&format!("{pad}- {}\n", scalar_str(item)));
                }
            }
        }
        _ => s.push_str(&format!("{pad}{}\n", scalar_str(v))),
    }
}

fn scalar_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

pub fn contract_summary() -> Value {
    json!({
        "stdout": "single-line-json when captured (pipe/redirect/ssh); human-readable on an interactive TTY (or force JSON with --json / OURO_JSON=1)",
        "exit_codes": [0, 10, 20, 30, 40],
        "secret_policy": "hashes, paths, counters and metadata only"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_json_rule() {
        // captured (no TTY) → JSON; TTY → human; --json / env force JSON even on a TTY.
        assert!(force_json(false, false, false)); // piped → JSON
        assert!(!force_json(false, false, true)); // interactive → human
        assert!(force_json(true, false, true)); // --json on a TTY → JSON
        assert!(force_json(false, true, true)); // OURO_JSON on a TTY → JSON
    }

    #[test]
    fn human_render_is_readable_not_json() {
        let v = serde_json::json!({
            "tool": "ouro.skill.list", "machine": null, "status": "ok", "changed": false,
            "checks": [], "audit_id": null,
            "data": {"embedded_digest": "3211abcd", "skills": ["deploy", "detect", "kes-rotation"]}
        });
        let out = render_human(&v);
        assert!(out.starts_with("✓ ouro.skill.list  ok"));
        assert!(out.contains("skills: deploy, detect, kes-rotation"));
        assert!(out.contains("embedded_digest: 3211abcd"));
        assert!(!out.contains("{\"")); // not raw JSON
    }

    #[test]
    fn human_render_shows_checks_and_error() {
        let v = serde_json::json!({
            "tool": "deploy/status", "machine": "bp1", "status": "error", "changed": false,
            "checks": [{"name": "bp1.tip_block_positive", "pass": true, "detail": "block=6"},
                       {"name": "bp1.slot_advancing", "pass": false, "detail": "slot 8->8"}],
            "error": {"code": "exit_20", "detail": "node status checks failed", "hint": "inspect failed checks"}
        });
        let out = render_human(&v);
        assert!(out.contains("✗ deploy/status  error  [bp1]"));
        assert!(out.contains("✓ bp1.tip_block_positive: block=6"));
        assert!(out.contains("✗ bp1.slot_advancing: slot 8->8"));
        assert!(out.contains("error: exit_20 — node status checks failed"));
        assert!(out.contains("hint: inspect failed checks"));
    }
}
