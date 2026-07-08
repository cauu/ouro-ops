use serde::Serialize;
use std::path::Path;

use crate::Result;

#[derive(Debug, Clone, Serialize)]
pub struct LegacyMigrationReport {
    pub source: String,
    pub migrated_tables: Vec<String>,
    pub skipped_tables: Vec<String>,
}

pub fn inspect_legacy_db(path: &Path) -> Result<LegacyMigrationReport> {
    let conn = rusqlite::Connection::open(path)?;
    let mut stmt = conn.prepare(
        "select name from sqlite_master where type = 'table' and name not like 'sqlite_%' order by name",
    )?;
    let tables = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let migrated_tables = tables
        .iter()
        .filter(|name| name.as_str() == "audit_events" || name.as_str() == "tasks")
        .cloned()
        .collect();
    let skipped_tables = tables
        .into_iter()
        .filter(|name| name != "audit_events" && name != "tasks")
        .collect();
    Ok(LegacyMigrationReport {
        source: path.display().to_string(),
        migrated_tables,
        skipped_tables,
    })
}
