use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

use crate::Result;

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub id: String,
    pub invocation_id: String,
    pub event: String,
    pub tool: String,
    pub machine: Option<String>,
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
            "insert into audit_events (id, invocation_id, event, tool, machine, detail, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id,
                event.invocation_id,
                event.event,
                event.tool,
                event.machine,
                event.detail,
                event.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn begin_invocation(&self, tool: &str, machine: Option<&str>) -> Result<String> {
        let invocation_id = Uuid::new_v4().to_string();
        self.append(&AuditEvent {
            id: Uuid::new_v4().to_string(),
            invocation_id: invocation_id.clone(),
            event: "start".to_string(),
            tool: tool.to_string(),
            machine: machine.map(str::to_string),
            detail: "invocation started".to_string(),
            created_at: Utc::now(),
        })?;
        Ok(invocation_id)
    }

    /// Record a terminal event (`finish` or `crash`) for an existing invocation so
    /// the audit trail can distinguish success from an aborted/crashed run.
    pub fn record_terminal(
        &self,
        invocation_id: &str,
        tool: &str,
        machine: Option<&str>,
        event: &str,
        detail: &str,
    ) -> Result<()> {
        self.append(&AuditEvent {
            id: Uuid::new_v4().to_string(),
            invocation_id: invocation_id.to_string(),
            event: event.to_string(),
            tool: tool.to_string(),
            machine: machine.map(str::to_string),
            detail: detail.to_string(),
            created_at: Utc::now(),
        })
    }

    pub fn finish_invocation(&self, invocation_id: &str, tool: &str) -> Result<()> {
        self.record_terminal(invocation_id, tool, None, "finish", "invocation finished")
    }

    pub fn record_crash(&self, invocation_id: &str, tool: &str, detail: &str) -> Result<()> {
        self.record_terminal(invocation_id, tool, None, "crash", detail)
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
        let mut stmt = self.conn.prepare(
            "select id, invocation_id, event, tool, machine, detail, created_at
             from audit_events
             order by created_at desc
             limit ?1",
        )?;
        let events = stmt
            .query_map([limit], |row| {
                let created_at: String = row.get(6)?;
                Ok(AuditEvent {
                    id: row.get(0)?,
                    invocation_id: row.get(1)?,
                    event: row.get(2)?,
                    tool: row.get(3)?,
                    machine: row.get(4)?,
                    detail: row.get(5)?,
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
                detail text not null,
                created_at text not null
            );",
        )?;
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
            .begin_invocation("deploy/preflight", Some("relay1"))
            .unwrap();
        assert!(!invocation.is_empty());
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn records_finish_and_crash_terminal_events() {
        let store = AuditStore::in_memory().unwrap();
        let ok = store.begin_invocation("deploy/provision", Some("bp1")).unwrap();
        store.finish_invocation(&ok, "deploy/provision").unwrap();
        let crashed = store.begin_invocation("deploy/sync", Some("bp1")).unwrap();
        store
            .record_crash(&crashed, "deploy/sync", "child exited with signal")
            .unwrap();
        assert!(store.invocation_has_start(&ok).unwrap());
        assert!(store.invocation_has_start(&crashed).unwrap());
        assert!(!store.invocation_has_start("fabricated-id").unwrap());
        // start + finish + start + crash
        assert_eq!(store.count().unwrap(), 4);
    }
}
