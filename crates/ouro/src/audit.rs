use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

use crate::Result;

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    /// Monotonic insertion sequence (sqlite rowid). Robust ordering that never collides,
    /// unlike `created_at` timestamps — the T3 failure-discipline invariant relies on it.
    #[serde(default)]
    pub seq: i64,
    pub id: String,
    pub invocation_id: String,
    pub event: String,
    pub tool: String,
    pub machine: Option<String>,
    /// Structured exit class for terminal events (0/10/20/30/40); None for `start`. Lets the
    /// T3 invariant asserter key off exit 30/40 without parsing `detail` strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_class: Option<i64>,
    pub detail: String,
    pub created_at: DateTime<Utc>,
}

pub struct AuditStore {
    conn: Connection,
}

impl AuditStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self> {
        let store = Self {
            conn: Connection::open_in_memory()?,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn append(&self, event: &AuditEvent) -> Result<()> {
        self.conn.execute(
            "insert into audit_events (id, invocation_id, event, tool, machine, exit_class, detail, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.id,
                event.invocation_id,
                event.event,
                event.tool,
                event.machine,
                event.exit_class,
                event.detail,
                event.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn begin_invocation(&self, tool: &str, machine: Option<&str>) -> Result<String> {
        let invocation_id = Uuid::new_v4().to_string();
        self.append(&AuditEvent {
            seq: 0,
            id: Uuid::new_v4().to_string(),
            invocation_id: invocation_id.clone(),
            event: "start".to_string(),
            tool: tool.to_string(),
            machine: machine.map(str::to_string),
            exit_class: None,
            detail: "invocation started".to_string(),
            created_at: Utc::now(),
        })?;
        Ok(invocation_id)
    }

    /// Record a terminal event (`finish` or `crash`) for an existing invocation, carrying the
    /// structured exit class so the audit trail distinguishes success from failure precisely.
    pub fn record_terminal(
        &self,
        invocation_id: &str,
        tool: &str,
        machine: Option<&str>,
        event: &str,
        exit_class: Option<i64>,
        detail: &str,
    ) -> Result<()> {
        self.append(&AuditEvent {
            seq: 0,
            id: Uuid::new_v4().to_string(),
            invocation_id: invocation_id.to_string(),
            event: event.to_string(),
            tool: tool.to_string(),
            machine: machine.map(str::to_string),
            exit_class,
            detail: detail.to_string(),
            created_at: Utc::now(),
        })
    }

    pub fn finish_invocation(&self, invocation_id: &str, tool: &str) -> Result<()> {
        self.record_terminal(
            invocation_id,
            tool,
            None,
            "finish",
            Some(0),
            "invocation finished",
        )
    }

    pub fn record_crash(&self, invocation_id: &str, tool: &str, detail: &str) -> Result<()> {
        self.record_terminal(invocation_id, tool, None, "crash", Some(40), detail)
    }

    /// Whether a `start` event exists for the given invocation id. Used by the
    /// audit-context gate to reject fabricated/forged invocation ids.
    pub fn invocation_has_start(&self, invocation_id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "select count(*) from audit_events where invocation_id = ?1 and event = 'start'",
            params![invocation_id],
            |row| row.get::<_, i64>(0),
        )? > 0)
    }

    pub fn count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("select count(*) from audit_events", [], |row| row.get(0))?)
    }

    pub fn list(&self, limit: u32) -> Result<Vec<AuditEvent>> {
        // Order by rowid (monotonic insertion order) — robust against timestamp collisions.
        let mut stmt = self.conn.prepare(
            "select rowid, id, invocation_id, event, tool, machine, exit_class, detail, created_at
             from audit_events
             order by rowid desc
             limit ?1",
        )?;
        let events = stmt
            .query_map([limit], |row| {
                let created_at: String = row.get(8)?;
                Ok(AuditEvent {
                    seq: row.get(0)?,
                    id: row.get(1)?,
                    invocation_id: row.get(2)?,
                    event: row.get(3)?,
                    tool: row.get(4)?,
                    machine: row.get(5)?,
                    exit_class: row.get(6)?,
                    detail: row.get(7)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                        .map(|value| value.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(events)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "create table if not exists audit_events (
                id text primary key,
                invocation_id text not null,
                event text not null,
                tool text not null,
                machine text,
                exit_class integer,
                detail text not null,
                created_at text not null
            );",
        )?;
        // Backward-compatible: add exit_class to a pre-existing table (ignore if present).
        let _ = self
            .conn
            .execute("alter table audit_events add column exit_class integer", []);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AuditStore;

    #[test]
    fn append_only_audit_records_invocation() {
        let store = AuditStore::in_memory().unwrap();
        let invocation = store
            .begin_invocation("fixture/read", Some("relay1"))
            .unwrap();
        assert!(!invocation.is_empty());
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn records_finish_and_crash_terminal_events() {
        let store = AuditStore::in_memory().unwrap();
        let ok = store
            .begin_invocation("fixture/write", Some("bp1"))
            .unwrap();
        store.finish_invocation(&ok, "fixture/write").unwrap();
        let crashed = store
            .begin_invocation("fixture/crash", Some("bp1"))
            .unwrap();
        store
            .record_crash(&crashed, "fixture/crash", "child exited with signal")
            .unwrap();
        assert!(store.invocation_has_start(&ok).unwrap());
        assert!(store.invocation_has_start(&crashed).unwrap());
        assert!(!store.invocation_has_start("fabricated-id").unwrap());
        // start + finish + start + crash
        assert_eq!(store.count().unwrap(), 4);
    }
}
