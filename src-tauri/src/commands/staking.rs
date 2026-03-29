use crate::db::DbState;
use crate::error::AppError;
use crate::db::pool_get_single;
use tauri::State;

fn koios_base_url(network: &str) -> &'static str {
    match network {
        "mainnet" => "https://api.koios.rest/api/v1",
        "preprod" => "https://preprod.koios.rest/api/v1",
        "preview" => "https://preview.koios.rest/api/v1",
        _ => "https://preprod.koios.rest/api/v1",
    }
}

fn require_onchain_pool_id(db: &DbState) -> Result<(String, String), AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    let pool =
        pool_get_single(&conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let pool_id = pool
        .onchain_pool_id
        .ok_or_else(|| AppError::Internal("pool is not bound to an on-chain pool id".into()))?;
    Ok((pool_id, pool.network))
}

// --- pool_staking_summary ---

#[derive(Debug, serde::Serialize)]
pub struct StakingSummary {
    pub live_delegators: i64,
    pub live_stake: i64,
    pub live_stake_ada: f64,
    pub active_stake: i64,
    pub active_stake_ada: f64,
}

#[derive(Debug, serde::Deserialize)]
struct KoiosPoolInfoEntry {
    live_delegators: Option<serde_json::Value>,
    live_stake: Option<serde_json::Value>,
    active_stake: Option<serde_json::Value>,
}

fn parse_i64_field(value: &Option<serde_json::Value>) -> i64 {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(serde_json::Value::String(s)) => s.parse::<i64>().unwrap_or(0),
        _ => 0,
    }
}

#[tauri::command]
pub async fn pool_staking_summary(db: State<'_, DbState>) -> Result<StakingSummary, AppError> {
    let (pool_id, network) = require_onchain_pool_id(&db)?;
    let url = format!("{}/pool_info", koios_base_url(&network));
    let body = serde_json::json!({ "_pool_bech32_ids": [pool_id] });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("koios request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "koios pool_info returned {status}: {text}"
        )));
    }

    let entries: Vec<KoiosPoolInfoEntry> = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("koios pool_info parse failed: {e}")))?;

    let entry = entries
        .first()
        .ok_or_else(|| AppError::Internal("koios pool_info returned empty result".into()))?;

    let live_delegators = parse_i64_field(&entry.live_delegators);
    let live_stake = parse_i64_field(&entry.live_stake);
    let active_stake = parse_i64_field(&entry.active_stake);

    Ok(StakingSummary {
        live_delegators,
        live_stake,
        live_stake_ada: live_stake as f64 / 1_000_000.0,
        active_stake,
        active_stake_ada: active_stake as f64 / 1_000_000.0,
    })
}

// --- pool_staking_history ---

#[derive(Debug, serde::Serialize)]
pub struct StakingEpochEntry {
    pub epoch_no: i64,
    pub delegator_cnt: i64,
    pub active_stake: i64,
    pub active_stake_ada: f64,
}

#[derive(Debug, serde::Deserialize)]
struct KoiosPoolHistoryEntry {
    epoch_no: Option<serde_json::Value>,
    delegator_cnt: Option<serde_json::Value>,
    active_stake: Option<serde_json::Value>,
}

#[tauri::command]
pub async fn pool_staking_history(
    epoch_count: Option<i64>,
    db: State<'_, DbState>,
) -> Result<Vec<StakingEpochEntry>, AppError> {
    let (pool_id, network) = require_onchain_pool_id(&db)?;
    let url = format!("{}/pool_history", koios_base_url(&network));
    let body = serde_json::json!({ "_pool_bech32_ids": [pool_id] });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("koios request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "koios pool_history returned {status}: {text}"
        )));
    }

    let entries: Vec<KoiosPoolHistoryEntry> = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("koios pool_history parse failed: {e}")))?;

    let mut result: Vec<StakingEpochEntry> = entries
        .iter()
        .map(|e| {
            let active_stake = parse_i64_field(&e.active_stake);
            StakingEpochEntry {
                epoch_no: parse_i64_field(&e.epoch_no),
                delegator_cnt: parse_i64_field(&e.delegator_cnt),
                active_stake,
                active_stake_ada: active_stake as f64 / 1_000_000.0,
            }
        })
        .collect();

    result.sort_by_key(|e| e.epoch_no);

    let limit = epoch_count.unwrap_or(20) as usize;
    if result.len() > limit {
        result = result.split_off(result.len() - limit);
    }

    Ok(result)
}

// --- pool_delegator_list ---

#[derive(Debug, serde::Serialize)]
pub struct Delegator {
    pub stake_address: String,
    pub amount: i64,
    pub amount_ada: f64,
    pub active_epoch_no: i64,
}

#[derive(Debug, serde::Deserialize)]
struct KoiosDelegatorEntry {
    stake_address: Option<String>,
    amount: Option<serde_json::Value>,
    active_epoch_no: Option<serde_json::Value>,
}

#[tauri::command]
pub async fn pool_delegator_list(
    db: State<'_, DbState>,
) -> Result<Vec<Delegator>, AppError> {
    let (pool_id, network) = require_onchain_pool_id(&db)?;
    let url = format!("{}/pool_delegators", koios_base_url(&network));
    let body = serde_json::json!({ "_pool_bech32_ids": [pool_id] });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("koios request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "koios pool_delegators returned {status}: {text}"
        )));
    }

    let entries: Vec<KoiosDelegatorEntry> = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("koios pool_delegators parse failed: {e}")))?;

    let mut result: Vec<Delegator> = entries
        .into_iter()
        .filter_map(|e| {
            let stake_address = e.stake_address?;
            let amount = parse_i64_field(&e.amount);
            Some(Delegator {
                stake_address,
                amount,
                amount_ada: amount as f64 / 1_000_000.0,
                active_epoch_no: parse_i64_field(&e.active_epoch_no),
            })
        })
        .collect();

    result.sort_by(|a, b| b.amount.cmp(&a.amount));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_staking_001_parse_i64_field_handles_string_and_number() {
        assert_eq!(parse_i64_field(&Some(serde_json::json!(42))), 42);
        assert_eq!(parse_i64_field(&Some(serde_json::json!("12345"))), 12345);
        assert_eq!(parse_i64_field(&Some(serde_json::json!("not_a_number"))), 0);
        assert_eq!(parse_i64_field(&None), 0);
    }

    #[test]
    fn tc_staking_002_koios_base_url_per_network() {
        assert!(koios_base_url("mainnet").contains("api.koios.rest"));
        assert!(koios_base_url("preprod").contains("preprod.koios.rest"));
        assert!(koios_base_url("preview").contains("preview.koios.rest"));
        assert!(koios_base_url("unknown").contains("preprod"));
    }
}
