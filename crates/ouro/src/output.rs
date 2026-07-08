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

pub fn print_json(output: &ToolOutput) -> Result<()> {
    println!("{}", serde_json::to_string(output)?);
    Ok(())
}

pub fn contract_summary() -> Value {
    json!({
        "stdout": "single-line-json",
        "exit_codes": [0, 10, 20, 30, 40],
        "secret_policy": "hashes, paths, counters and metadata only"
    })
}
