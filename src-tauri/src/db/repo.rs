//! Data access layer.

use rusqlite::{Connection, Row};

use crate::error::AppError;

const DEFAULT_IMAGE_REGISTRY: &str = "ghcr.io/blinklabs-io/cardano-node";

#[derive(Debug, Clone, serde::Serialize)]
pub struct PoolRow {
    pub id: i64,
    pub ticker: String,
    pub network: String,
    pub margin: Option<f64>,
    pub fixed_cost: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MachineRow {
    pub id: i64,
    pub pool_id: i64,
    pub name: String,
    pub ip: String,
    pub ssh_port: i64,
    pub ssh_user: String,
    pub role: String,
    pub network: String,
    pub ssh_key_fingerprint: Option<String>,
    pub os_version: Option<String>,
    pub cardano_version: Option<String>,
    pub image_registry: String,
    pub image_digest: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn map_pool_row(row: &Row<'_>) -> Result<PoolRow, rusqlite::Error> {
    Ok(PoolRow {
        id: row.get(0)?,
        ticker: row.get(1)?,
        network: row.get(2)?,
        margin: row.get(3)?,
        fixed_cost: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn map_machine_row(row: &Row<'_>) -> Result<MachineRow, rusqlite::Error> {
    Ok(MachineRow {
        id: row.get(0)?,
        pool_id: row.get(1)?,
        name: row.get(2)?,
        ip: row.get(3)?,
        ssh_port: row.get(4)?,
        ssh_user: row.get(5)?,
        role: row.get(6)?,
        network: row.get(7)?,
        ssh_key_fingerprint: row.get(8)?,
        os_version: row.get(9)?,
        cardano_version: row.get(10)?,
        image_registry: row.get(11)?,
        image_digest: row.get(12)?,
        sort_order: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

/// Insert single pool.
pub fn pool_insert(
    conn: &Connection,
    ticker: &str,
    network: &str,
    margin: Option<f64>,
    fixed_cost: Option<i64>,
) -> Result<i64, AppError> {
    conn.execute(
        "INSERT INTO pool (ticker, network, margin, fixed_cost) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![ticker, network, margin, fixed_cost],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get the only pool record in MVP mode.
pub fn pool_get_single(conn: &Connection) -> Result<Option<PoolRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, ticker, network, margin, fixed_cost, created_at, updated_at
         FROM pool
         ORDER BY id ASC
         LIMIT 1",
    )?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        Ok(Some(map_pool_row(row)?))
    } else {
        Ok(None)
    }
}

/// Update single pool and return updated record.
pub fn pool_update_single(
    conn: &Connection,
    ticker: Option<&str>,
    margin: Option<f64>,
    fixed_cost: Option<i64>,
) -> Result<PoolRow, AppError> {
    let current =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let next_ticker = ticker.unwrap_or(current.ticker.as_str());
    let next_margin = margin.or(current.margin);
    let next_fixed_cost = fixed_cost.or(current.fixed_cost);

    conn.execute(
        "UPDATE pool
         SET ticker = ?1, margin = ?2, fixed_cost = ?3, updated_at = datetime('now')
         WHERE id = ?4",
        rusqlite::params![next_ticker, next_margin, next_fixed_cost, current.id],
    )?;

    pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool disappeared after update".into()))
}

/// Insert one machine.
pub fn machine_insert(
    conn: &Connection,
    pool_id: i64,
    name: &str,
    ip: &str,
    ssh_port: i64,
    ssh_user: &str,
    role: &str,
    ssh_key_fingerprint: Option<&str>,
) -> Result<i64, AppError> {
    conn.execute(
        "INSERT INTO machine (pool_id, name, ip, ssh_port, ssh_user, role, ssh_key_fingerprint, image_registry)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            pool_id,
            name,
            ip,
            ssh_port,
            ssh_user,
            role,
            ssh_key_fingerprint,
            DEFAULT_IMAGE_REGISTRY
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get one machine by id.
pub fn machine_get(conn: &Connection, machine_id: i64) -> Result<Option<MachineRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT
            m.id, m.pool_id, m.name, m.ip, m.ssh_port, m.ssh_user, m.role,
            p.network, m.ssh_key_fingerprint, m.os_version, m.cardano_version,
            m.image_registry, m.image_digest, m.sort_order, m.created_at, m.updated_at
         FROM machine m
         JOIN pool p ON p.id = m.pool_id
         WHERE m.id = ?1",
    )?;
    let mut rows = stmt.query(rusqlite::params![machine_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(map_machine_row(row)?))
    } else {
        Ok(None)
    }
}

/// List machines with optional role / network filters.
pub fn machine_list(
    conn: &Connection,
    role_filter: Option<&str>,
    network_filter: Option<&str>,
) -> Result<Vec<MachineRow>, AppError> {
    let mut stmt = match (role_filter, network_filter) {
        (Some(_), Some(_)) => conn.prepare(
            "SELECT
                m.id, m.pool_id, m.name, m.ip, m.ssh_port, m.ssh_user, m.role,
                p.network, m.ssh_key_fingerprint, m.os_version, m.cardano_version,
                m.image_registry, m.image_digest, m.sort_order, m.created_at, m.updated_at
             FROM machine m
             JOIN pool p ON p.id = m.pool_id
             WHERE m.role = ?1 AND p.network = ?2
             ORDER BY m.sort_order ASC, m.id ASC",
        )?,
        (Some(_), None) => conn.prepare(
            "SELECT
                m.id, m.pool_id, m.name, m.ip, m.ssh_port, m.ssh_user, m.role,
                p.network, m.ssh_key_fingerprint, m.os_version, m.cardano_version,
                m.image_registry, m.image_digest, m.sort_order, m.created_at, m.updated_at
             FROM machine m
             JOIN pool p ON p.id = m.pool_id
             WHERE m.role = ?1
             ORDER BY m.sort_order ASC, m.id ASC",
        )?,
        (None, Some(_)) => conn.prepare(
            "SELECT
                m.id, m.pool_id, m.name, m.ip, m.ssh_port, m.ssh_user, m.role,
                p.network, m.ssh_key_fingerprint, m.os_version, m.cardano_version,
                m.image_registry, m.image_digest, m.sort_order, m.created_at, m.updated_at
             FROM machine m
             JOIN pool p ON p.id = m.pool_id
             WHERE p.network = ?1
             ORDER BY m.sort_order ASC, m.id ASC",
        )?,
        (None, None) => conn.prepare(
            "SELECT
                m.id, m.pool_id, m.name, m.ip, m.ssh_port, m.ssh_user, m.role,
                p.network, m.ssh_key_fingerprint, m.os_version, m.cardano_version,
                m.image_registry, m.image_digest, m.sort_order, m.created_at, m.updated_at
             FROM machine m
             JOIN pool p ON p.id = m.pool_id
             ORDER BY m.sort_order ASC, m.id ASC",
        )?,
    };
    let rows = match (role_filter, network_filter) {
        (Some(role), Some(network)) => {
            stmt.query_map(rusqlite::params![role, network], map_machine_row)?
        }
        (Some(role), None) => stmt.query_map(rusqlite::params![role], map_machine_row)?,
        (None, Some(network)) => stmt.query_map(rusqlite::params![network], map_machine_row)?,
        (None, None) => stmt.query_map([], map_machine_row)?,
    };
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Insert an audit record.
pub fn audit_log_insert(
    conn: &Connection,
    action: &str,
    detail: &serde_json::Value,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO audit_log (action, detail) VALUES (?1, ?2)",
        rusqlite::params![action, detail.to_string()],
    )?;
    Ok(())
}

/// Business-level cascade delete for Machine (no DB FK).
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

/// Business-level cascade delete for Pool (no DB FK).
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

/// Check whether a table exists.
pub fn table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

/// Read SQLite user_version.
pub fn get_user_version(conn: &Connection) -> Result<i32, AppError> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(AppError::from)
}
