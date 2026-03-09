use rusqlite::Connection;
use tauri::State;

use crate::db::{
    audit_log_insert, machine_get, pool_get_single, pool_insert, pool_update_single, DbState,
    PoolRow,
};
use crate::error::AppError;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PoolInitPayload {
    pub ticker: String,
    pub network: String,
    pub margin: Option<f64>,
    pub fixed_cost: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PoolUpdatePayload {
    pub ticker: Option<String>,
    pub margin: Option<f64>,
    pub fixed_cost: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PoolOnchainQueryPayload {
    pub machine_id: i64,
    pub pool_id: Option<String>,
    pub cold_vkey_path: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct Pool {
    pub id: i64,
    pub ticker: String,
    pub network: String,
    pub margin: Option<f64>,
    pub fixed_cost: Option<i64>,
    pub kes_expiry_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PoolOnchainRelay {
    pub address: String,
    pub port: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PoolOnchainRegistration {
    pub pool_id: Option<String>,
    pub ticker: Option<String>,
    pub margin: Option<f64>,
    pub fixed_cost: Option<i64>,
    pub pledge: Option<i64>,
    pub reward_account: Option<String>,
    pub owners: Vec<String>,
    pub relays: Vec<PoolOnchainRelay>,
    pub metadata_url: Option<String>,
    pub metadata_hash: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PoolOnchainStatus {
    pub machine_id: i64,
    pub machine_name: String,
    pub network: String,
    pub query_source: String,
    pub pool_id: Option<String>,
    pub cold_vkey_path: Option<String>,
    pub registered_onchain: bool,
    pub registration: Option<PoolOnchainRegistration>,
    pub missing_requirements: Vec<String>,
    pub note: String,
}

fn validate_network(network: &str) -> Result<(), AppError> {
    if matches!(network, "mainnet" | "preprod" | "preview") {
        Ok(())
    } else {
        Err(AppError::Internal(format!("invalid network: {network}")))
    }
}

fn validate_ticker(ticker: &str) -> Result<(), AppError> {
    if (3..=5).contains(&ticker.chars().count()) {
        Ok(())
    } else {
        Err(AppError::Internal(
            "ticker length must be between 3 and 5".into(),
        ))
    }
}

fn validate_margin(margin: Option<f64>) -> Result<(), AppError> {
    if let Some(v) = margin {
        if !(0.0..=1.0).contains(&v) {
            return Err(AppError::Internal("margin must be in [0, 1]".into()));
        }
    }
    Ok(())
}

fn validate_fixed_cost(fixed_cost: Option<i64>) -> Result<(), AppError> {
    if let Some(v) = fixed_cost {
        if v < 0 {
            return Err(AppError::Internal("fixed_cost must be >= 0".into()));
        }
    }
    Ok(())
}

fn into_pool(row: PoolRow) -> Pool {
    Pool {
        id: row.id,
        ticker: row.ticker,
        network: row.network,
        margin: row.margin,
        fixed_cost: row.fixed_cost,
        kes_expiry_date: None,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn determine_query_source(payload: &PoolOnchainQueryPayload) -> String {
    if payload.pool_id.is_some() {
        "pool_id".into()
    } else if payload.cold_vkey_path.is_some() {
        "cold_vkey".into()
    } else {
        "unresolved".into()
    }
}

fn pool_onchain_status_with_conn(
    conn: &Connection,
    payload: PoolOnchainQueryPayload,
) -> Result<PoolOnchainStatus, AppError> {
    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let machine = machine_get(conn, payload.machine_id)?
        .ok_or_else(|| AppError::Internal(format!("machine {} not found", payload.machine_id)))?;

    if !matches!(machine.role.as_str(), "relay" | "bp") {
        return Err(AppError::Internal(
            "onchain query requires relay or bp machine".into(),
        ));
    }

    let mut missing_requirements = Vec::new();
    if payload.pool_id.is_none() && payload.cold_vkey_path.is_none() {
        missing_requirements.push("pool_id or cold_vkey_path".into());
    }

    Ok(PoolOnchainStatus {
        machine_id: machine.id,
        machine_name: machine.name,
        network: pool.network,
        query_source: determine_query_source(&payload),
        pool_id: payload.pool_id,
        cold_vkey_path: payload.cold_vkey_path,
        registered_onchain: false,
        registration: None,
        missing_requirements,
        note: "On-chain registration query contract is defined; actual cardano-cli query will be implemented in p6-2.".into(),
    })
}

fn pool_init_with_conn(conn: &Connection, payload: PoolInitPayload) -> Result<Pool, AppError> {
    validate_ticker(&payload.ticker)?;
    validate_network(&payload.network)?;
    validate_margin(payload.margin)?;
    validate_fixed_cost(payload.fixed_cost)?;

    if pool_get_single(conn)?.is_some() {
        return Err(AppError::Internal("pool already initialized".into()));
    }
    pool_insert(
        conn,
        payload.ticker.as_str(),
        payload.network.as_str(),
        payload.margin,
        payload.fixed_cost,
    )?;
    let row =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool init failed".into()))?;
    let pool = into_pool(row);
    audit_log_insert(
        conn,
        "pool_init",
        &serde_json::json!({
            "pool_id": pool.id,
            "ticker": pool.ticker,
            "network": pool.network,
            "margin": pool.margin,
            "fixed_cost": pool.fixed_cost
        }),
    )?;
    Ok(pool)
}

fn pool_get_with_conn(conn: &Connection) -> Result<Pool, AppError> {
    let row =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    Ok(into_pool(row))
}

fn pool_update_with_conn(conn: &Connection, payload: PoolUpdatePayload) -> Result<Pool, AppError> {
    let current =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    if let Some(ticker) = payload.ticker.as_ref() {
        validate_ticker(ticker)?;
    }
    validate_margin(payload.margin)?;
    validate_fixed_cost(payload.fixed_cost)?;

    let row = pool_update_single(
        conn,
        payload.ticker.as_deref(),
        payload.margin,
        payload.fixed_cost,
    )?;
    let pool = into_pool(row);
    audit_log_insert(
        conn,
        "pool_update",
        &serde_json::json!({
            "pool_id": pool.id,
            "previous": {
                "ticker": current.ticker,
                "margin": current.margin,
                "fixed_cost": current.fixed_cost
            },
            "next": {
                "ticker": pool.ticker,
                "margin": pool.margin,
                "fixed_cost": pool.fixed_cost
            }
        }),
    )?;
    Ok(pool)
}

#[tauri::command]
pub async fn pool_init(payload: PoolInitPayload, db: State<'_, DbState>) -> Result<Pool, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    pool_init_with_conn(&conn, payload)
}

#[tauri::command]
pub async fn pool_get(db: State<'_, DbState>) -> Result<Pool, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    pool_get_with_conn(&conn)
}

#[tauri::command]
pub async fn pool_update(
    payload: PoolUpdatePayload,
    db: State<'_, DbState>,
) -> Result<Pool, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    pool_update_with_conn(&conn, payload)
}

#[tauri::command]
pub async fn pool_onchain_status(
    payload: PoolOnchainQueryPayload,
    db: State<'_, DbState>,
) -> Result<PoolOnchainStatus, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    pool_onchain_status_with_conn(&conn, payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_migrations;
    use rusqlite::Connection;

    #[test]
    fn validate_ticker_length() {
        assert!(validate_ticker("OURO").is_ok());
        assert!(validate_ticker("OO").is_err());
        assert!(validate_ticker("TOOLONG").is_err());
    }

    #[test]
    fn validate_margin_range() {
        assert!(validate_margin(Some(0.0)).is_ok());
        assert!(validate_margin(Some(1.0)).is_ok());
        assert!(validate_margin(Some(1.1)).is_err());
    }

    #[test]
    fn tc_pool_001_init_success() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool = pool_init_with_conn(
            &conn,
            PoolInitPayload {
                ticker: "OURO".into(),
                network: "preprod".into(),
                margin: Some(0.02),
                fixed_cost: Some(340000000),
            },
        )
        .expect("pool init");
        assert_eq!(pool.ticker, "OURO");
        assert_eq!(pool.network, "preprod");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pool", [], |r| r.get(0))
            .expect("count pool");
        assert_eq!(count, 1);
        let count_audit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'pool_init'",
                [],
                |r| r.get(0),
            )
            .expect("count audit");
        assert_eq!(count_audit, 1);
    }

    #[test]
    fn tc_pool_002_duplicate_init_rejected() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let _ = pool_init_with_conn(
            &conn,
            PoolInitPayload {
                ticker: "OURO".into(),
                network: "preprod".into(),
                margin: None,
                fixed_cost: None,
            },
        )
        .expect("first init");
        let err = pool_init_with_conn(
            &conn,
            PoolInitPayload {
                ticker: "ABCD".into(),
                network: "preprod".into(),
                margin: None,
                fixed_cost: None,
            },
        )
        .expect_err("second init should fail");
        assert!(format!("{err}").contains("already initialized"));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pool", [], |r| r.get(0))
            .expect("count pool");
        assert_eq!(count, 1);
    }

    #[test]
    fn tc_pool_003_get_without_pool_fails() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let err = pool_get_with_conn(&conn).expect_err("pool_get should fail");
        assert!(format!("{err}").contains("not initialized"));
    }

    #[test]
    fn tc_pool_004_get_with_pool_success() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let _ = pool_init_with_conn(
            &conn,
            PoolInitPayload {
                ticker: "OURO".into(),
                network: "preprod".into(),
                margin: Some(0.03),
                fixed_cost: Some(500000000),
            },
        )
        .expect("pool init");
        let pool = pool_get_with_conn(&conn).expect("pool get");
        assert_eq!(pool.ticker, "OURO");
        assert_eq!(pool.network, "preprod");
        assert_eq!(pool.margin, Some(0.03));
        assert_eq!(pool.fixed_cost, Some(500000000));
    }

    #[test]
    fn tc_pool_005_update_margin_cost() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let _ = pool_init_with_conn(
            &conn,
            PoolInitPayload {
                ticker: "OURO".into(),
                network: "preprod".into(),
                margin: Some(0.02),
                fixed_cost: Some(340000000),
            },
        )
        .expect("pool init");
        let pool = pool_update_with_conn(
            &conn,
            PoolUpdatePayload {
                ticker: None,
                margin: Some(0.05),
                fixed_cost: Some(510000000),
            },
        )
        .expect("pool update");
        assert_eq!(pool.margin, Some(0.05));
        assert_eq!(pool.fixed_cost, Some(510000000));
        let count_audit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'pool_update'",
                [],
                |r| r.get(0),
        )
        .expect("count audit");
        assert_eq!(count_audit, 1);
    }

    #[test]
    fn tc_pool_006_onchain_query_contract_prefers_pool_id_source() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-1",
            "10.0.0.1",
            22,
            "root",
            "relay",
            Some("SHA256:relay"),
        )
        .expect("insert machine");

        let status = pool_onchain_status_with_conn(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: Some("pool1xyz".into()),
                cold_vkey_path: Some("/tmp/cold.vkey".into()),
            },
        )
        .expect("query contract");

        assert_eq!(status.machine_name, "relay-1");
        assert_eq!(status.network, "mainnet");
        assert_eq!(status.query_source, "pool_id");
        assert_eq!(status.pool_id.as_deref(), Some("pool1xyz"));
        assert!(status.registration.is_none());
        assert!(!status.registered_onchain);
        assert!(status.missing_requirements.is_empty());
    }

    #[test]
    fn tc_pool_007_onchain_query_contract_reports_missing_inputs() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "preprod", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "bp-1",
            "10.0.0.2",
            22,
            "root",
            "bp",
            Some("SHA256:bp"),
        )
        .expect("insert machine");

        let status = pool_onchain_status_with_conn(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: None,
                cold_vkey_path: None,
            },
        )
        .expect("query contract");

        assert_eq!(status.query_source, "unresolved");
        assert_eq!(status.missing_requirements, vec!["pool_id or cold_vkey_path"]);
    }

    #[test]
    fn tc_pool_008_onchain_query_rejects_archive_machine() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "archive-1",
            "10.0.0.3",
            22,
            "root",
            "archive",
            Some("SHA256:archive"),
        )
        .expect("insert machine");

        let err = pool_onchain_status_with_conn(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: Some("pool1xyz".into()),
                cold_vkey_path: None,
            },
        )
        .expect_err("archive should be rejected");

        assert!(format!("{err}").contains("requires relay or bp machine"));
    }
}
