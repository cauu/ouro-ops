use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;
use tauri::State;

use crate::db::{
    audit_log_insert, machine_get, pool_get_single, pool_insert, pool_update_single, DbState,
    PoolRow,
};
use crate::error::AppError;

type SshExecFn = dyn Fn(&str, &str, i64, &str) -> Result<String, AppError>;

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

fn classify_ssh_error(target: &str, stderr: &str) -> AppError {
    if stderr.contains("Permission denied") || stderr.contains("Authentication failed") {
        return AppError::SshAuthFailed(target.to_string());
    }
    if stderr.contains("Connection timed out")
        || stderr.contains("Operation timed out")
        || stderr.contains("No route to host")
        || stderr.contains("Connection refused")
        || stderr.contains("Could not resolve hostname")
    {
        return AppError::SshTimeout(target.to_string());
    }
    AppError::Internal(format!("ssh command failed: {stderr}"))
}

fn shell_single_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\"'\"'"))
}

fn wrap_remote_command(remote_cmd: &str) -> String {
    if !remote_cmd.trim_start().starts_with("docker ") {
        return remote_cmd.to_string();
    }
    let wrapped = format!(
        "if [ \"$(id -u)\" -eq 0 ]; then {cmd}; else {cmd} 2>/dev/null || sudo -n {cmd}; fi",
        cmd = remote_cmd
    );
    format!("sh -lc {}", shell_single_quote(wrapped.as_str()))
}

fn run_ssh_command(
    ssh_user: &str,
    ip: &str,
    port: i64,
    remote_cmd: &str,
) -> Result<String, AppError> {
    let target = format!("{ssh_user}@{ip}");
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=8",
            "-p",
        ])
        .arg(port.to_string())
        .arg(target.as_str())
        .arg(wrap_remote_command(remote_cmd))
        .output()?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(classify_ssh_error(
        format!("{ssh_user}@{ip}:{port}").as_str(),
        stderr.as_str(),
    ))
}

fn cli_network_args(network: &str) -> Result<&'static str, AppError> {
    match network {
        "mainnet" => Ok("--mainnet"),
        "preprod" => Ok("--testnet-magic 1"),
        "preview" => Ok("--testnet-magic 2"),
        other => Err(AppError::Internal(format!("invalid network: {other}"))),
    }
}

fn docker_cardano_cli(args: &str) -> String {
    format!(
        "docker exec cardano-node sh -lc {}",
        shell_single_quote(format!("cardano-cli {args}").as_str())
    )
}

fn candidate_container_key_paths(cold_vkey_path: &str) -> Vec<String> {
    let mut candidates = vec![cold_vkey_path.to_string()];
    if let Some(rest) = cold_vkey_path.strip_prefix("/opt/cardano/keys/") {
        candidates.push(format!("/opt/cardano/config/keys/{rest}"));
    }
    if let Some(rest) = cold_vkey_path.strip_prefix("/opt/cardano/config/keys/") {
        candidates.push(format!("/opt/cardano/keys/{rest}"));
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn parse_i64_field(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn parse_f64_field(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        Some(Value::Object(map)) => {
            let numerator = map
                .get("numerator")
                .or_else(|| map.get("Numerator"))
                .and_then(Value::as_f64);
            let denominator = map
                .get("denominator")
                .or_else(|| map.get("Denominator"))
                .and_then(Value::as_f64);
            match (numerator, denominator) {
                (Some(n), Some(d)) if d != 0.0 => Some(n / d),
                _ => None,
            }
        }
        _ => None,
    }
}

fn extract_string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect(),
        Some(Value::Object(map)) => map
            .keys()
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_relay_list(value: Option<&Value>) -> Vec<PoolOnchainRelay> {
    let mut relays = Vec::new();
    let Some(Value::Array(items)) = value else {
        return relays;
    };
    for item in items {
        let Some(map) = item.as_object() else {
            continue;
        };
        let address = map
            .get("address")
            .or_else(|| map.get("ipv4"))
            .or_else(|| map.get("ipv6"))
            .or_else(|| map.get("dnsName"))
            .or_else(|| map.get("hostname"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let port = parse_i64_field(map.get("port"))
            .or_else(|| parse_i64_field(map.get("Port")))
            .unwrap_or_default();
        if !address.is_empty() {
            relays.push(PoolOnchainRelay { address, port });
        }
    }
    relays
}

fn has_pool_registration_fields(value: &Value) -> bool {
    value
        .as_object()
        .map(|map| {
            [
                "margin",
                "cost",
                "fixed_cost",
                "pledge",
                "rewardAccount",
                "reward_account",
                "owners",
                "relays",
                "metadata",
                "metadataUrl",
                "metadataHash",
                "poolParams",
                "poolParameters",
                "pool_params",
                "currentPoolParams",
                "futurePoolParams",
                "params",
            ]
            .iter()
            .any(|key| map.contains_key(*key))
        })
        .unwrap_or(false)
}

fn unwrap_pool_registration_fields<'a>(value: &'a Value) -> Option<&'a Value> {
    let Value::Object(map) = value else {
        return None;
    };
    for key in [
        "currentPoolParams",
        "futurePoolParams",
        "poolParams",
        "poolParameters",
        "pool_params",
        "params",
    ] {
        if let Some(entry) = map.get(key) {
            if has_pool_registration_fields(entry) || entry.is_object() {
                return Some(entry);
            }
        }
    }
    Some(value)
}

fn find_pool_registration_value<'a>(value: &'a Value, pool_id: &str) -> Option<&'a Value> {
    if has_pool_registration_fields(value) {
        return unwrap_pool_registration_fields(value);
    }
    match value {
        Value::Object(map) => {
            if let Some(pool_entry) = map.get(pool_id) {
                if let Some(found) = find_pool_registration_value(pool_entry, pool_id) {
                    return Some(found);
                }
            }
            let object_pool_id = map
                .get("poolId")
                .or_else(|| map.get("pool_id"))
                .or_else(|| map.get("stakePoolId"))
                .or_else(|| map.get("stake_pool_id"))
                .and_then(Value::as_str);
            if object_pool_id == Some(pool_id) {
                if let Some(found) = unwrap_pool_registration_fields(value) {
                    if has_pool_registration_fields(found) {
                        return Some(found);
                    }
                }
            }
            for key in [
                "currentPoolParams",
                "futurePoolParams",
                "poolParams",
                "poolParameters",
                "pool_params",
                "params",
            ] {
                if let Some(entry) = map.get(key) {
                    if let Some(found) = find_pool_registration_value(entry, pool_id) {
                        return Some(found);
                    }
                }
            }
            for entry in map.values() {
                if let Some(found) = find_pool_registration_value(entry, pool_id) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|entry| find_pool_registration_value(entry, pool_id)),
        _ => None,
    }
}

fn parse_registration_details(
    pool_id: &str,
    raw: &str,
) -> Result<Option<PoolOnchainRegistration>, AppError> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|e| AppError::Internal(format!("invalid onchain registration json: {e}")))?;
    let Some(entry) = find_pool_registration_value(&value, pool_id) else {
        return Ok(None);
    };
    let map = entry
        .as_object()
        .ok_or_else(|| AppError::Internal("invalid onchain registration payload".into()))?;
    let metadata = map.get("metadata").and_then(Value::as_object);
    Ok(Some(PoolOnchainRegistration {
        pool_id: Some(pool_id.to_string()),
        ticker: None,
        margin: parse_f64_field(map.get("margin")),
        fixed_cost: parse_i64_field(map.get("cost").or_else(|| map.get("fixed_cost"))),
        pledge: parse_i64_field(map.get("pledge")),
        reward_account: map
            .get("rewardAccount")
            .or_else(|| map.get("reward_account"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        owners: extract_string_list(map.get("owners")),
        relays: parse_relay_list(map.get("relays")),
        metadata_url: map
            .get("metadataUrl")
            .or_else(|| metadata.and_then(|m| m.get("url")))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        metadata_hash: map
            .get("metadataHash")
            .or_else(|| metadata.and_then(|m| m.get("hash")))
            .and_then(Value::as_str)
            .map(ToString::to_string),
    }))
}

fn resolve_pool_id_from_cold_vkey(
    machine: &crate::db::MachineRow,
    cold_vkey_path: &str,
    ssh_exec: &SshExecFn,
) -> Result<String, AppError> {
    let mut last_error = None;
    for candidate in candidate_container_key_paths(cold_vkey_path) {
        for cli in [
            format!(
                "latest stake-pool id --cold-verification-key-file {candidate} --output-format hex"
            ),
            format!("stake-pool id --cold-verification-key-file {candidate} --output-format hex"),
        ] {
            match ssh_exec(
                machine.ssh_user.as_str(),
                machine.ip.as_str(),
                machine.ssh_port,
                docker_cardano_cli(cli.as_str()).as_str(),
            ) {
                Ok(output) if !output.trim().is_empty() => return Ok(output.trim().to_string()),
                Ok(_) => last_error = Some("empty pool id output".to_string()),
                Err(err) => last_error = Some(err.to_string()),
            }
        }
    }
    Err(AppError::Internal(format!(
        "failed to derive pool id from cold_vkey_path: {}",
        last_error.unwrap_or_else(|| "cold.vkey not accessible from runtime container".into())
    )))
}

fn query_pool_registration_details(
    machine: &crate::db::MachineRow,
    network: &str,
    pool_id: &str,
    ssh_exec: &SshExecFn,
) -> Result<Option<PoolOnchainRegistration>, AppError> {
    let network_args = cli_network_args(network)?;
    let mut last_error = None;
    let mut parse_failures = 0usize;
    for cli in [
        format!(
            "latest query pool-state --stake-pool-id {pool_id} {network_args} --socket-path /ipc/node.socket"
        ),
        format!(
            "query pool-state --stake-pool-id {pool_id} {network_args} --socket-path /ipc/node.socket"
        ),
        format!(
            "latest query pool-params --stake-pool-id {pool_id} {network_args} --socket-path /ipc/node.socket"
        ),
        format!(
            "query pool-params --stake-pool-id {pool_id} {network_args} --socket-path /ipc/node.socket"
        ),
    ] {
        match ssh_exec(
            machine.ssh_user.as_str(),
            machine.ip.as_str(),
            machine.ssh_port,
            docker_cardano_cli(cli.as_str()).as_str(),
        ) {
            Ok(output) => match parse_registration_details(pool_id, output.as_str()) {
                Ok(Some(details)) => return Ok(Some(details)),
                Ok(None) => {
                    parse_failures += 1;
                    last_error = Some("onchain details command returned no pool fields".into());
                }
                Err(err) => last_error = Some(err.to_string()),
            },
            Err(err) => last_error = Some(err.to_string()),
        }
    }
    if parse_failures > 0 {
        return Ok(None);
    }
    if let Some(err) = last_error {
        return Err(AppError::Internal(format!(
            "failed to read onchain registration details: {err}"
        )));
    }
    Ok(None)
}

fn pool_onchain_status_with_conn_and_ssh(
    conn: &Connection,
    payload: PoolOnchainQueryPayload,
    ssh_exec: &SshExecFn,
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

    if !missing_requirements.is_empty() {
        return Ok(PoolOnchainStatus {
            machine_id: machine.id,
            machine_name: machine.name,
            network: pool.network,
            query_source: determine_query_source(&payload),
            pool_id: payload.pool_id,
            cold_vkey_path: payload.cold_vkey_path,
            registered_onchain: false,
            registration: None,
            missing_requirements,
            note: "Missing on-chain query requirements; provide pool_id or a cold.vkey path reachable from the target runtime.".into(),
        });
    }

    let query_source = determine_query_source(&payload);
    let resolved_pool_id = match (payload.pool_id.clone(), payload.cold_vkey_path.clone()) {
        (Some(pool_id), _) => pool_id,
        (None, Some(cold_vkey_path)) => {
            resolve_pool_id_from_cold_vkey(&machine, cold_vkey_path.as_str(), ssh_exec)?
        }
        (None, None) => unreachable!("missing requirements handled above"),
    };

    let registration =
        query_pool_registration_details(&machine, pool.network.as_str(), resolved_pool_id.as_str(), ssh_exec)?;
    let registered_onchain = registration.is_some();

    let note = if registered_onchain {
        "Pool is registered on-chain; registration details were loaded from cardano-cli query output.".into()
    } else {
        "Pool is not registered on-chain according to cardano-cli pool-state / pool-params query output.".into()
    };

    Ok(PoolOnchainStatus {
        machine_id: machine.id,
        machine_name: machine.name,
        network: pool.network,
        query_source,
        pool_id: Some(resolved_pool_id),
        cold_vkey_path: payload.cold_vkey_path,
        registered_onchain,
        registration,
        missing_requirements: Vec::new(),
        note,
    })
}

fn pool_onchain_status_with_conn(
    conn: &Connection,
    payload: PoolOnchainQueryPayload,
) -> Result<PoolOnchainStatus, AppError> {
    pool_onchain_status_with_conn_and_ssh(conn, payload, &run_ssh_command)
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

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if remote_cmd.contains("--stake-pool-id pool1xyz")
                && (remote_cmd.contains("query pool-state") || remote_cmd.contains("query pool-params"))
            {
                return Ok("{}".into());
            }
            Err(AppError::Internal(format!("unexpected ssh command: {remote_cmd}")))
        };

        let status = pool_onchain_status_with_conn_and_ssh(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: Some("pool1xyz".into()),
                cold_vkey_path: Some("/tmp/cold.vkey".into()),
            },
            &ssh,
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

    #[test]
    fn tc_pool_009_onchain_query_reads_registered_pool_details() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-1",
            "10.0.0.10",
            22,
            "root",
            "relay",
            Some("SHA256:relay"),
        )
        .expect("insert machine");

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if remote_cmd.contains("query pool-state") && remote_cmd.contains("pool1test") {
                return Ok(
                    r#"{
                        "pool1test": {
                            "currentPoolParams": {
                                "margin": {"numerator": 1, "denominator": 20},
                                "cost": 340000000,
                                "pledge": 500000000,
                                "rewardAccount": "stake1u9reward",
                                "owners": ["stake1u9owner1", "stake1u9owner2"],
                                "relays": [{"dnsName": "relay.example.com", "port": 3001}],
                                "metadata": {
                                    "url": "https://example.com/poolMeta.json",
                                    "hash": "deadbeef"
                                }
                            }
                        }
                    }"#
                    .into(),
                );
            }
            Err(AppError::Internal(format!("unexpected ssh command: {remote_cmd}")))
        };

        let status = pool_onchain_status_with_conn_and_ssh(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: Some("pool1test".into()),
                cold_vkey_path: None,
            },
            &ssh,
        )
        .expect("query onchain");

        assert!(status.registered_onchain);
        assert_eq!(status.pool_id.as_deref(), Some("pool1test"));
        let registration = status.registration.expect("registration");
        assert_eq!(registration.margin, Some(0.05));
        assert_eq!(registration.fixed_cost, Some(340000000));
        assert_eq!(registration.pledge, Some(500000000));
        assert_eq!(
            registration.reward_account.as_deref(),
            Some("stake1u9reward")
        );
        assert_eq!(registration.owners.len(), 2);
        assert_eq!(registration.relays.len(), 1);
        assert_eq!(
            registration.metadata_url.as_deref(),
            Some("https://example.com/poolMeta.json")
        );
    }

    #[test]
    fn tc_pool_010_onchain_query_derives_pool_id_from_cold_vkey() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "preprod", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "bp-1",
            "10.0.0.20",
            22,
            "root",
            "bp",
            Some("SHA256:bp"),
        )
        .expect("insert machine");

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if remote_cmd.contains("stake-pool id")
                && remote_cmd.contains("/opt/cardano/config/keys/cold.vkey")
            {
                return Ok("pool1derived".into());
            }
            if remote_cmd.contains("query pool-state")
                && remote_cmd.contains("--stake-pool-id pool1derived")
            {
                return Ok("{}".into());
            }
            Err(AppError::Internal(format!("unexpected ssh command: {remote_cmd}")))
        };

        let status = pool_onchain_status_with_conn_and_ssh(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: None,
                cold_vkey_path: Some("/opt/cardano/keys/cold.vkey".into()),
            },
            &ssh,
        )
        .expect("query onchain");

        assert_eq!(status.query_source, "cold_vkey");
        assert_eq!(status.pool_id.as_deref(), Some("pool1derived"));
        assert!(!status.registered_onchain);
        assert!(status.registration.is_none());
    }

    #[test]
    fn tc_pool_011_wrap_remote_command_hides_first_docker_permission_error() {
        let wrapped = wrap_remote_command("docker exec cardano-node cardano-cli query tip");
        assert!(wrapped.contains("docker exec cardano-node cardano-cli query tip"));
        assert!(wrapped.contains("2>/dev/null || sudo -n docker exec cardano-node cardano-cli query tip"));
    }

    #[test]
    fn tc_pool_012_onchain_query_parses_nested_pool_params_shape() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-2",
            "10.0.0.11",
            22,
            "root",
            "relay",
            Some("SHA256:relay2"),
        )
        .expect("insert machine");

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if remote_cmd.contains("query pool-state") && remote_cmd.contains("pool1nested") {
                return Ok(
                    r#"{
                        "pools": [
                            {
                                "poolId": "pool1nested",
                                "poolParams": {
                                    "margin": {"numerator": 1, "denominator": 10},
                                    "cost": 500000000,
                                    "pledge": 900000000,
                                    "reward_account": "stake1nestedreward",
                                    "owners": ["stake1nestedowner"],
                                    "relays": [{"hostname": "relay.nested.example", "port": 3001}],
                                    "metadataUrl": "https://example.com/nested.json",
                                    "metadataHash": "beadfeed"
                                }
                            }
                        ]
                    }"#
                    .into(),
                );
            }
            Err(AppError::Internal(format!("unexpected ssh command: {remote_cmd}")))
        };

        let status = pool_onchain_status_with_conn_and_ssh(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: Some("pool1nested".into()),
                cold_vkey_path: None,
            },
            &ssh,
        )
        .expect("query onchain");

        assert!(status.registered_onchain);
        let registration = status.registration.expect("registration");
        assert_eq!(registration.margin, Some(0.1));
        assert_eq!(registration.fixed_cost, Some(500000000));
        assert_eq!(registration.pledge, Some(900000000));
        assert_eq!(
            registration.reward_account.as_deref(),
            Some("stake1nestedreward")
        );
        assert_eq!(registration.metadata_url.as_deref(), Some("https://example.com/nested.json"));
        assert_eq!(registration.metadata_hash.as_deref(), Some("beadfeed"));
        assert_eq!(registration.relays.len(), 1);
    }
}
