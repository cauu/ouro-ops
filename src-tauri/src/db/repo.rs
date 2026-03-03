//! 数据访问层，Phase 1 提供占位接口，Phase 2 实现 Pool / Machine CRUD

use rusqlite::Connection;

use crate::error::AppError;

/// Phase 2 实现：插入 Pool，返回 id
pub fn pool_insert(conn: &Connection, ticker: &str, network: &str) -> Result<i64, AppError> {
    conn.execute(
        "INSERT INTO pool (ticker, network) VALUES (?1, ?2)",
        [ticker, network],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Phase 2 实现：获取当前唯一 Pool（MVP 单池）
pub fn pool_get_single(conn: &Connection) -> Result<Option<(i64, String, String)>, AppError> {
    let mut stmt = conn.prepare("SELECT id, ticker, network FROM pool LIMIT 1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        Ok(Some((row.get(0)?, row.get(1)?, row.get(2)?)))
    } else {
        Ok(None)
    }
}

/// Phase 2 实现：Machine 占位，仅用于测试外键 CASCADE
pub fn machine_insert(
    conn: &Connection,
    pool_id: i64,
    name: &str,
    ip: &str,
    role: &str,
) -> Result<i64, AppError> {
    conn.execute(
        "INSERT INTO machine (pool_id, name, ip, role) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![pool_id, name, ip, role],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 业务层级联删除 Machine（数据库无 FK，由应用层维护关联）
pub fn machine_delete_cascade(conn: &Connection, machine_id: i64) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM task_machine WHERE machine_id = ?1",
        rusqlite::params![machine_id],
    )?;
    conn.execute(
        "DELETE FROM machine_health WHERE machine_id = ?1",
        rusqlite::params![machine_id],
    )?;
    conn.execute(
        "DELETE FROM kes_state WHERE machine_id = ?1",
        rusqlite::params![machine_id],
    )?;
    conn.execute(
        "DELETE FROM machine WHERE id = ?1",
        rusqlite::params![machine_id],
    )?;
    Ok(())
}

/// 业务层级联删除 Pool（数据库无 FK，由应用层维护关联）
pub fn pool_delete_cascade(conn: &Connection, pool_id: i64) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM task_machine
         WHERE machine_id IN (SELECT id FROM machine WHERE pool_id = ?1)",
        rusqlite::params![pool_id],
    )?;
    conn.execute(
        "DELETE FROM machine_health
         WHERE machine_id IN (SELECT id FROM machine WHERE pool_id = ?1)",
        rusqlite::params![pool_id],
    )?;
    conn.execute(
        "DELETE FROM kes_state
         WHERE machine_id IN (SELECT id FROM machine WHERE pool_id = ?1)",
        rusqlite::params![pool_id],
    )?;
    conn.execute(
        "DELETE FROM machine WHERE pool_id = ?1",
        rusqlite::params![pool_id],
    )?;
    conn.execute("DELETE FROM pool WHERE id = ?1", rusqlite::params![pool_id])?;
    Ok(())
}

/// 检查表是否存在（用于 TC-DB-001）
pub fn table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

/// 获取当前 user_version（用于 TC-DB-001）
pub fn get_user_version(conn: &Connection) -> Result<i32, AppError> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(AppError::from)
}
