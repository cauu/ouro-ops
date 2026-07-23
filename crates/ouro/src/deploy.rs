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

const INSPECT_SCRIPT: &str = r#"set -u
ssh_port=$1
machine_id=$2
aggregator=$3
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
if command -v ufw >/dev/null 2>&1; then
  ufw_line=$(if test "$privilege" = root; then ufw status 2>/dev/null; else sudo -n ufw status 2>/dev/null; fi | head -n1)
  case "$ufw_line" in *active*) ufw_state=active ;; *inactive*) ufw_state=inactive ;; esac
fi
ssh_listener=false
if ss -Hln 2>/dev/null | awk '{print $4}' | grep -Eq "(^|:)$ssh_port$"; then ssh_listener=true; fi
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
  "ouro_state=$ouro_state" "legacy_cardano_state=$legacy_cardano_state" \
  "legacy_home_config_state=$legacy_home_config_state" "db_state=$db_state" \
  "keys_state=$keys_state" "keys_mode=$keys_mode" "identity_marker=$identity_marker" \
  "node_container_count=$node_container_count" "owned_count=$owned_count" \
  "owned_running=$owned_running" "owned_image=$owned_image" "owned_role=$owned_role" \
  "owned_lifecycle=$owned_lifecycle" "owned_network=$owned_network" \
  "owned_desired_digest=$owned_desired_digest"
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

pub fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("inspect") => run_inspect(&args[1..]),
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

fn parse_inspect_facts(stdout: &str) -> Result<InspectFacts> {
    const EXPECTED: [&str; 30] = [
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
    digest_json(&serde_json::json!({
        "schema_version": 1,
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
        "environment": desired_environment(spec.pool.network),
        "mounts": selective_mount_policy(machine.role),
    }))
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

#[derive(Debug, Clone, Serialize)]
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
}
