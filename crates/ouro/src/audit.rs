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
}
