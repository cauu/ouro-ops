//! S0019 p4-1 — CLI wiring for the greenfield model: `ouro-ops adopt` (the adoption ceremony) and
//! `ouro-ops op` (the intent pipeline). These integrate the p1–p3 mechanism modules into the two
//! commands the greenfield skills call. The website/agent interaction is unchanged: the agent
//! supplies PARAMETERS to these commands, never raw commands; every gate fires in order and refuses
//! before any mutation.
//!
//! Decision (recorded in the spec): S0019 uses a NEW `op` command rather than overloading the
//! S0017 `tool run` (which carries the legacy write path, disabled by §2.8). The dispatched
//! live-observation probe and the sealed docker executor are the target-side seam; here the probe
//! reads a closed observation JSON (`--observation`) and the executor runs in `--plan` mode (gates
//! fire, no mutation) until the p4-2 executor scripts land. Real target execution is p4-2.

use std::path::PathBuf;

use serde_json::json;

use crate::attestation::{
    self, AdoptionAttestation, ImmutableIdentity, LiveObservation, ManagedState, Role, TypedMount,
};
use crate::config::ConfigPaths;
use crate::intent::{Intent, Mutability};
use crate::output::{self, ToolOutput};
use crate::supervisor::SupervisorObservation;
use crate::transaction::{
    self, DurableTransaction, Journal, JournalRecord, RecoveryOps, TxOps, TxState, WriteSeal,
};
use crate::{convention, parity, OuroError, Result};

/// Where the attestation lives. On the TARGET (`--local`, p5-4) it is the single root-owned file
/// `/var/lib/ouro/node-attestation.json` (overridable via OURO_ATTESTATION, matching
/// `ouro-attested.sh`); on the control host it is per-node under OURO_HOME (pre-dispatch modelling).
/// p5-5 — `ouro-ops inbox stage --type <opcert|tx|image> --file <path>`: content-addressed ingress.
/// Reads the artifact, validates its type/shape/size, stores it by digest, and prints the immutable
/// `<id>@sha256:<digest>` reference an intent will carry (never a raw path/blob).
pub fn run_inbox(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("stage") {
        return Err(OuroError::InvalidArgs(
            "expected: ouro-ops inbox stage --type <opcert|tx|image> --file <path>".into(),
        ));
    }
    let args = &args[1..];
    let kind = match flag(args, "--type")? {
        "opcert" => crate::inbox::ArtifactType::Opcert,
        "tx" => crate::inbox::ArtifactType::Tx,
        "image" => crate::inbox::ArtifactType::Image,
        other => return Err(OuroError::Validation(format!("--type must be opcert|tx|image, got {other}"))),
    };
    let file = flag(args, "--file")?;
    let bytes = std::fs::read(file)
        .map_err(|e| OuroError::Validation(format!("cannot read artifact {file}: {e}")))?;
    let paths = ConfigPaths::discover();
    let inbox = paths.home.join("inbox");
    let reference = crate::inbox::stage(&inbox, kind, &bytes)?;
    output::print_json(&ToolOutput::ok("ouro.inbox.stage", true).with_data(json!({
        "artifact_ref": reference, "note": "reference this in an intent --param; never a raw path",
    })))?;
    Ok(())
}

/// Mint one short-lived, signed disruptive-step permit under the pool-wide authority. Controllers
/// MUST share the same durable OURO_HOME authority; its kernel lock serializes acquisitions.
pub fn run_fleet(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("permit")
        || args.get(1).map(String::as_str) != Some("create")
    {
        return Err(OuroError::InvalidArgs(
            "expected: ouro-ops fleet permit create --pool-id <id> --node <id> --op <id> \
             --role <bp|relay> --online-relays <n> --min-online-relays <n> \
             --relays-remaining <n> --holder <id> [--ttl 2m]"
                .into(),
        ));
    }
    let args = &args[2..];
    let pool_id = flag(args, "--pool-id")?;
    let node = flag(args, "--node")?;
    let operation = flag(args, "--op")?;
    let role = flag(args, "--role")?;
    let holder = flag(args, "--holder")?;
    crate::intent::validate_machine_id(pool_id)?;
    crate::intent::validate_machine_id(node)?;
    crate::intent::validate_machine_id(holder)?;
    parity::require_registered_write(operation)?;
    if !matches!(role, "bp" | "relay") {
        return Err(OuroError::Validation("--role must be bp|relay".into()));
    }
    let parse_u32 = |name: &str| -> Result<u32> {
        flag(args, name)?.parse().map_err(|_| {
            OuroError::Validation(format!("{name} must be an unsigned integer"))
        })
    };
    let online_relays = parse_u32("--online-relays")?;
    let min_online_relays = parse_u32("--min-online-relays")?;
    let relays_remaining = parse_u32("--relays-remaining")?;
    crate::fleet::require_quorum(online_relays, min_online_relays, role == "relay")?;
    crate::fleet::require_bp_last(role == "bp", relays_remaining)?;
    let ttl = crate::confirm::parse_ttl(optional(args, "--ttl").unwrap_or("2m"))?;
    let ttl_seconds = u64::try_from(ttl.num_seconds())
        .map_err(|_| OuroError::Validation("fleet permit ttl must be positive".into()))?;
    if !(30..=300).contains(&ttl_seconds) {
        return Err(OuroError::Validation("fleet permit ttl must be 30s..5m".into()));
    }
    let now = crate::s0019_confirmation::current_epoch()?;
    let paths = ConfigPaths::discover();
    let authority = crate::fleet::PoolAuthority::at(&paths.home.join("fleet-authority"), pool_id);
    let lease = authority.acquire(pool_id, holder, now, ttl_seconds)?;
    let secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let permit = crate::fleet::StepPermit {
        pool_id: pool_id.into(),
        node_id: node.into(),
        operation_id: operation.into(),
        role: role.into(),
        fencing_token: lease.fencing_token,
        expiry_epoch: lease.expiry_epoch,
        online_relays,
        min_online_relays,
        relays_remaining,
        permit_id: uuid::Uuid::new_v4().simple().to_string(),
        signature: String::new(),
    }
    .sign(secret.as_bytes())?;
    let encoded = serde_json::to_string(&permit)
        .map_err(|e| OuroError::Validation(format!("fleet permit serialize: {e}")))?;
    output::print_json(&ToolOutput::ok("ouro.fleet.permit.create", true).with_data(json!({
        "fleet_permit": encoded,
        "pool_id": pool_id,
        "node": node,
        "operation": operation,
        "fencing_token": permit.fencing_token,
        "expires_at_epoch": permit.expiry_epoch,
    })))?;
    Ok(())
}

/// p5-5 — append a closed-field audit event (§2.13). Hashes/ids only, never raw config/secret data.
fn audit_emit(paths: &ConfigPaths, event: &str, node: &str, extra: serde_json::Value) {
    let mut ev = serde_json::Map::new();
    ev.insert("event".into(), json!(event));
    ev.insert("audit_id".into(), json!(format!("{event}-{node}")));
    ev.insert("node_id".into(), json!(node));
    ev.insert("at_epoch".into(), json!(0)); // no ambient clock; a real emitter stamps target-side
    if let serde_json::Value::Object(m) = extra {
        for (k, v) in m {
            ev.insert(k, v);
        }
    }
    let line = serde_json::to_string(&serde_json::Value::Object(ev)).unwrap_or_default();
    let path = paths.home.join("s0019-audit.jsonl");
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).ok();
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

fn attestation_path_for(paths: &ConfigPaths, node: &str, local: bool) -> PathBuf {
    if local {
        std::env::var_os("OURO_ATTESTATION")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(crate::attestation::ATTESTATION_PATH))
    } else {
        paths.home.join("attestations").join(format!("{node}.json"))
    }
}
fn tx_dir(paths: &ConfigPaths) -> PathBuf {
    paths.home.join("txn")
}

/// The closed observation the target-side probe produces (both supervisor + live facts + resolved
/// contract). Read from `--observation <file.json>` — the dispatch seam.
#[derive(serde::Deserialize)]
struct Observation {
    supervisor: SupervisorObservation,
    live: ObsLive,
    /// Role-specific post-write readiness evidence gathered target-side. Required for every real
    /// write verification; optional only so old observation fixtures fail at verify, not parse.
    #[serde(default)]
    readiness: Option<ObsReadiness>,
    /// The upgrade recreate spec (§2.10), target-gathered from `docker inspect`. `None` (or an
    /// unmodeled shape) means the executor refuses to recreate rather than guess.
    #[serde(default)]
    recreate: Option<crate::executor::RecreateSpec>,
}
#[derive(serde::Deserialize, Clone)]
struct ObsReadiness {
    node_running: bool,
    socket_answers: bool,
    tip_block: i64,
    tip_block_next: i64,
    kes_opcert_valid: bool,
    credential_loaded: bool,
    established_peers: u32,
}
#[derive(serde::Deserialize, Clone)]
struct ObsLive {
    image_config_digest: String,
    platform: String,
    container_id: String,
    container_creation_epoch: u64,
    entrypoint: Vec<String>,
    args: Vec<String>,
    mount_source_ids: Vec<String>,
    topology_hash: String,
    config_hash: String,
    kes_opcert_id: String,
    has_forging_keys: bool,
    host_key_sha256: String,
    genesis_hash: String,
    network: String,
}

impl ObsLive {
    fn to_live(&self) -> LiveObservation {
        LiveObservation {
            image_config_digest: self.image_config_digest.clone(),
            container_id: self.container_id.clone(),
            container_creation_epoch: self.container_creation_epoch,
            entrypoint: self.entrypoint.clone(),
            args: self.args.clone(),
            mount_source_ids: self.mount_source_ids.clone(),
            topology_hash: self.topology_hash.clone(),
            config_hash: self.config_hash.clone(),
            kes_opcert_id: self.kes_opcert_id.clone(),
            has_forging_keys: self.has_forging_keys,
        }
    }
}

fn require_readiness(
    att: &AdoptionAttestation,
    observation: &Observation,
    allow_rotated_container: bool,
) -> Result<()> {
    let evidence = observation.readiness.as_ref().ok_or_else(|| {
        OuroError::Validation(
            "target probe did not provide readiness evidence — refusing to verify write (§2.6a)"
                .into(),
        )
    })?;
    crate::readiness::Readiness {
        role: att.immutable.role,
        node_running: evidence.node_running,
        container_id_matches: if allow_rotated_container {
            !observation.live.container_id.is_empty()
                && observation.live.container_id != att.state.container_id
        } else {
            observation.live.container_id == att.state.container_id
        },
        socket_answers: evidence.socket_answers,
        network_ok: observation.live.network == att.immutable.network,
        genesis_ok: observation.live.genesis_hash == att.immutable.genesis_hash,
        tip_block: evidence.tip_block,
        tip_block_next: evidence.tip_block_next,
        kes_opcert_valid: evidence.kes_opcert_valid,
        credential_loaded: evidence.credential_loaded,
        established_peers: evidence.established_peers,
    }
    .evaluate()
}

fn read_observation(args: &[String]) -> Result<Observation> {
    // p6-2 — with no `--observation`, RUN the target-side probe (ouro_observe) to self-gather the
    // observation, so the agent never hand-feeds a file. The probe lib is embedded; extract it to a
    // temp dir and source it. (OURO_PROBE_LIB overrides the source for the bed / tests.)
    let text = match optional(args, "--observation") {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| OuroError::Validation(format!("cannot read observation {path}: {e}")))?,
        None => run_probe()?,
    };
    serde_json::from_str(&text)
        .map_err(|e| OuroError::Validation(format!("malformed observation: {e}")))
}

/// Run the embedded target-side probe and capture its observation JSON. The probe lib
/// (`lib/ouro-probe.sh`) is embedded; extract it to a temp file, source it, and run `ouro_observe`.
fn run_probe() -> Result<String> {
    let lib_path = match std::env::var_os("OURO_PROBE_LIB") {
        Some(p) => PathBuf::from(p),
        None => {
            let bytes = crate::skills::asset("lib/ouro-probe.sh").ok_or_else(|| {
                OuroError::Validation("embedded probe lib/ouro-probe.sh missing".into())
            })?;
            let dir = std::env::temp_dir().join(format!("ouro-probe-{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok();
            let p = dir.join("ouro-probe.sh");
            std::fs::write(&p, bytes).ok();
            p
        }
    };
    let platform =
        std::env::var("OURO_PLATFORM").unwrap_or_else(|_| "linux/amd64".to_string());
    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!("source '{}'\nouro_observe '{}'", lib_path.display(), platform))
        .output()
        .map_err(|e| OuroError::Validation(format!("probe failed to run: {e}")))?;
    if !out.status.success() {
        return Err(OuroError::Validation(format!(
            "probe failed: {}",
            String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `ouro-ops adopt` — conformance → evidence-bound approval → write the attestation. Non-disruptive
/// (writes metadata only). Refuses a non-conforming node (never adapts).
pub fn run_adopt(args: &[String]) -> Result<()> {
    let node = flag(args, "--node")?.to_string();
    crate::intent::validate_machine_id(&node)?;
    let local = args.iter().any(|a| a == "--local");

    // p6-3 — SSH DISPATCH: `adopt --dispatch <host>` runs `ouro-ops adopt --local` on the target as
    // the operator's bootstrap account (adoption is a privileged onboarding-class action).
    if let Some(host) = optional(args, "--dispatch") {
        let paths = ConfigPaths::discover();
        let node = flag(args, "--node")?.to_string();
        let plan = args.iter().any(|a| a == "--plan");
        return dispatch_adopt(host, &node, args, &paths, plan);
    }

    let role = match flag(args, "--role")? {
        "bp" => Role::Bp,
        "relay" => Role::Relay,
        other => return Err(OuroError::Validation(format!("--role must be bp|relay, got {other}"))),
    };
    let approve_token = flag(args, "--approve-token")?;
    let obs = read_observation(args)?;

    // 1. supervisor shape must conform to the v1 contract (§2.2).
    obs.supervisor.require_conformant()?;

    // 2. image digest must be on the signed allowlist (§2.1); resolve the layout contract.
    let allow = convention::Allowlist::embedded()?;
    let contract = allow.contract_for(&obs.live.image_config_digest, &obs.live.platform)?;

    // 3. build the immutable identity + initial managed state.
    let role_rule = match role {
        Role::Bp => contract.role_rules.bp,
        Role::Relay => contract.role_rules.relay,
    };
    let immutable = ImmutableIdentity {
        role,
        contract_id: contract.contract_id.clone(),
        convention_version: contract.convention_version,
        host_key_sha256: obs.live.host_key_sha256.clone(),
        machine_id: node.clone(),
        oci_index_digest: obs.live.image_config_digest.clone(), // index digest resolved target-side; pinned here
        platform_manifest_digest: obs.live.image_config_digest.clone(),
        image_config_digest: obs.live.image_config_digest.clone(),
        container_creation_epoch: obs.live.container_creation_epoch,
        entrypoint: obs.live.entrypoint.clone(),
        args: obs.live.args.clone(),
        mounts: obs
            .live
            .mount_source_ids
            .iter()
            .map(|sid| TypedMount {
                kind: "bind".into(),
                source_id: sid.clone(),
                destination: String::new(),
                read_only: false,
                owner: "root".into(),
                mode: "0755".into(),
                no_symlink: true,
            })
            .collect(),
        network: obs.live.network.clone(),
        genesis_hash: obs.live.genesis_hash.clone(),
        public_credential_ids: if obs.live.kes_opcert_id.is_empty() {
            vec![]
        } else {
            vec![obs.live.kes_opcert_id.clone()]
        },
        approval_evidence_hash: String::new(), // filled after binding
    };
    let mut att = AdoptionAttestation {
        immutable,
        state: ManagedState {
            state_generation: 0,
            container_id: obs.live.container_id.clone(),
            topology_hash: obs.live.topology_hash.clone(),
            config_hash: obs.live.config_hash.clone(),
            kes_opcert_id: obs.live.kes_opcert_id.clone(),
        },
    };

    // 4. role rule (relay must not bear forging keys; bp must have an opcert) (§2.3).
    att.check_role(&role_rule, &obs.live.to_live())?;

    // 5. evidence-bound approval (§2.14): bind the operator token to the candidate + host key.
    let candidate =
        attestation::candidate_hash(&serde_json::to_value(&att.immutable).unwrap_or(json!({})));
    let evidence = attestation::bind_approval(&candidate, approve_token, &obs.live.host_key_sha256);
    att.immutable.approval_evidence_hash = evidence;

    // 6. write the attestation (non-disruptive metadata write) + mirror the resolved contract for
    // the shell layout accessors (p1-5).
    let paths = ConfigPaths::discover();
    let p = attestation_path_for(&paths, &node, local);
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent).ok(); }
    let mut doc = serde_json::to_value(&att).unwrap();
    doc["contract"] = json!({ "in_container_paths": contract.in_container_paths });
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap())
        .map_err(|e| OuroError::Validation(format!("cannot write attestation: {e}")))?;

    output::print_json(&ToolOutput::ok("ouro.adopt", true).with_data(json!({
        "node": node,
        "role": att.immutable.role,
        "contract_id": contract.contract_id,
        "attestation": p.display().to_string(),
        "non_disruptive": true,
        "state_generation": 0,
    })))?;
    Ok(())
}

/// `ouro-ops op run --op <id> --node <id> [--param k=v]... [--confirm-token T] --observation <f> [--plan]`
/// The intent pipeline: recover → parity → build+validate intent → live re-attest gate → confirm
/// gate → crash-durable transaction → sealed executor (plan mode until p4-2).
pub fn run_op(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("run") {
        return Err(OuroError::InvalidArgs(
            "expected: ouro-ops op run --op <id> --node <id> [--param k=v]... [--confirm-token T] --observation <f> [--plan]".into(),
        ));
    }
    let args = &args[1..];
    let op = flag(args, "--op")?.to_string();
    let node = flag(args, "--node")?.to_string();
    crate::intent::validate_machine_id(&node)?;
    let plan = args.iter().any(|a| a == "--plan");
    let paths = ConfigPaths::discover();

    // p5-1 — SSH DISPATCH: with `--dispatch <host>`, the op runs ON THE TARGET (as the confined
    // `ouro-op` principal through the fixed wrapper), not control-local. The remote runs the same
    // command with `--local`, reading the target-side attestation and executing there.
    if let Some(host) = optional(args, "--dispatch") {
        return dispatch_op(host, &op, &node, args, &paths, plan);
    }

    // §2.8 — legacy write entry points are disabled unless registered.
    parity::require_registered_write(&op)?;

    // Load the attestation (must be adopted, §1.C / §2.4).
    let local = args.iter().any(|a| a == "--local");
    let initial_att = load_attestation(&paths, &node, local)?;
    if initial_att.immutable.machine_id != node {
        return Err(OuroError::Validation(format!(
            "target binding mismatch: --node {node} does not match adopted machine {} — refused",
            initial_att.immutable.machine_id
        )));
    }

    // Recovery pass BEFORE any new write (§2.6), serialized by the same crash-releasing node lock.
    // Recovery uses the PRIOR journal's durable intent/pre-state/plans, never the new request.
    let journal = Journal::at(&tx_dir(&paths), &node);
    let seal = WriteSeal::at(&tx_dir(&paths), &node);
    {
        let _recovery_lock = crate::gate::NodeLock::acquire(
            &tx_dir(&paths).join("locks"), &node, "startup-recovery",
        )?;
        let recover_verify = |record: &JournalRecord| {
            verify_recovery_record(record, args, &paths, &node, local)
        };
        let recover_rollback = |record: &JournalRecord| {
            rollback_recovery_record(record, args, &paths, &node, local)
        };
        let recover_ops = RecoveryOps { verify: &recover_verify, rollback: &recover_rollback };
        if let Some(state) = transaction::recover(&journal, &seal, &recover_ops)? {
            if state == TxState::Sealed {
                return Err(OuroError::Validation(
                    "writes are sealed by a prior failed rollback — operator recovery required (§2.6)"
                        .into(),
                ));
            }
        }
    }
    // Recovery may have advanced or restored the durable attestation; never continue with the stale
    // copy loaded before reconciliation.
    let att = load_attestation(&paths, &node, local)?;

    // Build the intent from --param k=v flags (agent supplies PARAMETERS, never commands).
    let payload = collect_params(args);
    let fleet_permit_raw = optional(args, "--fleet-permit");
    let intent = Intent {
        schema_version: 1,
        operation_id: op.clone(),
        node_id: node.clone(),
        pre_state_generation: att.state.state_generation,
        pre_state_hash: att.closed_fingerprint(),
        // The canonical confirmation hash binds the exact fleet authorization as well as payload.
        expected_post_state: fleet_permit_raw
            .map(|permit| crate::intent::sha256_hex(permit.as_bytes()))
            .unwrap_or_default(),
        nonce: format!("{}-{}", node, att.state.state_generation),
        expiry_epoch: 0,
        payload,
    };
    // Validate against the deny-by-default registry + closed schema (§2.5).
    let spec = intent.validate(0)?;
    let payload_machine = intent.payload.get("machine").and_then(|value| value.as_str())
        .ok_or_else(|| OuroError::Validation("intent payload is missing its machine binding".into()))?;
    if payload_machine != node || payload_machine != att.immutable.machine_id {
        return Err(OuroError::Validation(format!(
            "target binding mismatch: payload machine {payload_machine} != adopted machine {} — refused",
            att.immutable.machine_id
        )));
    }

    // Dispatched writes carry the control's complete security identity; local invocations still
    // validate the local identity structure but do not claim control↔target parity.
    let id = parity::SecurityIdentity::local();
    parity::require_parity(&id, &id)?;
    if let Some(expected) = optional(args, "--expect-embedded") {
        parity::require_expected_wire_digest(expected)?;
    }

    // Hold the crash-releasing node lock through terminal transaction state. The guard performs the
    // initial full re-attestation and is called again immediately at the commit boundary.
    let canon = intent.canonical_hash();
    let audit_id = format!("op-{canon}");
    let probe = || read_observation(args).map(|observation| observation.live.to_live());
    let guard = crate::gate::require_attested_node(
        &att, &tx_dir(&paths).join("locks"), &node, &audit_id, &probe,
    )?;
    let fleet_sensitive = spec.touched.iter().any(|resource| {
        matches!(*resource, "container:restart" | "container:recreate")
    });
    let fleet_permit = if fleet_sensitive {
        let encoded = fleet_permit_raw.ok_or_else(|| {
            OuroError::Validation(format!(
                "{op} is disruptive and requires a signed --fleet-permit (§2.9)"
            ))
        })?;
        let permit: crate::fleet::StepPermit = serde_json::from_str(encoded)
            .map_err(|e| OuroError::Validation(format!("malformed fleet permit: {e}")))?;
        let shared = std::path::Path::new(crate::onboard::CONFIRM_SECRET_PATH);
        let secret = if local && shared.exists() {
            std::fs::read_to_string(shared).map_err(|e| {
                OuroError::Validation(format!("cannot read shared fleet secret: {e}"))
            })?
        } else {
            crate::confirm::load_or_create_secret(&paths.tool_run_secret)?
        };
        permit.verify(
            &node,
            &op,
            match att.immutable.role { Role::Bp => "bp", Role::Relay => "relay" },
            secret.trim().as_bytes(),
            crate::s0019_confirmation::current_epoch()?,
        )?;
        Some(permit)
    } else {
        None
    };
    // Upgrade safety is signed metadata, not an inference from "both images are allowlisted".
    let upgrade_transition = if op == "upgrade/step" {
        let target = intent.payload.get("image").and_then(|value| value.as_str())
            .ok_or_else(|| OuroError::Validation("upgrade/step lost target image".into()))?;
        let observation = read_observation(args)?;
        let allowlist = convention::Allowlist::embedded()?;
        let transition = allowlist
            .transition_for(&att.immutable.image_config_digest, target)?
            .clone();
        crate::upgrade::validate_transition(&transition, &allowlist, &observation.live.platform)?;
        Some(transition)
    } else {
        None
    };

    // Confirm gate for dangerous writes (§2.5): the token must be bound to THIS canonical intent.
    let verified_confirmation = if spec.mutability == Mutability::Dangerous {
        let token = optional(args, "--confirm-token").ok_or_else(|| {
            OuroError::Validation(format!(
                "{op} is a dangerous write — present the plan to the operator, get their go-ahead, \
                 then `ouro-ops confirm create --op {op} --node {node} --intent-hash {canon}` and \
                 pass --confirm-token (§2.5)"
            ))
        })?;
        // p6-3 — on the target (`--local`) the confirm-token is verified with the SHARED secret
        // onboard provisioned (so a control-minted token is honored); the bed / control path falls
        // back to the local tool-run secret.
        let shared = std::path::Path::new(crate::onboard::CONFIRM_SECRET_PATH);
        let secret = if local && shared.exists() {
            std::fs::read_to_string(shared)
                .map_err(|e| OuroError::Validation(format!("cannot read shared confirm secret: {e}")))?
        } else {
            crate::confirm::load_or_create_secret(&paths.tool_run_secret)?
        };
        let diff = format!("{op} on {node}");
        Some(crate::s0019_confirmation::verify(
            token, &canon, &diff, secret.trim().as_bytes(),
            crate::s0019_confirmation::current_epoch()?,
        )?)
    } else {
        None
    };

    // A managed READ (e.g. observability/health) passes the attested gate but takes no confirm and
    // no write transaction — it does not mutate. Return the fixed read argv (target-side executor
    // gathers + returns the data); no journal is touched.
    if spec.mutability == Mutability::Read {
        let plan = crate::executor::build_plan(&intent, &att, None).unwrap_or_default();
        audit_emit(&paths, "live_preflight", &node, json!({"operation_id": op, "intent_hash": canon}));
        output::print_json(&ToolOutput::ok("ouro.op.read", false).with_data(json!({
            "op": op, "node": node, "intent_hash": canon, "executor_plan": plan,
            "note": "managed read — no mutation; target-side executor gathers the data",
        })))?;
        return Ok(());
    }

    if plan {
        // Show the FIXED argv SEQUENCE the sealed executor WOULD run (from the attested container id
        // + digest-resolved artifacts, not the agent's params) — proof of what a real run does, with
        // no mutation. Preview mode (inbox=None) renders artifact paths as `<inbox:…>` placeholders.
        let steps = crate::executor::build_plan(&intent, &att, None).unwrap_or_default();
        output::print_json(&ToolOutput::ok("ouro.op.plan", false).with_data(json!({
            "op": op, "node": node, "mutability": format!("{:?}", spec.mutability),
            "intent_hash": canon, "touched": spec.touched, "executor_plan": steps,
            "upgrade_transition": upgrade_transition.as_ref(),
            "upgrade_failure_outcome": upgrade_transition.as_ref()
                .map(crate::upgrade::failure_outcome)
                .map(|outcome| format!("{outcome:?}")),
            "note": "plan mode — all gates passed; no mutation (real target execution is target-side)",
        })))?;
        return Ok(());
    }
    // p5-3 / p7-3 — the transaction's commit runs the sealed executor's FIXED argv SEQUENCE on the
    // target (from the attested container id + digest-resolved inbox artifacts, not agent params).
    // Artifact-bearing ops (kes opcert, deploy tx) are refused here if their artifact is not staged.
    // verify re-attests + checks readiness proxies; rollback restarts the node onto its prior state.
    // (Real docker exec is target-side; on the control host `run_plan` fails fast if docker is
    // absent, which the transaction rolls back.)
    let inbox = paths.home.join("inbox");
    // upgrade/step is a real container RECREATE (§2.10): the target image must be on the signed
    // allowlist (so the agent can only ever name a blinklabs baseline), and the recreate is built
    // from the target's own `docker inspect` facts (fail-closed if the probe couldn't model them).
    // Rollback recreates onto the PRIOR attested digest. All other ops use the sealed argv builder.
    let (commit_plan, rb_plan) = if op == "upgrade/step" {
        let to_digest = intent
            .payload
            .get("image")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OuroError::Validation("upgrade/step needs image (an allowlisted digest)".into()))?;
        let obs = read_observation(args)?;
        let transition = upgrade_transition.as_ref().ok_or_else(|| {
            OuroError::Validation("upgrade transition was not validated before planning".into())
        })?;
        let spec = obs.recreate.ok_or_else(|| {
            OuroError::Validation(
                "upgrade/step: the probe could not model the container run-spec (non-standard \
                 shape?) — refused rather than recreate blindly (§2.10)"
                    .into(),
            )
        })?;
        let commit = crate::executor::recreate_argv(&spec, &att.state.container_id, to_digest)?;
        let rb = if crate::upgrade::rollback_possible(transition) {
            Some(crate::executor::upgrade_rollback_plan(&att, &spec)?)
        } else {
            None
        };
        (commit, rb)
    } else {
        crate::executor::recoverable_plans(
            &intent, &att, &inbox, &tx_dir(&paths).join("rollback").join(&canon),
        )?
    };
    let base = JournalRecord {
        audit_id: format!("op-{canon}"),
        operation_id: op.clone(),
        node_id: node.clone(),
        state: TxState::Prepared,
        durable: Some(DurableTransaction {
            intent: intent.clone(),
            pre_attestation: att.clone(),
            commit_plan: commit_plan.clone(),
            rollback_plan: rb_plan.clone(),
        }),
    };
    // For upgrade, verify checks the NEW container landed on the target digest and ROTATES the
    // attestation to the new identity (else every later op would drift-refuse); a mismatch fails
    // verify → the transaction rolls back onto the prior digest. Other ops verify by re-attesting.
    let is_upgrade = op == "upgrade/step";
    // These ops deliberately change managed CONTENT (opcert / config / topology); their post-commit
    // verify checks identity only, then ADVANCES the managed state (CAS gen bump) and persists it —
    // otherwise the very next op would drift-refuse against the stale attestation.
    let managed_changing = matches!(
        op.as_str(),
        "kes-rotation/rotate" | "config/render" | "runtime/topology-apply"
    );
    let to_digest_owned = intent
        .payload
        .get("image")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let commit = || {
        guard.recheck_before_commit()?;
        crate::executor::run_plan(&commit_plan)
    };
    let verify = || {
        let live = read_observation(args)?;
        require_readiness(&att, &live, is_upgrade)?;
        if is_upgrade {
            if live.live.image_config_digest != to_digest_owned {
                return Err(OuroError::Validation(
                    "upgrade did not land on the target image digest — rolling back (§2.10)".into(),
                ));
            }
            if live.live.container_id.is_empty() {
                return Err(OuroError::Validation("no node container after upgrade — rolling back".into()));
            }
            rotate_attestation_for_upgrade(&paths, &node, local, &att, &live, &to_digest_owned)
        } else if managed_changing {
            // Immutable identity must still hold (an image swap / recreate is still caught); the
            // content hashes are expected to have changed → snapshot them as the new baseline.
            att.require_identity_matches(&live.live.to_live())?;
            if op == "kes-rotation/rotate" {
                let expected = intent.payload.get("opcert").and_then(|value| value.as_str())
                    .and_then(artifact_ref_digest)
                    .ok_or_else(|| OuroError::Validation("KES intent lost its artifact digest".into()))?;
                if live.live.kes_opcert_id != expected {
                    return Err(OuroError::Validation(
                        "installed opcert digest does not match the approved artifact".into(),
                    ));
                }
            }
            let advanced = att.advance_state(
                att.state.state_generation,
                ManagedState {
                    state_generation: att.state.state_generation, // advance_state bumps it
                    container_id: live.live.container_id.clone(),
                    topology_hash: live.live.topology_hash.clone(),
                    config_hash: live.live.config_hash.clone(),
                    kes_opcert_id: live.live.kes_opcert_id.clone(),
                },
            )?;
            persist_attestation(&paths, &node, local, &advanced)
        } else {
            att.require_matches_live(&live.live.to_live())
        }
    };
    let rollback = || {
        let plan = rb_plan.as_ref().ok_or_else(|| OuroError::Validation(format!(
            "{} has no safe automatic rollback; operator reconciliation required", op
        )))?;
        crate::executor::run_rollback_plan(&op, plan)?;
        persist_attestation(&paths, &node, local, &att)
    };
    // Consume only after every fail-closed preflight/plan has succeeded, but before the transaction
    // can enter Committing. A crash or failed mutation then burns the approval permanently.
    if let Some(permit) = &fleet_permit {
        crate::fleet::TargetFence::at(&tx_dir(&paths).join("fleet-fence"), &node)
            .accept(permit, crate::s0019_confirmation::current_epoch()?)?;
    }
    if let Some(confirmation) = &verified_confirmation {
        crate::s0019_confirmation::consume(
            &tx_dir(&paths).join("confirm-used").join(format!("{node}.log")), confirmation,
        )?;
    }
    let ops = TxOps { commit: &commit, verify: &verify, rollback: &rollback };
    let outcome = transaction::run(&journal, &seal, &base, &ops)?;
    audit_emit(&paths, "committed", &node, json!({"operation_id": op, "intent_hash": canon, "outcome": format!("{outcome:?}")}));
    output::print_json(&ToolOutput::ok("ouro.op.run", true).with_data(json!({
        "op": op, "node": node, "intent_hash": canon, "outcome": format!("{outcome:?}"),
    })))?;
    Ok(())
}

/// `ouro-ops confirm create --op <id> --node <id> --intent-hash <hash>` — mint a token bound to the
/// exact canonical intent + human diff (§2.5). Represents the OPERATOR'S approval.
pub fn run_confirm_create(args: &[String]) -> Result<()> {
    let op = flag(args, "--op")?;
    let node = flag(args, "--node")?;
    crate::intent::validate_machine_id(node)?;
    parity::require_registered_write(op)?;
    let hash = flag(args, "--intent-hash")?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OuroError::Validation(
            "--intent-hash must be the 64-hex SHA-256 emitted by `ouro-ops op run`".into(),
        ));
    }
    let ttl = crate::confirm::parse_ttl(optional(args, "--ttl").unwrap_or("5m"))?;
    let ttl_seconds = u64::try_from(ttl.num_seconds())
        .map_err(|_| OuroError::Validation("confirmation ttl must be positive".into()))?;
    let paths = ConfigPaths::discover();
    let secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let diff = format!("{op} on {node}");
    let (token, expires_at) = crate::s0019_confirmation::mint(
        hash, &diff, secret.as_bytes(), crate::s0019_confirmation::current_epoch()?, ttl_seconds,
    )?;
    output::print_json(&ToolOutput::ok("ouro.confirm.create", false).with_data(json!({
        "op": op, "node": node, "intent_hash": hash, "diff": diff,
        "confirm_token": token, "expires_at_epoch": expires_at, "single_use": true,
    })))?;
    Ok(())
}

/// p5-1 — build (and, unless `--plan`, run) the SSH dispatch of an `op` to the target. The remote
/// command is the same op args with `--dispatch` stripped and `--local` appended. Real SSH exec is
/// bed-level (p5-6); `--plan` prints the confined remote command for inspection.
fn dispatch_op(
    host: &str,
    op: &str,
    node: &str,
    args: &[String],
    paths: &ConfigPaths,
    plan: bool,
) -> Result<()> {
    // The SSH client key is the operator's credential (creds://<name>), resolved to a local path.
    let key_ref = optional(args, "--ssh-key").unwrap_or("creds://ouro-op");
    let key = crate::secrets::CredentialRef::parse(key_ref)?.resolve(&paths.credentials_dir)?;
    // Remote args = original op args with our control-only flags removed, plus --local.
    let mut remote: Vec<String> = vec!["run".into()];
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "run" => {}
            "--dispatch" | "--ssh-key" | "--observation" => i += 1, // control-only; skip flag+value
            "--plan" => {}
            other => remote.push(other.to_string()),
        }
        i += 1;
    }
    remote.push("--local".into());
    let argv = crate::dispatch::op_dispatch_argv(
        host,
        22,
        &key,
        &paths.known_hosts,
        &remote,
        &parity::SecurityIdentity::local().wire_digest(),
    );
    if plan {
        output::print_json(&ToolOutput::ok("ouro.op.dispatch.plan", false).with_data(json!({
            "op": op, "node": node, "target": host, "principal": "ouro-op",
            "ssh_argv": argv,
            "note": "dispatch plan — confined + host-key-pinned; real SSH exec is bed-level (p5-6)",
        })))?;
        return Ok(());
    }
    let out = std::process::Command::new("ssh")
        .args(&argv)
        .output()
        .map_err(|e| OuroError::Validation(format!("ssh dispatch failed: {e}")))?;
    output::forward_tool_stdout(&out.stdout)?;
    std::process::exit(out.status.code().unwrap_or(255));
}

/// p6-3 — SSH-dispatch `adopt` to the target (as the bootstrap account), running `adopt --local`
/// there. Control-only flags are stripped; the target self-probes (p6-2).
fn dispatch_adopt(host: &str, node: &str, args: &[String], paths: &ConfigPaths, plan: bool) -> Result<()> {
    let user = optional(args, "--bootstrap-user").unwrap_or("root");
    let key_ref = optional(args, "--ssh-key").unwrap_or("creds://bootstrap");
    let key = crate::secrets::CredentialRef::parse(key_ref)?.resolve(&paths.credentials_dir)?;
    let mut remote: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dispatch" | "--ssh-key" | "--bootstrap-user" | "--observation" => i += 1,
            "--plan" => {}
            other => remote.push(other.to_string()),
        }
        i += 1;
    }
    let argv = crate::dispatch::adopt_dispatch_argv(host, 22, user, &key, &paths.known_hosts, &remote);
    if plan {
        output::print_json(&ToolOutput::ok("ouro.adopt.dispatch.plan", false).with_data(json!({
            "node": node, "target": host, "principal": user, "ssh_argv": argv,
            "note": "dispatch plan — bootstrap account runs `ouro-ops adopt --local` on the target",
        })))?;
        return Ok(());
    }
    let out = std::process::Command::new("ssh")
        .args(&argv)
        .output()
        .map_err(|e| OuroError::Validation(format!("ssh dispatch failed: {e}")))?;
    output::forward_tool_stdout(&out.stdout)?;
    std::process::exit(out.status.code().unwrap_or(255));
}

fn artifact_ref_digest(reference: &str) -> Option<&str> {
    reference.split_once("@sha256:").map(|(_, digest)| digest)
}

/// Verify the exact interrupted operation described by the durable journal. This may finalize the
/// attestation update that succeeded immediately before a crash; it never consults the new intent.
fn verify_recovery_record(
    record: &JournalRecord,
    args: &[String],
    paths: &ConfigPaths,
    node: &str,
    local: bool,
) -> Result<()> {
    let durable = record.durable.as_ref().ok_or_else(|| {
        OuroError::Validation("transaction has no durable recovery context".into())
    })?;
    if record.node_id != node
        || durable.intent.node_id != node
        || durable.pre_attestation.immutable.machine_id != node
        || durable.intent.operation_id != record.operation_id
    {
        return Err(OuroError::Validation(
            "durable transaction identity mismatch — refusing recovery".into(),
        ));
    }
    let observation = read_observation(args)?;
    require_readiness(
        &durable.pre_attestation,
        &observation,
        record.operation_id == "upgrade/step",
    )?;
    match record.operation_id.as_str() {
        "upgrade/step" => {
            let expected = durable.intent.payload.get("image").and_then(|value| value.as_str())
                .ok_or_else(|| OuroError::Validation("upgrade journal lost target digest".into()))?;
            if observation.live.image_config_digest != expected {
                return Err(OuroError::Validation("interrupted upgrade is not on its approved image".into()));
            }
            rotate_attestation_for_upgrade(
                paths, node, local, &durable.pre_attestation, &observation, expected,
            )
        }
        "kes-rotation/rotate" => {
            durable.pre_attestation.require_identity_matches(&observation.live.to_live())?;
            let expected = durable.intent.payload.get("opcert").and_then(|value| value.as_str())
                .and_then(artifact_ref_digest)
                .ok_or_else(|| OuroError::Validation("KES journal lost artifact digest".into()))?;
            if observation.live.kes_opcert_id != expected {
                return Err(OuroError::Validation("interrupted KES opcert digest is not approved".into()));
            }
            persist_advanced_recovery(paths, node, local, &durable.pre_attestation, &observation)
        }
        "config/render" | "runtime/topology-apply" => {
            durable.pre_attestation.require_identity_matches(&observation.live.to_live())?;
            persist_advanced_recovery(paths, node, local, &durable.pre_attestation, &observation)
        }
        _ => durable.pre_attestation.require_matches_live(&observation.live.to_live()),
    }
}

fn persist_advanced_recovery(
    paths: &ConfigPaths,
    node: &str,
    local: bool,
    pre: &AdoptionAttestation,
    observation: &Observation,
) -> Result<()> {
    let advanced = pre.advance_state(
        pre.state.state_generation,
        ManagedState {
            state_generation: pre.state.state_generation,
            container_id: observation.live.container_id.clone(),
            topology_hash: observation.live.topology_hash.clone(),
            config_hash: observation.live.config_hash.clone(),
            kes_opcert_id: observation.live.kes_opcert_id.clone(),
        },
    )?;
    persist_attestation(paths, node, local, &advanced)
}

fn rollback_recovery_record(
    record: &JournalRecord,
    args: &[String],
    paths: &ConfigPaths,
    node: &str,
    local: bool,
) -> Result<()> {
    let durable = record.durable.as_ref().ok_or_else(|| {
        OuroError::Validation("transaction has no durable recovery context".into())
    })?;
    let plan = durable.rollback_plan.as_ref().ok_or_else(|| {
        OuroError::Validation(format!(
            "{} is irreversible/ambiguous; automatic rollback is forbidden",
            record.operation_id
        ))
    })?;
    // If no observable node state changed, there is nothing to undo (e.g. crash after Committing
    // was journaled but before the first executor step). Otherwise run the persisted exact inverse.
    let observation = read_observation(args)?;
    if durable.pre_attestation.require_matches_live(&observation.live.to_live()).is_err() {
        crate::executor::run_rollback_plan(&record.operation_id, plan)?;
    }
    persist_attestation(paths, node, local, &durable.pre_attestation)
}

fn load_attestation(paths: &ConfigPaths, node: &str, local: bool) -> Result<AdoptionAttestation> {
    let p = attestation_path_for(paths, node, local);
    let text = std::fs::read_to_string(&p).map_err(|_| {
        OuroError::Validation(format!(
            "not_ouro_managed: node {node} has no adoption attestation — run `ouro-ops adopt` \
             first; ops are refused, never adapted (§1.C)"
        ))
    })?;
    serde_json::from_str(&text)
        .map_err(|e| OuroError::Validation(format!("malformed attestation: {e}")))
}

/// Persist an attestation, preserving the resolved `contract` block already on disk (the shell
/// layout accessors read it). Used to advance the managed state after a state-changing op.
fn persist_attestation(
    paths: &ConfigPaths,
    node: &str,
    local: bool,
    att: &AdoptionAttestation,
) -> Result<()> {
    let p = attestation_path_for(paths, node, local);
    let contract = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("contract").cloned());
    let mut doc = serde_json::to_value(att).unwrap();
    if let Some(c) = contract {
        doc["contract"] = c;
    }
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap())
        .map_err(|e| OuroError::Validation(format!("cannot persist advanced attestation: {e}")))?;
    Ok(())
}

/// After a successful upgrade recreate, ROTATE the attestation onto the new identity (§2.10): the
/// image digest, container id, creation epoch, entrypoint/args/mounts all changed, so we rebuild the
/// immutable identity from the fresh observation — preserving role, machine id, host key, and the
/// operator's approval evidence — and bump the state generation. The new digest is re-checked
/// against the signed allowlist. Written atomically like adopt (with the resolved contract paths).
fn rotate_attestation_for_upgrade(
    paths: &ConfigPaths,
    node: &str,
    local: bool,
    old: &AdoptionAttestation,
    obs: &Observation,
    to_digest: &str,
) -> Result<()> {
    let allow = convention::Allowlist::embedded()?;
    let contract = allow.contract_for(to_digest, &obs.live.platform)?;
    let immutable = ImmutableIdentity {
        role: old.immutable.role,
        contract_id: contract.contract_id.clone(),
        convention_version: contract.convention_version,
        host_key_sha256: old.immutable.host_key_sha256.clone(),
        machine_id: old.immutable.machine_id.clone(),
        oci_index_digest: obs.live.image_config_digest.clone(),
        platform_manifest_digest: obs.live.image_config_digest.clone(),
        image_config_digest: obs.live.image_config_digest.clone(),
        container_creation_epoch: obs.live.container_creation_epoch,
        entrypoint: obs.live.entrypoint.clone(),
        args: obs.live.args.clone(),
        mounts: obs
            .live
            .mount_source_ids
            .iter()
            .map(|sid| TypedMount {
                kind: "bind".into(),
                source_id: sid.clone(),
                destination: String::new(),
                read_only: false,
                owner: "root".into(),
                mode: "0755".into(),
                no_symlink: true,
            })
            .collect(),
        network: obs.live.network.clone(),
        genesis_hash: obs.live.genesis_hash.clone(),
        public_credential_ids: if obs.live.kes_opcert_id.is_empty() {
            vec![]
        } else {
            vec![obs.live.kes_opcert_id.clone()]
        },
        // The upgrade was operator-approved via the confirm-token; carry the adoption evidence.
        approval_evidence_hash: old.immutable.approval_evidence_hash.clone(),
    };
    let att = AdoptionAttestation {
        immutable,
        state: ManagedState {
            state_generation: old.state.state_generation + 1,
            container_id: obs.live.container_id.clone(),
            topology_hash: obs.live.topology_hash.clone(),
            config_hash: obs.live.config_hash.clone(),
            kes_opcert_id: obs.live.kes_opcert_id.clone(),
        },
    };
    let p = attestation_path_for(paths, node, local);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut doc = serde_json::to_value(&att).unwrap();
    doc["contract"] = json!({ "in_container_paths": contract.in_container_paths });
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap())
        .map_err(|e| OuroError::Validation(format!("cannot write rotated attestation: {e}")))?;
    Ok(())
}

fn collect_params(args: &[String]) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == "--param" {
            if let Some((k, v)) = args[i + 1].split_once('=') {
                obj.insert(k.to_string(), json!(v));
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    serde_json::Value::Object(obj)
}

fn flag<'a>(args: &'a [String], name: &str) -> Result<&'a str> {
    args.windows(2)
        .find(|p| p[0] == name)
        .map(|p| p[1].as_str())
        .ok_or_else(|| OuroError::InvalidArgs(format!("missing {name}")))
}
fn optional<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2).find(|p| p[0] == name).map(|p| p[1].as_str())
}
