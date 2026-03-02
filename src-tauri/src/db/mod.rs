mod repo;
mod schema;

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use crate::error::AppError;

const MIGRATIONS: &[&str] = &[schema::MIGRATION_001];

/// 执行迁移：user_version 递增，仅执行未执行的迁移
pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    let current: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i32;
        if version > current {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", version)?;
        }
    }
    Ok(())
}

/// 打开应用数据目录下的 SQLite 连接并执行迁移
pub fn open_and_migrate(db_path: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open(db_path)?;
    run_migrations(&conn)?;
    Ok(conn)
}

pub struct DbState(pub Mutex<Connection>);

pub use repo::*;
