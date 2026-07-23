//! S0027 Fleet Deploy contracts shared by Inspect, Apply and Check.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::ConfigPaths;
use crate::convention::{Allowlist, DeployBootstrapContract, DeployNetworkContract};
use crate::domain::{Machine, MachineRole, Network, PoolSpec, TopologyMode};
use crate::output::{self, ToolOutput};
use crate::readiness::NodeLifecycle;
use crate::{OuroError, Result};

pub const TOPOLOGY_DESTINATION: &str = "/ouro/topology.json";
pub const DATA_DESTINATION: &str = "/data/db";
pub const IPC_DESTINATION: &str = "/ipc";
pub const KEYS_DESTINATION: &str = "/opt/cardano/config/keys";
const INSPECT_TIMEOUT_S: u32 = 30;
const HOST_PREPARE_TIMEOUT_S: u32 = 300;
const UFW_TIMEOUT_S: u32 = 30;
const ARTIFACT_TIMEOUT_S: u32 = 600;
const COMPOSE_UP_TIMEOUT_S: u32 = 120;

const INSPECT_SCRIPT: &str = r#"set -u
ssh_port=$1
machine_id=$2
aggregator=$3
p2p_port=$4
path_state() {
  path=$1
  if test -L "$path"; then printf symlink
  elif ! test -e "$path"; then printf absent
  elif test -d "$path"; then
    if find "$path" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null | grep -q .; then
      printf nonempty_dir
    else
      printf empty_dir
    fi
  elif test -f "$path"; then printf file
  else printf unsupported
  fi
}
if test "$(id -u)" = 0; then privilege=root
elif sudo -n true >/dev/null 2>&1; then privilege=sudo_n
else privilege=none
fi
if docker info >/dev/null 2>&1; then docker_mode=user
elif sudo -n docker info >/dev/null 2>&1; then docker_mode=sudo_n
elif command -v docker >/dev/null 2>&1; then docker_mode=unavailable
else docker_mode=missing
fi
docker_run() {
  case "$docker_mode" in
    user) docker "$@" ;;
    sudo_n) sudo -n docker "$@" ;;
    *) return 1 ;;
  esac
}
os_id=
os_version=
if test -r /etc/os-release; then
  os_id=$(sed -n 's/^ID=//p' /etc/os-release | tr -d '"'"'" | head -n1)
  os_version=$(sed -n 's/^VERSION_ID=//p' /etc/os-release | tr -d '"'"'" | head -n1)
fi
memory_bytes=$(awk '/^MemTotal:/ {print $2 * 1024; exit}' /proc/meminfo 2>/dev/null || true)
disk_path=/
test ! -e /opt/ouro || disk_path=/opt/ouro
free_disk_bytes=$(df -PB1 "$disk_path" 2>/dev/null | awk 'NR==2 {print $4}')
chrony_installed=false
chrony_synced=false
chrony_offset=
if command -v chronyc >/dev/null 2>&1; then
  chrony_installed=true
  tracking=$(timeout 5s chronyc -n tracking 2>/dev/null || true)
  leap=$(printf '%s\n' "$tracking" | awk -F: '/^Leap status/ {gsub(/^[ \t]+/,"",$2); print $2}')
  chrony_offset=$(printf '%s\n' "$tracking" | awk -F: '/^System time/ {gsub(/^[ \t]+/,"",$2); split($2,a," "); print a[1]}')
  test "$leap" = Normal && chrony_synced=true
fi
compose_v2=false
if docker_run compose version >/dev/null 2>&1; then compose_v2=true; fi
ufw_state=unavailable
ufw_rules=
if command -v ufw >/dev/null 2>&1; then
  ufw_rules=$(if test "$privilege" = root; then ufw status 2>/dev/null; else sudo -n ufw status 2>/dev/null; fi)
  ufw_line=$(printf '%s\n' "$ufw_rules" | head -n1)
  case "$ufw_line" in *active*) ufw_state=active ;; *inactive*) ufw_state=inactive ;; esac
fi
ssh_listener=false
if ss -Hln 2>/dev/null | awk '{print $4}' | grep -Eq "(^|:)$ssh_port$"; then ssh_listener=true; fi
p2p_listener=false
if test "$p2p_port" -gt 0 && ss -Hln 2>/dev/null | awk '{print $4}' | grep -Eq "(^|:)$p2p_port$"; then p2p_listener=true; fi
metrics_listener=false
if ss -Hln 2>/dev/null | awk '{print $4}' | grep -Eq '(^|:)12798$'; then metrics_listener=true; fi
metrics_public_listener=false
if ss -Hln 2>/dev/null | awk '{print $4}' | grep -Eq '(^0\.0\.0\.0:12798$|^\*:12798$|^\[?::\]?:12798$)'; then metrics_public_listener=true; fi
ufw_p2p_allow=false
if test "$p2p_port" -gt 0 && printf '%s\n' "$ufw_rules" | grep -Eq "^${p2p_port}/tcp[[:space:]]+ALLOW[[:space:]]+IN"; then ufw_p2p_allow=true; fi
ufw_metrics_allow=false
if printf '%s\n' "$ufw_rules" | grep -Eq '^12798/tcp[[:space:]]+ALLOW[[:space:]]+IN'; then ufw_metrics_allow=true; fi
aggregator_reachable=false
if command -v curl >/dev/null 2>&1 && curl -fsS -o /dev/null --max-time 5 "$aggregator" 2>/dev/null; then
  aggregator_reachable=true
fi
ouro_state=$(path_state /opt/ouro)
legacy_cardano_state=$(path_state /opt/cardano)
legacy_home_config_state=$(path_state /home/cardano/node-config)
db_state=$(path_state /opt/ouro/db)
if test -e /opt/ouro/db/protocolMagicId; then db_state=populated; fi
keys_state=$(path_state /opt/ouro/keys)
keys_mode=
if test -d /opt/ouro/keys && ! test -L /opt/ouro/keys; then keys_mode=$(stat -c %a /opt/ouro/keys 2>/dev/null || true); fi
identity_marker=
if test -f /opt/ouro/fleet-identity.json && ! test -L /opt/ouro/fleet-identity.json; then
  marker_size=$(wc -c </opt/ouro/fleet-identity.json 2>/dev/null || printf 999999)
  if test "$marker_size" -le 8192; then identity_marker=$(tr -d '\r\n' </opt/ouro/fleet-identity.json); fi
fi
node_container_count=0
owned_count=0
owned_running=false
owned_image=
owned_role=
owned_lifecycle=
owned_network=
owned_desired_digest=
if test "$docker_mode" = user || test "$docker_mode" = sudo_n; then
  node_container_count=$(docker_run ps -a --format '{{.Image}}|{{.Names}}' 2>/dev/null | awk -F'|' 'tolower($0) ~ /cardano-node/ {n++} END {print n+0}')
  owned_ids=$(docker_run ps -a -q --filter "label=io.ouro.machine-id=$machine_id" 2>/dev/null || true)
  owned_count=$(printf '%s\n' "$owned_ids" | awk 'NF {n++} END {print n+0}')
  if test "$owned_count" = 1; then
    owned_id=$(printf '%s\n' "$owned_ids" | awk 'NF {print; exit}')
    owned_running=$(docker_run inspect --format '{{.State.Running}}' "$owned_id" 2>/dev/null || printf false)
    owned_image=$(docker_run inspect --format '{{.Image}}' "$owned_id" 2>/dev/null || true)
    owned_role=$(docker_run inspect --format '{{index .Config.Labels "io.ouro.role"}}' "$owned_id" 2>/dev/null || true)
    owned_lifecycle=$(docker_run inspect --format '{{index .Config.Labels "io.ouro.lifecycle"}}' "$owned_id" 2>/dev/null || true)
    owned_network=$(docker_run inspect --format '{{index .Config.Labels "io.ouro.network"}}' "$owned_id" 2>/dev/null || true)
    owned_desired_digest=$(docker_run inspect --format '{{index .Config.Labels "io.ouro.desired-digest"}}' "$owned_id" 2>/dev/null || true)
  fi
fi
printf '%s\n' \
  "schema=ouro-deploy-inspect-v1" \
  "os_id=$os_id" "os_version=$os_version" "arch=$(uname -m)" \
  "memory_bytes=$memory_bytes" "free_disk_bytes=$free_disk_bytes" \
  "privilege=$privilege" "docker_mode=$docker_mode" "compose_v2=$compose_v2" \
  "chrony_installed=$chrony_installed" "chrony_synced=$chrony_synced" \
  "chrony_offset=$chrony_offset" "ufw_state=$ufw_state" \
  "ssh_listener=$ssh_listener" "aggregator_reachable=$aggregator_reachable" \
  "p2p_listener=$p2p_listener" "metrics_listener=$metrics_listener" \
  "metrics_public_listener=$metrics_public_listener" \
  "ufw_p2p_allow=$ufw_p2p_allow" "ufw_metrics_allow=$ufw_metrics_allow" \
  "ouro_state=$ouro_state" "legacy_cardano_state=$legacy_cardano_state" \
  "legacy_home_config_state=$legacy_home_config_state" "db_state=$db_state" \
  "keys_state=$keys_state" "keys_mode=$keys_mode" "identity_marker=$identity_marker" \
  "node_container_count=$node_container_count" "owned_count=$owned_count" \
  "owned_running=$owned_running" "owned_image=$owned_image" "owned_role=$owned_role" \
  "owned_lifecycle=$owned_lifecycle" "owned_network=$owned_network" \
  "owned_desired_digest=$owned_desired_digest"
"#;

const HOST_PREPARE_SCRIPT: &str = r#"set -eu
role=$1
marker=$2
as_root() {
  if test "$(id -u)" = 0; then "$@"; else sudo -n "$@"; fi
}
case "$role" in bp|relay) ;; *) exit 64 ;; esac
test -r /etc/os-release
os_id=$(sed -n 's/^ID=//p' /etc/os-release | tr -d '"'"'" | head -n1)
os_version=$(sed -n 's/^VERSION_ID=//p' /etc/os-release | tr -d '"'"'" | head -n1)
test "$os_id" = ubuntu
case "$os_version" in 22.04|24.04) ;; *) exit 65 ;; esac
need_packages=false
command -v docker >/dev/null 2>&1 || need_packages=true
command -v chronyc >/dev/null 2>&1 || need_packages=true
command -v ufw >/dev/null 2>&1 || need_packages=true
command -v curl >/dev/null 2>&1 || need_packages=true
if command -v docker >/dev/null 2>&1; then
  if docker compose version >/dev/null 2>&1; then :
  elif sudo -n docker compose version >/dev/null 2>&1; then :
  else need_packages=true
  fi
fi
packages_changed=false
if test "$need_packages" = true; then
  as_root env DEBIAN_FRONTEND=noninteractive apt-get update
  as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    docker.io docker-compose-v2 chrony ufw ca-certificates curl
  packages_changed=true
fi
as_root systemctl enable --now docker.service
as_root systemctl enable --now chrony.service
if docker info >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
  docker_mode=user
elif sudo -n docker info >/dev/null 2>&1 && sudo -n docker compose version >/dev/null 2>&1; then
  docker_mode=sudo_n
else
  exit 66
fi
tracking=$(timeout 5s chronyc -n tracking)
leap=$(printf '%s\n' "$tracking" | awk -F: '/^Leap status/ {gsub(/^[ \t]+/,"",$2); print $2}')
offset=$(printf '%s\n' "$tracking" | awk -F: '/^System time/ {gsub(/^[ \t]+/,"",$2); split($2,a," "); print a[1]}')
test "$leap" = Normal
test -n "$offset"
awk -v value="$offset" 'BEGIN { if (value < 0) value=-value; exit !(value <= 1.0) }'
if test -L /opt/ouro; then exit 67; fi
if test -e /opt/ouro && ! test -d /opt/ouro; then exit 67; fi
if ! test -e /opt/ouro; then as_root install -d -m 0755 /opt/ouro; fi
marker_path=/opt/ouro/fleet-identity.json
if test -e "$marker_path"; then
  test ! -L "$marker_path"
  current=$(tr -d '\r\n' <"$marker_path")
  test "$current" = "$marker"
else
  marker_tmp=$(as_root mktemp /opt/ouro/.fleet-identity.XXXXXX)
  printf '%s\n' "$marker" | as_root tee "$marker_tmp" >/dev/null
  as_root chmod 0644 "$marker_tmp"
  as_root mv "$marker_tmp" "$marker_path"
fi
for path in /opt/ouro/db /opt/ouro/ipc; do
  test ! -L "$path"
  if test -e "$path"; then test -d "$path"; else as_root install -d -m 0755 "$path"; fi
done
if test "$role" = bp; then
  test ! -L /opt/ouro/keys
  if test -e /opt/ouro/keys; then
    test -d /opt/ouro/keys
    as_root chmod 0700 /opt/ouro/keys
  else
    as_root install -d -m 0700 /opt/ouro/keys
  fi
fi
printf '%s\n' \
  "schema=ouro-deploy-host-prepare-v1" \
  "packages_changed=$packages_changed" \
  "docker_mode=$docker_mode" \
  "chrony_synced=true" \
  "chrony_offset=$offset" \
  "marker_installed=true"
"#;

const UFW_APPLY_SCRIPT: &str = r#"set -eu
role=$1
ssh_port=$2
p2p_port=$3
as_root() {
  if test "$(id -u)" = 0; then "$@"; else sudo -n "$@"; fi
}
case "$role" in bp|relay) ;; *) exit 64 ;; esac
case "$ssh_port" in ''|*[!0-9]*) exit 64 ;; esac
case "$p2p_port" in ''|*[!0-9]*) exit 64 ;; esac
status=$(as_root ufw status)
was_active=false
printf '%s\n' "$status" | head -n1 | grep -q 'Status: active' && was_active=true
added_ssh=false
added_p2p=false
enabled=false
completed=false
restore_delta() {
  test "$completed" = true && return 0
  if test "$added_p2p" = true; then
    as_root ufw --force delete allow "${p2p_port}/tcp" >/dev/null 2>&1 || true
  fi
  if test "$added_ssh" = true; then
    as_root ufw --force delete allow "${ssh_port}/tcp" >/dev/null 2>&1 || true
  fi
  if test "$enabled" = true; then
    as_root ufw --force disable >/dev/null 2>&1 || true
  fi
}
trap restore_delta EXIT HUP INT TERM
if ! printf '%s\n' "$status" | grep -Eq "^${ssh_port}/tcp[[:space:]]+ALLOW[[:space:]]+IN"; then
  as_root ufw allow "${ssh_port}/tcp" comment ouro-deploy-ssh
  added_ssh=true
fi
if test "$role" = relay; then
  test "$p2p_port" -gt 0
  if ! printf '%s\n' "$status" | grep -Eq "^${p2p_port}/tcp[[:space:]]+ALLOW[[:space:]]+IN"; then
    as_root ufw allow "${p2p_port}/tcp" comment ouro-deploy-relay-p2p
    added_p2p=true
  fi
fi
if test "$was_active" = false; then
  as_root ufw --force enable
  enabled=true
fi
as_root ufw reload
completed=true
printf '%s\n' \
  "schema=ouro-deploy-ufw-v1" \
  "added_ssh=$added_ssh" \
  "added_p2p=$added_p2p" \
  "enabled=$enabled"
"#;

const UFW_ROLLBACK_SCRIPT: &str = r#"set -eu
role=$1
ssh_port=$2
p2p_port=$3
added_ssh=$4
added_p2p=$5
enabled=$6
as_root() {
  if test "$(id -u)" = 0; then "$@"; else sudo -n "$@"; fi
}
if test "$added_p2p" = true && test "$role" = relay; then
  as_root ufw --force delete allow "${p2p_port}/tcp"
fi
if test "$added_ssh" = true; then
  as_root ufw --force delete allow "${ssh_port}/tcp"
fi
if test "$enabled" = true; then
  as_root ufw --force disable
else
  as_root ufw reload
fi
printf '%s\n' "schema=ouro-deploy-ufw-rollback-v1" "restored=true"
"#;

const ARTIFACT_INSTALL_SCRIPT: &str = r#"set -eu
compose_content=$1
topology_content=$2
image_ref=$3
expected_config=$4
project=$5
docker_run() {
  if docker info >/dev/null 2>&1; then docker "$@"; else sudo -n docker "$@"; fi
}
as_root() {
  if test "$(id -u)" = 0; then "$@"; else sudo -n "$@"; fi
}
case "$project" in ouro-[a-z0-9-]*) ;; *) exit 64 ;; esac
test ! -L /opt/ouro/compose.yaml
test ! -L /opt/ouro/topology.json
compose_tmp=$(as_root mktemp /opt/ouro/.compose.XXXXXX)
topology_tmp=$(as_root mktemp /opt/ouro/.topology.XXXXXX)
cleanup() {
  as_root rm -f "$compose_tmp" "$topology_tmp" >/dev/null 2>&1 || true
}
trap cleanup EXIT HUP INT TERM
printf '%s' "$compose_content" | as_root tee "$compose_tmp" >/dev/null
printf '%s' "$topology_content" | as_root tee "$topology_tmp" >/dev/null
as_root chmod 0644 "$compose_tmp" "$topology_tmp"
docker_run compose -p "$project" -f "$compose_tmp" config -q
as_root mv "$topology_tmp" /opt/ouro/topology.json
as_root mv "$compose_tmp" /opt/ouro/compose.yaml
docker_run pull "$image_ref"
actual_config=$(docker_run image inspect --format '{{.Id}}' "$image_ref")
test "$actual_config" = "$expected_config"
printf '%s\n' \
  "schema=ouro-deploy-artifacts-v1" \
  "compose_valid=true" \
  "topology_installed=true" \
  "image_config=$actual_config"
"#;

const COMPOSE_UP_SCRIPT: &str = r#"set -eu
project=$1
docker_run() {
  if docker info >/dev/null 2>&1; then docker "$@"; else sudo -n docker "$@"; fi
}
case "$project" in ouro-[a-z0-9-]*) ;; *) exit 64 ;; esac
test -f /opt/ouro/compose.yaml
test ! -L /opt/ouro/compose.yaml
docker_run compose -p "$project" -f /opt/ouro/compose.yaml up -d --no-build --pull never cardano-node
printf '%s\n' "schema=ouro-deploy-compose-up-v1" "started=true"
"#;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FleetIdentityMarker {
    schema_version: u8,
    fleet_identity_digest: String,
    machine_id: String,
    role: String,
    network: String,
    genesis_identity: String,
    repository: String,
    platform: String,
    platform_manifest_digest: String,
    image_config_digest: String,
}

#[derive(Debug, Clone, Serialize)]
struct InspectFacts {
    os_id: String,
    os_version: String,
    platform: String,
    memory_bytes: u64,
    free_disk_bytes: u64,
    privilege: String,
    docker_mode: String,
    compose_v2: bool,
    chrony_installed: bool,
    chrony_synced: bool,
    chrony_offset_seconds: Option<f64>,
    ufw_state: String,
    ssh_listener: bool,
    aggregator_reachable: bool,
    p2p_listener: bool,
    metrics_listener: bool,
    metrics_public_listener: bool,
    ufw_p2p_allow: bool,
    ufw_metrics_allow: bool,
    ouro_state: String,
    legacy_cardano_state: String,
    legacy_home_config_state: String,
    db_state: String,
    keys_state: String,
    keys_mode: String,
    identity_marker: Option<FleetIdentityMarker>,
    node_container_count: u32,
    owned_count: u32,
    owned_running: bool,
    owned_image: String,
    owned_role: String,
    owned_lifecycle: String,
    owned_network: String,
    owned_desired_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HostConvergence {
    pub packages_changed: bool,
    pub docker_mode: String,
    pub chrony_offset_seconds: f64,
    pub marker_installed: bool,
    pub ufw_added_ssh: bool,
    pub ufw_added_p2p: bool,
    pub ufw_enabled: bool,
    pub fresh_ssh_verified: bool,
}

#[derive(Debug, Clone)]
struct UfwDelta {
    added_ssh: bool,
    added_p2p: bool,
    enabled: bool,
}

pub fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("inspect") => run_inspect(&args[1..]),
        Some("apply") => run_apply(&args[1..]),
        _ => Err(OuroError::InvalidArgs(
            "expected deploy inspect|apply|check --spec <pool-spec>".into(),
        )),
    }
}

fn run_inspect(args: &[String]) -> Result<()> {
    if args.len() != 2 || args[0] != "--spec" {
        return Err(OuroError::InvalidArgs(
            "deploy inspect requires exactly --spec <pool-spec>".into(),
        ));
    }
    let spec = PoolSpec::from_file(std::path::Path::new(&args[1]))?;
    if spec.topology_mode != TopologyMode::P2p {
        return Err(OuroError::Validation(
            "Fleet Deploy requires topology_mode: p2p".into(),
        ));
    }
    let paths = ConfigPaths::discover();
    let mut untrusted = Vec::new();
    for machine in &spec.machines {
        let fingerprints = crate::ssh::existing_host_fingerprints(
            &paths.known_hosts,
            &machine.ssh.host,
            machine.ssh.port,
        )?;
        if fingerprints.is_empty() {
            untrusted.push(serde_json::json!({
                "machine": machine.id,
                "host": machine.ssh.host,
                "port": machine.ssh.port,
                "user_action": format!(
                    "ouro-ops ssh trust --spec {} --node {}",
                    args[1], machine.id
                ),
            }));
        }
    }
    if !untrusted.is_empty() {
        output::print_json(
            &ToolOutput::failure(
                "ouro.deploy.inspect",
                "ssh_host_key_untrusted",
                "one or more targets are absent from Ouro known_hosts; no target was contacted",
            )
            .with_data(serde_json::json!({
                "classification": "blocked",
                "reason": "ssh_host_key_untrusted",
                "targets": untrusted,
                "target_contacted": false,
            })),
        )?;
        return Err(OuroError::Reported(10));
    }
    let catalog = crate::convention::fetch_release_catalog()?;
    let policy_digest = catalog.policy.signed_digest()?;
    let fleet_digest = fleet_identity_digest(&spec)?;
    let genesis_digest = genesis_identity_digest(&spec)?;
    let (_, _, bootstrap) = catalog.policy.recommended_deploy_for("linux/amd64")?;
    let network_contract = bootstrap
        .networks
        .get(spec.pool.network.as_str())
        .ok_or_else(|| {
            OuroError::Validation("signed catalog omitted the pool network bootstrap facts".into())
        })?;
    if network_contract.genesis_hash != spec.pool.genesis_hashes.shelley {
        return Err(OuroError::Validation(
            "pool spec genesis does not match the signed image bootstrap contract".into(),
        ));
    }
    let mut nodes = Vec::new();
    let mut any_blocked = false;
    let mut all_complete = true;
    for machine in &spec.machines {
        match inspect_machine(
            &spec,
            machine,
            &paths,
            &catalog.policy,
            &fleet_digest,
            &genesis_digest,
            &network_contract.mithril_aggregator,
        ) {
            Ok(value) => {
                any_blocked |= value
                    .get("classification")
                    .and_then(serde_json::Value::as_str)
                    == Some("blocked");
                all_complete &= value
                    .get("deployment_complete")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true);
                nodes.push(value);
            }
            Err(error) => {
                any_blocked = true;
                all_complete = false;
                nodes.push(serde_json::json!({
                    "machine": machine.id,
                    "role": role_name(machine.role),
                    "classification": "blocked",
                    "deployment_complete": false,
                    "reasons": ["inspect_failed"],
                    "detail": error.to_string(),
                }));
            }
        }
    }
    let classification = if any_blocked {
        "blocked"
    } else if all_complete {
        "already_deployed"
    } else {
        "applicable"
    };
    output::print_json(
        &ToolOutput::ok("ouro.deploy.inspect", false).with_data(serde_json::json!({
            "classification": classification,
            "fleet_identity_digest": fleet_digest,
            "network": spec.pool.network.as_str(),
            "genesis_identity": genesis_digest,
            "signed_policy": {
                "version": catalog.policy.allowlist_version,
                "digest": policy_digest,
                "source": catalog.source,
                "repository": catalog.policy.repository,
            },
            "nodes": nodes,
            "target_writes": false,
        })),
    )
}

fn run_apply(args: &[String]) -> Result<()> {
    if args.len() != 2 || args[0] != "--spec" {
        return Err(OuroError::InvalidArgs(
            "deploy apply requires exactly --spec <pool-spec>".into(),
        ));
    }
    let spec = PoolSpec::from_file(std::path::Path::new(&args[1]))?;
    if spec.topology_mode != TopologyMode::P2p {
        return Err(OuroError::Validation(
            "Fleet Deploy requires topology_mode: p2p".into(),
        ));
    }
    let paths = ConfigPaths::discover();
    let mut missing_trust = Vec::new();
    for machine in &spec.machines {
        if crate::ssh::existing_host_fingerprints(
            &paths.known_hosts,
            &machine.ssh.host,
            machine.ssh.port,
        )?
        .is_empty()
        {
            missing_trust.push(machine.id.clone());
        }
    }
    if !missing_trust.is_empty() {
        output::print_json(
            &ToolOutput::failure(
                "ouro.deploy.apply",
                "ssh_host_key_untrusted",
                "all Fleet host keys must be user-trusted before Apply; no target was contacted",
            )
            .with_data(serde_json::json!({
                "classification": "blocked",
                "machines": missing_trust,
                "target_contacted": false,
                "target_writes": false,
            })),
        )?;
        return Err(OuroError::Reported(10));
    }
    let catalog = crate::convention::fetch_release_catalog()?;
    let fleet_digest = fleet_identity_digest(&spec)?;
    let genesis_digest = genesis_identity_digest(&spec)?;
    let (_, _, bootstrap) = catalog.policy.recommended_deploy_for("linux/amd64")?;
    let aggregator = bootstrap
        .networks
        .get(spec.pool.network.as_str())
        .ok_or_else(|| OuroError::Validation("signed network bootstrap facts are absent".into()))?
        .mithril_aggregator
        .clone();

    let mut inspections = BTreeMap::new();
    let mut blocked = Vec::new();
    let mut complete_count = 0_usize;
    for machine in &spec.machines {
        match inspect_machine(
            &spec,
            machine,
            &paths,
            &catalog.policy,
            &fleet_digest,
            &genesis_digest,
            &aggregator,
        ) {
            Ok(value) => {
                if value["classification"] == "blocked" {
                    blocked.push(serde_json::json!({
                        "machine": machine.id,
                        "reasons": value["reasons"],
                    }));
                }
                if value["deployment_complete"] == true {
                    complete_count += 1;
                }
                inspections.insert(machine.id.clone(), value);
            }
            Err(error) => blocked.push(serde_json::json!({
                "machine": machine.id,
                "reasons": ["inspect_failed"],
                "detail": error.to_string(),
            })),
        }
    }
    if !blocked.is_empty() {
        output::print_json(
            &ToolOutput::failure(
                "ouro.deploy.apply",
                "deploy_inspect_blocked",
                "Apply refused because one or more read-only Inspect preconditions failed",
            )
            .with_data(serde_json::json!({
                "classification": "blocked",
                "nodes": blocked,
                "target_writes": false,
            })),
        )?;
        return Err(OuroError::Reported(10));
    }
    if complete_count == spec.machines.len() {
        output::print_json(
            &ToolOutput::failure(
                "ouro.deploy.apply",
                "already_deployed",
                "the complete exact Fleet already exists; use deploy check or S0026 Upgrade",
            )
            .with_data(serde_json::json!({
                "classification": "already_deployed",
                "fleet_identity_digest": fleet_digest,
                "target_writes": false,
            })),
        )?;
        return Err(OuroError::Reported(10));
    }

    let mut states = BTreeMap::<String, serde_json::Value>::new();
    let mut any_write_attempted = false;
    let mut any_failure = false;
    for machine in &spec.machines {
        let inspected = inspections
            .get(&machine.id)
            .ok_or_else(|| OuroError::Validation("internal Inspect result is absent".into()))?;
        if inspected["deployment_complete"] == true {
            states.insert(
                machine.id.clone(),
                serde_json::json!({
                    "machine": machine.id,
                    "role": role_name(machine.role),
                    "configuration": "unchanged",
                    "compose_up": "unchanged",
                    "deployment_complete_before_apply": true,
                }),
            );
            continue;
        }
        let selection: SignedDeploySelection =
            serde_json::from_value(inspected["selection"].clone()).map_err(|error| {
                OuroError::Validation(format!("cannot consume signed Inspect selection: {error}"))
            })?;
        let desired_digest = inspected["desired_digest"]
            .as_str()
            .ok_or_else(|| OuroError::Validation("Inspect desired digest is absent".into()))?;
        let topology = render_deploy_topology(&spec, machine, &selection)?;
        let topology_content = serde_json::to_string_pretty(&topology).map_err(|error| {
            OuroError::Validation(format!("cannot render fixed topology: {error}"))
        })?;
        let compose_content = render_compose(&spec, machine, &selection, desired_digest)?;
        any_write_attempted = true;
        match converge_host(
            &spec,
            machine,
            &paths,
            &selection,
            &fleet_digest,
            &genesis_digest,
        )
        .and_then(|host| {
            install_deployment_artifacts(
                machine,
                &paths,
                &selection,
                &compose_content,
                &topology_content,
            )
            .map(|artifacts| (host, artifacts))
        }) {
            Ok((host, artifacts)) => {
                states.insert(
                    machine.id.clone(),
                    serde_json::json!({
                        "machine": machine.id,
                        "role": role_name(machine.role),
                        "configuration": "succeeded",
                        "compose_up": "pending",
                        "host": host,
                        "artifacts": artifacts,
                        "desired_digest": desired_digest,
                        "deployment_complete_before_apply": false,
                    }),
                );
            }
            Err(error) => {
                any_failure = true;
                states.insert(
                    machine.id.clone(),
                    serde_json::json!({
                        "machine": machine.id,
                        "role": role_name(machine.role),
                        "configuration": "failed",
                        "compose_up": "skipped",
                        "detail": error.to_string(),
                        "deployment_complete_before_apply": false,
                    }),
                );
            }
        }
    }

    let mut relay_available = spec.machines.iter().any(|machine| {
        machine.role == MachineRole::Relay
            && inspections
                .get(&machine.id)
                .is_some_and(|value| value["deployment_complete"] == true)
    });
    for machine in spec
        .machines
        .iter()
        .filter(|machine| machine.role == MachineRole::Relay)
    {
        let configuration_succeeded =
            states[&machine.id]["configuration"].as_str() == Some("succeeded");
        if !configuration_succeeded {
            continue;
        }
        match start_compose(machine, &paths) {
            Ok(()) => {
                relay_available = true;
                set_state_field(&mut states, &machine.id, "compose_up", "succeeded")?;
            }
            Err(error) => {
                any_failure = true;
                set_state_field(&mut states, &machine.id, "compose_up", "failed")?;
                set_state_field(&mut states, &machine.id, "detail", &error.to_string())?;
            }
        }
    }
    let bp = spec
        .machines
        .iter()
        .find(|machine| machine.role == MachineRole::Bp)
        .ok_or_else(|| OuroError::Validation("Fleet BP is absent".into()))?;
    if states[&bp.id]["configuration"].as_str() == Some("succeeded") {
        if relay_available {
            match start_compose(bp, &paths) {
                Ok(()) => set_state_field(&mut states, &bp.id, "compose_up", "succeeded")?,
                Err(error) => {
                    any_failure = true;
                    set_state_field(&mut states, &bp.id, "compose_up", "failed")?;
                    set_state_field(&mut states, &bp.id, "detail", &error.to_string())?;
                }
            }
        } else {
            any_failure = true;
            set_state_field(
                &mut states,
                &bp.id,
                "compose_up",
                "skipped_no_relay_command_succeeded",
            )?;
        }
    }
    let nodes = spec
        .machines
        .iter()
        .filter_map(|machine| states.remove(&machine.id))
        .collect::<Vec<_>>();
    let data = serde_json::json!({
        "classification": if any_failure { "partial_failure" } else { "command_success" },
        "fleet_identity_digest": fleet_digest,
        "order": ["configure_all", "relay_compose_up", "bootstrap_bp_compose_up"],
        "intermediate_readiness_checks": false,
        "nodes": nodes,
        "target_writes": any_write_attempted,
    });
    if any_failure {
        output::print_json(
            &ToolOutput::failure(
                "ouro.deploy.apply",
                "deploy_apply_partial_failure",
                "Apply completed all safe independent steps but one or more node commands failed",
            )
            .with_data(data),
        )?;
        Err(OuroError::Reported(10))
    } else {
        output::print_json(
            &ToolOutput::ok("ouro.deploy.apply", any_write_attempted).with_data(data),
        )
    }
}

fn set_state_field(
    states: &mut BTreeMap<String, serde_json::Value>,
    machine: &str,
    key: &str,
    value: &str,
) -> Result<()> {
    states
        .get_mut(machine)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| OuroError::Validation("internal Apply state is absent".into()))?
        .insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    Ok(())
}

fn inspect_machine(
    spec: &PoolSpec,
    machine: &Machine,
    paths: &ConfigPaths,
    policy: &Allowlist,
    fleet_digest: &str,
    genesis_digest: &str,
    aggregator: &str,
) -> Result<serde_json::Value> {
    let credential =
        crate::secrets::credential_status(&paths.credentials_dir, machine.ssh.key_ref.name())?;
    if !credential.usable {
        return Ok(serde_json::json!({
            "machine": machine.id,
            "role": role_name(machine.role),
            "classification": "blocked",
            "deployment_complete": false,
            "reasons": ["ssh_credential_unusable"],
            "credential_ref": machine.ssh.key_ref,
        }));
    }
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let command = vec![
        "sh".into(),
        "-c".into(),
        INSPECT_SCRIPT.into(),
        "ouro-deploy-inspect".into(),
        machine.ssh.port.to_string(),
        machine.id.clone(),
        aggregator.to_string(),
        machine
            .public_endpoint
            .as_ref()
            .map_or(0, |endpoint| endpoint.port)
            .to_string(),
    ];
    let outcome = crate::ssh::SshRunner::new(false).diag_exec(
        &machine.ssh,
        &key,
        &paths.known_hosts,
        &command,
        INSPECT_TIMEOUT_S,
    )?;
    if outcome.status != 0 {
        return Err(OuroError::Validation(format!(
            "strict SSH inspect failed with status {}; host key, account, privilege or reachability requires operator correction",
            outcome.status
        )));
    }
    let facts = parse_inspect_facts(&outcome.stdout)?;
    let mut reasons = Vec::new();
    if facts.os_id != "ubuntu" || !matches!(facts.os_version.as_str(), "22.04" | "24.04") {
        reasons.push("unsupported_ubuntu_release");
    }
    if facts.privilege == "none" {
        reasons.push("passwordless_privilege_unavailable");
    }
    if !facts.ssh_listener {
        reasons.push("declared_ssh_port_not_listening");
    }
    if facts.metrics_public_listener || facts.ufw_metrics_allow {
        reasons.push("metrics_publicly_exposed");
    }
    if matches!(
        facts.legacy_cardano_state.as_str(),
        "nonempty_dir" | "file" | "symlink" | "unsupported"
    ) || matches!(
        facts.legacy_home_config_state.as_str(),
        "nonempty_dir" | "file" | "symlink" | "unsupported"
    ) {
        reasons.push("legacy_or_unknown_deployment_present");
    }
    if facts.db_state == "nonempty_dir" {
        reasons.push("residual_database_without_protocol_magic");
    }
    if machine.role == MachineRole::Bp
        && (matches!(
            facts.keys_state.as_str(),
            "symlink" | "file" | "unsupported"
        ) || (!facts.keys_mode.is_empty()
            && u32::from_str_radix(&facts.keys_mode, 8)
                .map(|mode| mode & 0o002 != 0)
                .unwrap_or(true)))
    {
        reasons.push("unsafe_bp_keys_directory");
    }
    if matches!(facts.db_state.as_str(), "absent" | "empty_dir") && !facts.aggregator_reachable {
        reasons.push("mithril_aggregator_unreachable");
    }

    let mut selection = select_signed_deploy(
        policy,
        &facts.platform,
        spec.pool.network,
        &spec.pool.genesis_hashes.shelley,
    )?;
    if facts.memory_bytes < selection.network.min_memory_bytes {
        reasons.push("insufficient_memory");
    }
    if facts.free_disk_bytes < selection.network.min_free_disk_bytes {
        reasons.push("insufficient_free_disk");
    }

    let expected_role = role_name(machine.role);
    let expected_lifecycle = match lifecycle_for(machine.role) {
        NodeLifecycle::Bootstrap => "bootstrap",
        NodeLifecycle::Operational => "operational",
    };
    let mut marker_valid = false;
    if let Some(marker) = &facts.identity_marker {
        let marker_shape_matches = marker.schema_version == 1
            && marker.fleet_identity_digest == fleet_digest
            && marker.machine_id == machine.id
            && marker.role == expected_role
            && marker.network == spec.pool.network.as_str()
            && marker.genesis_identity == genesis_digest
            && marker.repository == crate::convention::BLINKLABS_REPOSITORY
            && marker.platform == facts.platform;
        if marker_shape_matches {
            match select_pinned_deploy(
                policy,
                &marker.platform,
                spec.pool.network,
                &spec.pool.genesis_hashes.shelley,
                &marker.platform_manifest_digest,
                &marker.image_config_digest,
            ) {
                Ok(pinned) => {
                    selection = pinned;
                    marker_valid = true;
                }
                Err(_) => reasons.push("pinned_deployment_image_no_longer_trusted"),
            }
        } else {
            reasons.push("fleet_identity_marker_mismatch");
        }
    } else if facts.ouro_state == "nonempty_dir" || facts.node_container_count > 0 {
        reasons.push("unknown_nonempty_deployment");
    }
    if facts.node_container_count > facts.owned_count {
        reasons.push("unowned_cardano_node_container");
    }

    if facts.owned_count > 1 {
        reasons.push("multiple_owned_containers");
    }
    if facts.owned_count == 0 && facts.metrics_listener {
        reasons.push("metrics_port_in_use");
    }
    if machine.role == MachineRole::Relay && facts.owned_count == 0 && facts.p2p_listener {
        reasons.push("relay_p2p_port_in_use");
    }
    let expected_desired_digest =
        deployment_desired_digest(spec, machine, &selection, fleet_digest, genesis_digest)?;
    let owned_shape_matches = facts.owned_count == 1
        && marker_valid
        && facts.owned_image == selection.image_config_digest
        && facts.owned_role == expected_role
        && facts.owned_lifecycle == expected_lifecycle
        && facts.owned_network == spec.pool.network.as_str()
        && facts.owned_desired_digest == expected_desired_digest;
    if facts.owned_count == 1 && !owned_shape_matches {
        reasons.push("owned_container_identity_mismatch");
    }
    let deployment_complete = owned_shape_matches && facts.owned_running;
    let classification = if reasons.is_empty() {
        "applicable"
    } else {
        "blocked"
    };
    let mut change_set = Vec::new();
    if reasons.is_empty() && !deployment_complete {
        if facts.docker_mode == "missing" {
            change_set.push("install_docker_engine");
        } else if facts.docker_mode == "unavailable" {
            reasons.push("docker_daemon_unavailable");
        }
        if !facts.compose_v2 {
            change_set.push("install_docker_compose_v2");
        }
        if !facts.chrony_installed
            || !facts.chrony_synced
            || facts
                .chrony_offset_seconds
                .is_none_or(|offset| offset.abs() > 5.0)
        {
            change_set.push("configure_chrony");
        }
        change_set.extend([
            "create_or_validate_owned_paths",
            "converge_ufw",
            "install_topology_and_compose",
            "pull_and_verify_signed_image",
            "compose_up",
        ]);
    }
    let classification = if reasons.is_empty() {
        classification
    } else {
        "blocked"
    };
    let restore_expected = matches!(facts.db_state.as_str(), "absent" | "empty_dir");
    Ok(serde_json::json!({
        "machine": machine.id,
        "role": expected_role,
        "ssh": {
            "host": machine.ssh.host,
            "port": machine.ssh.port,
            "user": machine.ssh.user,
            "credential_ref": machine.ssh.key_ref,
            "strict_host_key": true,
        },
        "classification": classification,
        "deployment_complete": deployment_complete,
        "reasons": reasons,
        "change_set": change_set,
        "facts": facts,
        "selection": selection,
        "desired_digest": expected_desired_digest,
        "mithril": {
            "restore_expected": restore_expected,
            "aggregator": aggregator,
            "host_side_restore": false,
        },
    }))
}

pub(crate) fn converge_host(
    spec: &PoolSpec,
    machine: &Machine,
    paths: &ConfigPaths,
    selection: &SignedDeploySelection,
    fleet_digest: &str,
    genesis_digest: &str,
) -> Result<HostConvergence> {
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let marker = FleetIdentityMarker {
        schema_version: 1,
        fleet_identity_digest: fleet_digest.to_string(),
        machine_id: machine.id.clone(),
        role: role_name(machine.role).to_string(),
        network: spec.pool.network.as_str().to_string(),
        genesis_identity: genesis_digest.to_string(),
        repository: selection.repository.clone(),
        platform: selection.platform.clone(),
        platform_manifest_digest: selection.platform_manifest_digest.clone(),
        image_config_digest: selection.image_config_digest.clone(),
    };
    let marker_json = serde_json::to_string(&marker).map_err(|error| {
        OuroError::Validation(format!("cannot render fleet identity marker: {error}"))
    })?;
    let runner = crate::ssh::SshRunner::new(false);
    let prepare = runner.diag_exec(
        &machine.ssh,
        &key,
        &paths.known_hosts,
        &[
            "sh".into(),
            "-c".into(),
            HOST_PREPARE_SCRIPT.into(),
            "ouro-deploy-host-prepare".into(),
            role_name(machine.role).into(),
            marker_json,
        ],
        HOST_PREPARE_TIMEOUT_S,
    )?;
    if prepare.status != 0 {
        return Err(OuroError::Validation(format!(
            "host preparation failed on {} with status {}: {}",
            machine.id,
            prepare.status,
            bounded_detail(&prepare.stderr)
        )));
    }
    let prepared = parse_closed_facts(
        &prepare.stdout,
        "ouro-deploy-host-prepare-v1",
        &[
            "schema",
            "packages_changed",
            "docker_mode",
            "chrony_synced",
            "chrony_offset",
            "marker_installed",
        ],
    )?;
    if fact(&prepared, "chrony_synced")? != "true" || fact(&prepared, "marker_installed")? != "true"
    {
        return Err(OuroError::Validation(
            "host preparation did not establish Chrony and identity marker invariants".into(),
        ));
    }
    let chrony_offset_seconds: f64 = fact(&prepared, "chrony_offset")?
        .parse()
        .map_err(|_| OuroError::Validation("host returned an invalid Chrony offset".into()))?;
    if !chrony_offset_seconds.is_finite() || chrony_offset_seconds.abs() > 1.0 {
        return Err(OuroError::Validation(
            "host Chrony absolute offset exceeds one second".into(),
        ));
    }
    let p2p_port = machine
        .public_endpoint
        .as_ref()
        .map_or(0, |endpoint| endpoint.port);
    let ufw = runner.diag_exec(
        &machine.ssh,
        &key,
        &paths.known_hosts,
        &[
            "sh".into(),
            "-c".into(),
            UFW_APPLY_SCRIPT.into(),
            "ouro-deploy-ufw".into(),
            role_name(machine.role).into(),
            machine.ssh.port.to_string(),
            p2p_port.to_string(),
        ],
        UFW_TIMEOUT_S,
    )?;
    if ufw.status != 0 {
        return Err(OuroError::Validation(format!(
            "UFW convergence failed on {} with status {}: {}",
            machine.id,
            ufw.status,
            bounded_detail(&ufw.stderr)
        )));
    }
    let delta_facts = parse_closed_facts(
        &ufw.stdout,
        "ouro-deploy-ufw-v1",
        &["schema", "added_ssh", "added_p2p", "enabled"],
    )?;
    let delta = UfwDelta {
        added_ssh: parse_fact_bool(&delta_facts, "added_ssh")?,
        added_p2p: parse_fact_bool(&delta_facts, "added_p2p")?,
        enabled: parse_fact_bool(&delta_facts, "enabled")?,
    };
    let fresh = runner.diag_exec(&machine.ssh, &key, &paths.known_hosts, &["true".into()], 10)?;
    if fresh.status != 0 {
        let rollback = rollback_ufw(&runner, machine, &key, paths, p2p_port, &delta);
        let rollback_detail = match rollback {
            Ok(()) => "the UFW delta was restored".to_string(),
            Err(error) => format!("UFW rollback also failed: {error}"),
        };
        return Err(OuroError::Validation(format!(
            "fresh SSH verification failed after UFW convergence on {}; {}",
            machine.id, rollback_detail
        )));
    }
    Ok(HostConvergence {
        packages_changed: parse_fact_bool(&prepared, "packages_changed")?,
        docker_mode: match fact(&prepared, "docker_mode")? {
            "user" => "user".into(),
            "sudo_n" => "sudo_n".into(),
            other => {
                return Err(OuroError::Validation(format!(
                    "host returned unsupported Docker mode {other}"
                )))
            }
        },
        chrony_offset_seconds,
        marker_installed: true,
        ufw_added_ssh: delta.added_ssh,
        ufw_added_p2p: delta.added_p2p,
        ufw_enabled: delta.enabled,
        fresh_ssh_verified: true,
    })
}

fn install_deployment_artifacts(
    machine: &Machine,
    paths: &ConfigPaths,
    selection: &SignedDeploySelection,
    compose_content: &str,
    topology_content: &str,
) -> Result<serde_json::Value> {
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let image_ref = format!(
        "{}@{}",
        selection.repository, selection.platform_manifest_digest
    );
    let project = format!("ouro-{}", machine.id);
    let outcome = crate::ssh::SshRunner::new(false).diag_exec(
        &machine.ssh,
        &key,
        &paths.known_hosts,
        &[
            "sh".into(),
            "-c".into(),
            ARTIFACT_INSTALL_SCRIPT.into(),
            "ouro-deploy-artifacts".into(),
            compose_content.into(),
            topology_content.into(),
            image_ref.clone(),
            selection.image_config_digest.clone(),
            project.clone(),
        ],
        ARTIFACT_TIMEOUT_S,
    )?;
    if outcome.status != 0 {
        return Err(OuroError::Validation(format!(
            "artifact install/image verification failed on {} with status {}: {}",
            machine.id,
            outcome.status,
            bounded_detail(&outcome.stderr)
        )));
    }
    let facts = parse_closed_facts(
        &outcome.stdout,
        "ouro-deploy-artifacts-v1",
        &[
            "schema",
            "compose_valid",
            "topology_installed",
            "image_config",
        ],
    )?;
    if !parse_fact_bool(&facts, "compose_valid")?
        || !parse_fact_bool(&facts, "topology_installed")?
        || fact(&facts, "image_config")? != selection.image_config_digest
    {
        return Err(OuroError::Validation(
            "artifact installer did not establish the exact signed runtime shape".into(),
        ));
    }
    Ok(serde_json::json!({
        "project": project,
        "service": "cardano-node",
        "image": image_ref,
        "image_config_digest": selection.image_config_digest,
        "compose_valid": true,
        "topology_installed": true,
    }))
}

fn start_compose(machine: &Machine, paths: &ConfigPaths) -> Result<()> {
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let project = format!("ouro-{}", machine.id);
    let outcome = crate::ssh::SshRunner::new(false).diag_exec(
        &machine.ssh,
        &key,
        &paths.known_hosts,
        &[
            "sh".into(),
            "-c".into(),
            COMPOSE_UP_SCRIPT.into(),
            "ouro-deploy-compose-up".into(),
            project,
        ],
        COMPOSE_UP_TIMEOUT_S,
    )?;
    if outcome.status != 0 {
        return Err(OuroError::Validation(format!(
            "docker compose up failed on {} with status {}: {}",
            machine.id,
            outcome.status,
            bounded_detail(&outcome.stderr)
        )));
    }
    let facts = parse_closed_facts(
        &outcome.stdout,
        "ouro-deploy-compose-up-v1",
        &["schema", "started"],
    )?;
    if !parse_fact_bool(&facts, "started")? {
        return Err(OuroError::Validation(
            "docker compose up did not report command success".into(),
        ));
    }
    Ok(())
}

fn rollback_ufw(
    runner: &crate::ssh::SshRunner,
    machine: &Machine,
    key: &std::path::Path,
    paths: &ConfigPaths,
    p2p_port: u16,
    delta: &UfwDelta,
) -> Result<()> {
    let outcome = runner.diag_exec(
        &machine.ssh,
        key,
        &paths.known_hosts,
        &[
            "sh".into(),
            "-c".into(),
            UFW_ROLLBACK_SCRIPT.into(),
            "ouro-deploy-ufw-rollback".into(),
            role_name(machine.role).into(),
            machine.ssh.port.to_string(),
            p2p_port.to_string(),
            delta.added_ssh.to_string(),
            delta.added_p2p.to_string(),
            delta.enabled.to_string(),
        ],
        UFW_TIMEOUT_S,
    )?;
    if outcome.status != 0 {
        return Err(OuroError::Validation(format!(
            "UFW rollback returned status {}: {}",
            outcome.status,
            bounded_detail(&outcome.stderr)
        )));
    }
    let values = parse_closed_facts(
        &outcome.stdout,
        "ouro-deploy-ufw-rollback-v1",
        &["schema", "restored"],
    )?;
    if !parse_fact_bool(&values, "restored")? {
        return Err(OuroError::Validation(
            "UFW rollback did not report restoration".into(),
        ));
    }
    Ok(())
}

fn bounded_detail(value: &str) -> String {
    value.chars().take(500).collect()
}

fn parse_closed_facts(
    stdout: &str,
    expected_schema: &str,
    expected_keys: &[&str],
) -> Result<BTreeMap<String, String>> {
    let expected = expected_keys
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut values = BTreeMap::new();
    for line in stdout.lines() {
        let (key, value) = line.split_once('=').ok_or_else(|| {
            OuroError::Validation("target output contains a malformed fact".into())
        })?;
        if !expected.contains(key) {
            return Err(OuroError::Validation(format!(
                "target output contains unknown fact {key}"
            )));
        }
        if values.insert(key.to_string(), value.to_string()).is_some() {
            return Err(OuroError::Validation(format!(
                "target output contains duplicate fact {key}"
            )));
        }
    }
    if values.len() != expected.len() || expected.iter().any(|key| !values.contains_key(*key)) {
        return Err(OuroError::Validation(
            "target output is missing required facts".into(),
        ));
    }
    if fact(&values, "schema")? != expected_schema {
        return Err(OuroError::Validation(
            "target output has an unsupported schema".into(),
        ));
    }
    Ok(values)
}

fn fact<'a>(facts: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    facts
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| OuroError::Validation(format!("target output omitted fact {key}")))
}

fn parse_fact_bool(facts: &BTreeMap<String, String>, key: &str) -> Result<bool> {
    match fact(facts, key)? {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(OuroError::Validation(format!(
            "target fact {key} is not a canonical boolean"
        ))),
    }
}

fn parse_inspect_facts(stdout: &str) -> Result<InspectFacts> {
    const EXPECTED: [&str; 35] = [
        "schema",
        "os_id",
        "os_version",
        "arch",
        "memory_bytes",
        "free_disk_bytes",
        "privilege",
        "docker_mode",
        "compose_v2",
        "chrony_installed",
        "chrony_synced",
        "chrony_offset",
        "ufw_state",
        "ssh_listener",
        "aggregator_reachable",
        "p2p_listener",
        "metrics_listener",
        "metrics_public_listener",
        "ufw_p2p_allow",
        "ufw_metrics_allow",
        "ouro_state",
        "legacy_cardano_state",
        "legacy_home_config_state",
        "db_state",
        "keys_state",
        "keys_mode",
        "identity_marker",
        "node_container_count",
        "owned_count",
        "owned_running",
        "owned_image",
        "owned_role",
        "owned_lifecycle",
        "owned_network",
        "owned_desired_digest",
    ];
    let allowed: std::collections::BTreeSet<&str> = EXPECTED.iter().copied().collect();
    let mut values = BTreeMap::new();
    for line in stdout.lines() {
        let (key, value) = line.split_once('=').ok_or_else(|| {
            OuroError::Validation("target inspect output contains a malformed fact".into())
        })?;
        if !allowed.contains(key) {
            return Err(OuroError::Validation(format!(
                "target inspect output contains unknown fact {key}"
            )));
        }
        if values.insert(key.to_string(), value.to_string()).is_some() {
            return Err(OuroError::Validation(format!(
                "target inspect output contains duplicate fact {key}"
            )));
        }
    }
    if values.len() != allowed.len() || allowed.iter().any(|key| !values.contains_key(*key)) {
        return Err(OuroError::Validation(
            "target inspect output is missing required facts".into(),
        ));
    }
    let get = |key: &str| {
        values
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| OuroError::Validation(format!("missing target fact {key}")))
    };
    if get("schema")? != "ouro-deploy-inspect-v1" {
        return Err(OuroError::Validation(
            "target inspect output has an unsupported schema".into(),
        ));
    }
    let parse_bool = |key: &str| -> Result<bool> {
        match get(key)? {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(OuroError::Validation(format!(
                "target fact {key} is not a canonical boolean"
            ))),
        }
    };
    let parse_u64 = |key: &str| -> Result<u64> {
        get(key)?.parse().map_err(|_| {
            OuroError::Validation(format!("target fact {key} is not an unsigned integer"))
        })
    };
    let require_enum = |key: &str, choices: &[&str]| -> Result<String> {
        let value = get(key)?;
        if choices.contains(&value) {
            Ok(value.to_string())
        } else {
            Err(OuroError::Validation(format!(
                "target fact {key} has unsupported value {value}"
            )))
        }
    };
    let platform = match get("arch")? {
        "x86_64" | "amd64" => "linux/amd64",
        "aarch64" | "arm64" => "linux/arm64",
        value => {
            return Err(OuroError::Validation(format!(
                "target architecture {value} is unsupported"
            )))
        }
    };
    let chrony_offset_seconds = match get("chrony_offset")? {
        "" => None,
        value => {
            let parsed: f64 = value
                .parse()
                .map_err(|_| OuroError::Validation("target chrony offset is not numeric".into()))?;
            if !parsed.is_finite() {
                return Err(OuroError::Validation(
                    "target chrony offset must be finite".into(),
                ));
            }
            Some(parsed)
        }
    };
    let identity_marker = match get("identity_marker")? {
        "" => None,
        value => Some(serde_json::from_str(value).map_err(|error| {
            OuroError::Validation(format!("target fleet identity marker is invalid: {error}"))
        })?),
    };
    let path_states = [
        "absent",
        "empty_dir",
        "nonempty_dir",
        "file",
        "symlink",
        "unsupported",
    ];
    let db_states = [
        "absent",
        "empty_dir",
        "nonempty_dir",
        "file",
        "symlink",
        "unsupported",
        "populated",
    ];
    Ok(InspectFacts {
        os_id: get("os_id")?.to_string(),
        os_version: get("os_version")?.to_string(),
        platform: platform.to_string(),
        memory_bytes: parse_u64("memory_bytes")?,
        free_disk_bytes: parse_u64("free_disk_bytes")?,
        privilege: require_enum("privilege", &["root", "sudo_n", "none"])?,
        docker_mode: require_enum("docker_mode", &["user", "sudo_n", "unavailable", "missing"])?,
        compose_v2: parse_bool("compose_v2")?,
        chrony_installed: parse_bool("chrony_installed")?,
        chrony_synced: parse_bool("chrony_synced")?,
        chrony_offset_seconds,
        ufw_state: require_enum("ufw_state", &["active", "inactive", "unavailable"])?,
        ssh_listener: parse_bool("ssh_listener")?,
        aggregator_reachable: parse_bool("aggregator_reachable")?,
        p2p_listener: parse_bool("p2p_listener")?,
        metrics_listener: parse_bool("metrics_listener")?,
        metrics_public_listener: parse_bool("metrics_public_listener")?,
        ufw_p2p_allow: parse_bool("ufw_p2p_allow")?,
        ufw_metrics_allow: parse_bool("ufw_metrics_allow")?,
        ouro_state: require_enum("ouro_state", &path_states)?,
        legacy_cardano_state: require_enum("legacy_cardano_state", &path_states)?,
        legacy_home_config_state: require_enum("legacy_home_config_state", &path_states)?,
        db_state: require_enum("db_state", &db_states)?,
        keys_state: require_enum("keys_state", &path_states)?,
        keys_mode: get("keys_mode")?.to_string(),
        identity_marker,
        node_container_count: u32::try_from(parse_u64("node_container_count")?)
            .map_err(|_| OuroError::Validation("node container count is too large".into()))?,
        owned_count: u32::try_from(parse_u64("owned_count")?)
            .map_err(|_| OuroError::Validation("owned container count is too large".into()))?,
        owned_running: parse_bool("owned_running")?,
        owned_image: get("owned_image")?.to_string(),
        owned_role: get("owned_role")?.to_string(),
        owned_lifecycle: get("owned_lifecycle")?.to_string(),
        owned_network: get("owned_network")?.to_string(),
        owned_desired_digest: get("owned_desired_digest")?.to_string(),
    })
}

fn digest_json<T: Serialize>(value: &T) -> Result<String> {
    let canonical = serde_json::to_vec(value)
        .map_err(|error| OuroError::Validation(format!("cannot hash deploy identity: {error}")))?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}

fn fleet_identity_digest(spec: &PoolSpec) -> Result<String> {
    let mut machines = spec
        .machines
        .iter()
        .map(|machine| {
            serde_json::json!({
                "id": machine.id,
                "role": role_name(machine.role),
                "public_endpoint": machine.public_endpoint,
            })
        })
        .collect::<Vec<_>>();
    machines.sort_by(|left, right| {
        left["id"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["id"].as_str().unwrap_or_default())
    });
    digest_json(&serde_json::json!({
        "schema_version": 1,
        "network": spec.pool.network.as_str(),
        "genesis_hashes": spec.pool.genesis_hashes,
        "machines": machines,
    }))
}

fn genesis_identity_digest(spec: &PoolSpec) -> Result<String> {
    digest_json(&spec.pool.genesis_hashes)
}

fn role_name(role: MachineRole) -> &'static str {
    match role {
        MachineRole::Bp => "bp",
        MachineRole::Relay => "relay",
    }
}

fn deployment_desired_digest(
    spec: &PoolSpec,
    machine: &Machine,
    selection: &SignedDeploySelection,
    fleet_digest: &str,
    genesis_digest: &str,
) -> Result<String> {
    let topology = render_deploy_topology(spec, machine, selection)?;
    let topology_digest = digest_json(&topology)?;
    digest_json(&serde_json::json!({
        "deployment_policy_version": 1,
        "fleet_identity_digest": fleet_digest,
        "genesis_identity": genesis_digest,
        "machine_id": machine.id,
        "role": role_name(machine.role),
        "lifecycle": lifecycle_for(machine.role),
        "network": spec.pool.network.as_str(),
        "repository": selection.repository,
        "platform": selection.platform,
        "platform_manifest_digest": selection.platform_manifest_digest,
        "image_config_digest": selection.image_config_digest,
        "compose_project": format!("ouro-{}", machine.id),
        "compose_service": "cardano-node",
        "environment": desired_environment(spec.pool.network),
        "mounts": selective_mount_policy(machine.role),
        "ports": desired_ports(machine),
        "logging": {
            "driver": "json-file",
            "max-size": "50m",
            "max-file": "3",
        },
        "security": {
            "privileged": false,
            "image_default_user_and_capabilities": true,
        },
        "restart": "unless-stopped",
        "command": ["run"],
        "topology_digest": topology_digest,
    }))
}

fn render_deploy_topology(
    spec: &PoolSpec,
    machine: &Machine,
    selection: &SignedDeploySelection,
) -> Result<serde_json::Value> {
    let relay_peers = spec
        .machines
        .iter()
        .filter(|candidate| candidate.role == MachineRole::Relay)
        .map(|relay| {
            let endpoint = relay.public_endpoint.as_ref().ok_or_else(|| {
                OuroError::Validation(format!("relay {} is missing its public endpoint", relay.id))
            })?;
            Ok(serde_json::json!({
                "address": endpoint.host,
                "port": endpoint.port,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let signed_peers = selection
        .network
        .bootstrap_peers
        .iter()
        .map(|peer| {
            serde_json::json!({
                "address": peer.address,
                "port": peer.port,
            })
        })
        .collect::<Vec<_>>();
    match machine.role {
        MachineRole::Bp => Ok(serde_json::json!({
            "bootstrapPeers": [],
            "localRoots": [{
                "accessPoints": relay_peers,
                "advertise": false,
                "trustable": true,
                "valency": 1,
            }],
            "publicRoots": [],
            "useLedgerAfterSlot": 0,
            "PeerSharing": false,
        })),
        MachineRole::Relay => Ok(serde_json::json!({
            "bootstrapPeers": signed_peers.clone(),
            "localRoots": [],
            "publicRoots": [{
                "accessPoints": signed_peers,
                "advertise": true,
            }],
            "useLedgerAfterSlot": 0,
            "PeerSharing": true,
        })),
    }
}

fn desired_ports(machine: &Machine) -> Vec<serde_json::Value> {
    let mut ports = vec![serde_json::json!({
        "host_ip": "127.0.0.1",
        "published": 12798,
        "target": 12798,
        "protocol": "tcp",
    })];
    if machine.role == MachineRole::Relay {
        if let Some(endpoint) = &machine.public_endpoint {
            ports.push(serde_json::json!({
                "host_ip": "0.0.0.0",
                "published": endpoint.port,
                "target": 3001,
                "protocol": "tcp",
            }));
        }
    }
    ports
}

fn render_compose(
    spec: &PoolSpec,
    machine: &Machine,
    selection: &SignedDeploySelection,
    desired_digest: &str,
) -> Result<String> {
    let environment = desired_environment(spec.pool.network)
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect::<BTreeMap<_, _>>();
    let volumes = selective_mount_policy(machine.role)
        .into_iter()
        .map(|mount| {
            let source = match mount.destination {
                DATA_DESTINATION => "/opt/ouro/db",
                IPC_DESTINATION => "/opt/ouro/ipc",
                TOPOLOGY_DESTINATION => "/opt/ouro/topology.json",
                KEYS_DESTINATION => "/opt/ouro/keys",
                _ => unreachable!("mount policy is fixed"),
            };
            serde_json::json!({
                "type": "bind",
                "source": source,
                "target": mount.destination,
                "read_only": mount.read_only,
            })
        })
        .collect::<Vec<_>>();
    let image = format!(
        "{}@{}",
        selection.repository, selection.platform_manifest_digest
    );
    let lifecycle = match lifecycle_for(machine.role) {
        NodeLifecycle::Bootstrap => "bootstrap",
        NodeLifecycle::Operational => "operational",
    };
    let compose = serde_json::json!({
        "services": {
            "cardano-node": {
                "image": image,
                "command": ["run"],
                "restart": "unless-stopped",
                "privileged": false,
                "environment": environment,
                "labels": {
                    "io.ouro.machine-id": machine.id,
                    "io.ouro.role": role_name(machine.role),
                    "io.ouro.lifecycle": lifecycle,
                    "io.ouro.network": spec.pool.network.as_str(),
                    "io.ouro.desired-digest": desired_digest,
                },
                "volumes": volumes,
                "ports": desired_ports(machine),
                "logging": {
                    "driver": "json-file",
                    "options": {
                        "max-size": "50m",
                        "max-file": "3",
                    },
                },
            }
        }
    });
    serde_yaml::to_string(&compose)
        .map_err(|error| OuroError::Validation(format!("cannot render fixed Compose: {error}")))
}

fn select_pinned_deploy(
    policy: &Allowlist,
    platform: &str,
    network: Network,
    expected_genesis_hash: &str,
    expected_manifest_digest: &str,
    expected_config_digest: &str,
) -> Result<SignedDeploySelection> {
    let (layout, image) = policy.contract_and_image_for(expected_config_digest, platform)?;
    if image.platform_manifest_digest != expected_manifest_digest {
        return Err(OuroError::Validation(
            "fleet marker manifest/config tuple is absent from signed policy".into(),
        ));
    }
    let bootstrap = layout.deploy.as_ref().ok_or_else(|| {
        OuroError::Validation("fleet marker image has no signed deploy contract".into())
    })?;
    let network_contract = bootstrap
        .networks
        .get(network.as_str())
        .ok_or_else(|| OuroError::Validation("signed deploy network facts are absent".into()))?;
    if network_contract.genesis_hash != expected_genesis_hash {
        return Err(OuroError::Validation(
            "fleet marker image genesis does not match the pool spec".into(),
        ));
    }
    Ok(SignedDeploySelection {
        repository: policy.repository.clone(),
        platform: platform.to_string(),
        release: image.release.clone(),
        oci_index_digest: image.oci_index_digest.clone(),
        platform_manifest_digest: image.platform_manifest_digest.clone(),
        image_config_digest: image.image_config_digest.clone(),
        contract_id: layout.contract_id.clone(),
        bootstrap: bootstrap.clone(),
        network: network_contract.clone(),
    })
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeployMountPolicy {
    pub destination: &'static str,
    pub read_only: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignedDeploySelection {
    pub repository: String,
    pub platform: String,
    pub release: String,
    pub oci_index_digest: String,
    pub platform_manifest_digest: String,
    pub image_config_digest: String,
    pub contract_id: String,
    pub bootstrap: DeployBootstrapContract,
    pub network: DeployNetworkContract,
}

pub fn select_signed_deploy(
    policy: &Allowlist,
    platform: &str,
    network: Network,
    expected_genesis_hash: &str,
) -> Result<SignedDeploySelection> {
    let (layout, image, bootstrap) = policy.recommended_deploy_for(platform)?;
    let network_name = network.as_str();
    let network_contract = bootstrap.networks.get(network_name).ok_or_else(|| {
        OuroError::Validation(format!(
            "signed Fleet Deploy contract has no {network_name} bootstrap facts"
        ))
    })?;
    if network_contract.genesis_hash != expected_genesis_hash {
        return Err(OuroError::Validation(format!(
            "pool spec genesis {} does not match signed {network_name} image genesis {}",
            expected_genesis_hash, network_contract.genesis_hash
        )));
    }
    Ok(SignedDeploySelection {
        repository: policy.repository.clone(),
        platform: platform.to_string(),
        release: image.release.clone(),
        oci_index_digest: image.oci_index_digest.clone(),
        platform_manifest_digest: image.platform_manifest_digest.clone(),
        image_config_digest: image.image_config_digest.clone(),
        contract_id: layout.contract_id.clone(),
        bootstrap: bootstrap.clone(),
        network: network_contract.clone(),
    })
}

pub fn desired_environment(network: Network) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("CARDANO_BLOCK_PRODUCER", "false".into()),
        ("CARDANO_DATABASE_PATH", DATA_DESTINATION.into()),
        ("CARDANO_NETWORK", network.as_str().into()),
        ("CARDANO_SOCKET_PATH", "/ipc/node.socket".into()),
        ("CARDANO_TOPOLOGY", TOPOLOGY_DESTINATION.into()),
        ("RESTORE_SNAPSHOT", "true".into()),
    ])
}

pub fn lifecycle_for(role: MachineRole) -> NodeLifecycle {
    match role {
        MachineRole::Bp => NodeLifecycle::Bootstrap,
        MachineRole::Relay => NodeLifecycle::Operational,
    }
}

pub fn selective_mount_policy(role: MachineRole) -> Vec<DeployMountPolicy> {
    let mut mounts = vec![
        DeployMountPolicy {
            destination: DATA_DESTINATION,
            read_only: false,
        },
        DeployMountPolicy {
            destination: IPC_DESTINATION,
            read_only: false,
        },
        DeployMountPolicy {
            destination: TOPOLOGY_DESTINATION,
            read_only: true,
        },
    ];
    if role == MachineRole::Bp {
        mounts.push(DeployMountPolicy {
            destination: KEYS_DESTINATION,
            read_only: false,
        });
    }
    mounts.sort_by_key(|mount| mount.destination);
    mounts
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/releases.json"));

    #[test]
    fn signed_deploy_selection_binds_exact_image_and_network_facts() {
        let policy = Allowlist::release_document(RELEASES).unwrap();
        let selected = select_signed_deploy(
            &policy,
            "linux/amd64",
            Network::Mainnet,
            "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81",
        )
        .unwrap();
        assert_eq!(selected.release, "11.0.1-1");
        assert_eq!(
            selected.image_config_digest,
            "sha256:0bb21e45159327c4e6109704df256c3c297c725a4b2cdf6d0e1899e3a9df468f"
        );
        assert_eq!(selected.network.config_sha256.len(), 64);
        assert_eq!(
            selected.bootstrap.database_marker,
            "/data/db/protocolMagicId"
        );
        assert_eq!(selected.bootstrap.metrics.host_ip, "127.0.0.1");
        assert!(
            select_signed_deploy(&policy, "linux/amd64", Network::Mainnet, &"0".repeat(64),)
                .is_err()
        );
    }

    #[test]
    fn fresh_shape_keeps_image_config_visible_and_bp_keys_writable() {
        let environment = desired_environment(Network::Preview);
        assert_eq!(environment.len(), 6);
        assert_eq!(environment["CARDANO_BLOCK_PRODUCER"], "false");
        assert_eq!(environment["RESTORE_SNAPSHOT"], "true");

        let relay = selective_mount_policy(MachineRole::Relay);
        let bp = selective_mount_policy(MachineRole::Bp);
        for mounts in [&relay, &bp] {
            assert!(mounts
                .iter()
                .all(|mount| mount.destination != "/opt/cardano/config"));
        }
        assert!(!relay
            .iter()
            .any(|mount| mount.destination == KEYS_DESTINATION));
        assert!(bp
            .iter()
            .any(|mount| { mount.destination == KEYS_DESTINATION && !mount.read_only }));
        assert_eq!(lifecycle_for(MachineRole::Bp), NodeLifecycle::Bootstrap);
        assert_eq!(
            lifecycle_for(MachineRole::Relay),
            NodeLifecycle::Operational
        );
    }

    #[test]
    fn host_convergence_scripts_are_fixed_bounded_and_shell_valid() {
        for script in [
            HOST_PREPARE_SCRIPT,
            UFW_APPLY_SCRIPT,
            UFW_ROLLBACK_SCRIPT,
            ARTIFACT_INSTALL_SCRIPT,
            COMPOSE_UP_SCRIPT,
        ] {
            let status = std::process::Command::new("sh")
                .args(["-n", "-c", script])
                .status()
                .unwrap();
            assert!(status.success());
        }
        for required in [
            "docker.io docker-compose-v2 chrony ufw ca-certificates curl",
            "timeout 5s chronyc -n tracking",
            "install -d -m 0755 /opt/ouro",
            "install -d -m 0700 /opt/ouro/keys",
        ] {
            assert!(HOST_PREPARE_SCRIPT.contains(required), "{required}");
        }
        for forbidden in [
            "waitsync",
            "sleep ",
            "apt-get upgrade",
            "daemon.json",
            "sshd_config",
            "fail2ban",
            "http://",
            "https://",
        ] {
            assert!(
                !HOST_PREPARE_SCRIPT.contains(forbidden),
                "forbidden host behavior: {forbidden}"
            );
        }
        assert!(UFW_APPLY_SCRIPT.contains("ouro-deploy-ssh"));
        assert!(UFW_APPLY_SCRIPT.contains("ouro-deploy-relay-p2p"));
        assert!(UFW_APPLY_SCRIPT.contains("test \"$role\" = relay"));
        assert!(!UFW_APPLY_SCRIPT.contains("12798"));
        assert!(UFW_ROLLBACK_SCRIPT.contains("delete allow"));
        assert!(!UFW_ROLLBACK_SCRIPT.contains("reset"));
        assert!(ARTIFACT_INSTALL_SCRIPT.contains("compose -p \"$project\""));
        assert!(ARTIFACT_INSTALL_SCRIPT.contains("image inspect --format '{{.Id}}'"));
        assert!(COMPOSE_UP_SCRIPT.contains("--no-build --pull never"));
        for forbidden in ["health", "socket", "query tip", "metrics", "sleep ", "poll"] {
            assert!(!COMPOSE_UP_SCRIPT.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn host_executor_target_output_is_closed_schema_data() {
        let valid = concat!(
            "schema=ouro-deploy-ufw-v1\n",
            "added_ssh=true\n",
            "added_p2p=false\n",
            "enabled=true\n"
        );
        let parsed = parse_closed_facts(
            valid,
            "ouro-deploy-ufw-v1",
            &["schema", "added_ssh", "added_p2p", "enabled"],
        )
        .unwrap();
        assert!(parse_fact_bool(&parsed, "added_ssh").unwrap());
        assert!(parse_closed_facts(
            &(valid.to_string() + "command=sudo ufw reset\n"),
            "ouro-deploy-ufw-v1",
            &["schema", "added_ssh", "added_p2p", "enabled"],
        )
        .is_err());
    }

    #[test]
    fn compose_and_topology_are_role_specific_and_signed() {
        let spec: PoolSpec = serde_yaml::from_str(
            r#"spec_version: 1
pool:
  network: mainnet
  network_magic: 764824073
  genesis_hashes:
    shelley: 1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh: {host: 192.0.2.1, port: 22, user: bp-admin, key_ref: creds://bp1}
  - id: relay1
    role: relay
    public_endpoint: {host: relay.example.com, port: 3001}
    ssh: {host: 192.0.2.2, port: 22, user: relay-admin, key_ref: creds://relay1}
"#,
        )
        .unwrap();
        spec.validate().unwrap();
        let policy = Allowlist::release_document(RELEASES).unwrap();
        let selection = select_signed_deploy(
            &policy,
            "linux/amd64",
            Network::Mainnet,
            &spec.pool.genesis_hashes.shelley,
        )
        .unwrap();
        let bp = &spec.machines[0];
        let relay = &spec.machines[1];
        let bp_topology = render_deploy_topology(&spec, bp, &selection).unwrap();
        let relay_topology = render_deploy_topology(&spec, relay, &selection).unwrap();
        assert_eq!(
            bp_topology["localRoots"][0]["accessPoints"][0]["address"],
            "relay.example.com"
        );
        assert_eq!(bp_topology["bootstrapPeers"], serde_json::json!([]));
        assert_eq!(
            relay_topology["bootstrapPeers"],
            serde_json::to_value(&selection.network.bootstrap_peers).unwrap()
        );
        let bp_compose = render_compose(&spec, bp, &selection, "sha256:bp").unwrap();
        let relay_compose = render_compose(&spec, relay, &selection, "sha256:relay").unwrap();
        let bp_yaml: serde_yaml::Value = serde_yaml::from_str(&bp_compose).unwrap();
        let relay_yaml: serde_yaml::Value = serde_yaml::from_str(&relay_compose).unwrap();
        let bp_service = &bp_yaml["services"]["cardano-node"];
        let relay_service = &relay_yaml["services"]["cardano-node"];
        assert_eq!(bp_service["environment"]["CARDANO_BLOCK_PRODUCER"], "false");
        assert_eq!(bp_service["labels"]["io.ouro.lifecycle"], "bootstrap");
        assert_eq!(relay_service["labels"]["io.ouro.lifecycle"], "operational");
        assert!(bp_compose.contains("/opt/cardano/config/keys"));
        assert!(!relay_compose.contains("/opt/cardano/config/keys"));
        assert!(!bp_compose.contains("/opt/cardano/config:"));
        assert!(relay_compose.contains("0.0.0.0"));
        assert!(!bp_compose.contains("target: 3001"));
        assert!(bp_compose.contains("127.0.0.1"));
        assert_eq!(bp_service["logging"]["options"]["max-size"], "50m");
        assert_eq!(bp_service["privileged"], false);
    }
}
