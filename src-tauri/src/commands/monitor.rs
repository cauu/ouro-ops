use std::collections::HashMap;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{audit_log_insert, machine_list as repo_machine_list, DbState, MachineRow};
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
    pub sync_percent: Option<f64>,
    pub tip_diff_blocks: Option<i64>,
    pub peer_count: Option<i64>,
    pub cpu_sys_percent: Option<f64>,
    pub mem_live_bytes: Option<f64>,
    pub mem_rss_bytes: Option<f64>,
    pub mem_heap_bytes: Option<f64>,
    pub gc_minor_total: Option<i64>,
    pub gc_major_total: Option<i64>,
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
    epoch: Option<i64>,
    sync_percent: Option<f64>,
    tip_diff_blocks: Option<i64>,
    peer_count: Option<i64>,
    cpu_sys_percent: Option<f64>,
    mem_live_bytes: Option<f64>,
    mem_rss_bytes: Option<f64>,
    mem_heap_bytes: Option<f64>,
    gc_minor_total: Option<i64>,
    gc_major_total: Option<i64>,
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
    node: Option<String>,
    role: Option<String>,
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

const RELAY_TELEMETRY_ENDPOINTS: [(&str, &str); 10] = [
    ("epoch", "epoch"),
    ("sync_percent", "sync-percent"),
    ("tip_diff_blocks", "tip-diff-blocks"),
    ("peer_count", "peer-count"),
    ("cpu_sys_percent", "cpu-sys-percent"),
    ("mem_live_bytes", "mem-live-bytes"),
    ("mem_rss_bytes", "mem-rss-bytes"),
    ("mem_heap_bytes", "mem-heap-bytes"),
    ("gc_minor_total", "gc-minor-total"),
    ("gc_major_total", "gc-major-total"),
];

impl PrometheusMetrics {
    fn has_any_value(&self) -> bool {
        self.epoch.is_some()
            || self.sync_percent.is_some()
            || self.tip_diff_blocks.is_some()
            || self.peer_count.is_some()
            || self.cpu_sys_percent.is_some()
            || self.mem_live_bytes.is_some()
            || self.mem_rss_bytes.is_some()
            || self.mem_heap_bytes.is_some()
            || self.gc_minor_total.is_some()
            || self.gc_major_total.is_some()
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
        epoch: pick_prometheus_i64(
            metrics,
            &[
                "nview_epoch",
                "cardano_node_metrics_epoch_int",
                "cardano_node_metrics_epoch",
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
        let samples = series
            .iter()
            .filter_map(|entry| {
                let value = parse_metric_sample_value(entry.get("value"))?;
                Some(RelayMetricSample {
                    node: entry
                        .get("node")
                        .and_then(Value::as_str)
                        .map(|raw| raw.to_string()),
                    role: entry
                        .get("role")
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
                node: metric
                    .get("node")
                    .and_then(Value::as_str)
                    .map(|raw| raw.to_string()),
                role: metric
                    .get("role")
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
) -> Option<&'a RelayMetricSample> {
    let mut selected: Option<&RelayMetricSample> = None;
    let mut selected_score = 0_u8;
    let mut selected_timestamp = i64::MIN;

    for sample in samples {
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

        let score = if node_matches {
            3
        } else if instance_matches {
            2
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
    selected
}

fn apply_relay_metric_value(
    mapped: &mut PrometheusMetrics,
    metric_key: &str,
    sample_value: f64,
    sample_timestamp: Option<i64>,
    latest_timestamp: &mut Option<i64>,
) {
    match metric_key {
        "epoch" => mapped.epoch = Some(sample_value.round() as i64),
        "sync_percent" => mapped.sync_percent = normalize_percent(Some(sample_value)),
        "tip_diff_blocks" => mapped.tip_diff_blocks = Some(sample_value.round() as i64),
        "peer_count" => mapped.peer_count = Some(sample_value.round() as i64),
        "cpu_sys_percent" => mapped.cpu_sys_percent = normalize_percent(Some(sample_value)),
        "mem_live_bytes" => mapped.mem_live_bytes = Some(sample_value),
        "mem_rss_bytes" => mapped.mem_rss_bytes = Some(sample_value),
        "mem_heap_bytes" => mapped.mem_heap_bytes = Some(sample_value),
        "gc_minor_total" => mapped.gc_minor_total = Some(sample_value.round() as i64),
        "gc_major_total" => mapped.gc_major_total = Some(sample_value.round() as i64),
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

    for (metric_key, endpoint) in RELAY_TELEMETRY_ENDPOINTS {
        match fetch_relay_metric_samples(config, relay, endpoint) {
            Ok(samples) => {
                if let Some(sample) = select_relay_metric_sample(machine, &samples) {
                    apply_relay_metric_value(
                        &mut mapped,
                        metric_key,
                        sample.value,
                        sample.timestamp,
                        &mut latest_timestamp,
                    );
                }
            }
            Err(err) => {
                errors.push(err.to_string());
            }
        }
    }

    if mapped.has_any_value() {
        mapped.note = if errors.is_empty() {
            None
        } else {
            Some(format!(
                "relay api partial data on {}: {} endpoint(s) failed",
                relay.name,
                errors.len()
            ))
        };
        if let Some(timestamp) = latest_timestamp {
            mapped.source = Some(format!(
                "relay-api:{}@{}#{}",
                relay.name, relay.ip, timestamp
            ));
        }
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
        endpoint_errors: errors.len(),
    }
}

fn collect_relay_prometheus_metrics(conn: &Connection, machine: &MachineRow) -> PrometheusMetrics {
    let Some(config) = relay_telemetry_config_from_env() else {
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

    let mut best: Option<(PrometheusMetrics, i64, String)> = None;
    let mut degraded_notes: Vec<String> = Vec::new();

    for relay in attempts {
        let attempt = collect_relay_metrics_from_single_relay(&config, &relay, machine);
        if attempt.metrics.has_any_value() {
            relay_clear_backoff(&relay);
            let latest_timestamp = attempt.latest_timestamp.unwrap_or_default();
            let source = attempt
                .metrics
                .source
                .clone()
                .unwrap_or_else(|| format!("relay-api:{}@{}", relay.name, relay.ip));
            match &best {
                Some((_, best_timestamp, _)) if latest_timestamp < *best_timestamp => {}
                _ => best = Some((attempt.metrics, latest_timestamp, source)),
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

    if let Some((mut metrics, _, source)) = best {
        metrics.source = Some(source);
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
    let local_metrics = collect_local_prometheus_metrics(machine);
    if local_metrics.has_any_value() {
        return local_metrics;
    }

    let merged_note = match (relay_metrics.note, local_metrics.note) {
        (Some(relay), Some(local)) => Some(format!("relay: {relay}; local: {local}")),
        (Some(relay), None) => Some(relay),
        (None, Some(local)) => Some(local),
        (None, None) => None,
    };

    PrometheusMetrics {
        source: None,
        note: merged_note,
        ..PrometheusMetrics::default()
    }
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

fn collect_machine_snapshot(
    conn: &Connection,
    machine: &MachineRow,
) -> Result<MonitorSnapshot, AppError> {
    let collected_at_epoch = current_epoch_seconds()?;
    let collected_at = sqlite_datetime(conn, collected_at_epoch)?;
    let previous = get_previous_health_sample(conn, machine.id)?;
    let runtime_context = collect_runtime_monitor_context(machine);
    let remote_cmd = format!(
        "docker exec cardano-node cardano-cli query tip --socket-path /ipc/node.socket --{}",
        machine.network
    );

    let tip_raw = match ssh_exec(machine, remote_cmd.as_str()) {
        Ok(output) => output,
        Err(err) => {
            if let Some((sync_stage, status, recovered_sync_progress, sync_note)) =
                recover_restore_stage_from_tip_error(
                    previous.as_ref(),
                    runtime_context.as_ref(),
                    collected_at_epoch,
                )
            {
                let prometheus = collect_prometheus_metrics(conn, machine);
                insert_health_sample(
                    conn,
                    machine.id,
                    None,
                    recovered_sync_progress,
                    sync_stage.as_str(),
                    sync_note.as_deref(),
                    collected_at_epoch,
                )?;
                return Ok(MonitorSnapshot {
                    machine_id: machine.id,
                    machine_name: machine.name.clone(),
                    role: machine.role.clone(),
                    network: machine.network.clone(),
                    block_height: None,
                    sync_progress: recovered_sync_progress,
                    blocks_per_minute: None,
                    health_level: "warning".into(),
                    status,
                    sync_stage,
                    restore_snapshot_requested: runtime_context
                        .as_ref()
                        .map(|ctx| ctx.restore_snapshot_requested)
                        .unwrap_or(false),
                    stalled: false,
                    collected_at,
                    note: sync_note,
                    epoch: prometheus.epoch,
                    sync_percent: prometheus.sync_percent.or(recovered_sync_progress),
                    tip_diff_blocks: prometheus.tip_diff_blocks,
                    peer_count: prometheus.peer_count,
                    cpu_sys_percent: prometheus.cpu_sys_percent,
                    mem_live_bytes: prometheus.mem_live_bytes,
                    mem_rss_bytes: prometheus.mem_rss_bytes,
                    mem_heap_bytes: prometheus.mem_heap_bytes,
                    gc_minor_total: prometheus.gc_minor_total,
                    gc_major_total: prometheus.gc_major_total,
                    prometheus_source: prometheus.source,
                    prometheus_note: prometheus.note,
                });
            }

            let note = err.to_string();
            return Ok(MonitorSnapshot {
                machine_id: machine.id,
                machine_name: machine.name.clone(),
                role: machine.role.clone(),
                network: machine.network.clone(),
                block_height: None,
                sync_progress: None,
                blocks_per_minute: None,
                health_level: "critical".into(),
                status: determine_status(None, false, Some(note.as_str())),
                sync_stage: "unreachable".into(),
                restore_snapshot_requested: runtime_context
                    .as_ref()
                    .map(|ctx| ctx.restore_snapshot_requested)
                    .unwrap_or(false),
                stalled: false,
                collected_at,
                note: Some(note),
                epoch: None,
                sync_percent: None,
                tip_diff_blocks: None,
                peer_count: None,
                cpu_sys_percent: None,
                mem_live_bytes: None,
                mem_rss_bytes: None,
                mem_heap_bytes: None,
                gc_minor_total: None,
                gc_major_total: None,
                prometheus_source: None,
                prometheus_note: None,
            });
        }
    };

    let (block_height, sync_progress) = match parse_tip(tip_raw.as_str()) {
        Ok(parsed) => parsed,
        Err(err) => {
            let prometheus = collect_prometheus_metrics(conn, machine);
            let note = err.to_string();
            return Ok(MonitorSnapshot {
                machine_id: machine.id,
                machine_name: machine.name.clone(),
                role: machine.role.clone(),
                network: machine.network.clone(),
                block_height: None,
                sync_progress: None,
                blocks_per_minute: None,
                health_level: "critical".into(),
                status: determine_status(None, false, Some(note.as_str())),
                sync_stage: "unreachable".into(),
                restore_snapshot_requested: runtime_context
                    .as_ref()
                    .map(|ctx| ctx.restore_snapshot_requested)
                    .unwrap_or(false),
                stalled: false,
                collected_at,
                note: Some(note),
                epoch: prometheus.epoch,
                sync_percent: prometheus.sync_percent,
                tip_diff_blocks: prometheus.tip_diff_blocks,
                peer_count: prometheus.peer_count,
                cpu_sys_percent: prometheus.cpu_sys_percent,
                mem_live_bytes: prometheus.mem_live_bytes,
                mem_rss_bytes: prometheus.mem_rss_bytes,
                mem_heap_bytes: prometheus.mem_heap_bytes,
                gc_minor_total: prometheus.gc_minor_total,
                gc_major_total: prometheus.gc_major_total,
                prometheus_source: prometheus.source,
                prometheus_note: prometheus.note,
            });
        }
    };

    let blocks_per_minute =
        compute_blocks_per_minute(previous.as_ref(), block_height, collected_at_epoch);
    let stalled = determine_stalled(
        previous.as_ref(),
        block_height,
        sync_progress,
        collected_at_epoch,
    );
    let (sync_stage, sync_note) = determine_sync_stage(
        block_height,
        sync_progress,
        previous.as_ref(),
        runtime_context.as_ref(),
        None,
        collected_at_epoch,
    );
    let status = match sync_stage.as_str() {
        "restore_failed" | "restore_timeout" => "stalled".into(),
        "fallback_syncing" => "syncing".into(),
        _ => determine_status(sync_progress, stalled, None),
    };
    let health_level = determine_health_level(
        status.as_str(),
        sync_stage.as_str(),
        blocks_per_minute,
        stalled,
    );
    let prometheus = collect_prometheus_metrics(conn, machine);
    insert_health_sample(
        conn,
        machine.id,
        block_height,
        sync_progress,
        sync_stage.as_str(),
        sync_note.as_deref(),
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
        health_level,
        status,
        sync_stage,
        restore_snapshot_requested: runtime_context
            .as_ref()
            .map(|ctx| ctx.restore_snapshot_requested)
            .unwrap_or(false),
        stalled,
        collected_at,
        note: sync_note,
        epoch: prometheus.epoch,
        sync_percent: prometheus.sync_percent.or(sync_progress),
        tip_diff_blocks: prometheus.tip_diff_blocks,
        peer_count: prometheus.peer_count,
        cpu_sys_percent: prometheus.cpu_sys_percent,
        mem_live_bytes: prometheus.mem_live_bytes,
        mem_rss_bytes: prometheus.mem_rss_bytes,
        mem_heap_bytes: prometheus.mem_heap_bytes,
        gc_minor_total: prometheus.gc_minor_total,
        gc_major_total: prometheus.gc_major_total,
        prometheus_source: prometheus.source,
        prometheus_note: prometheus.note,
    })
}

#[tauri::command]
pub async fn monitor_snapshot(
    machine_ids: Option<Vec<i64>>,
    db: State<'_, DbState>,
) -> Result<Vec<MonitorSnapshot>, AppError> {
    collect_snapshots_from_db_state(&db, machine_ids).await
}

async fn collect_snapshots_from_db_state(
    db: &DbState,
    machine_ids: Option<Vec<i64>>,
) -> Result<Vec<MonitorSnapshot>, AppError> {
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

    let mut snapshots = Vec::with_capacity(selected.len());
    for machine in selected {
        snapshots.push(collect_machine_snapshot(&conn, &machine)?);
    }
    Ok(snapshots)
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
    let _ = app_handle.emit("monitor:snapshot", &initial);

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
                Ok(snapshots) => {
                    let _ = app_handle_for_task.emit("monitor:snapshot", &snapshots);
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
                    "metric": {"node": "bp-1", "role": "bp", "instance": "10.0.0.12:12798"},
                    "value": [1773374400, "98.41"]
                }]
            }
        });
        let samples = parse_relay_metric_samples(&payload).expect("samples");
        assert_eq!(samples.len(), 1);
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
                node: Some("relay-1".into()),
                role: Some("relay".into()),
                instance: Some("10.0.0.10:12798".into()),
                timestamp: Some(100),
                value: 10.0,
            },
            RelayMetricSample {
                node: Some("bp-1".into()),
                role: Some("bp".into()),
                instance: Some("10.0.0.12:12798".into()),
                timestamp: Some(120),
                value: 98.41,
            },
        ];
        let selected = select_relay_metric_sample(&machine, &samples).expect("selected");
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
}
