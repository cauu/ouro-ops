use tauri::State;

use crate::db::{pool_get_single, pool_insert, pool_update_single, DbState, PoolRow};
use crate::error::AppError;

#[derive(Debug, serde::Deserialize)]
pub struct PoolInitPayload {
    pub ticker: String,
    pub network: String,
    pub margin: Option<f64>,
    pub fixed_cost: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
pub struct PoolUpdatePayload {
    pub ticker: Option<String>,
    pub margin: Option<f64>,
    pub fixed_cost: Option<i64>,
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

#[tauri::command]
pub async fn pool_init(payload: PoolInitPayload, db: State<'_, DbState>) -> Result<Pool, AppError> {
    validate_ticker(&payload.ticker)?;
    validate_network(&payload.network)?;
    validate_margin(payload.margin)?;
    validate_fixed_cost(payload.fixed_cost)?;

    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    if pool_get_single(&conn)?.is_some() {
        return Err(AppError::Internal("pool already initialized".into()));
    }
    pool_insert(
        &conn,
        payload.ticker.as_str(),
        payload.network.as_str(),
        payload.margin,
        payload.fixed_cost,
    )?;
    let row =
        pool_get_single(&conn)?.ok_or_else(|| AppError::Internal("pool init failed".into()))?;
    Ok(into_pool(row))
}

#[tauri::command]
pub async fn pool_get(db: State<'_, DbState>) -> Result<Pool, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let row =
        pool_get_single(&conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    Ok(into_pool(row))
}

#[tauri::command]
pub async fn pool_update(
    payload: PoolUpdatePayload,
    db: State<'_, DbState>,
) -> Result<Pool, AppError> {
    if let Some(ticker) = payload.ticker.as_ref() {
        validate_ticker(ticker)?;
    }
    validate_margin(payload.margin)?;
    validate_fixed_cost(payload.fixed_cost)?;

    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let row = pool_update_single(
        &conn,
        payload.ticker.as_deref(),
        payload.margin,
        payload.fixed_cost,
    )?;
    Ok(into_pool(row))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
