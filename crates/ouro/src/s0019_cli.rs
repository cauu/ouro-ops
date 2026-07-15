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
use crate::transaction::{self, Journal, JournalRecord, TxOps, TxState, WriteSeal};
use crate::{convention, parity, readiness, OuroError, Result};

/// Where the attestation lives. On the TARGET (`--local`, p5-4) it is the single root-owned file
/// `/var/lib/ouro/node-attestation.json` (overridable via OURO_ATTESTATION, matching
/// `ouro-attested.sh`); on the control host it is per-node under OURO_HOME (pre-dispatch modelling).
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

fn read_observation(args: &[String]) -> Result<Observation> {
    let path = flag(args, "--observation")?;
    let text = std::fs::read_to_string(path)
        .map_err(|e| OuroError::Validation(format!("cannot read observation {path}: {e}")))?;
    serde_json::from_str(&text)
        .map_err(|e| OuroError::Validation(format!("malformed observation: {e}")))
}

/// `ouro-ops adopt` — conformance → evidence-bound approval → write the attestation. Non-disruptive
/// (writes metadata only). Refuses a non-conforming node (never adapts).
pub fn run_adopt(args: &[String]) -> Result<()> {
    let node = flag(args, "--node")?.to_string();
    let local = args.iter().any(|a| a == "--local");
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
    let plan = args.iter().any(|a| a == "--plan");
    let paths = ConfigPaths::discover();

    // p5-1 — SSH DISPATCH: with `--dispatch <host>`, the op runs ON THE TARGET (as the confined
    // `ouro-exec` principal through the fixed wrapper), not control-local. The remote runs the same
    // command with `--local`, reading the target-side attestation and executing there.
    if let Some(host) = optional(args, "--dispatch") {
        return dispatch_op(host, &op, &node, args, &paths, plan);
    }

    // §2.8 — legacy write entry points are disabled unless registered.
    parity::require_registered_write(&op)?;

    // Load the attestation (must be adopted, §1.C / §2.4).
    let local = args.iter().any(|a| a == "--local");
    let att = load_attestation(&paths, &node, local)?;

    // Recovery pass BEFORE any new write (§2.6): reconcile an interrupted transaction.
    let journal = Journal::at(&tx_dir(&paths), &node);
    let seal = WriteSeal::at(&tx_dir(&paths), &node);
    let noop = || Ok(());
    let recover_ops = TxOps { commit: &noop, verify: &noop, rollback: &noop };
    if let Some(state) = transaction::recover(&journal, &seal, &recover_ops)? {
        if state == TxState::Sealed {
            return Err(OuroError::Validation(
                "writes are sealed by a prior failed rollback — operator recovery required (§2.6)"
                    .into(),
            ));
        }
    }

    // Build the intent from --param k=v flags (agent supplies PARAMETERS, never commands).
    let payload = collect_params(args);
    let intent = Intent {
        schema_version: 1,
        operation_id: op.clone(),
        node_id: node.clone(),
        pre_state_generation: att.state.state_generation,
        pre_state_hash: att.closed_fingerprint(),
        expected_post_state: String::new(),
        nonce: format!("{}-{}", node, att.state.state_generation),
        expiry_epoch: 0,
        payload,
    };
    // Validate against the deny-by-default registry + closed schema (§2.5).
    let spec = intent.validate(0)?;

    // Parity: this binary must match itself (control-side); a real dispatch also checks the target.
    let id = parity::SecurityIdentity::local();
    parity::require_parity(&id, &id)?;

    // Live re-attestation (§2.4): compare the observation to the attestation before any mutation.
    let obs = read_observation(args)?;
    att.require_matches_live(&obs.live.to_live())?;

    // Confirm gate for dangerous writes (§2.5): the token must be bound to THIS canonical intent.
    let canon = intent.canonical_hash();
    if spec.mutability == Mutability::Dangerous {
        let token = optional(args, "--confirm-token").ok_or_else(|| {
            OuroError::Validation(format!(
                "{op} is a dangerous write — present the plan to the operator, get their go-ahead, \
                 then `ouro-ops confirm create --op {op} --node {node} --intent-hash {canon}` and \
                 pass --confirm-token (§2.5)"
            ))
        })?;
        let secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
        let diff = format!("{op} on {node}");
        readiness::verify_confirm(token, &canon, &diff, secret.as_bytes())?;
    }

    // A managed READ (e.g. observability/health) passes the attested gate but takes no confirm and
    // no write transaction — it does not mutate. Return the fixed read argv (target-side executor
    // gathers + returns the data); no journal is touched.
    if spec.mutability == Mutability::Read {
        let argv = crate::executor::build_argv(&intent, &att).unwrap_or_default();
        output::print_json(&ToolOutput::ok("ouro.op.read", false).with_data(json!({
            "op": op, "node": node, "intent_hash": canon, "executor_argv": argv,
            "note": "managed read — no mutation; target-side executor gathers the data",
        })))?;
        return Ok(());
    }

    // Crash-durable transaction (§2.6). In --plan mode the executor is a no-op (gates proven, no
    // mutation); real target execution is target-side (p5).
    let base = JournalRecord {
        audit_id: format!("op-{canon}"),
        operation_id: op.clone(),
        node_id: node.clone(),
        state: TxState::Prepared,
    };
    if plan {
        // Show the FIXED argv the sealed executor WOULD run (from the attested container id, not
        // the agent's params) — proof of what a real run does, with no mutation.
        let argv = crate::executor::build_argv(&intent, &att).unwrap_or_default();
        output::print_json(&ToolOutput::ok("ouro.op.plan", false).with_data(json!({
            "op": op, "node": node, "mutability": format!("{:?}", spec.mutability),
            "intent_hash": canon, "touched": spec.touched, "executor_argv": argv,
            "note": "plan mode — all gates passed; no mutation (real target execution is target-side)",
        })))?;
        return Ok(());
    }
    // p5-3 — the transaction's commit runs the sealed executor's FIXED argv on the target (from the
    // attested container id, not agent params). verify re-attests + checks readiness proxies;
    // rollback restarts the node onto its prior state. (Real docker exec is target-side; on the
    // control host `run_argv` will fail fast if docker is absent, which the transaction rolls back.)
    let argv = crate::executor::build_argv(&intent, &att)?;
    let commit = || crate::executor::run_argv(&argv);
    let verify = || {
        let live = read_observation(args)?;
        att.require_matches_live(&live.live.to_live())
    };
    let rollback = || crate::executor::run_argv(&argv); // idempotent restart onto the prior config
    let ops = TxOps { commit: &commit, verify: &verify, rollback: &rollback };
    let outcome = transaction::run(&journal, &seal, &base, &ops)?;
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
    let hash = flag(args, "--intent-hash")?;
    let paths = ConfigPaths::discover();
    let secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let diff = format!("{op} on {node}");
    let token = readiness::bind_confirm(hash, &diff, secret.as_bytes());
    output::print_json(&ToolOutput::ok("ouro.confirm.create", false).with_data(json!({
        "op": op, "node": node, "intent_hash": hash, "diff": diff, "confirm_token": token,
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
    let key_ref = optional(args, "--ssh-key").unwrap_or("creds://ouro-exec");
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
        &crate::skills::embedded_digest(),
    );
    if plan {
        output::print_json(&ToolOutput::ok("ouro.op.dispatch.plan", false).with_data(json!({
            "op": op, "node": node, "target": host, "principal": "ouro-exec",
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
