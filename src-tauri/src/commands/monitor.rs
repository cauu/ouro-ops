use std::collections::HashMap;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{
    app_config_get, audit_log_insert, machine_list as repo_machine_list, DbState, MachineRow,
};
use crate::error::AppError;

pub struct MonitorPollingState(pub Mutex<Option<MonitorPollingHandle>>);

pub struct MonitorPollingHandle {
    stop: Arc<AtomicBool>,
    join: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MonitorSnapshot {
    pub machine_id: i64,
    pub machine_name: String,
    pub role: String,
    pub network: String,
    pub block_height: Option<i64>,
    pub sync_progress: Option<f64>,
    pub blocks_per_minute: Option<f64>,
    pub health_level: String,
    pub status: String,
    pub sync_stage: String,
    pub restore_snapshot_requested: bool,
    pub stalled: bool,
    pub collected_at: String,
    pub note: Option<String>,
    pub epoch: Option<i64>,
    pub slot_num: Option<i64>,
    pub slot_in_epoch: Option<i64>,
    pub sync_percent: Option<f64>,
    pub tip_diff_blocks: Option<i64>,
    pub late_blocks: Option<i64>,
    pub peer_count: Option<i64>,
    pub cpu_sys_percent: Option<f64>,
    pub mem_live_bytes: Option<f64>,
    pub mem_rss_bytes: Option<f64>,
    pub mem_heap_bytes: Option<f64>,
    pub gc_minor_total: Option<i64>,
    pub gc_major_total: Option<i64>,
    pub txs_in_mempool: Option<i64>,
    pub mempool_bytes: Option<f64>,
    pub forks: Option<i64>,
    pub forging_enabled: Option<i64>,
    pub prometheus_source: Option<String>,
    pub prometheus_note: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MonitorPollingStatus {
    pub running: bool,
    pub interval_seconds: u64,
}

#[derive(Debug)]
struct PreviousHealthSample {
    block_height: Option<i64>,
    sync_stage: Option<String>,
    collected_at_epoch: i64,
}

#[derive(Debug, Clone)]
struct RuntimeMonitorContext {
    restore_snapshot_requested: bool,
    protocol_magic_id_present: bool,
    recent_logs: String,
}

#[derive(Debug, Clone, Default)]
struct PrometheusMetrics {
    block_height: Option<i64>,
    epoch: Option<i64>,
    slot_num: Option<i64>,
    slot_in_epoch: Option<i64>,
    sync_percent: Option<f64>,
    tip_diff_blocks: Option<i64>,
    late_blocks: Option<i64>,
    peer_count: Option<i64>,
    cpu_sys_percent: Option<f64>,
    mem_live_bytes: Option<f64>,
    mem_rss_bytes: Option<f64>,
    mem_heap_bytes: Option<f64>,
    gc_minor_total: Option<i64>,
    gc_major_total: Option<i64>,
    txs_in_mempool: Option<i64>,
    mempool_bytes: Option<f64>,
    forks: Option<i64>,
    forging_enabled: Option<i64>,
    collected_at_epoch: Option<i64>,
    source: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Clone)]
struct RelayTelemetryConfig {
    username: String,
    password: String,
    scheme: String,
    port: u16,
    timeout_seconds: u64,
    insecure_tls: bool,
    backoff_seconds: i64,
}

#[derive(Debug, Clone)]
struct RelayMetricSample {
    metric_name: Option<String>,
    node: Option<String>,
    role: Option<String>,
    host_ip: Option<String>,
    instance: Option<String>,
    timestamp: Option<i64>,
    value: f64,
}

#[derive(Debug, Clone)]
struct RelayMetricsAttempt {
    metrics: PrometheusMetrics,
    latest_timestamp: Option<i64>,
    endpoint_errors: usize,
}

const RELAY_TELEMETRY_RAW_ENDPOINT: &str = "raw";
const RELAY_TELEMETRY_CFG_USERNAME: &str = "relay.telemetry.username";
const RELAY_TELEMETRY_CFG_PASSWORD: &str = "relay.telemetry.password";
const RELAY_TELEMETRY_CFG_SCHEME: &str = "relay.telemetry.scheme";
const RELAY_TELEMETRY_CFG_PORT: &str = "relay.telemetry.port";
const RELAY_TELEMETRY_CFG_INSECURE: &str = "relay.telemetry.insecure";
const RELAY_TELEMETRY_CFG_TIMEOUT_SECONDS: &str = "relay.telemetry.timeout_seconds";
const RELAY_TELEMETRY_CFG_BACKOFF_SECONDS: &str = "relay.telemetry.backoff_seconds";
const TELEMETRY_STALE_THRESHOLD_SECONDS: i64 = 120;
const RELAY_TELEMETRY_FIELD_METRICS: [(&str, &[&str]); 18] = [
    (
        "block_height",
        &[
            "cardano_node_metrics_blockNum_int",
            "cardano_node_metrics_blockNum",
        ],
    ),
    ("epoch", &["cardano_node_metrics_epoch_int"]),
    ("slot_num", &["cardano_node_metrics_slotNum_int"]),
    ("slot_in_epoch", &["cardano_node_metrics_slotInEpoch_int"]),
    ("sync_percent", &["cardano_node_metrics_syncProgress"]),
    (
        "tip_diff_blocks",
        &["cardano_node_metrics_chainDensityTipDiff_int"],
    ),
    ("late_blocks", &["cardano_node_metrics_blockfetchclient_lateblocks"]),
    (
        "peer_count",
        &[
            "cardano_node_metrics_connectedPeers_int",
            "cardano_node_metrics_connectionManager_duplexConns",
            "cardano_node_metrics_peerSelection_EstablishedPeers",
        ],
    ),
    (
        "cpu_sys_percent",
        &["cardano_node_resources_cpuSys_percent"],
    ),
    (
        "mem_live_bytes",
        &[
            "cardano_node_resources_memLive_bytes",
            "cardano_node_metrics_RTS_gcLiveBytes_int",
            "rts_gc_current_bytes_used",
        ],
    ),
    (
        "mem_rss_bytes",
        &[
            "cardano_node_resources_memRss_bytes",
            "cardano_node_metrics_Mem_resident_int",
        ],
    ),
    (
        "mem_heap_bytes",
        &[
            "cardano_node_resources_memHeap_bytes",
            "cardano_node_metrics_RTS_gcHeapBytes_int",
        ],
    ),
    (
        "gc_minor_total",
        &[
            "rts_gc_minor_num_gcs",
            "cardano_node_metrics_RTS_gcMinorNum_int",
        ],
    ),
    (
        "gc_major_total",
        &[
            "rts_gc_major_num_gcs",
            "cardano_node_metrics_RTS_gcMajorNum_int",
        ],
    ),
    ("txs_in_mempool", &["cardano_node_metrics_txsInMempool_int"]),
    ("mempool_bytes", &["cardano_node_metrics_mempoolBytes_int"]),
    ("forks", &["cardano_node_metrics_forks_int"]),
    ("forging_enabled", &["cardano_node_metrics_forging_enabled"]),
];

impl PrometheusMetrics {
    fn has_any_value(&self) -> bool {
        self.block_height.is_some()
            || self.epoch.is_some()
            || self.slot_num.is_some()
            || self.slot_in_epoch.is_some()
            || self.sync_percent.is_some()
            || self.tip_diff_blocks.is_some()
            || self.late_blocks.is_some()
            || self.peer_count.is_some()
            || self.cpu_sys_percent.is_some()
            || self.mem_live_bytes.is_some()
            || self.mem_rss_bytes.is_some()
            || self.mem_heap_bytes.is_some()
            || self.gc_minor_total.is_some()
            || self.gc_major_total.is_some()
            || self.txs_in_mempool.is_some()
            || self.mempool_bytes.is_some()
            || self.forks.is_some()
            || self.forging_enabled.is_some()
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

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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

fn ssh_exec(machine: &MachineRow, remote_cmd: &str) -> Result<String, AppError> {
    let target = format!("{}@{}", machine.ssh_user, machine.ip);
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
        .arg(machine.ssh_port.to_string())
        .arg(target.as_str())
        .arg(wrap_remote_command(remote_cmd))
        .output()?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(classify_ssh_error(
        format!("{}@{}:{}", machine.ssh_user, machine.ip, machine.ssh_port).as_str(),
        stderr.as_str(),
    ))
}

fn parse_sync_progress(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::String(raw)) => raw.trim().parse::<f64>().ok(),
        Some(Value::Number(raw)) => raw.as_f64(),
        _ => None,
    }
}

fn parse_prometheus_line(raw: &str) -> Option<(String, f64)> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split_whitespace();
    let raw_name = parts.next()?;
    let raw_value = parts.next()?;
    let value = raw_value.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    let name = raw_name
        .split_once('{')
        .map(|(prefix, _)| prefix)
        .unwrap_or(raw_name)
        .to_string();
    Some((name, value))
}

fn parse_prometheus_exposition(raw: &str) -> HashMap<String, f64> {
    let mut metrics = HashMap::new();
    for line in raw.lines() {
        if let Some((name, value)) = parse_prometheus_line(line) {
            metrics
                .entry(name)
                .and_modify(|previous| {
                    if value > *previous {
                        *previous = value;
                    }
                })
                .or_insert(value);
        }
    }
    metrics
}

fn pick_prometheus_value(metrics: &HashMap<String, f64>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| metrics.get(*key).copied())
}

fn pick_prometheus_i64(metrics: &HashMap<String, f64>, keys: &[&str]) -> Option<i64> {
    pick_prometheus_value(metrics, keys).map(|value| value.round() as i64)
}

fn normalize_percent(value: Option<f64>) -> Option<f64> {
    value.map(|raw| {
        if (0.0..=1.0).contains(&raw) {
            raw * 100.0
        } else {
            raw
        }
    })
}

fn map_prometheus_metrics(source: &str, metrics: &HashMap<String, f64>) -> PrometheusMetrics {
    PrometheusMetrics {
        block_height: pick_prometheus_i64(
            metrics,
            &[
                "nview_block_height",
                "cardano_node_metrics_blockNum_int",
                "cardano_node_metrics_blockNum",
            ],
        ),
        epoch: pick_prometheus_i64(
            metrics,
            &[
                "nview_epoch",
                "cardano_node_metrics_epoch_int",
                "cardano_node_metrics_epoch",
            ],
        ),
        slot_num: pick_prometheus_i64(
            metrics,
            &[
                "cardano_node_metrics_slotNum_int",
                "cardano_node_metrics_slotNum",
            ],
        ),
        slot_in_epoch: pick_prometheus_i64(
            metrics,
            &[
                "cardano_node_metrics_slotInEpoch_int",
                "cardano_node_metrics_slotInEpoch",
            ],
        ),
        sync_percent: normalize_percent(pick_prometheus_value(
            metrics,
            &[
                "nview_sync_percent",
                "cardano_node_metrics_syncPercent",
                "cardano_node_metrics_syncProgress",
                "cardano_node_metrics_syncProgress_int",
            ],
        )),
        tip_diff_blocks: pick_prometheus_i64(
            metrics,
            &[
                "nview_tip_diff_blocks",
                "cardano_node_metrics_tipDiff_int",
                "cardano_node_metrics_chainDensityTipDiff_int",
            ],
        ),
        late_blocks: pick_prometheus_i64(metrics, &["cardano_node_metrics_blockfetchclient_lateblocks"]),
        peer_count: pick_prometheus_i64(
            metrics,
            &[
                "nview_peer_count",
                "cardano_node_metrics_connectedPeers_int",
                "cardano_node_metrics_connectedPeers",
            ],
        ),
        cpu_sys_percent: normalize_percent(pick_prometheus_value(
            metrics,
            &[
                "nview_cpu_sys_percent",
                "cardano_node_resources_cpuSys_percent",
                "cardano_node_resources_cpuSys_int",
            ],
        )),
        mem_live_bytes: pick_prometheus_value(
            metrics,
            &[
                "nview_mem_live_bytes",
                "cardano_node_resources_memLive_bytes",
                "cardano_node_resources_rts_mem_live_bytes",
            ],
        ),
        mem_rss_bytes: pick_prometheus_value(
            metrics,
            &[
                "nview_mem_rss_bytes",
                "cardano_node_resources_memRss_bytes",
                "cardano_node_resources_rts_mem_rss_bytes",
                "process_resident_memory_bytes",
            ],
        ),
        mem_heap_bytes: pick_prometheus_value(
            metrics,
            &[
                "nview_mem_heap_bytes",
                "cardano_node_resources_memHeap_bytes",
                "cardano_node_resources_rts_mem_heap_bytes",
            ],
        ),
        gc_minor_total: pick_prometheus_i64(
            metrics,
            &[
                "nview_gc_minor_total",
                "cardano_node_resources_gc_minor_total",
                "rts_gc_minor_num",
                "rts_gc_minor_num_gcs",
            ],
        ),
        gc_major_total: pick_prometheus_i64(
            metrics,
            &[
                "nview_gc_major_total",
                "cardano_node_resources_gc_major_total",
                "rts_gc_major_num",
                "rts_gc_major_num_gcs",
            ],
        ),
        txs_in_mempool: pick_prometheus_i64(metrics, &["cardano_node_metrics_txsInMempool_int"]),
        mempool_bytes: pick_prometheus_value(metrics, &["cardano_node_metrics_mempoolBytes_int"]),
        forks: pick_prometheus_i64(metrics, &["cardano_node_metrics_forks_int"]),
        forging_enabled: pick_prometheus_i64(metrics, &["cardano_node_metrics_forging_enabled"]),
        collected_at_epoch: None,
        source: Some(source.to_string()),
        note: None,
    }
}

fn parse_env_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn relay_failure_backoff_registry() -> &'static Mutex<HashMap<String, i64>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn relay_backoff_key(relay: &MachineRow) -> String {
    format!("{}:{}:{}", relay.network, relay.name, relay.ip)
}

fn relay_backoff_retry_at(relay: &MachineRow) -> Option<i64> {
    relay_failure_backoff_registry()
        .lock()
        .ok()
        .and_then(|registry| registry.get(relay_backoff_key(relay).as_str()).copied())
}

fn relay_mark_backoff(relay: &MachineRow, now_epoch: i64, backoff_seconds: i64) {
    if let Ok(mut registry) = relay_failure_backoff_registry().lock() {
        registry.insert(relay_backoff_key(relay), now_epoch + backoff_seconds.max(1));
    }
}

fn relay_clear_backoff(relay: &MachineRow) {
    if let Ok(mut registry) = relay_failure_backoff_registry().lock() {
        registry.remove(relay_backoff_key(relay).as_str());
    }
}

fn relay_telemetry_config_from_env() -> Option<RelayTelemetryConfig> {
    let username = std::env::var("OURO_OPS_RELAY_TELEMETRY_USERNAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let password = std::env::var("OURO_OPS_RELAY_TELEMETRY_PASSWORD")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let scheme = std::env::var("OURO_OPS_RELAY_TELEMETRY_SCHEME")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| value == "http" || value == "https")
        .unwrap_or_else(|| "https".into());
    let port = std::env::var("OURO_OPS_RELAY_TELEMETRY_PORT")
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(443);
    let timeout_seconds = std::env::var("OURO_OPS_RELAY_TELEMETRY_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.clamp(1, 15))
        .unwrap_or(3);
    let insecure_tls = std::env::var("OURO_OPS_RELAY_TELEMETRY_INSECURE")
        .ok()
        .map(|value| parse_env_bool(value.as_str()))
        .unwrap_or(false);
    let backoff_seconds = std::env::var("OURO_OPS_RELAY_TELEMETRY_BACKOFF_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .map(|seconds| seconds.clamp(5, 300))
        .unwrap_or(30);

    Some(RelayTelemetryConfig {
        username,
        password,
        scheme,
        port,
        timeout_seconds,
        insecure_tls,
        backoff_seconds,
    })
}

fn relay_telemetry_config_from_app_config(conn: &Connection) -> Option<RelayTelemetryConfig> {
    let username = app_config_get(conn, RELAY_TELEMETRY_CFG_USERNAME)
        .ok()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let password = app_config_get(conn, RELAY_TELEMETRY_CFG_PASSWORD)
        .ok()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let scheme = app_config_get(conn, RELAY_TELEMETRY_CFG_SCHEME)
        .ok()
        .flatten()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| value == "http" || value == "https")
        .unwrap_or_else(|| "https".into());
    let port = app_config_get(conn, RELAY_TELEMETRY_CFG_PORT)
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(443);
    let timeout_seconds = app_config_get(conn, RELAY_TELEMETRY_CFG_TIMEOUT_SECONDS)
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.clamp(1, 15))
        .unwrap_or(3);
    let insecure_tls = app_config_get(conn, RELAY_TELEMETRY_CFG_INSECURE)
        .ok()
        .flatten()
        .map(|value| parse_env_bool(value.as_str()))
        .unwrap_or(true);
    let backoff_seconds = app_config_get(conn, RELAY_TELEMETRY_CFG_BACKOFF_SECONDS)
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .map(|seconds| seconds.clamp(5, 300))
        .unwrap_or(30);

    Some(RelayTelemetryConfig {
        username,
        password,
        scheme,
        port,
        timeout_seconds,
        insecure_tls,
        backoff_seconds,
    })
}

fn relay_telemetry_config(conn: &Connection) -> Option<RelayTelemetryConfig> {
    relay_telemetry_config_from_env().or_else(|| relay_telemetry_config_from_app_config(conn))
}

fn parse_metric_sample_timestamp(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(raw)) => raw.as_f64().map(|v| v.floor() as i64),
        Some(Value::String(raw)) => raw.trim().parse::<f64>().ok().map(|v| v.floor() as i64),
        _ => None,
    }
}

fn parse_metric_sample_value(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(raw)) => raw.as_f64(),
        Some(Value::String(raw)) => raw.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn parse_relay_metric_samples(payload: &Value) -> Result<Vec<RelayMetricSample>, AppError> {
    if let Some(series) = payload.get("series").and_then(Value::as_array) {
        let series_metric_name = payload
            .get("metric")
            .and_then(Value::as_str)
            .map(|raw| raw.to_string());
        let samples = series
            .iter()
            .filter_map(|entry| {
                let value = parse_metric_sample_value(entry.get("value"))?;
                Some(RelayMetricSample {
                    metric_name: entry
                        .get("metric_name")
                        .and_then(Value::as_str)
                        .map(|raw| raw.to_string())
                        .or_else(|| series_metric_name.clone()),
                    node: entry
                        .get("node")
                        .and_then(Value::as_str)
                        .map(|raw| raw.to_string()),
                    role: entry
                        .get("role")
                        .and_then(Value::as_str)
                        .map(|raw| raw.to_string()),
                    host_ip: entry
                        .get("host_ip")
                        .and_then(Value::as_str)
                        .map(|raw| raw.to_string()),
                    instance: entry
                        .get("instance")
                        .and_then(Value::as_str)
                        .map(|raw| raw.to_string()),
                    timestamp: parse_metric_sample_timestamp(entry.get("timestamp")),
                    value,
                })
            })
            .collect::<Vec<_>>();
        return Ok(samples);
    }

    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if status != "success" {
        return Err(AppError::Internal(format!(
            "relay api returned non-success status: {status}"
        )));
    }

    let result = payload
        .get("data")
        .and_then(|data| data.get("result"))
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Internal("relay api response missing data.result".into()))?;
    let samples = result
        .iter()
        .filter_map(|entry| {
            let metric = entry.get("metric").and_then(Value::as_object)?;
            let value_vector = entry.get("value").and_then(Value::as_array)?;
            let timestamp = parse_metric_sample_timestamp(value_vector.first());
            let value = parse_metric_sample_value(value_vector.get(1))?;
            Some(RelayMetricSample {
                metric_name: metric
                    .get("__name__")
                    .and_then(Value::as_str)
                    .map(|raw| raw.to_string()),
                node: metric
                    .get("node")
                    .and_then(Value::as_str)
                    .map(|raw| raw.to_string()),
                role: metric
                    .get("role")
                    .and_then(Value::as_str)
                    .map(|raw| raw.to_string()),
                host_ip: metric
                    .get("host_ip")
                    .and_then(Value::as_str)
                    .map(|raw| raw.to_string()),
                instance: metric
                    .get("instance")
                    .and_then(Value::as_str)
                    .map(|raw| raw.to_string()),
                timestamp,
                value,
            })
        })
        .collect::<Vec<_>>();
    Ok(samples)
}

fn relay_metrics_url(config: &RelayTelemetryConfig, relay: &MachineRow, endpoint: &str) -> String {
    format!(
        "{}://{}:{}/api/ops/v1/telemetry/{}",
        config.scheme, relay.ip, config.port, endpoint
    )
}

fn fetch_relay_metric_samples(
    config: &RelayTelemetryConfig,
    relay: &MachineRow,
    endpoint: &str,
) -> Result<Vec<RelayMetricSample>, AppError> {
    let url = relay_metrics_url(config, relay, endpoint);
    let mut command = Command::new("curl");
    command
        .arg("-fsS")
        .arg("--max-time")
        .arg(config.timeout_seconds.to_string())
        .arg("-u")
        .arg(format!("{}:{}", config.username, config.password));
    if config.insecure_tls {
        command.arg("-k");
    }
    let output = command.arg(url.as_str()).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Internal(format!(
            "relay api request failed ({endpoint}) on {}: {}",
            relay.name,
            if stderr.is_empty() {
                format!("curl exit {}", output.status)
            } else {
                stderr
            }
        )));
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Err(AppError::Internal(format!(
            "relay api request returned empty response ({endpoint}) on {}",
            relay.name
        )));
    }
    let payload: Value = serde_json::from_str(raw.as_str())
        .map_err(|err| AppError::Internal(format!("relay api invalid json: {err}")))?;
    parse_relay_metric_samples(&payload)
}

fn select_relay_metric_sample<'a>(
    machine: &MachineRow,
    samples: &'a [RelayMetricSample],
    metric_names: &[&str],
) -> Option<&'a RelayMetricSample> {
    let mut selected: Option<&RelayMetricSample> = None;
    let mut selected_score = 0_u8;
    let mut selected_timestamp = i64::MIN;

    for sample in samples {
        if !metric_names.is_empty() {
            let metric_matches = sample
                .metric_name
                .as_deref()
                .map(|metric_name| {
                    metric_names
                        .iter()
                        .any(|candidate| metric_name == *candidate)
                })
                .unwrap_or(false);
            if !metric_matches {
                continue;
            }
        }

        let role_matches = sample
            .role
            .as_deref()
            .map(|role| role.eq_ignore_ascii_case(machine.role.as_str()))
            .unwrap_or(true);
        if !role_matches {
            continue;
        }
        let node_matches = sample
            .node
            .as_deref()
            .map(|node| node.eq_ignore_ascii_case(machine.name.as_str()))
            .unwrap_or(false);
        let instance_matches = sample
            .instance
            .as_deref()
            .map(|instance| instance.contains(machine.ip.as_str()))
            .unwrap_or(false);
        let host_ip_matches = sample
            .host_ip
            .as_deref()
            .map(|host_ip| host_ip == machine.ip.as_str())
            .unwrap_or(false);

        let score = if node_matches {
            3
        } else if host_ip_matches {
            2
        } else if instance_matches {
            1
        } else {
            0
        };
        if score == 0 {
            continue;
        }
        let timestamp = sample.timestamp.unwrap_or_default();
        if score > selected_score || (score == selected_score && timestamp >= selected_timestamp) {
            selected = Some(sample);
            selected_score = score;
            selected_timestamp = timestamp;
        }
    }
    if selected.is_some() {
        return selected;
    }

    let mut role_only: Vec<&RelayMetricSample> = samples
        .iter()
        .filter(|sample| {
            let metric_matches = sample
                .metric_name
                .as_deref()
                .map(|metric_name| {
                    metric_names
                        .iter()
                        .any(|candidate| metric_name == *candidate)
                })
                .unwrap_or(false);
            let role_matches = sample
                .role
                .as_deref()
                .map(|role| role.eq_ignore_ascii_case(machine.role.as_str()))
                .unwrap_or(false);
            metric_matches && role_matches
        })
        .collect();
    if role_only.len() == 1 {
        return role_only.pop();
    }
    None
}

fn select_unlabeled_local_relay_sample<'a>(
    machine: &MachineRow,
    relay: &MachineRow,
    samples: &'a [RelayMetricSample],
    metric_names: &[&str],
) -> Option<&'a RelayMetricSample> {
    if machine.role != "relay" || machine.ip != relay.ip {
        return None;
    }
    let mut candidate: Option<&RelayMetricSample> = None;
    let mut latest_timestamp = i64::MIN;
    for sample in samples {
        let metric_matches = sample
            .metric_name
            .as_deref()
            .map(|metric_name| {
                metric_names
                    .iter()
                    .any(|candidate_name| metric_name == *candidate_name)
            })
            .unwrap_or(false);
        let role_matches = sample
            .role
            .as_deref()
            .map(|role| role.eq_ignore_ascii_case("relay"))
            .unwrap_or(false);
        if !metric_matches || !role_matches {
            continue;
        }
        let has_identity_labels = sample.node.is_some() || sample.host_ip.is_some();
        if has_identity_labels {
            continue;
        }
        let timestamp = sample.timestamp.unwrap_or_default();
        if timestamp >= latest_timestamp {
            candidate = Some(sample);
            latest_timestamp = timestamp;
        }
    }
    candidate
}

fn apply_relay_metric_value(
    mapped: &mut PrometheusMetrics,
    metric_key: &str,
    sample_value: f64,
    sample_timestamp: Option<i64>,
    latest_timestamp: &mut Option<i64>,
) {
    match metric_key {
        "block_height" => mapped.block_height = Some(sample_value.round() as i64),
        "epoch" => mapped.epoch = Some(sample_value.round() as i64),
        "slot_num" => mapped.slot_num = Some(sample_value.round() as i64),
        "slot_in_epoch" => mapped.slot_in_epoch = Some(sample_value.round() as i64),
        "sync_percent" => mapped.sync_percent = normalize_percent(Some(sample_value)),
        "tip_diff_blocks" => mapped.tip_diff_blocks = Some(sample_value.round() as i64),
        "late_blocks" => mapped.late_blocks = Some(sample_value.round() as i64),
        "peer_count" => mapped.peer_count = Some(sample_value.round() as i64),
        "cpu_sys_percent" => mapped.cpu_sys_percent = normalize_percent(Some(sample_value)),
        "mem_live_bytes" => mapped.mem_live_bytes = Some(sample_value),
        "mem_rss_bytes" => mapped.mem_rss_bytes = Some(sample_value),
        "mem_heap_bytes" => mapped.mem_heap_bytes = Some(sample_value),
        "gc_minor_total" => mapped.gc_minor_total = Some(sample_value.round() as i64),
        "gc_major_total" => mapped.gc_major_total = Some(sample_value.round() as i64),
        "txs_in_mempool" => mapped.txs_in_mempool = Some(sample_value.round() as i64),
        "mempool_bytes" => mapped.mempool_bytes = Some(sample_value),
        "forks" => mapped.forks = Some(sample_value.round() as i64),
        "forging_enabled" => mapped.forging_enabled = Some(sample_value.round() as i64),
        _ => {}
    }
    if let Some(timestamp) = sample_timestamp {
        *latest_timestamp = Some(
            latest_timestamp
                .map(|current| current.max(timestamp))
                .unwrap_or(timestamp),
        );
    }
}

fn collect_relay_metrics_from_single_relay(
    config: &RelayTelemetryConfig,
    relay: &MachineRow,
    machine: &MachineRow,
) -> RelayMetricsAttempt {
    let mut mapped = PrometheusMetrics {
        source: Some(format!("relay-api:{}@{}", relay.name, relay.ip)),
        note: None,
        ..PrometheusMetrics::default()
    };
    let mut latest_timestamp: Option<i64> = None;
    let mut errors: Vec<String> = Vec::new();
    let mut endpoint_errors = 0_usize;

    match fetch_relay_metric_samples(config, relay, RELAY_TELEMETRY_RAW_ENDPOINT) {
        Ok(samples) => {
            for (metric_key, metric_names) in RELAY_TELEMETRY_FIELD_METRICS {
                let selected =
                    select_relay_metric_sample(machine, &samples, metric_names).or_else(|| {
                        select_unlabeled_local_relay_sample(machine, relay, &samples, metric_names)
                    });
                if let Some(sample) = selected {
                    apply_relay_metric_value(
                        &mut mapped,
                        metric_key,
                        sample.value,
                        sample.timestamp,
                        &mut latest_timestamp,
                    );
                }
            }
        }
        Err(err) => {
            endpoint_errors = 1;
            errors.push(err.to_string());
        }
    }

    if mapped.has_any_value() {
        mapped.note = if errors.is_empty() {
            None
        } else {
            Some(format!(
                "relay api partial data on {}: raw endpoint degraded",
                relay.name,
            ))
        };
        mapped.collected_at_epoch = latest_timestamp;
    } else if errors.is_empty() {
        mapped.note = Some(format!(
            "relay api reachable on {} but no matching series for {}",
            relay.name, machine.name
        ));
    } else {
        mapped.note = Some(format!(
            "relay api unavailable on {}: {}",
            relay.name,
            errors
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown error".into())
        ));
    }

    RelayMetricsAttempt {
        metrics: mapped,
        latest_timestamp,
        endpoint_errors,
    }
}

fn collect_relay_prometheus_metrics(conn: &Connection, machine: &MachineRow) -> PrometheusMetrics {
    let Some(config) = relay_telemetry_config(conn) else {
        return PrometheusMetrics::default();
    };
    let mut relays = match repo_machine_list(conn, Some("relay"), Some(machine.network.as_str())) {
        Ok(machines) => machines,
        Err(err) => {
            return PrometheusMetrics {
                source: None,
                note: Some(format!("relay api disabled: failed to load relays: {err}")),
                ..PrometheusMetrics::default()
            };
        }
    };
    if relays.is_empty() {
        return PrometheusMetrics {
            source: None,
            note: Some("relay api disabled: no relay found in current network".into()),
            ..PrometheusMetrics::default()
        };
    }
    relays.sort_by_key(|relay| relay.sort_order);

    let now_epoch = current_epoch_seconds().unwrap_or_default();
    let mut active_relays: Vec<MachineRow> = Vec::new();
    let mut deferred_relays: Vec<(MachineRow, i64)> = Vec::new();
    for relay in relays {
        let retry_at = relay_backoff_retry_at(&relay).unwrap_or_default();
        if retry_at > now_epoch {
            deferred_relays.push((relay, retry_at));
        } else {
            active_relays.push(relay);
        }
    }
    deferred_relays.sort_by_key(|(_, retry_at)| *retry_at);

    let mut attempts = active_relays;
    if attempts.is_empty() {
        if let Some((relay, _)) = deferred_relays.first() {
            attempts.push(relay.clone());
        }
    }

    let mut best: Option<(PrometheusMetrics, i64)> = None;
    let mut degraded_notes: Vec<String> = Vec::new();

    for relay in attempts {
        let attempt = collect_relay_metrics_from_single_relay(&config, &relay, machine);
        if attempt.metrics.has_any_value() {
            relay_clear_backoff(&relay);
            let latest_timestamp = attempt.latest_timestamp.unwrap_or_default();
            match &best {
                Some((_, best_timestamp)) if latest_timestamp < *best_timestamp => {}
                _ => best = Some((attempt.metrics, latest_timestamp)),
            }
        } else {
            if attempt.endpoint_errors > 0 {
                relay_mark_backoff(&relay, now_epoch, config.backoff_seconds);
            }
            if let Some(note) = attempt.metrics.note {
                degraded_notes.push(note);
            }
        }
    }

    if let Some((mut metrics, _)) = best {
        if !degraded_notes.is_empty() {
            let failed_count = degraded_notes.len();
            metrics.note = Some(format!(
                "relay failover active: {} relay endpoint(s) degraded",
                failed_count
            ));
        }
        return metrics;
    }

    PrometheusMetrics {
        source: None,
        note: if degraded_notes.is_empty() {
            Some("relay api unavailable after failover attempts".into())
        } else {
            Some(degraded_notes.join(" | "))
        },
        ..PrometheusMetrics::default()
    }
}

fn collect_local_prometheus_metrics(machine: &MachineRow) -> PrometheusMetrics {
    let candidates: [(&str, &str); 4] = [
        (
            "nview:9090",
            "docker exec nview sh -lc 'wget -qO- http://127.0.0.1:9090/metrics 2>/dev/null || curl -fsS http://127.0.0.1:9090/metrics 2>/dev/null'",
        ),
        (
            "cardano-node:12798",
            "docker exec cardano-node sh -lc 'wget -qO- http://127.0.0.1:12798/metrics 2>/dev/null || curl -fsS http://127.0.0.1:12798/metrics 2>/dev/null'",
        ),
        (
            "host:12798",
            "sh -lc 'wget -qO- http://127.0.0.1:12798/metrics 2>/dev/null || curl -fsS http://127.0.0.1:12798/metrics 2>/dev/null'",
        ),
        (
            "host:12788",
            "sh -lc 'wget -qO- http://127.0.0.1:12788/metrics 2>/dev/null || curl -fsS http://127.0.0.1:12788/metrics 2>/dev/null'",
        ),
    ];
    let mut last_error: Option<String> = None;
    for (source, command) in candidates {
        match ssh_exec(machine, command) {
            Ok(raw) => {
                if raw.trim().is_empty() {
                    last_error = Some(format!("{source} returned empty metrics response"));
                    continue;
                }
                let metrics = parse_prometheus_exposition(raw.as_str());
                if metrics.is_empty() {
                    last_error = Some(format!("{source} returned non-parseable metrics response"));
                    continue;
                }
                let mut mapped = map_prometheus_metrics(source, &metrics);
                if mapped.has_any_value() {
                    return mapped;
                }
                mapped.note = Some(format!("{source} reachable but mapped fields are missing"));
                return mapped;
            }
            Err(err) => {
                last_error = Some(err.to_string());
            }
        }
    }
    PrometheusMetrics {
        source: None,
        note: last_error,
        ..PrometheusMetrics::default()
    }
}

fn collect_prometheus_metrics(conn: &Connection, machine: &MachineRow) -> PrometheusMetrics {
    let relay_metrics = collect_relay_prometheus_metrics(conn, machine);
    if relay_metrics.has_any_value() {
        return relay_metrics;
    }
    relay_metrics
}

fn parse_tip(raw: &str) -> Result<(Option<i64>, Option<f64>), AppError> {
    let tip: Value = serde_json::from_str(raw)
        .map_err(|err| AppError::Internal(format!("invalid tip json: {err}")))?;
    let block_height = tip.get("block").and_then(Value::as_i64);
    let sync_progress = parse_sync_progress(tip.get("syncProgress"));
    Ok((block_height, sync_progress))
}

fn parse_restore_snapshot_requested(raw: &str) -> bool {
    serde_json::from_str::<Vec<String>>(raw)
        .ok()
        .and_then(|envs| {
            envs.into_iter()
                .find(|entry| entry.starts_with("RESTORE_SNAPSHOT="))
                .map(|entry| entry.eq_ignore_ascii_case("RESTORE_SNAPSHOT=true"))
        })
        .unwrap_or(false)
}

fn parse_protocol_magic_id_present(raw: &str) -> bool {
    matches!(raw.trim(), "yes" | "true" | "1")
}

fn last_restore_related_log_line(raw: &str) -> Option<String> {
    raw.lines()
        .rev()
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("mithril") || lower.contains("snapshot") || lower.contains("restore")
        })
        .map(|line| line.trim().to_string())
}

fn has_restore_failure(raw: &str) -> bool {
    raw.lines().rev().any(|line| {
        let lower = line.to_ascii_lowercase();
        (lower.contains("mithril") || lower.contains("snapshot") || lower.contains("restore"))
            && (lower.contains("error") || lower.contains("failed") || lower.contains("exception"))
    })
}

fn has_restore_activity(raw: &str) -> bool {
    raw.lines().rev().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("mithril")
            || lower.contains("snapshot")
            || lower.contains("restore")
            || lower.contains("replayed block")
            || lower.contains("ledger state")
    })
}

fn parse_restore_progress_from_logs(raw: &str) -> Option<f64> {
    raw.lines().rev().find_map(|line| {
        let (_, tail) = line.split_once("Progress:")?;
        let value = tail.trim().trim_end_matches('%').trim();
        value.parse::<f64>().ok()
    })
}

fn current_epoch_seconds() -> Result<i64, AppError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| AppError::Internal(format!("time error: {err}")))?;
    Ok(now.as_secs() as i64)
}

fn sqlite_datetime(conn: &Connection, epoch_seconds: i64) -> Result<String, AppError> {
    conn.query_row(
        "SELECT datetime(?1, 'unixepoch')",
        params![epoch_seconds],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

fn resolve_snapshot_collected_at(
    conn: &Connection,
    fallback: &str,
    prometheus: &PrometheusMetrics,
) -> String {
    prometheus
        .collected_at_epoch
        .and_then(|epoch| sqlite_datetime(conn, epoch).ok())
        .unwrap_or_else(|| fallback.to_string())
}

fn get_previous_health_sample(
    conn: &Connection,
    machine_id: i64,
) -> Result<Option<PreviousHealthSample>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT block_height, sync_stage, CAST(strftime('%s', collected_at) AS INTEGER)
         FROM machine_health
         WHERE machine_id = ?1
         ORDER BY collected_at DESC, id DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![machine_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(PreviousHealthSample {
            block_height: row.get(0)?,
            sync_stage: row.get(1)?,
            collected_at_epoch: row.get(2)?,
        }))
    } else {
        Ok(None)
    }
}

fn insert_health_sample(
    conn: &Connection,
    machine_id: i64,
    block_height: Option<i64>,
    sync_progress: Option<f64>,
    sync_stage: &str,
    sync_note: Option<&str>,
    collected_at_epoch: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO machine_health (machine_id, block_height, sync_progress, sync_stage, sync_note, collected_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime(?6, 'unixepoch'))",
        params![machine_id, block_height, sync_progress, sync_stage, sync_note, collected_at_epoch],
    )?;
    Ok(())
}

fn compute_blocks_per_minute(
    previous: Option<&PreviousHealthSample>,
    current_block_height: Option<i64>,
    current_epoch_seconds: i64,
) -> Option<f64> {
    let previous = previous?;
    let previous_height = previous.block_height?;
    let current_height = current_block_height?;
    let elapsed_seconds = current_epoch_seconds - previous.collected_at_epoch;
    if elapsed_seconds <= 0 {
        return None;
    }
    Some(((current_height - previous_height) as f64) * 60.0 / (elapsed_seconds as f64))
}

fn determine_stalled(
    previous: Option<&PreviousHealthSample>,
    current_block_height: Option<i64>,
    current_sync_progress: Option<f64>,
    current_epoch_seconds: i64,
) -> bool {
    let Some(progress) = current_sync_progress else {
        return false;
    };
    if progress >= 100.0 {
        return false;
    }
    let Some(previous) = previous else {
        return false;
    };
    let Some(previous_height) = previous.block_height else {
        return false;
    };
    let Some(current_height) = current_block_height else {
        return false;
    };
    current_height == previous_height
        && (current_epoch_seconds - previous.collected_at_epoch) >= 300
}

fn determine_status(sync_progress: Option<f64>, stalled: bool, note: Option<&str>) -> String {
    if note.is_some() {
        return "unreachable".into();
    }
    if stalled {
        return "stalled".into();
    }
    match sync_progress {
        Some(progress) if progress >= 100.0 => "synced".into(),
        Some(_) => "syncing".into(),
        None => "unknown".into(),
    }
}

fn determine_health_level(
    status: &str,
    sync_stage: &str,
    blocks_per_minute: Option<f64>,
    stalled: bool,
) -> String {
    if stalled
        || status == "unreachable"
        || matches!(
            sync_stage,
            "restore_failed" | "restore_timeout" | "unreachable"
        )
    {
        return "critical".into();
    }

    if status == "synced" && sync_stage == "synced" {
        return "healthy".into();
    }

    match blocks_per_minute {
        Some(value) if value >= 30.0 && status == "syncing" && sync_stage == "syncing" => {
            "healthy".into()
        }
        Some(value) if value > 0.0 => "warning".into(),
        _ => "warning".into(),
    }
}

fn is_zero_progress(block_height: Option<i64>, sync_progress: Option<f64>) -> bool {
    block_height.unwrap_or_default() == 0 && sync_progress.unwrap_or_default() <= 0.0
}

fn determine_sync_stage(
    block_height: Option<i64>,
    sync_progress: Option<f64>,
    previous: Option<&PreviousHealthSample>,
    runtime: Option<&RuntimeMonitorContext>,
    note: Option<&str>,
    current_epoch_seconds: i64,
) -> (String, Option<String>) {
    if let Some(note) = note {
        return ("unreachable".into(), Some(note.to_string()));
    }

    if let Some(runtime) = runtime {
        if runtime.restore_snapshot_requested {
            let zero_progress = is_zero_progress(block_height, sync_progress);

            if has_restore_failure(runtime.recent_logs.as_str()) && !zero_progress {
                return (
                    "fallback_syncing".into(),
                    last_restore_related_log_line(runtime.recent_logs.as_str()).or_else(|| {
                        Some(
                            "Mithril restore failed earlier; node is continuing with regular sync."
                                .into(),
                        )
                    }),
                );
            }

            if has_restore_failure(runtime.recent_logs.as_str()) {
                return (
                    "restore_failed".into(),
                    last_restore_related_log_line(runtime.recent_logs.as_str())
                        .or_else(|| Some("Mithril restore failed; check container logs.".into())),
                );
            }

            if !runtime.protocol_magic_id_present && zero_progress {
                if previous
                    .and_then(|sample| sample.sync_stage.as_deref())
                    .map(|stage| stage == "snapshot_restoring")
                    .unwrap_or(false)
                    && previous
                        .map(|sample| current_epoch_seconds - sample.collected_at_epoch >= 900)
                        .unwrap_or(false)
                {
                    return (
                        "restore_timeout".into(),
                        Some(
                            "Mithril restore has not initialized the database for at least 15 minutes; inspect logs or allow fallback to normal sync."
                                .into(),
                        ),
                    );
                }

                return (
                    "snapshot_restoring".into(),
                    last_restore_related_log_line(runtime.recent_logs.as_str()).or_else(|| {
                        Some(
                            "Mithril restore requested; database is still being initialized."
                                .into(),
                        )
                    }),
                );
            }
        }
    }

    match sync_progress {
        Some(progress) if progress >= 100.0 => ("synced".into(), None),
        Some(_) => ("syncing".into(), None),
        None if block_height.unwrap_or_default() > 0 => ("syncing".into(), None),
        _ => ("unknown".into(), None),
    }
}

fn recover_restore_stage_from_tip_error(
    previous: Option<&PreviousHealthSample>,
    runtime: Option<&RuntimeMonitorContext>,
    current_epoch_seconds: i64,
) -> Option<(String, String, Option<f64>, Option<String>)> {
    let runtime = runtime?;
    if runtime.restore_snapshot_requested && has_restore_activity(runtime.recent_logs.as_str()) {
        return Some((
            "snapshot_restoring".into(),
            "syncing".into(),
            parse_restore_progress_from_logs(runtime.recent_logs.as_str()),
            last_restore_related_log_line(runtime.recent_logs.as_str()).or_else(|| {
                Some(
                    "Mithril restore is still replaying the database; tip is not ready yet.".into(),
                )
            }),
        ));
    }

    let (sync_stage, sync_note) = determine_sync_stage(
        None,
        None,
        previous,
        Some(runtime),
        None,
        current_epoch_seconds,
    );
    let status = match sync_stage.as_str() {
        "restore_failed" | "restore_timeout" => "stalled".into(),
        "fallback_syncing" | "snapshot_restoring" => "syncing".into(),
        _ => return None,
    };
    Some((sync_stage, status, None, sync_note))
}

fn collect_runtime_monitor_context(machine: &MachineRow) -> Option<RuntimeMonitorContext> {
    let env_raw = ssh_exec(
        machine,
        "docker inspect cardano-node --format '{{json .Config.Env}}'",
    )
    .ok()?;
    let protocol_magic_raw = ssh_exec(
        machine,
        "docker exec cardano-node sh -lc '[ -f /data/db/protocolMagicId ] && echo yes || echo no'",
    )
    .ok()
    .unwrap_or_else(|| "no".into());
    let recent_logs = ssh_exec(machine, "docker logs --tail 120 cardano-node 2>&1")
        .ok()
        .unwrap_or_default();

    Some(RuntimeMonitorContext {
        restore_snapshot_requested: parse_restore_snapshot_requested(env_raw.as_str()),
        protocol_magic_id_present: parse_protocol_magic_id_present(protocol_magic_raw.as_str()),
        recent_logs,
    })
}

#[derive(Debug)]
struct SnapshotBatch {
    snapshots: Vec<MonitorSnapshot>,
    degraded_message: Option<String>,
}

fn telemetry_snapshot_state(metrics: &PrometheusMetrics, now_epoch: i64) -> (&'static str, &'static str, &'static str) {
    if !metrics.has_any_value() {
        return (
            "telemetry_unavailable",
            "telemetry_unavailable",
            "critical",
        );
    }
    let stale = metrics
        .collected_at_epoch
        .map(|collected| (now_epoch - collected) > TELEMETRY_STALE_THRESHOLD_SECONDS)
        .unwrap_or(true);
    if stale {
        return ("telemetry_stale", "telemetry_stale", "warning");
    }
    ("telemetry_live", "telemetry_live", "healthy")
}

fn append_note(base: Option<String>, message: String) -> Option<String> {
    match base {
        Some(existing) if existing.trim().is_empty() => Some(message),
        Some(existing) => Some(format!("{existing}; {message}")),
        None => Some(message),
    }
}

fn snapshot_cache_registry() -> &'static Mutex<Vec<MonitorSnapshot>> {
    static REGISTRY: OnceLock<Mutex<Vec<MonitorSnapshot>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

fn cache_latest_live_snapshots(snapshots: &[MonitorSnapshot]) {
    if !snapshots
        .iter()
        .any(|snapshot| snapshot.sync_stage == "telemetry_live")
    {
        return;
    }
    if let Ok(mut cache) = snapshot_cache_registry().lock() {
        *cache = snapshots.to_vec();
    }
}

fn stale_from_cached_snapshot(mut snapshot: MonitorSnapshot, reason: &str) -> MonitorSnapshot {
    snapshot.status = "telemetry_stale".into();
    snapshot.sync_stage = "telemetry_stale".into();
    snapshot.health_level = "warning".into();
    snapshot.note = append_note(snapshot.note, reason.to_string());
    snapshot.prometheus_note = append_note(snapshot.prometheus_note, reason.to_string());
    snapshot
}

fn load_latest_health_snapshot(
    conn: &Connection,
    machine: &MachineRow,
    reason: &str,
) -> Result<Option<MonitorSnapshot>, AppError> {
    let latest = conn
        .query_row(
            "SELECT block_height, sync_progress, sync_note, collected_at
             FROM machine_health
             WHERE machine_id = ?1
             ORDER BY collected_at DESC, id DESC
             LIMIT 1",
            params![machine.id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<f64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((block_height, sync_progress, sync_note, collected_at)) = latest else {
        return Ok(None);
    };
    let note = append_note(sync_note, reason.to_string());
    Ok(Some(MonitorSnapshot {
        machine_id: machine.id,
        machine_name: machine.name.clone(),
        role: machine.role.clone(),
        network: machine.network.clone(),
        block_height,
        sync_progress,
        blocks_per_minute: None,
        health_level: "warning".into(),
        status: "telemetry_stale".into(),
        sync_stage: "telemetry_stale".into(),
        restore_snapshot_requested: false,
        stalled: false,
        collected_at: collected_at.unwrap_or_else(|| "1970-01-01 00:00:00".into()),
        note: note.clone(),
        epoch: None,
        slot_num: None,
        slot_in_epoch: None,
        sync_percent: sync_progress,
        tip_diff_blocks: None,
        late_blocks: None,
        peer_count: None,
        cpu_sys_percent: None,
        mem_live_bytes: None,
        mem_rss_bytes: None,
        mem_heap_bytes: None,
        gc_minor_total: None,
        gc_major_total: None,
        txs_in_mempool: None,
        mempool_bytes: None,
        forks: None,
        forging_enabled: None,
        prometheus_source: Some("cache:machine_health".into()),
        prometheus_note: note,
    }))
}

fn load_cached_snapshots(
    conn: &Connection,
    machines: &[MachineRow],
    reason: &str,
) -> Result<Vec<MonitorSnapshot>, AppError> {
    let machine_ids: std::collections::HashSet<i64> =
        machines.iter().map(|machine| machine.id).collect();
    if let Ok(cache) = snapshot_cache_registry().lock() {
        let cached: Vec<MonitorSnapshot> = cache
            .iter()
            .filter(|snapshot| machine_ids.contains(&snapshot.machine_id))
            .cloned()
            .map(|snapshot| stale_from_cached_snapshot(snapshot, reason))
            .collect();
        if !cached.is_empty() {
            return Ok(cached);
        }
    }

    let mut snapshots = Vec::with_capacity(machines.len());
    for machine in machines {
        if let Some(snapshot) = load_latest_health_snapshot(conn, machine, reason)? {
            snapshots.push(snapshot);
        }
    }
    Ok(snapshots)
}

fn collect_machine_snapshot(
    conn: &Connection,
    machine: &MachineRow,
) -> Result<MonitorSnapshot, AppError> {
    let collected_at_epoch = current_epoch_seconds()?;
    let collected_at = sqlite_datetime(conn, collected_at_epoch)?;
    let previous = get_previous_health_sample(conn, machine.id)?;
    let prometheus = collect_prometheus_metrics(conn, machine);
    let block_height = prometheus.block_height;
    let sync_progress = prometheus.sync_percent;
    let blocks_per_minute =
        compute_blocks_per_minute(previous.as_ref(), block_height, collected_at_epoch);
    let (status, sync_stage, health_level) =
        telemetry_snapshot_state(&prometheus, collected_at_epoch);
    let telemetry_age = prometheus
        .collected_at_epoch
        .map(|ts| collected_at_epoch.saturating_sub(ts));
    let note = match sync_stage {
        "telemetry_stale" => append_note(
            prometheus.note.clone(),
            format!(
                "telemetry sample is stale ({}s old)",
                telemetry_age.unwrap_or_default()
            ),
        ),
        "telemetry_unavailable" => append_note(
            prometheus.note.clone(),
            "relay api unavailable after failover attempts".into(),
        ),
        _ => prometheus.note.clone(),
    };
    let snapshot_collected_at =
        resolve_snapshot_collected_at(conn, collected_at.as_str(), &prometheus);
    insert_health_sample(
        conn,
        machine.id,
        block_height,
        sync_progress,
        sync_stage,
        note.as_deref(),
        collected_at_epoch,
    )?;

    Ok(MonitorSnapshot {
        machine_id: machine.id,
        machine_name: machine.name.clone(),
        role: machine.role.clone(),
        network: machine.network.clone(),
        block_height,
        sync_progress,
        blocks_per_minute,
        health_level: health_level.into(),
        status: status.into(),
        sync_stage: sync_stage.into(),
        restore_snapshot_requested: false,
        stalled: false,
        collected_at: snapshot_collected_at,
        note: note.clone(),
        epoch: prometheus.epoch,
        slot_num: prometheus.slot_num,
        slot_in_epoch: prometheus.slot_in_epoch,
        sync_percent: prometheus.sync_percent.or(sync_progress),
        tip_diff_blocks: prometheus.tip_diff_blocks,
        late_blocks: prometheus.late_blocks,
        peer_count: prometheus.peer_count,
        cpu_sys_percent: prometheus.cpu_sys_percent,
        mem_live_bytes: prometheus.mem_live_bytes,
        mem_rss_bytes: prometheus.mem_rss_bytes,
        mem_heap_bytes: prometheus.mem_heap_bytes,
        gc_minor_total: prometheus.gc_minor_total,
        gc_major_total: prometheus.gc_major_total,
        txs_in_mempool: prometheus.txs_in_mempool,
        mempool_bytes: prometheus.mempool_bytes,
        forks: prometheus.forks,
        forging_enabled: prometheus.forging_enabled,
        prometheus_source: prometheus.source,
        prometheus_note: note,
    })
}

#[tauri::command]
pub async fn monitor_snapshot(
    machine_ids: Option<Vec<i64>>,
    db: State<'_, DbState>,
) -> Result<Vec<MonitorSnapshot>, AppError> {
    Ok(collect_snapshots_from_db_state(&db, machine_ids)
        .await?
        .snapshots)
}

async fn collect_snapshots_from_db_state(
    db: &DbState,
    machine_ids: Option<Vec<i64>>,
) -> Result<SnapshotBatch, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let selected = repo_machine_list(&conn, None, None)?
        .into_iter()
        .filter(|machine| {
            machine_ids
                .as_ref()
                .map(|ids| ids.contains(&machine.id))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    if selected.is_empty() {
        return Ok(SnapshotBatch {
            snapshots: Vec::new(),
            degraded_message: None,
        });
    }

    let mut snapshots = Vec::with_capacity(selected.len());
    for machine in &selected {
        snapshots.push(collect_machine_snapshot(&conn, machine)?);
    }
    let all_unavailable = snapshots
        .iter()
        .all(|snapshot| snapshot.sync_stage == "telemetry_unavailable");
    if all_unavailable {
        let degraded_message = "relay telemetry api unavailable after failover attempts";
        let cached = load_cached_snapshots(&conn, &selected, degraded_message)?;
        if !cached.is_empty() {
            return Ok(SnapshotBatch {
                snapshots: cached,
                degraded_message: Some(degraded_message.to_string()),
            });
        }
        return Ok(SnapshotBatch {
            snapshots,
            degraded_message: Some(degraded_message.to_string()),
        });
    }

    cache_latest_live_snapshots(&snapshots);
    Ok(SnapshotBatch {
        snapshots,
        degraded_message: None,
    })
}

fn audit_telemetry_degraded_retry(db: &DbState, machine_ids: &Option<Vec<i64>>, message: &str) {
    let Ok(conn) = db.0.lock() else {
        return;
    };
    let detail = serde_json::json!({
        "machine_ids": machine_ids,
        "reason": message,
    });
    let _ = audit_log_insert(&conn, "telemetry_degraded_retry", &detail);
}

#[tauri::command]
pub async fn monitor_start_polling(
    machine_ids: Option<Vec<i64>>,
    interval_seconds: Option<u64>,
    db: State<'_, DbState>,
    polling: State<'_, MonitorPollingState>,
    app_handle: AppHandle,
) -> Result<MonitorPollingStatus, AppError> {
    let interval_seconds = interval_seconds.unwrap_or(30).clamp(5, 300);

    {
        let mut guard = polling
            .0
            .lock()
            .map_err(|_| AppError::Internal("lock".into()))?;
        if let Some(existing) = guard.take() {
            existing.stop.store(true, Ordering::SeqCst);
            existing.join.abort();
        }
    }

    let initial = collect_snapshots_from_db_state(&db, machine_ids.clone()).await?;
    let _ = app_handle.emit("monitor:snapshot", &initial.snapshots);
    if let Some(message) = initial.degraded_message {
        audit_telemetry_degraded_retry(&db, &machine_ids, message.as_str());
        let _ = app_handle.emit("monitor:error", serde_json::json!({ "message": message }));
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_task = Arc::clone(&stop);
    let app_handle_for_task = app_handle.clone();
    let join = tauri::async_runtime::spawn(async move {
        loop {
            if stop_for_task.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(interval_seconds)).await;
            if stop_for_task.load(Ordering::SeqCst) {
                break;
            }
            let Some(db_state) = app_handle_for_task.try_state::<DbState>() else {
                let _ = app_handle_for_task.emit(
                    "monitor:error",
                    serde_json::json!({ "message": "monitor db state unavailable" }),
                );
                break;
            };
            match collect_snapshots_from_db_state(&db_state, machine_ids.clone()).await {
                Ok(batch) => {
                    let _ = app_handle_for_task.emit("monitor:snapshot", &batch.snapshots);
                    if let Some(message) = batch.degraded_message {
                        audit_telemetry_degraded_retry(&db_state, &machine_ids, message.as_str());
                        let _ = app_handle_for_task.emit(
                            "monitor:error",
                            serde_json::json!({ "message": message }),
                        );
                    }
                }
                Err(err) => {
                    let err_message = err.to_string();
                    audit_telemetry_degraded_retry(&db_state, &machine_ids, err_message.as_str());
                    let _ = app_handle_for_task.emit(
                        "monitor:error",
                        serde_json::json!({ "message": err_message }),
                    );
                }
            }
        }
    });

    let mut guard = polling
        .0
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    *guard = Some(MonitorPollingHandle { stop, join });

    Ok(MonitorPollingStatus {
        running: true,
        interval_seconds,
    })
}

#[tauri::command]
pub async fn monitor_stop_polling(
    polling: State<'_, MonitorPollingState>,
) -> Result<MonitorPollingStatus, AppError> {
    let mut guard = polling
        .0
        .lock()
        .map_err(|_| AppError::Internal("lock".into()))?;
    if let Some(existing) = guard.take() {
        existing.stop.store(true, Ordering::SeqCst);
        existing.join.abort();
    }
    Ok(MonitorPollingStatus {
        running: false,
        interval_seconds: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{app_config_set, run_migrations};
    use rusqlite::Connection;
    use serde_json::json;

    #[test]
    fn tc_mon_001_parse_tip_supports_string_progress() {
        let raw = r#"{"block":123,"syncProgress":"42.50"}"#;
        let (block_height, sync_progress) = parse_tip(raw).expect("parse tip");
        assert_eq!(block_height, Some(123));
        assert_eq!(sync_progress, Some(42.5));
    }

    #[test]
    fn tc_mon_002_compute_blocks_per_minute() {
        let previous = PreviousHealthSample {
            block_height: Some(1_000),
            sync_stage: None,
            collected_at_epoch: 1_000,
        };
        let blocks_per_minute =
            compute_blocks_per_minute(Some(&previous), Some(1_120), 1_600).expect("speed");
        assert!((blocks_per_minute - 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tc_mon_003_stalled_when_height_not_moving_for_five_minutes() {
        let previous = PreviousHealthSample {
            block_height: Some(1_000),
            sync_stage: None,
            collected_at_epoch: 1_000,
        };
        assert!(determine_stalled(
            Some(&previous),
            Some(1_000),
            Some(12.5),
            1_301
        ));
        assert!(!determine_stalled(
            Some(&previous),
            Some(1_001),
            Some(12.5),
            1_301
        ));
    }

    #[test]
    fn tc_mon_004_wrap_remote_command_uses_sudo_for_docker() {
        let wrapped = wrap_remote_command("docker exec cardano-node cardano-cli query tip");
        assert!(wrapped.contains("sudo -n docker exec cardano-node cardano-cli query tip"));
        assert!(wrapped
            .contains("2>/dev/null || sudo -n docker exec cardano-node cardano-cli query tip"));
        assert!(wrapped.starts_with("sh -lc "));
    }

    #[test]
    fn tc_mon_005_snapshot_restoring_stage_when_restore_requested_and_db_not_initialized() {
        let runtime = RuntimeMonitorContext {
            restore_snapshot_requested: true,
            protocol_magic_id_present: false,
            recent_logs: "Mithril: restoring snapshot chunk 1/42".into(),
        };
        let (stage, note) = determine_sync_stage(Some(0), Some(0.0), None, Some(&runtime), None, 0);
        assert_eq!(stage, "snapshot_restoring");
        assert!(note.expect("note").contains("Mithril"));
    }

    #[test]
    fn tc_mon_006_restore_failed_stage_when_logs_show_failure() {
        let runtime = RuntimeMonitorContext {
            restore_snapshot_requested: true,
            protocol_magic_id_present: false,
            recent_logs: "mithril restore failed: checksum mismatch".into(),
        };
        let (stage, note) = determine_sync_stage(Some(0), Some(0.0), None, Some(&runtime), None, 0);
        assert_eq!(stage, "restore_failed");
        assert!(note.expect("note").contains("failed"));
    }

    #[test]
    fn tc_mon_007_parse_restore_snapshot_requested_from_env_json() {
        let raw = json!(["FOO=bar", "RESTORE_SNAPSHOT=true"]).to_string();
        assert!(parse_restore_snapshot_requested(raw.as_str()));
        let raw = json!(["FOO=bar", "RESTORE_SNAPSHOT=false"]).to_string();
        assert!(!parse_restore_snapshot_requested(raw.as_str()));
    }

    #[test]
    fn tc_mon_008_restore_timeout_after_fifteen_minutes_without_progress() {
        let previous = PreviousHealthSample {
            block_height: Some(0),
            sync_stage: Some("snapshot_restoring".into()),
            collected_at_epoch: 1_000,
        };
        let runtime = RuntimeMonitorContext {
            restore_snapshot_requested: true,
            protocol_magic_id_present: false,
            recent_logs: "Mithril: restoring snapshot chunk 1/42".into(),
        };
        let (stage, note) = determine_sync_stage(
            Some(0),
            Some(0.0),
            Some(&previous),
            Some(&runtime),
            None,
            1_901,
        );
        assert_eq!(stage, "restore_timeout");
        assert!(note.expect("note").contains("15 minutes"));
    }

    #[test]
    fn tc_mon_009_fallback_syncing_after_restore_failure_when_blocks_move() {
        let runtime = RuntimeMonitorContext {
            restore_snapshot_requested: true,
            protocol_magic_id_present: false,
            recent_logs: "mithril restore failed: checksum mismatch".into(),
        };
        let (stage, note) =
            determine_sync_stage(Some(12), Some(0.1), None, Some(&runtime), None, 0);
        assert_eq!(stage, "fallback_syncing");
        assert!(note.expect("note").contains("failed"));
    }

    #[test]
    fn tc_mon_010_tip_error_during_restore_is_not_unreachable() {
        let runtime = RuntimeMonitorContext {
            restore_snapshot_requested: true,
            protocol_magic_id_present: false,
            recent_logs: "Mithril: restoring snapshot chunk 3/42".into(),
        };
        let recovered = recover_restore_stage_from_tip_error(None, Some(&runtime), 0)
            .expect("recover restore stage");
        assert_eq!(recovered.0, "snapshot_restoring");
        assert_eq!(recovered.1, "syncing");
        assert_eq!(recovered.2, None);
        assert!(recovered.3.expect("note").contains("Mithril"));
    }

    #[test]
    fn tc_mon_011_tip_error_during_replay_is_not_unreachable() {
        let runtime = RuntimeMonitorContext {
            restore_snapshot_requested: true,
            protocol_magic_id_present: true,
            recent_logs:
                "[cardano.node.ChainDB:Info:5] Replayed block: slot 2116799 out of 181310513. Progress: 1.17%"
                    .into(),
        };
        let recovered = recover_restore_stage_from_tip_error(None, Some(&runtime), 0)
            .expect("recover replay stage");
        assert_eq!(recovered.0, "snapshot_restoring");
        assert_eq!(recovered.1, "syncing");
        assert_eq!(recovered.2, Some(1.17));
    }

    #[test]
    fn tc_mon_012_parse_restore_progress_from_logs() {
        let raw = "[cardano.node.ChainDB:Info:5] Replayed block: slot 2116799 out of 181310513. Progress: 1.17%";
        assert_eq!(parse_restore_progress_from_logs(raw), Some(1.17));
    }

    #[test]
    fn tc_mon_013_monitor_polling_interval_is_clamped() {
        assert_eq!(1_u64.clamp(5, 300), 5);
        assert_eq!(30_u64.clamp(5, 300), 30);
        assert_eq!(600_u64.clamp(5, 300), 300);
    }

    #[test]
    fn tc_mon_014_health_level_is_healthy_for_fast_sync_or_synced() {
        assert_eq!(
            determine_health_level("synced", "synced", None, false),
            "healthy"
        );
        assert_eq!(
            determine_health_level("syncing", "syncing", Some(45.0), false),
            "healthy"
        );
    }

    #[test]
    fn tc_mon_015_health_level_is_warning_for_restore_or_slow_sync() {
        assert_eq!(
            determine_health_level("syncing", "snapshot_restoring", None, false),
            "warning"
        );
        assert_eq!(
            determine_health_level("syncing", "syncing", Some(5.0), false),
            "warning"
        );
    }

    #[test]
    fn tc_mon_016_health_level_is_critical_for_unreachable_or_stalled() {
        assert_eq!(
            determine_health_level("unreachable", "unreachable", None, false),
            "critical"
        );
        assert_eq!(
            determine_health_level("stalled", "syncing", Some(0.0), true),
            "critical"
        );
    }

    fn sample_machine(name: &str, role: &str, ip: &str) -> MachineRow {
        MachineRow {
            id: 1,
            pool_id: 1,
            name: name.into(),
            ip: ip.into(),
            ssh_port: 22,
            ssh_user: "root".into(),
            role: role.into(),
            network: "mainnet".into(),
            ssh_key_fingerprint: None,
            os_version: None,
            cardano_version: None,
            image_registry: "ghcr.io/blinklabs-io/cardano-node".into(),
            image_digest: None,
            sort_order: 1,
            created_at: "2026-03-13 00:00:00".into(),
            updated_at: "2026-03-13 00:00:00".into(),
        }
    }

    #[test]
    fn tc_mon_017_parse_relay_metric_samples_supports_prometheus_vector_payload() {
        let payload = json!({
            "status": "success",
            "data": {
                "resultType": "vector",
                "result": [{
                    "metric": {
                        "__name__": "cardano_node_metrics_syncProgress",
                        "node": "bp-1",
                        "role": "bp",
                        "instance": "10.0.0.12:12798"
                    },
                    "value": [1773374400, "98.41"]
                }]
            }
        });
        let samples = parse_relay_metric_samples(&payload).expect("samples");
        assert_eq!(samples.len(), 1);
        assert_eq!(
            samples[0].metric_name.as_deref(),
            Some("cardano_node_metrics_syncProgress")
        );
        assert_eq!(samples[0].node.as_deref(), Some("bp-1"));
        assert_eq!(samples[0].role.as_deref(), Some("bp"));
        assert_eq!(samples[0].timestamp, Some(1_773_374_400));
        assert_eq!(samples[0].value, 98.41);
    }

    #[test]
    fn tc_mon_018_select_relay_metric_sample_prefers_node_role_match() {
        let machine = sample_machine("bp-1", "bp", "10.0.0.12");
        let samples = vec![
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_syncProgress".into()),
                node: Some("relay-1".into()),
                role: Some("relay".into()),
                host_ip: Some("10.0.0.10".into()),
                instance: Some("10.0.0.10:12798".into()),
                timestamp: Some(100),
                value: 10.0,
            },
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_syncProgress".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(120),
                value: 98.41,
            },
        ];
        let selected =
            select_relay_metric_sample(&machine, &samples, &["cardano_node_metrics_syncProgress"])
                .expect("selected");
        assert_eq!(selected.value, 98.41);
        assert_eq!(selected.timestamp, Some(120));
    }

    #[test]
    fn tc_mon_019_parse_env_bool_supports_truthy_values() {
        assert!(parse_env_bool("true"));
        assert!(parse_env_bool(" YES "));
        assert!(parse_env_bool("1"));
        assert!(!parse_env_bool("false"));
        assert!(!parse_env_bool("0"));
    }

    #[test]
    fn tc_mon_022_relay_telemetry_config_falls_back_to_app_config() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        app_config_set(&conn, RELAY_TELEMETRY_CFG_USERNAME, "ouro_app").expect("set username");
        app_config_set(&conn, RELAY_TELEMETRY_CFG_PASSWORD, "secret").expect("set password");
        app_config_set(&conn, RELAY_TELEMETRY_CFG_INSECURE, "true").expect("set insecure");

        std::env::remove_var("OURO_OPS_RELAY_TELEMETRY_USERNAME");
        std::env::remove_var("OURO_OPS_RELAY_TELEMETRY_PASSWORD");
        std::env::remove_var("OURO_OPS_RELAY_TELEMETRY_INSECURE");

        let config = relay_telemetry_config(&conn).expect("config");
        assert_eq!(config.username, "ouro_app");
        assert_eq!(config.password, "secret");
        assert!(config.insecure_tls);
        assert_eq!(config.port, 443);
    }

    #[test]
    fn tc_mon_020_parse_relay_metric_samples_supports_series_payload() {
        let payload = json!({
            "metric": "sync_percent",
            "series": [{
                "node": "relay-1",
                "role": "relay",
                "instance": "10.0.0.10:12798",
                "timestamp": 1773374410,
                "value": "100.00"
            }]
        });
        let samples = parse_relay_metric_samples(&payload).expect("samples");
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].metric_name.as_deref(), Some("sync_percent"));
        assert_eq!(samples[0].node.as_deref(), Some("relay-1"));
        assert_eq!(samples[0].timestamp, Some(1_773_374_410));
        assert_eq!(samples[0].value, 100.0);
    }

    #[test]
    fn tc_mon_021_relay_backoff_registry_round_trip() {
        let relay = sample_machine("relay-1", "relay", "10.0.0.10");
        relay_clear_backoff(&relay);
        relay_mark_backoff(&relay, 1_000, 30);
        assert_eq!(relay_backoff_retry_at(&relay), Some(1_030));
        relay_clear_backoff(&relay);
        assert_eq!(relay_backoff_retry_at(&relay), None);
    }

    #[test]
    fn tc_mon_023_select_unlabeled_local_relay_sample_supports_legacy_payload() {
        let machine = sample_machine("relay-1", "relay", "10.0.0.10");
        let relay = sample_machine("relay-1", "relay", "10.0.0.10");
        let samples = vec![RelayMetricSample {
            metric_name: Some("cardano_node_metrics_epoch_int".into()),
            node: None,
            role: Some("relay".into()),
            host_ip: None,
            instance: Some("172.17.0.2:12798".into()),
            timestamp: Some(1_773_410_000),
            value: 618.0,
        }];

        let selected =
            select_relay_metric_sample(&machine, &samples, &["cardano_node_metrics_epoch_int"])
                .or_else(|| {
                    select_unlabeled_local_relay_sample(
                        &machine,
                        &relay,
                        &samples,
                        &["cardano_node_metrics_epoch_int"],
                    )
                })
                .expect("selected sample");
        assert_eq!(selected.value, 618.0);
    }

    #[test]
    fn tc_mon_024_telemetry_snapshot_state_live_stale_unavailable() {
        let live = PrometheusMetrics {
            epoch: Some(618),
            collected_at_epoch: Some(1_000),
            ..PrometheusMetrics::default()
        };
        let (status, stage, health) = telemetry_snapshot_state(&live, 1_030);
        assert_eq!(status, "telemetry_live");
        assert_eq!(stage, "telemetry_live");
        assert_eq!(health, "healthy");

        let stale = PrometheusMetrics {
            epoch: Some(618),
            collected_at_epoch: Some(1_000),
            ..PrometheusMetrics::default()
        };
        let (status, stage, health) =
            telemetry_snapshot_state(&stale, 1_000 + TELEMETRY_STALE_THRESHOLD_SECONDS + 1);
        assert_eq!(status, "telemetry_stale");
        assert_eq!(stage, "telemetry_stale");
        assert_eq!(health, "warning");

        let unavailable = PrometheusMetrics::default();
        let (status, stage, health) = telemetry_snapshot_state(&unavailable, 1_000);
        assert_eq!(status, "telemetry_unavailable");
        assert_eq!(stage, "telemetry_unavailable");
        assert_eq!(health, "critical");
    }

    #[test]
    fn tc_mon_025_relay_metric_mapping_supports_block_height_without_sync_percent() {
        let machine = sample_machine("bp-1", "bp", "10.0.0.12");
        let samples = vec![
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_blockNum_int".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 13_153_866.0,
            },
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_epoch_int".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 618.0,
            },
        ];
        let mut mapped = PrometheusMetrics::default();
        let mut latest_ts: Option<i64> = None;
        for (metric_key, metric_names) in RELAY_TELEMETRY_FIELD_METRICS {
            if let Some(sample) = select_relay_metric_sample(&machine, &samples, metric_names) {
                apply_relay_metric_value(
                    &mut mapped,
                    metric_key,
                    sample.value,
                    sample.timestamp,
                    &mut latest_ts,
                );
            }
        }
        assert_eq!(mapped.block_height, Some(13_153_866));
        assert_eq!(mapped.epoch, Some(618));
        assert_eq!(mapped.sync_percent, None);
    }

    #[test]
    fn tc_mon_026_stale_from_cached_snapshot_marks_warning_and_appends_note() {
        let snapshot = MonitorSnapshot {
            machine_id: 1,
            machine_name: "bp-1".into(),
            role: "bp".into(),
            network: "mainnet".into(),
            block_height: Some(100),
            sync_progress: Some(99.0),
            blocks_per_minute: Some(1.0),
            health_level: "healthy".into(),
            status: "telemetry_live".into(),
            sync_stage: "telemetry_live".into(),
            restore_snapshot_requested: false,
            stalled: false,
            collected_at: "2026-03-14 00:00:00".into(),
            note: None,
            epoch: Some(1),
            slot_num: Some(100),
            slot_in_epoch: Some(25),
            sync_percent: Some(99.0),
            tip_diff_blocks: Some(0),
            late_blocks: Some(0),
            peer_count: Some(10),
            cpu_sys_percent: Some(1.0),
            mem_live_bytes: Some(1.0),
            mem_rss_bytes: Some(1.0),
            mem_heap_bytes: Some(1.0),
            gc_minor_total: Some(1),
            gc_major_total: Some(1),
            txs_in_mempool: Some(3),
            mempool_bytes: Some(1024.0),
            forks: Some(0),
            forging_enabled: Some(1),
            prometheus_source: Some("relay-api:relay-1@10.0.0.10".into()),
            prometheus_note: None,
        };
        let next = stale_from_cached_snapshot(snapshot, "relay api unavailable");
        assert_eq!(next.status, "telemetry_stale");
        assert_eq!(next.sync_stage, "telemetry_stale");
        assert_eq!(next.health_level, "warning");
        assert!(next.note.unwrap_or_default().contains("relay api unavailable"));
    }

    #[test]
    fn tc_mon_027_relay_metric_mapping_supports_catalog_chain_and_tx_fields() {
        let machine = sample_machine("bp-1", "bp", "10.0.0.12");
        let samples = vec![
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_slotNum_int".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 181_843_743.0,
            },
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_slotInEpoch_int".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 230_943.0,
            },
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_blockfetchclient_lateblocks".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 87.0,
            },
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_txsInMempool_int".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 12.0,
            },
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_mempoolBytes_int".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 12_615.0,
            },
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_forks_int".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 261.0,
            },
            RelayMetricSample {
                metric_name: Some("cardano_node_metrics_forging_enabled".into()),
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                host_ip: Some("10.0.0.12".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(1_000),
                value: 1.0,
            },
        ];

        let mut mapped = PrometheusMetrics::default();
        let mut latest_ts: Option<i64> = None;
        for (metric_key, metric_names) in RELAY_TELEMETRY_FIELD_METRICS {
            if let Some(sample) = select_relay_metric_sample(&machine, &samples, metric_names) {
                apply_relay_metric_value(
                    &mut mapped,
                    metric_key,
                    sample.value,
                    sample.timestamp,
                    &mut latest_ts,
                );
            }
        }

        assert_eq!(mapped.slot_num, Some(181_843_743));
        assert_eq!(mapped.slot_in_epoch, Some(230_943));
        assert_eq!(mapped.late_blocks, Some(87));
        assert_eq!(mapped.txs_in_mempool, Some(12));
        assert_eq!(mapped.mempool_bytes, Some(12_615.0));
        assert_eq!(mapped.forks, Some(261));
        assert_eq!(mapped.forging_enabled, Some(1));
    }
}
