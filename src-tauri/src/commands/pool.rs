use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;
use tauri::State;

use crate::db::{
    audit_log_insert, machine_get, machine_list as repo_machine_list, pool_bind_onchain_single,
    pool_get_single, pool_insert, pool_unbind_onchain_single, pool_update_single, DbState,
    MachineRow, PoolOnchainBindingUpdate, PoolRow,
};
use crate::error::AppError;

type SshExecFn = dyn Fn(&str, &str, i64, &str) -> Result<String, AppError>;
type MetadataTickerFetchFn = dyn Fn(&str) -> Result<Option<String>, AppError>;

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

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PoolBindOnchainPayload {
    pub machine_id: i64,
    pub pool_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PoolRegistrationPreparePayload {
    pub machine_id: i64,
    pub pool_id: String,
    pub certificate_path: String,
    pub payment_addr_path: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PoolRegistrationSubmitPayload {
    pub machine_id: i64,
    pub pool_id: String,
    pub confirm_pool_id: String,
    pub tx_signed_path: String,
}

#[derive(Debug, serde::Serialize)]
pub struct Pool {
    pub id: i64,
    pub ticker: String,
    pub network: String,
    pub margin: Option<f64>,
    pub fixed_cost: Option<i64>,
    pub onchain_pool_id: Option<String>,
    pub onchain_registered: bool,
    pub pledge: Option<i64>,
    pub reward_account: Option<String>,
    pub metadata_url: Option<String>,
    pub metadata_hash: Option<String>,
    pub owners: Vec<String>,
    pub relays: Vec<PoolOnchainRelay>,
    pub onchain_synced_at: Option<String>,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PoolRegistrationTxDraft {
    pub kind: String,
    pub certificate_path: Option<String>,
    pub required_deposit: Option<i64>,
    pub payment_address: Option<String>,
    pub tx_body_path: Option<String>,
    pub offline_signing_required: bool,
    pub command_preview: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PoolRegistrationPrepareResult {
    pub machine_id: i64,
    pub machine_name: String,
    pub network: String,
    pub pool_id: Option<String>,
    pub registration_relays: Vec<PoolOnchainRelay>,
    pub certificate_generated: bool,
    pub certificate_path: Option<String>,
    pub missing_requirements: Vec<String>,
    pub tx_draft: PoolRegistrationTxDraft,
    pub note: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PoolRegistrationSubmitResult {
    pub machine_id: i64,
    pub machine_name: String,
    pub network: String,
    pub pool_id: String,
    pub submitted: bool,
    pub tx_body_path: Option<String>,
    pub tx_signed_path: Option<String>,
    pub tx_hash: Option<String>,
    pub tx_inputs: Vec<String>,
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
        onchain_pool_id: row.onchain_pool_id,
        onchain_registered: row.onchain_registered,
        pledge: row.pledge,
        reward_account: row.reward_account,
        metadata_url: row.metadata_url,
        metadata_hash: row.metadata_hash,
        owners: row.owners,
        relays: parse_relay_list(Some(&Value::Array(row.relays))),
        onchain_synced_at: row.onchain_synced_at,
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

fn docker_exec_shell(cmd: &str) -> String {
    format!(
        "docker exec cardano-node sh -lc {}",
        shell_single_quote(cmd)
    )
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
    docker_exec_shell(format!("cardano-cli {args}").as_str())
}

fn candidate_container_paths(path: &str) -> Vec<String> {
    let mut candidates = vec![path.to_string()];
    if let Some(rest) = path.strip_prefix("/opt/cardano/keys/") {
        candidates.push(format!("/opt/cardano/config/keys/{rest}"));
    }
    if let Some(rest) = path.strip_prefix("/opt/cardano/config/keys/") {
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
        Some(Value::Object(map)) => map.keys().map(ToString::to_string).collect(),
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
        let relay_entry = map
            .get("single host name")
            .or_else(|| map.get("singleHostName"))
            .or_else(|| map.get("single host addr"))
            .or_else(|| map.get("singleHostAddr"))
            .or_else(|| map.get("multi host name"))
            .or_else(|| map.get("multiHostName"))
            .unwrap_or(item);
        let relay_map = relay_entry.as_object().unwrap_or(map);
        let address = relay_map
            .get("address")
            .or_else(|| relay_map.get("ipv4"))
            .or_else(|| relay_map.get("ipv6"))
            .or_else(|| relay_map.get("dnsName"))
            .or_else(|| relay_map.get("hostname"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let port = parse_i64_field(relay_map.get("port"))
            .or_else(|| parse_i64_field(relay_map.get("Port")))
            .unwrap_or_default();
        if !address.is_empty() {
            relays.push(PoolOnchainRelay { address, port });
        }
    }
    relays
}

fn parse_reward_account_field(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.to_string()),
        Some(Value::Object(map)) => {
            if let Some(key_hash) = map
                .get("credential")
                .and_then(Value::as_object)
                .and_then(|credential| {
                    credential
                        .get("keyHash")
                        .or_else(|| credential.get("scriptHash"))
                })
                .and_then(Value::as_str)
            {
                return Some(key_hash.to_string());
            }
            Some(Value::Object(map.clone()).to_string())
        }
        _ => None,
    }
}

fn parse_pool_metadata_ticker(metadata_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(metadata_json).ok()?;
    let ticker = value.get("ticker")?.as_str()?.trim();
    if ticker.is_empty() || validate_ticker(ticker).is_err() {
        return None;
    }
    Some(ticker.to_string())
}

fn fetch_pool_metadata_ticker(url: &str) -> Result<Option<String>, AppError> {
    let output = Command::new("curl")
        .args([
            "-L",
            "--max-time",
            "8",
            "--silent",
            "--show-error",
            "--fail",
            url,
        ])
        .output()?;
    if !output.status.success() {
        return Err(AppError::Internal(format!(
            "metadata fetch failed for {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(parse_pool_metadata_ticker(
        String::from_utf8_lossy(&output.stdout).as_ref(),
    ))
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
                "spsCost",
                "spsMargin",
                "spsPledge",
                "spsRewardAccount",
                "spsOwners",
                "spsRelays",
                "spsMetadata",
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
        margin: parse_f64_field(map.get("margin").or_else(|| map.get("spsMargin"))),
        fixed_cost: parse_i64_field(
            map.get("cost")
                .or_else(|| map.get("fixed_cost"))
                .or_else(|| map.get("spsCost")),
        ),
        pledge: parse_i64_field(map.get("pledge").or_else(|| map.get("spsPledge"))),
        reward_account: parse_reward_account_field(
            map.get("rewardAccount")
                .or_else(|| map.get("reward_account"))
                .or_else(|| map.get("spsRewardAccount")),
        ),
        owners: extract_string_list(map.get("owners").or_else(|| map.get("spsOwners"))),
        relays: parse_relay_list(map.get("relays").or_else(|| map.get("spsRelays"))),
        metadata_url: map
            .get("metadataUrl")
            .or_else(|| metadata.and_then(|m| m.get("url")))
            .or_else(|| {
                map.get("spsMetadata")
                    .and_then(Value::as_object)
                    .and_then(|m| m.get("url"))
            })
            .and_then(Value::as_str)
            .map(ToString::to_string),
        metadata_hash: map
            .get("metadataHash")
            .or_else(|| metadata.and_then(|m| m.get("hash")))
            .or_else(|| {
                map.get("spsMetadata")
                    .and_then(Value::as_object)
                    .and_then(|m| m.get("hash"))
            })
            .and_then(Value::as_str)
            .map(ToString::to_string),
    }))
}

fn resolve_pool_id_from_container_cold_vkey(
    machine: &crate::db::MachineRow,
    cold_vkey_path: &str,
    ssh_exec: &SshExecFn,
) -> Result<String, AppError> {
    let mut last_error = None;
    for cli in [
        format!(
            "latest stake-pool id --cold-verification-key-file {cold_vkey_path} --output-format hex"
        ),
        format!("stake-pool id --cold-verification-key-file {cold_vkey_path} --output-format hex"),
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
    Err(AppError::Internal(format!(
        "failed to derive pool id from cold_vkey_path: {}",
        last_error.unwrap_or_else(|| "cold.vkey not accessible from runtime container".into())
    )))
}

fn resolve_pool_id_from_cold_vkey(
    machine: &crate::db::MachineRow,
    cold_vkey_path: &str,
    ssh_exec: &SshExecFn,
) -> Result<String, AppError> {
    let mut last_error = None;
    for candidate in candidate_container_paths(cold_vkey_path) {
        match resolve_pool_id_from_container_cold_vkey(machine, candidate.as_str(), ssh_exec) {
            Ok(pool_id) => return Ok(pool_id),
            Err(err) => last_error = Some(err.to_string()),
        }
    }
    Err(AppError::Internal(format!(
        "failed to derive pool id from cold_vkey_path: {}",
        last_error.unwrap_or_else(|| "cold.vkey not accessible from runtime container".into())
    )))
}

fn resolve_remote_file_in_container(
    machine: &crate::db::MachineRow,
    path: &str,
    ssh_exec: &SshExecFn,
) -> Result<String, AppError> {
    let mut last_error = None;
    for candidate in candidate_container_paths(path) {
        let probe = docker_exec_shell(
            format!(
                "test -f {} && printf found",
                shell_single_quote(candidate.as_str())
            )
            .as_str(),
        );
        match ssh_exec(
            machine.ssh_user.as_str(),
            machine.ip.as_str(),
            machine.ssh_port,
            probe.as_str(),
        ) {
            Ok(output) if output.trim() == "found" => return Ok(candidate),
            Ok(_) => last_error = Some("file probe returned no result".to_string()),
            Err(err) => last_error = Some(err.to_string()),
        }
    }
    Err(AppError::Internal(format!(
        "path not accessible from runtime container: {}",
        last_error.unwrap_or_else(|| path.to_string())
    )))
}

fn read_remote_file_from_container(
    machine: &crate::db::MachineRow,
    path: &str,
    ssh_exec: &SshExecFn,
) -> Result<String, AppError> {
    let resolved = resolve_remote_file_in_container(machine, path, ssh_exec)?;
    ssh_exec(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        docker_exec_shell(format!("cat {}", shell_single_quote(resolved.as_str())).as_str())
            .as_str(),
    )
    .map(|output| output.trim().to_string())
}

fn query_pool_deposit(
    machine: &crate::db::MachineRow,
    network: &str,
    ssh_exec: &SshExecFn,
) -> Result<Option<i64>, AppError> {
    let network_args = cli_network_args(network)?;
    for cli in [
        format!("latest query protocol-parameters {network_args} --socket-path /ipc/node.socket"),
        format!("query protocol-parameters {network_args} --socket-path /ipc/node.socket"),
    ] {
        if let Ok(output) = ssh_exec(
            machine.ssh_user.as_str(),
            machine.ip.as_str(),
            machine.ssh_port,
            docker_cardano_cli(cli.as_str()).as_str(),
        ) {
            if let Ok(value) = serde_json::from_str::<Value>(&output) {
                if let Some(deposit) = value
                    .get("stakePoolDeposit")
                    .or_else(|| value.get("stake_pool_deposit"))
                    .or_else(|| value.get("poolDeposit"))
                    .or_else(|| value.get("pool_deposit"))
                    .and_then(|v| parse_i64_field(Some(v)))
                {
                    return Ok(Some(deposit));
                }
            }
        }
    }
    Ok(None)
}

fn infer_registration_relays(
    conn: &Connection,
    machine: &MachineRow,
) -> Result<Vec<PoolOnchainRelay>, AppError> {
    Ok(
        repo_machine_list(conn, Some("relay"), Some(machine.network.as_str()))?
            .into_iter()
            .filter(|candidate| candidate.pool_id == machine.pool_id)
            .map(|candidate| PoolOnchainRelay {
                address: candidate.ip,
                port: 3001,
            })
            .collect(),
    )
}

fn build_registration_tx_command_preview(
    network: &str,
    tx_inputs: &[String],
    payment_address: Option<&str>,
    certificate_path: Option<&str>,
    tx_body_path: Option<&str>,
) -> Result<String, AppError> {
    let network_arg = match network {
        "mainnet" => "--mainnet",
        "preprod" => "--testnet-magic 1",
        "preview" => "--testnet-magic 2",
        other => return Err(AppError::Internal(format!("invalid network: {other}"))),
    };
    let tx_in = if tx_inputs.is_empty() {
        "--tx-in <select-utxo>".to_string()
    } else {
        tx_inputs
            .iter()
            .map(|tx_input| format!("--tx-in {}", shell_single_quote(tx_input)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    Ok(format!(
        "cardano-cli latest transaction build {tx_in} --change-address {} --certificate-file {} {} --socket-path /ipc/node.socket --witness-override 1 --out-file {}",
        shell_single_quote(payment_address.unwrap_or("<payment-address>")),
        shell_single_quote(certificate_path.unwrap_or("<registration-certificate>")),
        network_arg,
        shell_single_quote(tx_body_path.unwrap_or("<unsigned-tx-body>")),
    ))
}

fn push_missing_once(list: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !list.iter().any(|existing| existing == &value) {
        list.push(value);
    }
}

fn build_registration_tx_paths(certificate_path: &str) -> (String, String) {
    let draft_dir = certificate_path
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("/opt/cardano/config/registration-drafts");
    (
        format!("{draft_dir}/pool-registration.raw"),
        format!("{draft_dir}/pool-registration.signed"),
    )
}

fn parse_utxo_inputs(raw: &str) -> Result<Vec<String>, AppError> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|e| AppError::Internal(format!("invalid utxo query json: {e}")))?;
    let Value::Object(map) = value else {
        return Err(AppError::Internal("invalid utxo query payload".into()));
    };
    Ok(map.keys().cloned().collect())
}

fn query_payment_utxo_inputs(
    machine: &crate::db::MachineRow,
    network: &str,
    payment_address: &str,
    ssh_exec: &SshExecFn,
) -> Result<Vec<String>, AppError> {
    let network_args = cli_network_args(network)?;
    for cli in [
        format!(
            "latest query utxo --address {} {network_args} --socket-path /ipc/node.socket --out-file /dev/stdout",
            shell_single_quote(payment_address),
        ),
        format!(
            "query utxo --address {} {network_args} --socket-path /ipc/node.socket --out-file /dev/stdout",
            shell_single_quote(payment_address),
        ),
    ] {
        if let Ok(output) = ssh_exec(
            machine.ssh_user.as_str(),
            machine.ip.as_str(),
            machine.ssh_port,
            docker_cardano_cli(cli.as_str()).as_str(),
        ) {
            let inputs = parse_utxo_inputs(output.as_str())?;
            return Ok(inputs);
        }
    }
    Err(AppError::Internal(
        "failed to query payment address utxo set".into(),
    ))
}

fn build_registration_submit_build_command(
    network: &str,
    tx_inputs: &[String],
    payment_address: &str,
    certificate_path: &str,
    tx_body_path: &str,
    witness_count: usize,
) -> Result<String, AppError> {
    if tx_inputs.is_empty() {
        return Err(AppError::Internal(
            "registration submission requires at least one tx input".into(),
        ));
    }
    let mut parts = vec!["cardano-cli latest transaction build".to_string()];
    for tx_input in tx_inputs {
        parts.push(format!("--tx-in {}", shell_single_quote(tx_input)));
    }
    parts.push(format!(
        "--change-address {}",
        shell_single_quote(payment_address)
    ));
    parts.push(format!(
        "--certificate-file {}",
        shell_single_quote(certificate_path)
    ));
    match network {
        "mainnet" => parts.push("--mainnet".into()),
        "preprod" => parts.push("--testnet-magic 1".into()),
        "preview" => parts.push("--testnet-magic 2".into()),
        other => return Err(AppError::Internal(format!("invalid network: {other}"))),
    }
    parts.push("--socket-path /ipc/node.socket".into());
    parts.push(format!("--witness-override {}", witness_count.max(1)));
    parts.push(format!("--out-file {}", shell_single_quote(tx_body_path)));
    Ok(parts.join(" "))
}

fn build_registration_submit_command(
    network: &str,
    tx_signed_path: &str,
) -> Result<String, AppError> {
    let mut parts = vec![
        "cardano-cli latest transaction submit".to_string(),
        format!("--tx-file {}", shell_single_quote(tx_signed_path)),
        "--socket-path /ipc/node.socket".into(),
    ];
    match network {
        "mainnet" => parts.push("--mainnet".into()),
        "preprod" => parts.push("--testnet-magic 1".into()),
        "preview" => parts.push("--testnet-magic 2".into()),
        other => return Err(AppError::Internal(format!("invalid network: {other}"))),
    }
    Ok(parts.join(" "))
}

fn query_signed_tx_hash(
    machine: &crate::db::MachineRow,
    tx_signed_path: &str,
    ssh_exec: &SshExecFn,
) -> Result<String, AppError> {
    for cli in [
        format!(
            "latest transaction txid --tx-file {}",
            shell_single_quote(tx_signed_path)
        ),
        format!(
            "transaction txid --tx-file {}",
            shell_single_quote(tx_signed_path)
        ),
    ] {
        let output = ssh_exec(
            machine.ssh_user.as_str(),
            machine.ip.as_str(),
            machine.ssh_port,
            docker_cardano_cli(cli.as_str()).as_str(),
        )?;
        if !output.trim().is_empty() {
            return Ok(output.trim().to_string());
        }
    }
    Err(AppError::Internal(
        "failed to compute submitted transaction id".into(),
    ))
}

fn pool_registration_prepare_with_conn_and_ssh(
    conn: &Connection,
    payload: PoolRegistrationPreparePayload,
    ssh_exec: &SshExecFn,
) -> Result<PoolRegistrationPrepareResult, AppError> {
    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let machine = machine_get(conn, payload.machine_id)?
        .ok_or_else(|| AppError::Internal(format!("machine {} not found", payload.machine_id)))?;

    if !matches!(machine.role.as_str(), "relay" | "bp") {
        return Err(AppError::Internal(
            "registration preparation requires relay or bp machine".into(),
        ));
    }

    validate_network(pool.network.as_str())?;
    let pool_id = payload.pool_id.trim().to_string();
    if pool_id.is_empty() {
        return Err(AppError::Internal("pool_id is required".into()));
    }

    let registration_relays = infer_registration_relays(conn, &machine)?;
    let mut missing_requirements = Vec::new();

    if registration_relays.is_empty() {
        push_missing_once(
            &mut missing_requirements,
            "registration requires at least one relay machine in the same pool/network",
        );
    }
    let certificate_path =
        resolve_remote_file_in_container(&machine, payload.certificate_path.as_str(), ssh_exec)
            .map_err(|_| {
                push_missing_once(
                    &mut missing_requirements,
                    format!(
                        "certificate_path not accessible from runtime container: {}",
                        payload.certificate_path
                    ),
                );
                AppError::Internal("registration certificate missing".into())
            })
            .ok();

    let payment_address = match read_remote_file_from_container(
        &machine,
        payload.payment_addr_path.as_str(),
        ssh_exec,
    ) {
        Ok(address) if !address.trim().is_empty() => Some(address),
        Ok(_) => {
            push_missing_once(
                &mut missing_requirements,
                format!(
                    "payment_addr_path resolved but file is empty: {}",
                    payload.payment_addr_path
                ),
            );
            None
        }
        Err(_) => {
            push_missing_once(
                &mut missing_requirements,
                format!(
                    "payment_addr_path not accessible from runtime container: {}",
                    payload.payment_addr_path
                ),
            );
            None
        }
    };

    let required_deposit = query_pool_deposit(&machine, pool.network.as_str(), ssh_exec)?;
    let tx_inputs = if let Some(payment_address) = payment_address.as_deref() {
        match query_payment_utxo_inputs(&machine, pool.network.as_str(), payment_address, ssh_exec)
        {
            Ok(inputs) if !inputs.is_empty() => inputs,
            Ok(_) => {
                push_missing_once(
                    &mut missing_requirements,
                    "payment address has no spendable UTxO for registration transaction build",
                );
                Vec::new()
            }
            Err(err) => {
                push_missing_once(
                    &mut missing_requirements,
                    format!("failed to query payment UTxO set: {err}"),
                );
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let (tx_body_path, _tx_signed_path) = match certificate_path.as_deref() {
        Some(path) => {
            let (body, signed) = build_registration_tx_paths(path);
            (Some(body), Some(signed))
        }
        None => (None, None),
    };

    let resolved_certificate_path = certificate_path.clone();
    let mut certificate_generated = false;
    if missing_requirements.is_empty() {
        let build_cmd = build_registration_submit_build_command(
            pool.network.as_str(),
            tx_inputs.as_slice(),
            payment_address
                .as_deref()
                .ok_or_else(|| AppError::Internal("payment address resolution failed".into()))?,
            resolved_certificate_path
                .as_deref()
                .ok_or_else(|| AppError::Internal("certificate resolution failed".into()))?,
            tx_body_path
                .as_deref()
                .ok_or_else(|| AppError::Internal("tx body path unavailable".into()))?,
            1,
        )?;
        ssh_exec(
            machine.ssh_user.as_str(),
            machine.ip.as_str(),
            machine.ssh_port,
            docker_exec_shell(build_cmd.as_str()).as_str(),
        )?;
        certificate_generated = true;
    }

    let tx_draft = PoolRegistrationTxDraft {
        kind: "unsigned_tx".into(),
        certificate_path: resolved_certificate_path.clone(),
        required_deposit,
        payment_address: payment_address.clone(),
        tx_body_path: tx_body_path.clone(),
        offline_signing_required: true,
        command_preview: build_registration_tx_command_preview(
            pool.network.as_str(),
            tx_inputs.as_slice(),
            payment_address.as_deref(),
            resolved_certificate_path.as_deref(),
            tx_body_path.as_deref(),
        )?,
    };

    let note = if certificate_generated {
        "Unsigned registration transaction built on the hot node. Sign the tx body in a cold environment, then submit the signed tx from the hot node.".into()
    } else if !missing_requirements.is_empty() {
        "Registration preparation is incomplete; provide the missing runtime inputs before building the unsigned transaction.".into()
    } else {
        "Registration preparation completed, but the unsigned transaction was not built.".into()
    };

    let result = PoolRegistrationPrepareResult {
        machine_id: machine.id,
        machine_name: machine.name,
        network: pool.network,
        pool_id: Some(pool_id),
        registration_relays,
        certificate_generated,
        certificate_path: resolved_certificate_path,
        missing_requirements,
        tx_draft,
        note,
    };

    audit_log_insert(
        conn,
        "pool_registration_prepare",
        &serde_json::json!({
            "machine_id": result.machine_id,
            "machine_name": result.machine_name,
            "network": result.network,
            "pool_id": result.pool_id,
            "certificate_generated": result.certificate_generated,
            "certificate_path": result.certificate_path,
            "missing_requirements": result.missing_requirements,
            "tx_body_path": result.tx_draft.tx_body_path,
            "registration_relays": result.registration_relays,
        }),
    )?;

    Ok(result)
}

fn pool_registration_prepare_with_conn(
    conn: &Connection,
    payload: PoolRegistrationPreparePayload,
) -> Result<PoolRegistrationPrepareResult, AppError> {
    pool_registration_prepare_with_conn_and_ssh(conn, payload, &run_ssh_command)
}

fn pool_registration_submit_with_conn_and_ssh(
    conn: &Connection,
    payload: PoolRegistrationSubmitPayload,
    ssh_exec: &SshExecFn,
) -> Result<PoolRegistrationSubmitResult, AppError> {
    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let machine = machine_get(conn, payload.machine_id)?
        .ok_or_else(|| AppError::Internal(format!("machine {} not found", payload.machine_id)))?;

    if !matches!(machine.role.as_str(), "relay" | "bp") {
        return Err(AppError::Internal(
            "registration submission requires relay or bp machine".into(),
        ));
    }

    validate_network(pool.network.as_str())?;
    let pool_id = payload.pool_id.trim().to_string();
    if pool_id.is_empty() {
        return Err(AppError::Internal("pool_id is required".into()));
    }
    if payload.confirm_pool_id.trim() != pool_id {
        return Err(AppError::Internal(
            "registration confirmation mismatch; re-enter the pool id to submit".into(),
        ));
    }

    let mut missing_requirements = Vec::new();
    let tx_signed_path =
        resolve_remote_file_in_container(&machine, payload.tx_signed_path.as_str(), ssh_exec)
            .map_err(|_| {
                push_missing_once(
                    &mut missing_requirements,
                    format!(
                        "tx_signed_path not accessible from runtime container: {}",
                        payload.tx_signed_path
                    ),
                );
                AppError::Internal("signed transaction missing".into())
            })
            .ok();
    let tx_body_path = tx_signed_path.as_deref().map(|path| {
        if let Some(prefix) = path.strip_suffix(".signed") {
            format!("{prefix}.raw")
        } else {
            format!("{path}.raw")
        }
    });

    let mut submitted = false;
    let mut tx_hash = None;
    let tx_inputs = Vec::new();

    if missing_requirements.is_empty() {
        tx_hash = Some(query_signed_tx_hash(
            &machine,
            tx_signed_path
                .as_deref()
                .ok_or_else(|| AppError::Internal("tx signed path unavailable".into()))?,
            ssh_exec,
        )?);

        let submit_cmd = build_registration_submit_command(
            pool.network.as_str(),
            tx_signed_path
                .as_deref()
                .ok_or_else(|| AppError::Internal("tx signed path unavailable".into()))?,
        )?;
        ssh_exec(
            machine.ssh_user.as_str(),
            machine.ip.as_str(),
            machine.ssh_port,
            docker_exec_shell(submit_cmd.as_str()).as_str(),
        )?;
        submitted = true;
    }

    let note = if submitted {
        format!(
            "Pre-signed registration transaction submitted to {} from {}.",
            pool.network, machine.name,
        )
    } else {
        "Registration submission is blocked; provide the missing signed transaction artifact on the hot node before submission.".into()
    };

    let result = PoolRegistrationSubmitResult {
        machine_id: machine.id,
        machine_name: machine.name.clone(),
        network: pool.network,
        pool_id: pool_id.clone(),
        submitted,
        tx_body_path,
        tx_signed_path,
        tx_hash: tx_hash.clone(),
        tx_inputs: tx_inputs.clone(),
        missing_requirements: missing_requirements.clone(),
        note,
    };

    audit_log_insert(
        conn,
        "pool_registration_submit",
        &serde_json::json!({
            "machine_id": result.machine_id,
            "machine_name": result.machine_name,
            "network": result.network,
            "pool_id": result.pool_id,
            "submitted": result.submitted,
            "tx_hash": result.tx_hash,
            "tx_body_path": result.tx_body_path,
            "tx_signed_path": result.tx_signed_path,
            "tx_input_count": result.tx_inputs.len(),
            "missing_requirements": result.missing_requirements,
        }),
    )?;

    Ok(result)
}

fn pool_registration_submit_with_conn(
    conn: &Connection,
    payload: PoolRegistrationSubmitPayload,
) -> Result<PoolRegistrationSubmitResult, AppError> {
    pool_registration_submit_with_conn_and_ssh(conn, payload, &run_ssh_command)
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
    pool_onchain_status_with_conn_ssh_and_metadata(conn, payload, ssh_exec, &|_| Ok(None))
}

fn pool_onchain_status_with_conn_ssh_and_metadata(
    conn: &Connection,
    payload: PoolOnchainQueryPayload,
    ssh_exec: &SshExecFn,
    metadata_fetch: &MetadataTickerFetchFn,
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

    let mut note = String::new();
    let registration = match query_pool_registration_details(
        &machine,
        pool.network.as_str(),
        resolved_pool_id.as_str(),
        ssh_exec,
    ) {
        Ok(details) => details,
        Err(err)
            if machine.role == "bp"
                && is_pool_query_unsupported_error(err.to_string().as_str()) =>
        {
            if let Some(relay_machine) =
                find_pool_relay_fallback(conn, &machine, pool.network.as_str())?
            {
                note = format!(
                    "Primary bp query path is not supported by the current cardano-cli era handling; fell back to relay {}.",
                    relay_machine.name
                );
                query_pool_registration_details(
                    &relay_machine,
                    pool.network.as_str(),
                    resolved_pool_id.as_str(),
                    ssh_exec,
                )?
            } else {
                return Err(err);
            }
        }
        Err(err) => return Err(err),
    };
    let mut registration = registration;
    if let Some(reg) = registration.as_mut() {
        if reg.ticker.is_none() {
            if let Some(metadata_url) = reg.metadata_url.as_deref() {
                reg.ticker = metadata_fetch(metadata_url).unwrap_or(None);
            }
        }
    }
    let registered_onchain = registration.is_some();

    let note = if registered_onchain {
        if note.is_empty() {
            "Pool is registered on-chain; registration details were loaded from cardano-cli query output.".into()
        } else {
            format!(
                "Pool is registered on-chain; registration details were loaded from cardano-cli query output. {note}"
            )
        }
    } else {
        if note.is_empty() {
            "Pool is not registered on-chain according to cardano-cli pool-state / pool-params query output."
                .into()
        } else {
            format!(
                "Pool is not registered on-chain according to cardano-cli pool-state / pool-params query output. {note}"
            )
        }
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

fn is_pool_query_unsupported_error(error: &str) -> bool {
    error.contains("This query is not supported in the era")
        || error.contains("query pool-state Error:")
        || error.contains("query pool-params Error:")
}

fn find_pool_relay_fallback(
    conn: &Connection,
    machine: &MachineRow,
    network: &str,
) -> Result<Option<MachineRow>, AppError> {
    Ok(repo_machine_list(conn, Some("relay"), Some(network))?
        .into_iter()
        .find(|candidate| candidate.pool_id == machine.pool_id && candidate.id != machine.id))
}

fn pool_onchain_status_with_conn(
    conn: &Connection,
    payload: PoolOnchainQueryPayload,
) -> Result<PoolOnchainStatus, AppError> {
    pool_onchain_status_with_conn_ssh_and_metadata(
        conn,
        payload,
        &run_ssh_command,
        &fetch_pool_metadata_ticker,
    )
}

fn persist_bound_pool_status(
    conn: &Connection,
    status: &PoolOnchainStatus,
) -> Result<Pool, AppError> {
    let registration = status.registration.as_ref().ok_or_else(|| {
        AppError::Internal("pool is not registered on-chain; cannot bind local pool".into())
    })?;
    let resolved_pool_id = status.pool_id.as_deref().ok_or_else(|| {
        AppError::Internal("resolved pool id missing from on-chain registration status".into())
    })?;
    let owners_json = serde_json::to_string(&registration.owners)
        .map_err(|e| AppError::Internal(format!("serialize owners json failed: {e}")))?;
    let relays_json = serde_json::to_string(&registration.relays)
        .map_err(|e| AppError::Internal(format!("serialize relays json failed: {e}")))?;
    let row = pool_bind_onchain_single(
        conn,
        PoolOnchainBindingUpdate {
            pool_id: resolved_pool_id,
            ticker: registration.ticker.as_deref(),
            margin: registration.margin,
            fixed_cost: registration.fixed_cost,
            pledge: registration.pledge,
            reward_account: registration.reward_account.as_deref(),
            metadata_url: registration.metadata_url.as_deref(),
            metadata_hash: registration.metadata_hash.as_deref(),
            owners_json: owners_json.as_str(),
            relays_json: relays_json.as_str(),
        },
    )?;
    let pool = into_pool(row);
    audit_log_insert(
        conn,
        "pool_bind_onchain",
        &serde_json::json!({
            "machine_id": status.machine_id,
            "machine_name": status.machine_name,
            "query_source": status.query_source,
            "pool_id": pool.onchain_pool_id,
            "ticker": pool.ticker,
            "margin": pool.margin,
            "fixed_cost": pool.fixed_cost,
            "pledge": pool.pledge,
            "metadata_url": pool.metadata_url,
            "metadata_hash": pool.metadata_hash,
        }),
    )?;
    Ok(pool)
}

fn select_pool_query_machine(
    conn: &Connection,
    network: &str,
) -> Result<Option<MachineRow>, AppError> {
    if let Some(relay) = repo_machine_list(conn, Some("relay"), Some(network))?
        .into_iter()
        .next()
    {
        return Ok(Some(relay));
    }
    Ok(repo_machine_list(conn, Some("bp"), Some(network))?
        .into_iter()
        .next())
}

fn pool_bind_onchain_with_conn(
    conn: &Connection,
    payload: PoolBindOnchainPayload,
) -> Result<Pool, AppError> {
    let status = pool_onchain_status_with_conn(
        conn,
        PoolOnchainQueryPayload {
            machine_id: payload.machine_id,
            pool_id: Some(payload.pool_id),
            cold_vkey_path: None,
        },
    )?;
    if !status.registered_onchain {
        return Err(AppError::Internal(
            "pool is not registered on-chain; cannot bind local pool".into(),
        ));
    }
    persist_bound_pool_status(conn, &status)
}

fn pool_refresh_bound_onchain_with_conn(conn: &Connection) -> Result<Pool, AppError> {
    let current =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let onchain_pool_id = current
        .onchain_pool_id
        .as_deref()
        .ok_or_else(|| AppError::Internal("pool is not bound to an on-chain pool id".into()))?;
    let machine = select_pool_query_machine(conn, current.network.as_str())?.ok_or_else(|| {
        AppError::Internal("no relay or bp machine available for on-chain refresh".into())
    })?;
    let status = pool_onchain_status_with_conn(
        conn,
        PoolOnchainQueryPayload {
            machine_id: machine.id,
            pool_id: Some(onchain_pool_id.to_string()),
            cold_vkey_path: None,
        },
    )?;
    if !status.registered_onchain {
        return Err(AppError::Internal(
            "bound on-chain pool no longer appears registered".into(),
        ));
    }
    persist_bound_pool_status(conn, &status)
}

fn pool_unbind_onchain_with_conn(conn: &Connection) -> Result<Pool, AppError> {
    let current =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    let row = pool_unbind_onchain_single(conn)?;
    let pool = into_pool(row);
    audit_log_insert(
        conn,
        "pool_unbind_onchain",
        &serde_json::json!({
            "previous_onchain_pool_id": current.onchain_pool_id,
            "previous_onchain_registered": current.onchain_registered,
            "previous_metadata_url": current.metadata_url,
            "previous_metadata_hash": current.metadata_hash,
            "previous_onchain_synced_at": current.onchain_synced_at,
        }),
    )?;
    Ok(pool)
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
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_init_with_conn(&conn, payload)
}

#[tauri::command]
pub async fn pool_get(db: State<'_, DbState>) -> Result<Pool, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_get_with_conn(&conn)
}

#[tauri::command]
pub async fn pool_update(
    payload: PoolUpdatePayload,
    db: State<'_, DbState>,
) -> Result<Pool, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_update_with_conn(&conn, payload)
}

#[tauri::command]
pub async fn pool_onchain_status(
    payload: PoolOnchainQueryPayload,
    db: State<'_, DbState>,
) -> Result<PoolOnchainStatus, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_onchain_status_with_conn(&conn, payload)
}

#[tauri::command]
pub async fn pool_bind_onchain(
    payload: PoolBindOnchainPayload,
    db: State<'_, DbState>,
) -> Result<Pool, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_bind_onchain_with_conn(&conn, payload)
}

#[tauri::command]
pub async fn pool_refresh_bound_onchain(db: State<'_, DbState>) -> Result<Pool, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_refresh_bound_onchain_with_conn(&conn)
}

#[tauri::command]
pub async fn pool_unbind_onchain(db: State<'_, DbState>) -> Result<Pool, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_unbind_onchain_with_conn(&conn)
}

#[tauri::command]
pub async fn pool_registration_prepare(
    payload: PoolRegistrationPreparePayload,
    db: State<'_, DbState>,
) -> Result<PoolRegistrationPrepareResult, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_registration_prepare_with_conn(&conn, payload)
}

#[tauri::command]
pub async fn pool_registration_submit(
    payload: PoolRegistrationSubmitPayload,
    db: State<'_, DbState>,
) -> Result<PoolRegistrationSubmitResult, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    pool_registration_submit_with_conn(&conn, payload)
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
                && (remote_cmd.contains("query pool-state")
                    || remote_cmd.contains("query pool-params"))
            {
                return Ok("{}".into());
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
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
        assert_eq!(
            status.missing_requirements,
            vec!["pool_id or cold_vkey_path"]
        );
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
                return Ok(r#"{
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
                .into());
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
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
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
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
        assert!(wrapped
            .contains("2>/dev/null || sudo -n docker exec cardano-node cardano-cli query tip"));
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
                return Ok(r#"{
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
                .into());
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
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
        assert_eq!(
            registration.metadata_url.as_deref(),
            Some("https://example.com/nested.json")
        );
        assert_eq!(registration.metadata_hash.as_deref(), Some("beadfeed"));
        assert_eq!(registration.relays.len(), 1);
    }

    #[test]
    fn tc_pool_013_onchain_query_parses_sps_pool_params_shape() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-3",
            "10.0.0.12",
            22,
            "root",
            "relay",
            Some("SHA256:relay3"),
        )
        .expect("insert machine");

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if remote_cmd.contains("query pool-state")
                && remote_cmd.contains("95e81a695eb880668a5c5a73311e208782a02efdad9905c6f079d458")
            {
                return Ok(
                    r#"{
                        "95e81a695eb880668a5c5a73311e208782a02efdad9905c6f079d458": {
                            "futurePoolParams": null,
                            "poolParams": {
                                "spsCost": 170000000,
                                "spsDeposit": 500000000,
                                "spsMargin": 3.0e-2,
                                "spsMetadata": {
                                    "hash": "0ad6ff2157b5d574095f7286cf1ff93ed263a75519860e5779a97f356a4df443",
                                    "url": "https://www.bubble-studio.xyz/md.json"
                                },
                                "spsOwners": [
                                    "8ccea9173f172483703057243e4abf4eca825a89c389e5636428993e"
                                ],
                                "spsPledge": 500000000,
                                "spsRelays": [
                                    {
                                        "single host name": {
                                            "dnsName": "pool-relay-1.bubble-studio.xyz",
                                            "port": 3001
                                        }
                                    }
                                ],
                                "spsRewardAccount": {
                                    "credential": {
                                        "keyHash": "8ccea9173f172483703057243e4abf4eca825a89c389e5636428993e"
                                    },
                                    "network": "Mainnet"
                                },
                                "spsVrf": "04da16f2ed3fd5bae9b45fa6bbcbf7ad59b99de172382480af4fff4229bab903"
                            },
                            "retiring": null
                        }
                    }"#
                    .into(),
                );
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
        };

        let status = pool_onchain_status_with_conn_and_ssh(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: Some("95e81a695eb880668a5c5a73311e208782a02efdad9905c6f079d458".into()),
                cold_vkey_path: None,
            },
            &ssh,
        )
        .expect("query onchain");

        assert!(status.registered_onchain);
        let registration = status.registration.expect("registration");
        assert_eq!(registration.margin, Some(0.03));
        assert_eq!(registration.fixed_cost, Some(170000000));
        assert_eq!(registration.pledge, Some(500000000));
        assert_eq!(
            registration.reward_account.as_deref(),
            Some("8ccea9173f172483703057243e4abf4eca825a89c389e5636428993e")
        );
        assert_eq!(registration.owners.len(), 1);
        assert_eq!(registration.relays.len(), 1);
        assert_eq!(
            registration.relays[0],
            PoolOnchainRelay {
                address: "pool-relay-1.bubble-studio.xyz".into(),
                port: 3001
            }
        );
        assert_eq!(
            registration.metadata_url.as_deref(),
            Some("https://www.bubble-studio.xyz/md.json")
        );
        assert_eq!(
            registration.metadata_hash.as_deref(),
            Some("0ad6ff2157b5d574095f7286cf1ff93ed263a75519860e5779a97f356a4df443")
        );
    }

    #[test]
    fn tc_pool_014_onchain_query_falls_back_from_bp_to_relay_when_era_query_is_unsupported() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let bp_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "bp-1",
            "10.0.0.20",
            22,
            "root",
            "bp",
            Some("SHA256:bp"),
        )
        .expect("insert bp");
        let _relay_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-1",
            "10.0.0.10",
            22,
            "root",
            "relay",
            Some("SHA256:relay"),
        )
        .expect("insert relay");

        let ssh = |_: &str, ip: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if ip == "10.0.0.20"
                && (remote_cmd.contains("query pool-state")
                    || remote_cmd.contains("query pool-params"))
            {
                return Err(AppError::Internal(
                    "ssh command failed: Command failed: query pool-state Error: This query is not supported in the era: Babbage.".into(),
                ));
            }
            if ip == "10.0.0.10" && remote_cmd.contains("query pool-state") {
                return Ok(r#"{
                        "pool1fallback": {
                            "poolParams": {
                                "spsCost": 170000000,
                                "spsMargin": 3.0e-2,
                                "spsPledge": 500000000,
                                "spsOwners": ["owner1"],
                                "spsRelays": [
                                    {
                                        "single host name": {
                                            "dnsName": "pool-relay-1.bubble-studio.xyz",
                                            "port": 3001
                                        }
                                    }
                                ],
                                "spsMetadata": {
                                    "hash": "abcd",
                                    "url": "https://example.com/md.json"
                                }
                            }
                        }
                    }"#
                .into());
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {ip} {remote_cmd}"
            )))
        };

        let status = pool_onchain_status_with_conn_and_ssh(
            &conn,
            PoolOnchainQueryPayload {
                machine_id: bp_id,
                pool_id: Some("pool1fallback".into()),
                cold_vkey_path: None,
            },
            &ssh,
        )
        .expect("query onchain");

        assert!(status.registered_onchain);
        assert!(status.note.contains("fell back to relay relay-1"));
        let registration = status.registration.expect("registration");
        assert_eq!(registration.fixed_cost, Some(170000000));
        assert_eq!(registration.margin, Some(0.03));
        assert_eq!(registration.pledge, Some(500000000));
    }

    #[test]
    fn tc_pool_015_onchain_query_reads_ticker_from_metadata_url() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-4",
            "10.0.0.13",
            22,
            "root",
            "relay",
            Some("SHA256:relay4"),
        )
        .expect("insert machine");

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if remote_cmd.contains("query pool-state") && remote_cmd.contains("pool1meta") {
                return Ok(
                    r#"{
                        "pool1meta": {
                            "poolParams": {
                                "spsCost": 170000000,
                                "spsMargin": 3.0e-2,
                                "spsPledge": 500000000,
                                "spsOwners": ["owner1"],
                                "spsRelays": [
                                    {
                                        "single host name": {
                                            "dnsName": "pool-relay-1.bubble-studio.xyz",
                                            "port": 3001
                                        }
                                    }
                                ],
                                "spsMetadata": {
                                    "hash": "0ad6ff2157b5d574095f7286cf1ff93ed263a75519860e5779a97f356a4df443",
                                    "url": "https://www.bubble-studio.xyz/md.json"
                                }
                            }
                        }
                    }"#
                    .into(),
                );
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
        };
        let fetch_ticker = |url: &str| -> Result<Option<String>, AppError> {
            assert_eq!(url, "https://www.bubble-studio.xyz/md.json");
            Ok(Some("BUB".into()))
        };

        let status = pool_onchain_status_with_conn_ssh_and_metadata(
            &conn,
            PoolOnchainQueryPayload {
                machine_id,
                pool_id: Some("pool1meta".into()),
                cold_vkey_path: None,
            },
            &ssh,
            &fetch_ticker,
        )
        .expect("query onchain");

        assert!(status.registered_onchain);
        let registration = status.registration.expect("registration");
        assert_eq!(registration.ticker.as_deref(), Some("BUB"));
        assert_eq!(
            registration.metadata_url.as_deref(),
            Some("https://www.bubble-studio.xyz/md.json")
        );
    }

    #[test]
    fn tc_pool_016_unbind_clears_onchain_binding_fields() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let _ = pool_init_with_conn(
            &conn,
            PoolInitPayload {
                ticker: "BUBL".into(),
                network: "mainnet".into(),
                margin: Some(0.03),
                fixed_cost: Some(170000000),
            },
        )
        .expect("pool init");
        let _ = pool_bind_onchain_single(
            &conn,
            PoolOnchainBindingUpdate {
                pool_id: "pool1test",
                ticker: Some("BUBL"),
                margin: Some(0.03),
                fixed_cost: Some(170000000),
                pledge: Some(500000000),
                reward_account: Some("stake1test"),
                metadata_url: Some("https://example.com/md.json"),
                metadata_hash: Some("abc"),
                owners_json: "[\"owner1\"]",
                relays_json: "[{\"address\":\"relay.example.com\",\"port\":3001}]",
            },
        )
        .expect("pool bind");

        let pool = pool_unbind_onchain_with_conn(&conn).expect("pool unbind");

        assert_eq!(pool.onchain_pool_id, None);
        assert!(!pool.onchain_registered);
        assert_eq!(pool.pledge, None);
        assert_eq!(pool.reward_account, None);
        assert_eq!(pool.metadata_url, None);
        assert_eq!(pool.metadata_hash, None);
        assert!(pool.owners.is_empty());
        assert!(pool.relays.is_empty());
        assert_eq!(pool.onchain_synced_at, None);

        let count_audit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'pool_unbind_onchain'",
                [],
                |r| r.get(0),
            )
            .expect("count audit");
        assert_eq!(count_audit, 1);
    }

    #[test]
    fn tc_pool_017_registration_prepare_reports_missing_inputs_and_signers() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "bp-prepare",
            "10.0.0.30",
            22,
            "root",
            "bp",
            Some("SHA256:bp-prepare"),
        )
        .expect("insert bp");

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if remote_cmd.contains("query protocol-parameters") {
                return Ok(r#"{"stakePoolDeposit":500000000}"#.into());
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
        };

        let result = pool_registration_prepare_with_conn_and_ssh(
            &conn,
            PoolRegistrationPreparePayload {
                machine_id,
                pool_id: "pool1prepare".into(),
                certificate_path: "/offline/pool-registration.cert".into(),
                payment_addr_path: "/opt/cardano/keys/payment.addr".into(),
            },
            &ssh,
        )
        .expect("prepare result");

        assert_eq!(result.pool_id.as_deref(), Some("pool1prepare"));
        assert!(!result.certificate_generated);
        assert!(result.certificate_path.is_none());
        assert!(result
            .missing_requirements
            .iter()
            .any(|entry| entry.contains("certificate_path not accessible")));
        assert!(result
            .missing_requirements
            .iter()
            .any(|entry| entry.contains("payment_addr_path not accessible")));
        assert_eq!(result.tx_draft.required_deposit, Some(500000000));
        assert!(result.tx_draft.offline_signing_required);
    }

    #[test]
    fn tc_pool_018_registration_prepare_generates_certificate_and_draft() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let relay_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-prepare",
            "10.0.0.31",
            22,
            "root",
            "relay",
            Some("SHA256:relay-prepare"),
        )
        .expect("insert relay");
        crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-secondary",
            "10.0.0.32",
            22,
            "root",
            "relay",
            Some("SHA256:relay-secondary"),
        )
        .expect("insert second relay");

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if (remote_cmd.contains("test -f")
                && remote_cmd
                    .contains("/opt/cardano/config/registration-drafts/pool-registration.cert"))
                || (remote_cmd.contains("test -f")
                    && remote_cmd.contains("/opt/cardano/config/keys/payment.addr"))
            {
                return Ok("found".into());
            }
            if remote_cmd.contains("cat")
                && remote_cmd.contains("/opt/cardano/config/keys/payment.addr")
            {
                return Ok("addr_test1qpzexample".into());
            }
            if remote_cmd.contains("query protocol-parameters") {
                return Ok(r#"{"stakePoolDeposit":500000000}"#.into());
            }
            if remote_cmd.contains("query utxo")
                && remote_cmd.contains("addr_test1qpzexample")
                && remote_cmd.contains("--out-file /dev/stdout")
            {
                return Ok(
                    r#"{"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb#0":{"value":{"lovelace":5000000000}}}"#
                        .into(),
                );
            }
            if remote_cmd.contains("transaction build")
                && remote_cmd.contains("pool-registration.cert")
                && remote_cmd.contains("pool-registration.raw")
            {
                return Ok(String::new());
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
        };

        let result = pool_registration_prepare_with_conn_and_ssh(
            &conn,
            PoolRegistrationPreparePayload {
                machine_id: relay_id,
                pool_id: "pool1certificate".into(),
                certificate_path: "/opt/cardano/config/registration-drafts/pool-registration.cert"
                    .into(),
                payment_addr_path: "/opt/cardano/keys/payment.addr".into(),
            },
            &ssh,
        )
        .expect("prepare result");

        assert!(result.certificate_generated);
        assert_eq!(result.pool_id.as_deref(), Some("pool1certificate"));
        assert!(result.missing_requirements.is_empty());
        assert_eq!(result.tx_draft.required_deposit, Some(500000000));
        assert_eq!(
            result.tx_draft.payment_address.as_deref(),
            Some("addr_test1qpzexample")
        );
        assert_eq!(
            result
                .registration_relays
                .iter()
                .map(|relay| relay.address.as_str())
                .collect::<Vec<_>>(),
            vec!["10.0.0.31", "10.0.0.32"]
        );
        assert!(result
            .certificate_path
            .as_deref()
            .unwrap_or_default()
            .contains("pool-registration.cert"));
        assert!(result
            .tx_draft
            .tx_body_path
            .as_deref()
            .unwrap_or_default()
            .ends_with("pool-registration.raw"));
        assert!(result.tx_draft.offline_signing_required);
        assert!(result
            .tx_draft
            .command_preview
            .contains("cardano-cli latest transaction build"));
    }

    #[test]
    fn tc_pool_019_registration_submit_requires_matching_confirmation() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-submit",
            "10.0.0.40",
            22,
            "root",
            "relay",
            Some("SHA256:relay-submit"),
        )
        .expect("insert relay");

        let err = pool_registration_submit_with_conn_and_ssh(
            &conn,
            PoolRegistrationSubmitPayload {
                machine_id,
                pool_id: "pool1submit".into(),
                confirm_pool_id: "pool1different".into(),
                tx_signed_path: "/opt/cardano/config/registration-drafts/pool-registration.signed"
                    .into(),
            },
            &|_, _, _, remote_cmd| {
                Err(AppError::Internal(format!(
                    "unexpected ssh command: {remote_cmd}"
                )))
            },
        )
        .expect_err("confirmation mismatch should fail");

        assert!(err
            .to_string()
            .contains("registration confirmation mismatch"));
    }

    #[test]
    fn tc_pool_020_registration_submit_builds_signs_submits_and_audits() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        let pool_id = pool_insert(&conn, "OURO", "mainnet", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = crate::db::machine_insert(
            &conn,
            pool_id,
            "relay-submit",
            "10.0.0.41",
            22,
            "root",
            "relay",
            Some("SHA256:relay-submit"),
        )
        .expect("insert relay");

        let ssh = |_: &str, _: &str, _: i64, remote_cmd: &str| -> Result<String, AppError> {
            if remote_cmd.contains("test -f") && remote_cmd.contains("pool-registration.signed") {
                return Ok("found".into());
            }
            if remote_cmd.contains("transaction txid")
                && remote_cmd.contains("pool-registration.signed")
            {
                return Ok("txhashsubmit123".into());
            }
            if remote_cmd.contains("transaction submit")
                && remote_cmd.contains("pool-registration.signed")
            {
                return Ok(String::new());
            }
            Err(AppError::Internal(format!(
                "unexpected ssh command: {remote_cmd}"
            )))
        };

        let result = pool_registration_submit_with_conn_and_ssh(
            &conn,
            PoolRegistrationSubmitPayload {
                machine_id,
                pool_id: "pool1submit".into(),
                confirm_pool_id: "pool1submit".into(),
                tx_signed_path: "/opt/cardano/config/registration-drafts/pool-registration.signed"
                    .into(),
            },
            &ssh,
        )
        .expect("submit result");

        assert!(result.submitted);
        assert_eq!(result.pool_id, "pool1submit");
        assert_eq!(result.machine_id, machine_id);
        assert_eq!(result.tx_hash.as_deref(), Some("txhashsubmit123"));
        assert!(result.tx_inputs.is_empty());
        assert!(result.missing_requirements.is_empty());
        assert!(result
            .tx_body_path
            .as_deref()
            .unwrap_or_default()
            .ends_with("pool-registration.raw"));
        assert!(result
            .tx_signed_path
            .as_deref()
            .unwrap_or_default()
            .ends_with("pool-registration.signed"));

        let count_audit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'pool_registration_submit'",
                [],
                |r| r.get(0),
            )
            .expect("count audit");
        assert_eq!(count_audit, 1);
    }
}
