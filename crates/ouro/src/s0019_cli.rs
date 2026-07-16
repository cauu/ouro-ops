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

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::attestation::{
    self, AdoptionAttestation, ImmutableIdentity, LiveObservation, ManagedState, Role, TypedMount,
};
use crate::config::ConfigPaths;
use crate::domain::{Machine, MachineRole, PoolSpec, RuntimeMode};
use crate::intent::{Intent, Mutability};
use crate::output::{self, ToolOutput};
use crate::supervisor::SupervisorObservation;
use crate::transaction::{
    self, DurableTransaction, Journal, JournalRecord, TxOps, TxState, WriteSeal,
};
use crate::{convention, parity, OuroError, Result};

const DISPATCH_DIAGNOSTIC_CAP: usize = 2048;
const INBOX_OUTPUT_CAP: usize = 16 * 1024;

/// Preserve the remote command's one-record contract. A well-formed remote ToolOutput is forwarded
/// byte-for-byte and its exit code is retained without a second local error. SSH/protocol failures
/// that produced no ToolOutput become one bounded local record, including enough untrusted stderr
/// to diagnose authentication/host-key failures without flooding the agent context.
fn finish_ssh_dispatch(tool: &str, result: &crate::ssh::SshOutcome) -> Result<()> {
    let exit = result.status;
    let typed = serde_json::from_slice::<serde_json::Value>(result.stdout.as_bytes())
        .ok()
        .is_some_and(|value| {
            value.is_object() && value.get("tool").is_some() && value.get("status").is_some()
        });
    if typed {
        output::forward_tool_stdout(result.stdout.as_bytes())?;
        return if exit == 0 { Ok(()) } else { Err(OuroError::Reported(exit)) };
    }

    let bounded = |raw: &[u8]| {
        String::from_utf8_lossy(raw)
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
            .take(DISPATCH_DIAGNOSTIC_CAP)
            .collect::<String>()
            .trim()
            .to_string()
    };
    let stdout = bounded(result.stdout.as_bytes());
    let stderr = bounded(result.stderr.as_bytes());
    let detail = if exit == 0 {
        format!(
            "target returned no typed ToolOutput (bounded stdout: {})",
            if stdout.is_empty() { "<empty>" } else { &stdout }
        )
    } else {
        format!(
            "SSH/remote dispatch failed with exit {exit} (bounded stderr: {}; bounded stdout: {})",
            if stderr.is_empty() { "<empty>" } else { &stderr },
            if stdout.is_empty() { "<empty>" } else { &stdout },
        )
    };
    let reported_exit = if exit == 0 { 20 } else { exit };
    output::print_json(&ToolOutput::failure(
        tool,
        if exit == 0 {
            "invalid_remote_output".to_string()
        } else {
            format!("ssh_exit_{exit}")
        },
        detail,
    ))?;
    Err(OuroError::Reported(reported_exit))
}

/// Where the attestation lives. On the TARGET (`--local`, p5-4) it is the single root-owned file
/// `/var/lib/ouro/node-attestation.json` (overridable via OURO_ATTESTATION, matching
/// `ouro-attested.sh`); on the control host it is per-node under OURO_HOME (pre-dispatch modelling).
/// `ouro-ops inbox stage`: bounded local or SSH-streamed target ingress. A dispatched artifact is
/// sent over stdin to the fixed target wrapper, never referenced by a control-local path.
pub fn run_inbox(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("stage") {
        return Err(OuroError::InvalidArgs(
            "expected: ouro-ops inbox stage --type <opcert|tx|image> \
             (--file <path> [--dispatch <host>] | --stdin --local)"
                .into(),
        ));
    }
    let args = &args[1..];
    validate_closed_args(
        args,
        &["--type", "--file", "--dispatch", "--ssh-key", "--expect-ref"],
        &["--stdin", "--local", "--plan"],
        &[],
    )?;
    let kind = match flag(args, "--type")? {
        "opcert" => crate::inbox::ArtifactType::Opcert,
        "tx" => crate::inbox::ArtifactType::Tx,
        "image" => crate::inbox::ArtifactType::Image,
        other => return Err(OuroError::Validation(format!("--type must be opcert|tx|image, got {other}"))),
    };
    let paths = ConfigPaths::discover();
    if let Some(host) = optional(args, "--dispatch") {
        if args.iter().any(|arg| arg == "--stdin" || arg == "--local") {
            return Err(OuroError::Validation(
                "control dispatch requires --file; --stdin/--local are target-wrapper only".into(),
            ));
        }
        let file = flag(args, "--file")?;
        let (mut source, preview) =
            crate::inbox::preview_source(kind, std::path::Path::new(file))?;
        let key_ref = optional(args, "--ssh-key").unwrap_or("creds://ouro-op");
        let key = crate::secrets::CredentialRef::parse(key_ref)?.resolve(&paths.credentials_dir)?;
        let argv = crate::dispatch::inbox_dispatch_argv(
            host,
            22,
            &key,
            &paths.known_hosts,
            kind.prefix(),
            &preview.artifact_ref,
        );
        if args.iter().any(|arg| arg == "--plan") {
            output::print_json(&ToolOutput::ok("ouro.inbox.dispatch.plan", false).with_data(json!({
                "target": host,
                "artifact_type": kind,
                "planned_artifact_ref": preview.artifact_ref,
                "size_bytes": preview.size_bytes,
                "ssh_argv": argv,
                "transport": "bounded validated stdin to fixed target wrapper; source path/content omitted",
            })))?;
            return Ok(());
        }
        let expected_ref = flag(args, "--expect-ref")?;
        if expected_ref != preview.artifact_ref {
            return Err(OuroError::Validation(
                "artifact bytes no longer match the operator-reviewed --expect-ref; preview again"
                    .into(),
            ));
        }
        let mut child = std::process::Command::new("ssh")
            .args(&argv)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| OuroError::Validation(format!("inbox SSH dispatch failed: {e}")))?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            OuroError::Validation("inbox SSH dispatch has no stdin pipe".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            OuroError::Validation("inbox SSH dispatch has no stdout pipe".into())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            OuroError::Validation("inbox SSH dispatch has no stderr pipe".into())
        })?;
        let copy = std::thread::spawn(move || -> std::io::Result<u64> {
            let copied = std::io::copy(&mut source, &mut stdin)?;
            drop(stdin);
            Ok(copied)
        });
        let drain = |mut pipe: Box<dyn Read + Send>| {
            std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
                let mut bounded = Vec::new();
                pipe.by_ref().take((INBOX_OUTPUT_CAP + 1) as u64).read_to_end(&mut bounded)?;
                Ok(bounded)
            })
        };
        let stdout_drain = drain(Box::new(stdout));
        let stderr_drain = drain(Box::new(stderr));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().ok();
                child.wait().ok();
                return Err(OuroError::Validation(
                    "target inbox transport exceeded the 10-minute bounded deadline".into(),
                ));
            }
            // A completed drain before process exit can mean the peer exceeded the cap and the
            // pipe is no longer being consumed. Terminate promptly rather than allowing it to
            // block until the deadline; the joined length below distinguishes EOF from overflow.
            if stdout_drain.is_finished() || stderr_drain.is_finished() {
                std::thread::sleep(std::time::Duration::from_millis(10));
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                child.kill().ok();
                child.wait().ok();
                return Err(OuroError::Validation(
                    "target inbox closed an output channel early or exceeded the bounded protocol limit"
                        .into(),
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let stdout = stdout_drain.join().map_err(|_| {
            OuroError::Validation("target inbox stdout drain panicked".into())
        })??;
        let stderr = stderr_drain.join().map_err(|_| {
            OuroError::Validation("target inbox stderr drain panicked".into())
        })??;
        if stdout.len() > INBOX_OUTPUT_CAP || stderr.len() > INBOX_OUTPUT_CAP {
            return Err(OuroError::Validation(
                "target inbox output exceeded the bounded protocol limit".into(),
            ));
        }
        let copied = copy.join().map_err(|_| {
            OuroError::Validation("artifact transport worker panicked".into())
        })?;
        if !status.success() {
            return Err(OuroError::Validation(format!(
                "target inbox rejected the artifact: {}",
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        let copied = copied?;
        if copied != preview.size_bytes {
            return Err(OuroError::Validation(
                "artifact transport byte count changed unexpectedly".into(),
            ));
        }
        let response: serde_json::Value = serde_json::from_slice(&stdout).map_err(|error| {
            OuroError::Validation(format!(
                "target inbox returned invalid or multiple protocol records: {error}"
            ))
        })?;
        let accepted_ref = response.pointer("/data/artifact_ref")
            .and_then(serde_json::Value::as_str);
        if response.get("tool").and_then(serde_json::Value::as_str) != Some("ouro.inbox.stage")
            || response.get("status").and_then(serde_json::Value::as_str) != Some("ok")
            || response.get("changed").and_then(serde_json::Value::as_bool) != Some(true)
            || accepted_ref != Some(expected_ref)
        {
            return Err(OuroError::Validation(
                "target inbox success did not bind the exact reviewed artifact reference".into(),
            ));
        }
        output::forward_tool_stdout(&stdout)?;
        return Ok(());
    }

    let inbox = paths.home.join("inbox");
    let reference = if args.iter().any(|arg| arg == "--stdin") {
        if !args.iter().any(|arg| arg == "--local") {
            return Err(OuroError::Validation(
                "--stdin is accepted only by the target-local fixed wrapper".into(),
            ));
        }
        let expected_ref = flag(args, "--expect-ref")?;
        crate::inbox::stage_reader_expected(
            &inbox,
            kind,
            std::io::stdin().lock(),
            Some(expected_ref),
        )?
    } else {
        let file = flag(args, "--file")?;
        crate::inbox::stage_file(&inbox, kind, std::path::Path::new(file))?
    };
    output::print_json(&ToolOutput::ok("ouro.inbox.stage", true).with_data(json!({
        "artifact_ref": reference, "note": "reference this in an intent --param; never a raw path",
    })))?;
    Ok(())
}

#[derive(Debug)]
struct FleetLiveStatus {
    node: String,
    role: String,
    network: String,
    genesis_hash: String,
    host_key_sha256: String,
    online: bool,
    image_config_digest: String,
    state_generation: u64,
}

/// Read one target's closed fleet facts through the same confined, pinned, parity-checked op
/// channel used for managed operations. Any transport/protocol/adoption/drift failure refuses the
/// entire fleet snapshot; an unavailable node is never silently converted into an agent-supplied
/// count.
fn fetch_fleet_status(
    machine: &Machine,
    paths: &ConfigPaths,
    allowlist_digest: &str,
) -> Result<FleetLiveStatus> {
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let remote = vec![
        "run".into(),
        "--op".into(),
        "fleet/status".into(),
        "--node".into(),
        machine.id.clone(),
        "--param".into(),
        format!("machine={}", machine.id),
        "--local".into(),
        "--expect-allowlist".into(),
        allowlist_digest.into(),
    ];
    let argv = crate::dispatch::op_dispatch_argv(
        &machine.ssh.host,
        machine.ssh.port,
        &key,
        &paths.known_hosts,
        &remote,
        &parity::SecurityIdentity::local().wire_digest(),
    );
    let result = crate::ssh::bounded_ssh(
        &argv,
        std::time::Duration::from_secs(45),
        256 * 1024,
        "fleet live-facts SSH",
    ).map_err(|e| OuroError::Validation(format!(
        "fleet live-facts SSH failed for {}: {e}", machine.id
    )))?;
    let bounded = |raw: &[u8]| {
        String::from_utf8_lossy(raw)
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
            .take(DISPATCH_DIAGNOSTIC_CAP)
            .collect::<String>()
            .trim()
            .to_string()
    };
    if result.status != 0 {
        let stderr = bounded(result.stderr.as_bytes());
        let stdout = bounded(result.stdout.as_bytes());
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(OuroError::Validation(format!(
            "fleet live-facts target {} refused/unreachable (exit {}): {}",
            machine.id,
            result.status,
            if detail.is_empty() { "<no diagnostic>" } else { &detail }
        )));
    }
    let value: serde_json::Value = serde_json::from_slice(result.stdout.as_bytes()).map_err(|error| {
        OuroError::Validation(format!(
            "fleet live-facts target {} returned malformed JSON: {error} (bounded DATA: {})",
            machine.id,
            bounded(result.stdout.as_bytes())
        ))
    })?;
    if value.get("tool").and_then(serde_json::Value::as_str) != Some("ouro.op.read")
        || value.get("status").and_then(serde_json::Value::as_str) != Some("ok")
        || value.get("changed").and_then(serde_json::Value::as_bool) != Some(false)
        || value.pointer("/data/op").and_then(serde_json::Value::as_str) != Some("fleet/status")
        || value.pointer("/data/node").and_then(serde_json::Value::as_str)
            != Some(machine.id.as_str())
    {
        return Err(OuroError::Validation(format!(
            "fleet live-facts target {} returned an unexpected typed record (bounded DATA: {})",
            machine.id,
            bounded(result.stdout.as_bytes())
        )));
    }
    let result = value.pointer("/data/result").ok_or_else(|| {
        OuroError::Validation(format!("fleet live-facts target {} omitted result", machine.id))
    })?;
    let role = result.get("role").and_then(serde_json::Value::as_str).ok_or_else(|| {
        OuroError::Validation(format!("fleet live-facts target {} omitted role", machine.id))
    })?;
    let expected_role = match machine.role { MachineRole::Bp => "bp", MachineRole::Relay => "relay" };
    let node = result.get("node").and_then(serde_json::Value::as_str).unwrap_or("");
    if node != machine.id || role != expected_role {
        return Err(OuroError::Validation(format!(
            "fleet live-facts identity mismatch for {}: target reported node={node:?} role={role:?}",
            machine.id
        )));
    }
    let network = result.get("network").and_then(serde_json::Value::as_str).ok_or_else(|| {
        OuroError::Validation(format!("fleet live-facts target {} omitted network", machine.id))
    })?;
    let genesis_hash = result
        .get("genesis_hash")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| OuroError::Validation(format!(
            "fleet live-facts target {} omitted genesis_hash",
            machine.id
        )))?;
    let host_key_sha256 = result
        .get("host_key_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| OuroError::Validation(format!(
            "fleet live-facts target {} omitted host_key_sha256",
            machine.id
        )))?;
    if !valid_ssh_sha256_fingerprint(host_key_sha256) {
        return Err(OuroError::Validation(format!(
            "fleet live-facts target {} returned an invalid OpenSSH SHA256 host-key fingerprint",
            machine.id
        )));
    }
    let image = result
        .get("image_config_digest")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let valid_image = image.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.chars().all(|ch| ch.is_ascii_hexdigit())
    });
    if !valid_image {
        return Err(OuroError::Validation(format!(
            "fleet live-facts target {} returned an invalid image digest", machine.id
        )));
    }
    Ok(FleetLiveStatus {
        node: node.into(),
        role: role.into(),
        network: network.into(),
        genesis_hash: genesis_hash.into(),
        host_key_sha256: host_key_sha256.into(),
        online: result.get("online").and_then(serde_json::Value::as_bool).ok_or_else(|| {
            OuroError::Validation(format!("fleet live-facts target {} omitted online", machine.id))
        })?,
        image_config_digest: image.into(),
        state_generation: result
            .get("state_generation")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| OuroError::Validation(format!(
                "fleet live-facts target {} omitted state_generation", machine.id
            )))?,
    })
}

fn validate_fleet_create_args(args: &[String]) -> Result<()> {
    const ALLOWED: &[&str] = &[
        "--spec",
        "--node",
        "--op",
        "--intent-hash",
        "--min-online-relays",
        "--holder",
        "--target-image",
    ];
    let mut seen = std::collections::BTreeSet::new();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        if !ALLOWED.contains(&name) {
            return Err(OuroError::InvalidArgs(format!(
                "unexpected fleet permit argument {name:?}"
            )));
        }
        if !seen.insert(name) {
            return Err(OuroError::InvalidArgs(format!(
                "duplicate fleet permit argument {name}"
            )));
        }
        let value = args.get(index + 1).ok_or_else(|| {
            OuroError::InvalidArgs(format!("missing value for fleet permit argument {name}"))
        })?;
        if value.starts_with("--") {
            return Err(OuroError::InvalidArgs(format!(
                "missing value for fleet permit argument {name}"
            )));
        }
        index += 2;
    }
    Ok(())
}

fn pool_spec_identity(spec: &PoolSpec) -> Result<(String, String)> {
    let bytes = serde_json::to_vec(spec)
        .map_err(|e| OuroError::Validation(format!("cannot canonicalize pool spec: {e}")))?;
    let digest = format!("sha256:{}", crate::intent::sha256_hex(&bytes));
    let bp = spec.machines.iter().find(|machine| machine.role == MachineRole::Bp)
        .ok_or_else(|| OuroError::Validation("pool spec has no block producer identity".into()))?;
    // Stable v1 pool namespace: network/genesis + immutable logical BP id. Full spec digest is a
    // separate revision binding; changing node_version/metadata/SSH endpoints must not create an
    // independent lease namespace that could bypass single-writer quorum arbitration.
    let stable = format!(
        "{}\n{}\n{}",
        spec.pool.network.as_str(), spec.pool.genesis_hashes.shelley, bp.id
    );
    let stable_hash = crate::intent::sha256_hex(stable.as_bytes());
    let pool_id = format!("pool-{}", &stable_hash[..24]);
    Ok((digest, pool_id))
}

/// Mint one short-lived, signed disruptive-step permit under the pool-wide authority. Controllers
/// MUST share the same durable OURO_HOME authority; its kernel lock serializes acquisitions. Fleet
/// availability/order facts are fetched from every declared target, never accepted from the agent.
pub fn run_fleet(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) == Some("spec")
        && args.get(1).map(String::as_str) == Some("identity")
    {
        if args.len() != 4 || args.get(2).map(String::as_str) != Some("--spec") {
            return Err(OuroError::InvalidArgs(
                "expected: ouro-ops fleet spec identity --spec <pool-spec>".into(),
            ));
        }
        let spec = PoolSpec::from_file(std::path::Path::new(&args[3]))?;
        let (pool_spec_digest, pool_id) = pool_spec_identity(&spec)?;
        output::print_json(&ToolOutput::ok("ouro.fleet.spec.identity", false).with_data(json!({
            "pool_spec_digest": pool_spec_digest,
            "pool_id": pool_id,
            "network": spec.pool.network.as_str(),
            "genesis_hash": spec.pool.genesis_hashes.shelley,
            "machines": spec.machines.iter().map(|machine| &machine.id).collect::<Vec<_>>(),
            "min_online_relays": spec.upgrade.min_online_relays,
        })))?;
        return Ok(());
    }
    if args.first().map(String::as_str) != Some("permit")
        || args.get(1).map(String::as_str) != Some("create")
    {
        return Err(OuroError::InvalidArgs(
            "expected: ouro-ops fleet permit create --spec <pool-spec> --node <id> --op <id> \
             --intent-hash <final-plan-hash> --holder <id> \
             [--target-image sha256:<digest>]"
                .into(),
        ));
    }
    let args = &args[2..];
    for forbidden in ["--role", "--online-relays", "--relays-remaining"] {
        if args.iter().any(|arg| arg == forbidden) {
            return Err(OuroError::Validation(format!(
                "{forbidden} is not accepted: role/quorum/order are derived from target-validated \
                 live facts and the pool spec"
            )));
        }
    }
    validate_fleet_create_args(args)?;
    let spec = PoolSpec::from_file(std::path::Path::new(flag(args, "--spec")?))?;
    let (pool_spec_digest, pool_id) = pool_spec_identity(&spec)?;
    let node = flag(args, "--node")?;
    let operation = flag(args, "--op")?;
    let intent_hash = flag(args, "--intent-hash")?;
    let holder = flag(args, "--holder")?;
    crate::intent::validate_machine_id(node)?;
    crate::intent::validate_machine_id(holder)?;
    if intent_hash.len() != 64 || !intent_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(OuroError::Validation(
            "--intent-hash must be the 64-hex final target plan hash".into(),
        ));
    }
    parity::require_registered_write(operation)?;
    let fleet_operation = crate::intent::lookup(operation).ok_or_else(|| {
        OuroError::Validation(format!("operation {operation:?} is not registered"))
    })?;
    if !fleet_operation.touched.iter().any(|resource| {
        matches!(*resource, "container:restart" | "container:recreate")
    }) {
        return Err(OuroError::Validation(format!(
            "{operation} is not disruptive and cannot consume a fleet lease/permit"
        )));
    }
    let min_online_relays = spec.upgrade.min_online_relays;
    if let Some(supplied) = optional(args, "--min-online-relays") {
        let supplied = supplied.parse::<u32>().map_err(|_| {
            OuroError::Validation("--min-online-relays must be an unsigned integer".into())
        })?;
        if supplied != min_online_relays {
            return Err(OuroError::Validation(format!(
                "--min-online-relays={supplied} conflicts with the pool-spec policy \
                 upgrade.min_online_relays={min_online_relays}; caller policy cannot override the spec"
            )));
        }
    }
    let target = spec.machines.iter().find(|machine| machine.id == node).ok_or_else(|| {
        OuroError::Validation(format!("fleet target {node} is not declared in the pool spec"))
    })?;
    let role = match target.role { MachineRole::Bp => "bp", MachineRole::Relay => "relay" };
    let target_image = optional(args, "--target-image");
    if operation == "upgrade/step" && target_image.is_none() {
        return Err(OuroError::Validation(
            "upgrade/step fleet permit requires --target-image sha256:<digest>".into(),
        ));
    }
    if operation != "upgrade/step" && target_image.is_some() {
        return Err(OuroError::Validation(
            "--target-image is accepted only for upgrade/step".into(),
        ));
    }
    if let Some(image) = target_image {
        let valid = image.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64 && hex.chars().all(|ch| ch.is_ascii_hexdigit())
        });
        if !valid {
            return Err(OuroError::Validation(
                "--target-image must be sha256:<64hex>".into(),
            ));
        }
    }

    // Complete the read-only live snapshot BEFORE acquiring a lease or creating a signing secret.
    // Thus any unmanaged/unreachable/drifted target refuses without minting a permit or authority
    // state. The signed permit carries the resulting counts.
    let paths = ConfigPaths::discover();
    let allowlist_digest = convention::Allowlist::active_verified()?.signed_digest()?;
    let facts_epoch = crate::s0019_confirmation::current_epoch()?;
    let mut statuses = Vec::with_capacity(spec.machines.len());
    for machine in &spec.machines {
        statuses.push(fetch_fleet_status(machine, &paths, &allowlist_digest)?);
    }
    let expected_network = spec.pool.network.as_str();
    let expected_genesis = spec.pool.genesis_hashes.shelley.as_str();
    for status in &statuses {
        if status.network != expected_network || status.genesis_hash != expected_genesis {
            return Err(OuroError::Validation(format!(
                "fleet live-facts pool mismatch for {}: target reported network={:?} genesis={:?}, \
                 spec requires network={expected_network:?} genesis={expected_genesis:?}",
                status.node, status.network, status.genesis_hash
            )));
        }
    }
    let target_status = statuses.iter().find(|status| status.node == node).ok_or_else(|| {
        OuroError::Validation(format!("fleet live-facts snapshot omitted target {node}"))
    })?;
    if !target_status.online {
        return Err(OuroError::Validation(format!(
            "fleet target {node} is not ready/online — permit refused"
        )));
    }
    let online_relays = u32::try_from(
        statuses.iter().filter(|status| status.role == "relay" && status.online).count(),
    ).map_err(|_| OuroError::Validation("relay count exceeds supported range".into()))?;
    let relays_remaining = if let Some(image) = target_image {
        u32::try_from(statuses.iter().filter(|status| {
            status.role == "relay" && status.image_config_digest != image
        }).count()).map_err(|_| OuroError::Validation("relay count exceeds supported range".into()))?
    } else {
        0
    };
    crate::fleet::require_quorum(online_relays, min_online_relays, role == "relay")?;
    crate::fleet::require_bp_last(role == "bp", relays_remaining)?;
    let ttl_seconds = 30;
    let now = crate::s0019_confirmation::current_epoch()?;
    if now.saturating_sub(facts_epoch) > 30 {
        return Err(OuroError::Validation(
            "fleet live-facts collection exceeded the 30-second authorization window — retry".into(),
        ));
    }
    let authority = crate::fleet::PoolAuthority::at(&paths.home.join("fleet-authority"), &pool_id);
    let lease = authority.acquire(&pool_id, holder, now, ttl_seconds)?;
    let secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let permit = crate::fleet::StepPermit {
        pool_id: pool_id.clone(),
        pool_spec_digest: pool_spec_digest.clone(),
        network: expected_network.into(),
        genesis_hash: expected_genesis.into(),
        target_host_key_sha256: target_status.host_key_sha256.clone(),
        node_id: node.into(),
        operation_id: operation.into(),
        intent_hash: intent_hash.into(),
        role: role.into(),
        target_image: target_image.map(str::to_string),
        fencing_token: lease.fencing_token,
        expiry_epoch: lease.expiry_epoch,
        facts_epoch,
        online_relays,
        min_online_relays,
        relays_remaining,
        relay_health_endpoints: spec.machines.iter().filter_map(|machine| {
            let ready = statuses.iter().any(|status| {
                status.node == machine.id && status.role == "relay" && status.online
            });
            if machine.role != MachineRole::Relay || !ready {
                return None;
            }
            machine.public_endpoint.as_ref().map(|endpoint| crate::fleet::RelayHealthEndpoint {
                node_id: machine.id.clone(),
                host: endpoint.host.clone(),
                port: endpoint.port,
            })
        }).collect(),
        permit_id: uuid::Uuid::new_v4().simple().to_string(),
        signature: String::new(),
    }
    .sign(secret.as_bytes())?;
    let encoded = serde_json::to_string(&permit)
        .map_err(|e| OuroError::Validation(format!("fleet permit serialize: {e}")))?;
    output::print_json(&ToolOutput::ok("ouro.fleet.permit.create", true).with_data(json!({
        "fleet_permit": encoded,
        "pool_id": pool_id,
        "pool_spec_digest": pool_spec_digest,
        "node": node,
        "operation": operation,
        "fencing_token": permit.fencing_token,
        "expires_at_epoch": permit.expiry_epoch,
        "facts": {
            "source": "target-validated-live-snapshot",
            "collected_from_epoch": facts_epoch,
            "valid_until_epoch": facts_epoch.saturating_add(30),
            "online_relays": online_relays,
            "min_online_relays": min_online_relays,
            "relays_remaining": relays_remaining,
            "target_role": role,
            "target_online": target_status.online,
            "target_state_generation": target_status.state_generation,
            "target_host_key_sha256": target_status.host_key_sha256,
            "target_image": target_status.image_config_digest,
            "authorized_upgrade_image": target_image,
        },
    })))?;
    Ok(())
}

/// Append one closed-field, durable audit event (§2.13). Hashes/ids only, never raw
/// config/secret data. Audit failure is an operation failure; it is never silently swallowed.
fn audit_emit(
    paths: &ConfigPaths,
    event: &str,
    node: &str,
    extra: serde_json::Value,
) -> Result<()> {
    const EVENTS: &[&str] = &[
        "adopt", "live_preflight", "intent_approval", "prepared", "committing",
        "committed", "verifying", "verified", "rolling_back", "rolled_back", "sealed",
        "recovery", "attestation_rotation", "refusal",
    ];
    const EXTRA_FIELDS: &[&str] = &[
        "operation_id", "intent_hash", "approval_evidence_hash", "pre_state_generation",
        "post_state_generation", "fencing_token", "outcome", "refusal_code",
    ];
    if !EVENTS.contains(&event) {
        return Err(OuroError::Validation(format!("unknown audit event {event:?}")));
    }
    let mut ev = serde_json::Map::new();
    ev.insert("event".into(), json!(event));
    let audit_id = extra
        .get("intent_hash")
        .and_then(serde_json::Value::as_str)
        .map(|hash| format!("op-{hash}"))
        .unwrap_or_else(|| format!("{event}-{}", uuid::Uuid::new_v4().simple()));
    ev.insert("audit_id".into(), json!(audit_id));
    ev.insert("node_id".into(), json!(node));
    ev.insert("at_epoch".into(), json!(crate::s0019_confirmation::current_epoch()?));
    if let serde_json::Value::Object(m) = extra {
        for (k, v) in m {
            if !EXTRA_FIELDS.contains(&k.as_str()) {
                return Err(OuroError::Validation(format!(
                    "audit field {k:?} is outside the closed schema"
                )));
            }
            ev.insert(k, v);
        }
    }
    let mut line = serde_json::to_vec(&serde_json::Value::Object(ev))
        .map_err(|error| OuroError::Validation(format!("audit serialize: {error}")))?;
    line.push(b'\n');
    let path = paths.home.join("s0019-audit.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| OuroError::Validation(format!("audit mkdir: {error}")))?;
    }
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(OuroError::Validation(
                    "audit path is not a regular file — refused".into(),
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                if metadata.nlink() != 1 || metadata.permissions().mode() & 0o022 != 0 {
                    return Err(OuroError::Validation(
                        "audit file has unsafe links or permissions — refused".into(),
                    ));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(OuroError::Validation(format!(
                "cannot inspect audit path: {error}"
            )))
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options
        .open(&path)
        .map_err(|error| OuroError::Validation(format!("audit open: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| OuroError::Validation(format!("audit metadata: {error}")))?;
    if !metadata.file_type().is_file() {
        return Err(OuroError::Validation("audit destination is not a regular file".into()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.nlink() != 1 || metadata.permissions().mode() & 0o022 != 0 {
            return Err(OuroError::Validation(
                "audit destination has unsafe links or permissions — refused".into(),
            ));
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    use std::io::Write;
    file.write_all(&line)
        .map_err(|error| OuroError::Validation(format!("audit append: {error}")))?;
    file.sync_data()
        .map_err(|error| OuroError::Validation(format!("audit fsync: {error}")))?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| OuroError::Validation(format!("audit directory fsync: {error}")))?;
    }
    Ok(())
}

fn tx_audit_event(state: TxState) -> &'static str {
    match state {
        TxState::Prepared => "prepared",
        TxState::Committing => "committing",
        TxState::Committed => "committed",
        TxState::Verifying => "verifying",
        TxState::Verified => "verified",
        TxState::RollingBack => "rolling_back",
        TxState::RolledBack => "rolled_back",
        TxState::Sealed => "sealed",
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

fn adoption_pending_path(paths: &ConfigPaths, node: &str) -> PathBuf {
    tx_dir(paths).join("adoption-pending").join(format!("{node}.pending"))
}

fn begin_adoption_commit(paths: &ConfigPaths, node: &str, candidate: &str) -> Result<PathBuf> {
    let path = adoption_pending_path(paths, node);
    let parent = path.parent().ok_or_else(|| OuroError::Validation("invalid adoption journal path".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options.open(&path).map_err(|error| {
        OuroError::Validation(format!(
            "adoption reconciliation is already pending for {node}: {error}"
        ))
    })?;
    file.write_all(candidate.as_bytes())?;
    file.sync_all()?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(path)
}

fn clear_adoption_commit(path: &Path) -> Result<()> {
    std::fs::remove_file(path)?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
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
    /// The legacy readiness metric prefers slot over block. The explicit fields below preserve the
    /// actual cardano-cli tip vocabulary for stateless observability without changing S0019 write
    /// verification semantics or invalidating old fixtures.
    tip_block: i64,
    tip_block_next: i64,
    #[serde(default)]
    tip_block_height: Option<i64>,
    #[serde(default)]
    tip_slot: Option<i64>,
    #[serde(default)]
    tip_era: Option<String>,
    #[serde(default)]
    sync_progress: Option<String>,
    tip_synced: bool,
    kes_opcert_valid: bool,
    forging_credentials_ready: bool,
    established_peers: u32,
}

/// The only S0020 target-side read entry point. It is deliberately closed and stateless: the
/// ephemeral runner gathers one live observation and does not consult attestation, Ouro home,
/// transaction journals, allowlist floors or an installed target version.
pub fn run_target(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("observe") => {
            let args = &args[1..];
            validate_closed_args(args, &["--node"], &[], &[])?;
            let node = flag(args, "--node")?;
            crate::intent::validate_machine_id(node)?;
            let observation = read_observation(&[])?;
            output::print_json(&stateless_observation_output(node, &observation))
        }
        Some("plan") => run_stateless_target_plan(&args[1..]),
        _ => Err(OuroError::InvalidArgs(
            "expected internal target observe|plan with closed arguments".into(),
        )),
    }
}

fn run_stateless_target_plan(args: &[String]) -> Result<()> {
    validate_closed_args(
        args,
        &[
            "--op",
            "--node",
            "--role",
            "--network",
            "--genesis",
            "--pool-id",
            "--pool-spec-digest",
            "--min-online-relays",
            "--param",
        ],
        &[],
        &["--param"],
    )?;
    let op = flag(args, "--op")?;
    if op == "deploy/register-submit" {
        return Err(OuroError::Validation(
            "deploy is outside the S0020 ephemeral-runner migration scope".into(),
        ));
    }
    let node = flag(args, "--node")?;
    crate::intent::validate_machine_id(node)?;
    let role = match flag(args, "--role")? {
        "bp" => Role::Bp,
        "relay" => Role::Relay,
        value => {
            return Err(OuroError::Validation(format!(
                "target plan role must be bp|relay, got {value:?}"
            )))
        }
    };
    let network = flag(args, "--network")?;
    if !matches!(network, "mainnet" | "preprod" | "preview") {
        return Err(OuroError::Validation(
            "target plan network must be mainnet|preprod|preview".into(),
        ));
    }
    let genesis = flag(args, "--genesis")?;
    validate_digest_selector("--genesis", &format!("sha256:{genesis}"))?;
    let pool_id = flag(args, "--pool-id")?;
    crate::intent::validate_machine_id(pool_id)?;
    let pool_spec_digest = flag(args, "--pool-spec-digest")?;
    validate_digest_selector("--pool-spec-digest", pool_spec_digest)?;
    let min_online_relays = flag(args, "--min-online-relays")?
        .parse::<u32>()
        .map_err(|_| {
            OuroError::Validation("--min-online-relays must be an unsigned integer".into())
        })?;

    let payload = collect_params(args)?;
    let registered = crate::intent::lookup(op).ok_or_else(|| {
        OuroError::Validation(format!("operation {op:?} is not in the typed registry"))
    })?;
    if registered.mutability == Mutability::Read {
        return Err(OuroError::Validation(
            "read operations use target observe, not target plan".into(),
        ));
    }
    let fleet_sensitive = registered
        .touched
        .iter()
        .any(|resource| matches!(*resource, "container:restart" | "container:recreate"));
    if op == "kes-rotation/install-opcert" && role != Role::Bp {
        return Err(OuroError::Validation(
            "kes-rotation/install-opcert is BP-only".into(),
        ));
    }

    let observation = read_observation(&[])?;
    observation.supervisor.require_conformant()?;
    require_typed_mounts(&observation.live.mounts)?;
    let allowlist = convention::Allowlist::active_verified()?;
    let (contract, image) = allowlist.contract_and_image_for(
        &observation.live.image_config_digest,
        &observation.live.platform,
    )?;
    require_adoption_contract(contract, &observation, network, genesis, None, None)?;
    match role {
        Role::Relay if contract.role_rules.relay.forbids_forging_keys
            && observation.live.has_forging_keys =>
        {
            return Err(OuroError::Validation(
                "pool spec declares relay but live node bears forging keys".into(),
            ))
        }
        Role::Bp
            if contract.role_rules.bp.requires_opcert
                && observation.live.kes_opcert_id.is_empty() =>
        {
            return Err(OuroError::Validation(
                "pool spec declares BP but live node has no operational certificate".into(),
            ))
        }
        _ => {}
    }

    let payload_machine = payload
        .get("machine")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| OuroError::Validation("intent payload is missing machine".into()))?;
    if payload_machine != node {
        return Err(OuroError::Validation(format!(
            "intent payload machine {payload_machine:?} does not match target node {node:?}"
        )));
    }
    if let Some(target) = payload.get("image").and_then(serde_json::Value::as_str) {
        allowlist.contract_and_image_for(target, &observation.live.platform)?;
        match op {
            "upgrade/preload-image" => crate::executor::require_image_absent(target)?,
            "upgrade/step" => {
                let transition = allowlist
                    .transition_for(&observation.live.image_config_digest, target)?;
                crate::upgrade::validate_transition(
                    transition,
                    &allowlist,
                    &observation.live.platform,
                )?;
                crate::executor::require_image_present(target)?;
            }
            _ => {}
        }
    }

    let live_binding = json!({
        "supervisor": observation.supervisor,
        "live": observation.live,
        "recreate": observation.recreate,
    });
    let live_bytes = serde_json::to_vec(&live_binding)
        .map_err(|error| OuroError::Validation(format!("cannot bind live state: {error}")))?;
    let live_state_hash = crate::intent::sha256_hex(&live_bytes);
    let recreate_binding = observation.recreate.as_ref().map(|spec| {
        serde_json::to_vec(spec)
            .map(|bytes| crate::intent::sha256_hex(&bytes))
            .unwrap_or_default()
    });
    let expected_post_state = serde_json::to_string(&json!({
        "pool": {
            "pool_id": pool_id,
            "pool_spec_digest": pool_spec_digest,
            "role": match role { Role::Bp => "bp", Role::Relay => "relay" },
            "network": network,
            "genesis_hash": genesis,
            "min_online_relays": min_online_relays,
        },
        "operation": op,
        "recreate_binding": if op == "upgrade/step" { recreate_binding.as_deref() } else { None },
    }))
    .map_err(|error| OuroError::Validation(format!("cannot bind expected state: {error}")))?;
    let intent = Intent {
        schema_version: 1,
        operation_id: op.to_string(),
        node_id: node.to_string(),
        pre_state_generation: observation.live.container_creation_epoch,
        pre_state_hash: live_state_hash.clone(),
        expected_post_state,
        nonce: format!("ephemeral-{}", &live_state_hash[..24]),
        expiry_epoch: 0,
        payload,
    };
    let validated = intent.validate(0)?;
    let candidate_hash = intent.canonical_hash();
    let executor_plan = match op {
        "runtime/restart" => vec![vec![
            "docker".into(),
            "restart".into(),
            observation.live.container_id.clone(),
        ]],
        "kes-rotation/install-opcert" => {
            let reference = intent
                .payload
                .get("opcert")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("KES plan lost opcert reference".into()))?;
            vec![
                vec![
                    "docker".into(),
                    "cp".into(),
                    format!("<ephemeral-inbox:{reference}>"),
                    format!(
                        "{}:/opt/cardano/config/keys/node.cert",
                        observation.live.container_id
                    ),
                ],
                vec![
                    "docker".into(),
                    "restart".into(),
                    observation.live.container_id.clone(),
                ],
            ]
        }
        "upgrade/preload-image" => {
            let reference = intent
                .payload
                .get("artifact")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("preload plan lost artifact reference".into()))?;
            vec![vec![
                "docker".into(),
                "load".into(),
                "--input".into(),
                format!("<ephemeral-inbox:{reference}>"),
            ]]
        }
        "upgrade/step" => {
            let target = intent
                .payload
                .get("image")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("upgrade plan lost image".into()))?;
            let recreate = observation.recreate.as_ref().ok_or_else(|| {
                OuroError::Validation(
                    "upgrade plan unavailable: live container run-spec is not fully modeled".into(),
                )
            })?;
            crate::executor::recreate_approval_argv(
                recreate,
                &observation.live.container_id,
                target,
            )?
        }
        other => {
            return Err(OuroError::Validation(format!(
                "operation {other} has not migrated to stateless planning"
            )))
        }
    };

    let mut result = ToolOutput::ok("ouro.op.plan", false).with_data(json!({
        "op": op,
        "node": node,
        "assurance": "live_target_validated",
        "management_state": "not_required",
        "candidate_hash": candidate_hash,
        "intent_hash": candidate_hash,
        "intent_hash_final": true,
        "live_state_hash": live_state_hash,
        "live_container_creation_epoch": observation.live.container_creation_epoch,
        "mutability": format!("{:?}", validated.mutability),
        "touched": validated.touched,
        "executor_plan": executor_plan,
        "executor_plan_secret_values_redacted": op == "upgrade/step",
        "runtime_policy": {
            "allowlist_version": allowlist.allowlist_version,
            "contract_id": contract.contract_id,
            "convention_version": contract.convention_version,
            "running_image_config_digest": observation.live.image_config_digest,
            "oci_index_digest": image.oci_index_digest,
            "platform_manifest_digest": image.platform_manifest_digest,
        },
        "pool_binding": {
            "pool_id": pool_id,
            "pool_spec_digest": pool_spec_digest,
            "role": match role { Role::Bp => "bp", Role::Relay => "relay" },
            "network": network,
            "genesis_hash": genesis,
            "min_online_relays": min_online_relays,
        },
        "fleet_permit_required": fleet_sensitive,
        "confirmation_required": validated.mutability == Mutability::Dangerous,
        "apply_revalidation_required": true,
        "artifact_validation": if matches!(op, "kes-rotation/install-opcert" | "upgrade/preload-image") {
            "content digest is candidate-bound; public artifact shape/domain and live compatibility are revalidated before apply"
        } else {
            "not_applicable"
        },
        "persistent_target_state_written": false,
        "note": "final stateless candidate from current signed policy + live target facts; no mutation or durable Ouro ownership state",
    }));
    result.machine = Some(node.to_string());
    output::print_json(&result)
}

fn stateless_observation_output(node: &str, observation: &Observation) -> ToolOutput {
    let readiness = observation.readiness.as_ref();
    let runtime_policy = match convention::Allowlist::active_verified() {
        Ok(allowlist) => match allowlist.contract_and_image_for(
            &observation.live.image_config_digest,
            &observation.live.platform,
        ) {
            Ok((contract, image)) => json!({
                "supported": true,
                "contract_id": contract.contract_id,
                "convention_version": contract.convention_version,
                "oci_index_digest": image.oci_index_digest,
                "platform_manifest_digest": image.platform_manifest_digest,
            }),
            Err(error) => json!({
                "supported": false,
                "detail": error.to_string(),
                "effect": "informational_for_read; typed writes remain policy-gated",
            }),
        },
        Err(error) => json!({
            "supported": null,
            "detail": error.to_string(),
            "effect": "policy unavailable; live read evidence is still returned",
        }),
    };
    let mut result = ToolOutput::ok("ouro.observe", false).with_data(json!({
        "op": "observability/health",
        "node": node,
        "assurance": "live_observation",
        "management_state": "not_required",
        "result": {
            "node_running": readiness.map(|value| value.node_running).unwrap_or(false),
            "socket_answers": readiness.map(|value| value.socket_answers).unwrap_or(false),
            "tip_synced": readiness.map(|value| value.tip_synced).unwrap_or(false),
            "tip": {
                "block": readiness.and_then(|value| value.tip_block_height),
                "slot": readiness.and_then(|value| value.tip_slot),
                "era": readiness.and_then(|value| value.tip_era.as_deref()),
                "sync_progress": readiness.and_then(|value| value.sync_progress.as_deref()),
            },
            "network": observation.live.network,
            "container": {
                "running_count": observation.supervisor.node_container_count,
                "runtime": observation.supervisor.runtime,
                "id": observation.live.container_id,
                "name": observation.live.container_name,
                "image_reference": observation.live.image_reference,
                "image_config_digest": observation.live.image_config_digest,
                "platform": observation.live.platform,
            },
            "runtime_policy": runtime_policy,
        },
        "not_claimed": [
            "block production",
            "end-to-end peer reachability",
            "disk capacity or growth safety",
            "future availability",
        ],
    }));
    result.machine = Some(node.to_string());
    result
}
#[derive(serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
struct ObsLive {
    image_config_digest: String,
    platform: String,
    container_id: String,
    #[serde(default)]
    container_name: String,
    #[serde(default)]
    image_reference: String,
    container_creation_epoch: u64,
    entrypoint: Vec<String>,
    args: Vec<String>,
    #[serde(default)]
    image_entrypoint: Vec<String>,
    #[serde(default)]
    image_cmd: Vec<String>,
    mounts: Vec<TypedMount>,
    topology_hash: String,
    config_hash: String,
    kes_opcert_id: String,
    has_forging_keys: bool,
    #[serde(default)]
    forging_key_permissions_safe: bool,
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
            mounts: self.mounts.clone(),
            topology_hash: self.topology_hash.clone(),
            config_hash: self.config_hash.clone(),
            kes_opcert_id: self.kes_opcert_id.clone(),
            has_forging_keys: self.has_forging_keys,
        }
    }
}

fn require_typed_mounts(mounts: &[TypedMount]) -> Result<()> {
    let mut destinations = std::collections::HashSet::new();
    if mounts.is_empty() {
        return Err(OuroError::Validation(
            "probe did not provide typed bind-mount evidence — adoption refused".into(),
        ));
    }
    for mount in mounts {
        let source_ok = mount.source_id.split_once(':').map(|(device, inode)| {
            !device.is_empty()
                && !inode.is_empty()
                && device.bytes().all(|byte| byte.is_ascii_digit())
                && inode.bytes().all(|byte| byte.is_ascii_digit())
        }) == Some(true);
        let destination = std::path::Path::new(&mount.destination);
        let destination_ok = destination.is_absolute()
            && destination.components().all(|component| !matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            ));
        let owner_ok = mount.owner.split_once(':').map(|(uid, gid)| {
            !uid.is_empty() && !gid.is_empty()
                && uid.bytes().all(|byte| byte.is_ascii_digit())
                && gid.bytes().all(|byte| byte.is_ascii_digit())
        }) == Some(true);
        let mode_ok = (3..=4).contains(&mount.mode.len())
            && mount.mode.bytes().all(|byte| (b'0'..=b'7').contains(&byte));
        if mount.kind != "bind"
            || !mount.no_symlink
            || !source_ok
            || !destination_ok
            || !destinations.insert(mount.destination.as_str())
            || !owner_ok
            || !mode_ok
        {
            return Err(OuroError::Validation(
                "probe supplied an unsafe/ambiguous typed mount — adoption refused".into(),
            ));
        }
    }
    Ok(())
}

fn require_adoption_contract(
    contract: &crate::convention::LayoutContract,
    observation: &Observation,
    expected_network: &str,
    expected_genesis: &str,
    expected_container: Option<&str>,
    expected_image: Option<&str>,
) -> Result<()> {
    if expected_network.is_empty()
        || expected_genesis.is_empty()
        || observation.live.network != expected_network
        || observation.live.genesis_hash != expected_genesis
    {
        return Err(OuroError::Validation(format!(
            "adoption pool binding mismatch: observed network={:?} genesis={:?}, expected \
             network={expected_network:?} genesis={expected_genesis:?}",
            observation.live.network, observation.live.genesis_hash
        )));
    }
    let process_matches = match contract.contract_id.as_str() {
        // The signed/versioned Blink Labs layout uses the image entrypoint plus the explicit
        // `run` command. The production image intentionally has no Config.Cmd, so comparing live
        // Args to Config.Cmd would reject the standard deployment.
        "blinklabs-cardano-node-v1" => {
            observation.live.image_entrypoint == ["/usr/local/bin/entrypoint"]
                && observation.live.image_cmd.is_empty()
                && observation.live.entrypoint == observation.live.image_entrypoint
                && observation.live.args == ["run"]
        }
        _ => false,
    };
    if !process_matches {
        return Err(OuroError::Validation(
            "container process differs from the signed/versioned convention invocation — adoption refused".into(),
        ));
    }
    if expected_container.is_some_and(|name| observation.live.container_name != name)
        || expected_image.is_some_and(|image| observation.live.image_reference != image)
    {
        return Err(OuroError::Validation(format!(
            "adoption runtime declaration mismatch: observed container={:?} image={:?}, expected \
             container={expected_container:?} image={expected_image:?}",
            observation.live.container_name, observation.live.image_reference
        )));
    }
    let covered = |required: &str| {
        observation.live.mounts.iter().any(|mount| {
            let destination = mount.destination.trim_end_matches('/');
            required == destination
                || required.strip_prefix(destination).is_some_and(|tail| tail.starts_with('/'))
        })
    };
    for required in [
        contract.in_container_paths.socket.as_str(),
        contract.in_container_paths.db.as_str(),
        contract.in_container_paths.keys.as_str(),
        contract.in_container_paths.config.as_str(),
        contract.in_container_paths.topology.as_str(),
        contract.in_container_paths.genesis.as_str(),
    ] {
        if !covered(required) {
            return Err(OuroError::Validation(format!(
                "adoption layout is missing bind-mount coverage for required contract path {required}"
            )));
        }
    }
    Ok(())
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
        tip_synced: evidence.tip_synced,
        kes_opcert_valid: evidence.kes_opcert_valid,
        forging_credentials_ready: evidence.forging_credentials_ready,
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

struct ExtractedProbe {
    dir: PathBuf,
    path: PathBuf,
}

impl Drop for ExtractedProbe {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// Extract a root-executed embedded script without following an attacker-controlled path. The
/// unpredictable directory is created exclusively as 0700 and the script as create-new 0600 with
/// `O_NOFOLLOW`; every failure is propagated instead of silently falling back to stale bytes.
fn extract_embedded_probe(temp_root: &Path, bytes: &[u8]) -> Result<ExtractedProbe> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    for _ in 0..16 {
        let dir = temp_root.join(format!(
            "ouro-probe-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(OuroError::Validation(format!(
                    "cannot create private probe directory: {error}"
                )))
            }
        }
        let path = dir.join("ouro-probe.sh");
        let opened = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path);
        let mut file = match opened {
            Ok(file) => file,
            Err(error) => {
                let _ = std::fs::remove_dir(&dir);
                return Err(OuroError::Validation(format!(
                    "cannot create private probe script: {error}"
                )));
            }
        };
        if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir(&dir);
            return Err(OuroError::Validation(format!(
                "cannot persist private probe script: {error}"
            )));
        }
        drop(file);
        return Ok(ExtractedProbe { dir, path });
    }
    Err(OuroError::Validation(
        "cannot allocate a unique private probe directory".into(),
    ))
}

/// Run the embedded target-side probe and capture its observation JSON. The probe lib
/// (`lib/ouro-probe.sh`) is embedded; extract it safely, source it by positional argument, and run
/// `ouro_observe`. The RAII guard removes the private copy after bash exits.
fn run_probe() -> Result<String> {
    let extracted;
    let lib_path = match std::env::var_os("OURO_PROBE_LIB") {
        Some(p) => PathBuf::from(p),
        None => {
            let bytes = crate::skills::asset("lib/ouro-probe.sh").ok_or_else(|| {
                OuroError::Validation("embedded probe lib/ouro-probe.sh missing".into())
            })?;
            extracted = extract_embedded_probe(&std::env::temp_dir(), bytes)?;
            extracted.path.clone()
        }
    };
    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg("source \"$1\"\nouro_observe")
        .arg("ouro-probe")
        .arg(&lib_path)
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

fn audit_refusal(args: &[String], operation: &str, error: &OuroError) -> Result<()> {
    let node = optional(args, "--node").unwrap_or("unknown");
    let paths = ConfigPaths::discover();
    audit_emit(&paths, "refusal", node, json!({
        "operation_id": operation,
        "outcome": "refused",
        "refusal_code": format!("exit_{}", error.exit_code()),
    }))
}

fn require_contract_shape_and_role(
    att: &AdoptionAttestation,
    contract: &crate::convention::LayoutContract,
    observation: &Observation,
) -> Result<()> {
    observation.supervisor.require_conformant()?;
    require_typed_mounts(&observation.live.mounts)?;
    require_adoption_contract(
        contract,
        observation,
        &att.immutable.network,
        &att.immutable.genesis_hash,
        None,
        None,
    )?;
    let role_rule = match att.immutable.role {
        Role::Bp => contract.role_rules.bp,
        Role::Relay => contract.role_rules.relay,
    };
    if att.immutable.role == Role::Bp && !observation.live.forging_key_permissions_safe {
        return Err(OuroError::Validation(
            "BP forging key directory/KES/VRF permissions are not owner-only; the unprivileged diagnostic boundary is not proven"
                .into(),
        ));
    }
    att.check_role(&role_rule, &observation.live.to_live())
}

fn require_current_contract_observation(
    att: &AdoptionAttestation,
    contract: &crate::convention::LayoutContract,
    observation: &Observation,
) -> Result<()> {
    require_contract_shape_and_role(att, contract, observation)?;
    att.require_matches_live(&observation.live.to_live())
}

fn operation_secret(paths: &ConfigPaths, local: bool) -> Result<String> {
    let shared = std::path::Path::new(crate::onboard::CONFIRM_SECRET_PATH);
    if local && shared.exists() {
        std::fs::read_to_string(shared)
            .map_err(|e| OuroError::Validation(format!("cannot read shared operation secret: {e}")))
    } else {
        crate::confirm::load_or_create_secret(&paths.tool_run_secret)
    }
}

/// Opaque, target-secret-keyed binding to the exact recreate spec, including environment values.
/// Unlike a plain digest this does not publish an offline oracle for low-entropy container secrets.
fn recreate_spec_binding(
    paths: &ConfigPaths,
    local: bool,
    spec: &crate::executor::RecreateSpec,
) -> Result<String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let secret = operation_secret(paths, local)?;
    let bytes = serde_json::to_vec(spec)
        .map_err(|e| OuroError::Validation(format!("cannot bind upgrade recreate spec: {e}")))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.trim().as_bytes())
        .map_err(|_| OuroError::Validation("invalid operation secret".into()))?;
    mac.update(&bytes);
    Ok(format!(
        "hmac-sha256:{}",
        mac.finalize().into_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
    ))
}

fn validate_digest_selector(name: &str, value: &str) -> Result<()> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    if valid {
        Ok(())
    } else {
        Err(OuroError::Validation(format!("{name} must be sha256:<64hex>")))
    }
}

fn valid_ssh_sha256_fingerprint(value: &str) -> bool {
    value.strip_prefix("SHA256:").is_some_and(|encoded| {
        encoded.len() == 43
            && encoded.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/'
            })
    })
}

/// `ouro-ops adopt` — conformance → evidence-bound approval → write the attestation. Non-disruptive
/// (writes metadata only). Refuses a non-conforming node (never adapts).
pub fn run_adopt(args: &[String]) -> Result<()> {
    match run_adopt_inner(args) {
        Ok(()) => Ok(()),
        Err(error) => {
            audit_refusal(args, "adopt", &error).map_err(|audit_error| {
                OuroError::Validation(format!(
                    "{error}; additionally failed to append refusal audit: {audit_error}"
                ))
            })?;
            Err(error)
        }
    }
}

fn run_adopt_inner(args: &[String]) -> Result<()> {
    validate_closed_args(
        args,
        &[
            "--node", "--role", "--observation", "--dispatch", "--bootstrap-user", "--ssh-key",
            "--spec", "--approve-token", "--expect-embedded", "--expect-allowlist",
            "--expected-role", "--expected-network", "--expected-genesis",
            "--expected-container", "--expected-image", "--expected-host-key",
        ],
        &["--local", "--preview", "--plan", "--identity-only"],
        &[],
    )?;
    if args.iter().any(|arg| arg == "--identity-only") {
        let allowed = ["--local", "--identity-only", "--expect-embedded"];
        if args.len() != 4 || args.iter().any(|arg| arg.starts_with("--") && !allowed.contains(&arg.as_str())) {
            return Err(OuroError::InvalidArgs(
                "--identity-only is target-internal and accepts only --local --expect-embedded <digest>"
                    .into(),
            ));
        }
        let expected = flag(args, "--expect-embedded")?;
        parity::require_expected_wire_digest(expected)?;
        output::print_json(&ToolOutput::ok("ouro.adopt.identity", false).with_data(json!({
            "security_identity": parity::SecurityIdentity::local().wire_digest(),
        })))?;
        return Ok(());
    }
    let node = flag(args, "--node")?.to_string();
    crate::intent::validate_machine_id(&node)?;
    let local = args.iter().any(|a| a == "--local");
    if optional(args, "--observation").is_some() && (local || !cfg!(debug_assertions)) {
        return Err(OuroError::Validation(
            "--observation is a debug control-host fixture only; target-local/release adoption \
             must use the embedded live probe"
                .into(),
        ));
    }

    // p6-3 — SSH DISPATCH: `adopt --dispatch <host>` runs `ouro-ops adopt --local` on the target as
    // the operator's bootstrap account (adoption is a privileged onboarding-class action).
    if let Some(host) = optional(args, "--dispatch") {
        for internal in [
            "--local", "--observation", "--expect-embedded", "--expect-allowlist",
            "--expected-role", "--expected-network", "--expected-genesis",
            "--expected-container", "--expected-image", "--expected-host-key",
            "--identity-only",
        ] {
            if args.iter().any(|arg| arg == internal) {
                return Err(OuroError::Validation(format!(
                    "{internal} is target-internal and cannot be supplied to control dispatch"
                )));
            }
        }
        let paths = ConfigPaths::discover();
        let node = flag(args, "--node")?.to_string();
        let plan = args.iter().any(|a| a == "--plan");
        return dispatch_adopt(host, &node, args, &paths, plan);
    }

    let expected_embedded = flag(args, "--expect-embedded")?;
    parity::require_expected_wire_digest(expected_embedded)?;

    let role = match flag(args, "--role")? {
        "bp" => Role::Bp,
        "relay" => Role::Relay,
        other => return Err(OuroError::Validation(format!("--role must be bp|relay, got {other}"))),
    };
    let expected_role = flag(args, "--expected-role")?;
    if expected_role != flag(args, "--role")? {
        return Err(OuroError::Validation(
            "adoption role differs from the control-validated pool spec".into(),
        ));
    }
    let expected_network = flag(args, "--expected-network")?;
    let expected_genesis = flag(args, "--expected-genesis")?;
    let expected_host_key = flag(args, "--expected-host-key")?;
    if !valid_ssh_sha256_fingerprint(expected_host_key) {
        return Err(OuroError::Validation(
            "control supplied an invalid OpenSSH SHA256 host-key fingerprint".into(),
        ));
    }
    let expected_container = optional(args, "--expected-container");
    let expected_image = optional(args, "--expected-image");
    let paths = ConfigPaths::discover();
    let attestation_path = attestation_path_for(&paths, &node, local);
    let preview = args.iter().any(|argument| argument == "--preview");
    let _adoption_lock = if preview {
        None
    } else {
        Some(crate::gate::NodeLock::acquire(
            &tx_dir(&paths).join("locks"),
            &node,
            "adoption",
        )?)
    };
    let obs = read_observation(args)?;

    // 1. supervisor shape must conform to the v1 contract (§2.2).
    obs.supervisor.require_conformant()?;
    require_typed_mounts(&obs.live.mounts)?;
    if !valid_ssh_sha256_fingerprint(&obs.live.host_key_sha256) {
        return Err(OuroError::Validation(
            "probe did not provide an OpenSSH SHA256 target host-key fingerprint".into(),
        ));
    }
    if obs.live.host_key_sha256 != expected_host_key {
        return Err(OuroError::Validation(
            "target-reported Ed25519 host key differs from the control's pinned known_hosts key"
                .into(),
        ));
    }

    // 2. image digest must be on the signed allowlist (§2.1); resolve the layout contract.
    let allow = if preview {
        convention::Allowlist::active_verified()?
    } else {
        convention::Allowlist::load(&paths.home, !attestation_path.exists())?
    };
    if let Some(expected) = optional(args, "--expect-allowlist") {
        let actual = allow.signed_digest()?;
        if expected != actual {
            return Err(OuroError::Validation(format!(
                "control→target allowlist mismatch: expected {expected}, target has {actual}"
            )));
        }
    }
    let (contract, allowed_image) =
        allow.contract_and_image_for(&obs.live.image_config_digest, &obs.live.platform)?;
    let allowlist_digest = allow.signed_digest()?;
    require_adoption_contract(
        contract, &obs, expected_network, expected_genesis, expected_container, expected_image,
    )?;

    // 3. build the immutable identity + initial managed state.
    let role_rule = match role {
        Role::Bp => contract.role_rules.bp,
        Role::Relay => contract.role_rules.relay,
    };
    let immutable = ImmutableIdentity {
        role,
        contract_id: contract.contract_id.clone(),
        convention_version: contract.convention_version,
        allowlist_version: allow.allowlist_version,
        allowlist_digest,
        host_key_sha256: obs.live.host_key_sha256.clone(),
        machine_id: node.clone(),
        oci_index_digest: allowed_image.oci_index_digest.clone(),
        platform_manifest_digest: allowed_image.platform_manifest_digest.clone(),
        image_config_digest: obs.live.image_config_digest.clone(),
        platform: obs.live.platform.clone(),
        container_creation_epoch: obs.live.container_creation_epoch,
        entrypoint: obs.live.entrypoint.clone(),
        args: obs.live.args.clone(),
        mounts: obs.live.mounts.clone(),
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
    if role == Role::Bp && !obs.live.forging_key_permissions_safe {
        return Err(OuroError::Validation(
            "BP forging key directory/KES/VRF permissions are not owner-only; adoption refused"
                .into(),
        ));
    }
    require_readiness(&att, &obs, false)?;

    // 5. evidence-bound approval (§2.14): bind the operator token to the candidate + host key.
    let candidate =
        attestation::candidate_hash(&serde_json::to_value(&att.immutable).unwrap_or(json!({})));
    let approval_diff = format!("adopt {node} host {}", obs.live.host_key_sha256);
    if preview {
        output::print_json(&ToolOutput::ok("ouro.adopt.preview", false).with_data(json!({
            "node": node,
            "role": role,
            "candidate_hash": candidate,
            "host_key_sha256": obs.live.host_key_sha256,
            "allowlist_version": allow.allowlist_version,
            "allowlist_digest": allow.signed_digest()?,
            "security_identity": parity::SecurityIdentity::local().wire_digest(),
            "network": obs.live.network,
            "genesis_hash": obs.live.genesis_hash,
            "process": {
                "entrypoint": obs.live.entrypoint,
                "args": obs.live.args,
                "image_entrypoint": obs.live.image_entrypoint,
                "image_cmd": obs.live.image_cmd,
                "container_name": obs.live.container_name,
                "image_reference": obs.live.image_reference,
            },
            "mounts": obs.live.mounts.iter().map(|mount| json!({
                "kind": mount.kind,
                "source_id": mount.source_id,
                "destination": mount.destination,
                "read_only": mount.read_only,
                "owner": mount.owner,
                "mode": mount.mode,
                "no_symlink": mount.no_symlink,
            })).collect::<Vec<_>>(),
            "forging_key_permissions_safe": obs.live.forging_key_permissions_safe,
            "readiness": {
                "node_running": obs.readiness.as_ref().map(|r| r.node_running),
                "socket_answers": obs.readiness.as_ref().map(|r| r.socket_answers),
                "tip_synced": obs.readiness.as_ref().map(|r| r.tip_synced),
                "established_peers": obs.readiness.as_ref().map(|r| r.established_peers),
                "kes_opcert_valid": obs.readiness.as_ref().map(|r| r.kes_opcert_valid),
                "forging_credentials_ready": obs.readiness.as_ref().map(|r| r.forging_credentials_ready),
            },
            "diff": approval_diff,
            "non_disruptive": true,
            "next": "mint `ouro-ops confirm adopt create` for this candidate, then rerun adopt with --approve-token",
        })))?;
        return Ok(());
    }
    let approve_token = flag(args, "--approve-token")?;
    let shared = std::path::Path::new(crate::onboard::CONFIRM_SECRET_PATH);
    let secret = if local && shared.exists() {
        std::fs::read_to_string(shared)
            .map_err(|e| OuroError::Validation(format!("cannot read shared adoption secret: {e}")))?
    } else {
        crate::confirm::load_or_create_secret(&paths.tool_run_secret)?
    };
    let verified = crate::s0019_confirmation::verify(
        approve_token,
        &candidate,
        &approval_diff,
        secret.trim().as_bytes(),
        crate::s0019_confirmation::current_epoch()?,
    )?;
    // Final target-side comparison under the adoption lock closes preview→write drift.
    let final_observation = read_observation(args)?;
    if final_observation.supervisor != obs.supervisor || final_observation.live != obs.live {
        return Err(OuroError::Validation(
            "adoption candidate changed after approval preview — refused; preview again".into(),
        ));
    }
    require_readiness(&att, &final_observation, false)?;
    // Durable fail-closed marker spans token consumption, attestation write, and audit fsync. A
    // crash or audit failure leaves it in place; every op refuses until explicit reconciliation,
    // so an unaudited adoption can never silently become operable.
    let adoption_pending = begin_adoption_commit(&paths, &node, &candidate)?;
    crate::s0019_confirmation::consume(
        &tx_dir(&paths).join("adopt-confirm-used").join(format!("{node}.log")),
        &verified,
    )?;
    let evidence = attestation::bind_approval(&candidate, approve_token, &obs.live.host_key_sha256);
    att.immutable.approval_evidence_hash = evidence;

    // 6. write the attestation (non-disruptive metadata write) + mirror the resolved contract for
    // the shell layout accessors (p1-5).
    let p = attestation_path;
    let mut doc = serde_json::to_value(&att).unwrap();
    doc["contract"] = json!({ "in_container_paths": contract.in_container_paths });
    attestation::write_document(&p, &doc)?;
    audit_emit(&paths, "adopt", &node, json!({
        "approval_evidence_hash": att.immutable.approval_evidence_hash,
        "post_state_generation": 0,
        "outcome": "adopted",
    }))?;
    clear_adoption_commit(&adoption_pending)?;

    output::print_json(&ToolOutput::ok("ouro.adopt", true).with_data(json!({
        "node": node,
        "role": att.immutable.role,
        "contract_id": contract.contract_id,
        "attestation": p.display().to_string(),
        "non_disruptive": true,
        "state_generation": 0,
        "security_identity": parity::SecurityIdentity::local().wire_digest(),
    })))?;
    Ok(())
}

/// `ouro-ops op run --op <id> --node <id> [--param k=v]... [--confirm-token T] --observation <f> [--plan]`
/// The intent pipeline: recover → parity → build+validate intent → live re-attest gate → confirm
/// gate → crash-durable transaction → sealed executor (plan mode until p4-2).
pub fn run_op(args: &[String]) -> Result<()> {
    let operation = optional(args, "--op").unwrap_or("unknown").to_string();
    match run_op_inner(args) {
        Ok(()) => Ok(()),
        Err(error) => {
            audit_refusal(args, &operation, &error).map_err(|audit_error| {
                OuroError::Validation(format!(
                    "{error}; additionally failed to append refusal audit: {audit_error}"
                ))
            })?;
            Err(error)
        }
    }
}

fn run_op_inner(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("run") {
        return Err(OuroError::InvalidArgs(
            "expected: ouro-ops op run --op <id> --node <id> [--param k=v]... [--confirm-token T] --observation <f> [--plan]".into(),
        ));
    }
    let args = &args[1..];
    validate_closed_args(
        args,
        &[
            "--op", "--node", "--param", "--confirm-token", "--dispatch", "--ssh-key",
            "--spec", "--observation", "--expect-embedded", "--expect-allowlist", "--fleet-pool-id",
            "--fleet-spec-digest", "--fleet-min-online-relays", "--fleet-permit",
        ],
        &["--plan", "--transport-plan", "--local"],
        &["--param"],
    )?;
    let op = flag(args, "--op")?.to_string();
    let node = flag(args, "--node")?.to_string();
    crate::intent::validate_machine_id(&node)?;
    let plan = args.iter().any(|a| a == "--plan");
    let transport_plan = args.iter().any(|a| a == "--transport-plan");
    if plan && transport_plan {
        return Err(OuroError::InvalidArgs(
            "--plan (target-validated) and --transport-plan (SSH argv only) are mutually exclusive"
                .into(),
        ));
    }
    if (plan || transport_plan) && optional(args, "--confirm-token").is_some() {
        return Err(OuroError::Validation(
            "do not pass a confirm-token to --plan/--transport-plan; review the target-validated \
             plan first, then mint approval for its final intent_hash"
                .into(),
        ));
    }
    if (plan || transport_plan) && optional(args, "--fleet-permit").is_some() {
        return Err(OuroError::Validation(
            "do not pass a fleet permit to --plan/--transport-plan; the final intent is planned \
             and approved first, then a 30-second live permit is minted last for immediate execution"
                .into(),
        ));
    }
    let paths = ConfigPaths::discover();

    // S0020 p1-2: reads do not depend on prior ownership metadata. A dispatched read streams the
    // control-selected runner; a local/debug read directly runs the same sealed live probe.
    if op == "observability/health" {
        if let Some(host) = optional(args, "--dispatch") {
            for internal in [
                "--local",
                "--observation",
                "--expect-embedded",
                "--expect-allowlist",
            ] {
                if args.iter().any(|arg| arg == internal) {
                    return Err(OuroError::Validation(format!(
                        "{internal} is target-internal and cannot be supplied to control dispatch"
                    )));
                }
            }
            return dispatch_op(host, &op, &node, args, &paths, transport_plan);
        }
        if transport_plan {
            return Err(OuroError::InvalidArgs(
                "--transport-plan requires --dispatch <host>".into(),
            ));
        }
        let observation = read_observation(args)?;
        return output::print_json(&stateless_observation_output(&node, &observation));
    }

    // S0020 p2-1: a target-validated write preview is built by the ephemeral runner from a fresh
    // probe. Control derives every pool binding from the operator spec; no target installation,
    // adoption attestation, parity record or prior Ouro transaction state participates.
    if plan {
        if let Some(host) = optional(args, "--dispatch") {
            for internal in [
                "--local",
                "--observation",
                "--expect-embedded",
                "--expect-allowlist",
            ] {
                if args.iter().any(|arg| arg == internal) {
                    return Err(OuroError::Validation(format!(
                        "{internal} is target-internal and cannot be supplied to control dispatch"
                    )));
                }
            }
            return dispatch_stateless_plan(host, &op, &node, args, &paths);
        }
    }

    if adoption_pending_path(&paths, &node).exists() {
        return Err(OuroError::Validation(format!(
            "adoption_reconciliation_required: node {node} has a pending adoption commit/audit marker; no operations are allowed until operator reconciliation"
        )));
    }

    // p5-1 — SSH DISPATCH: with `--dispatch <host>`, the op runs ON THE TARGET (as the confined
    // `ouro-op` principal through the fixed wrapper), not control-local. The remote runs the same
    // command with `--local`, reading the target-side attestation and executing there.
    if let Some(host) = optional(args, "--dispatch") {
        for internal in ["--local", "--observation", "--expect-embedded", "--expect-allowlist"] {
            if args.iter().any(|arg| arg == internal) {
                return Err(OuroError::Validation(format!(
                    "{internal} is target-internal and cannot be supplied to control dispatch"
                )));
            }
        }
        return dispatch_op(host, &op, &node, args, &paths, transport_plan);
    }
    if transport_plan {
        return Err(OuroError::InvalidArgs(
            "--transport-plan requires --dispatch <host>".into(),
        ));
    }
    // §2.8 — legacy write entry points are disabled unless registered.
    parity::require_registered_write(&op)?;

    // Load the attestation (must be adopted, §1.C / §2.4).
    let local = args.iter().any(|a| a == "--local");
    if optional(args, "--observation").is_some() && (local || !cfg!(debug_assertions)) {
        return Err(OuroError::Validation(
            "--observation is a debug control-host fixture only; target-local/release operations \
             must use the embedded live probe"
                .into(),
        ));
    }
    let initial_att = load_attestation(&paths, &node, local)?;
    if initial_att.immutable.machine_id != node {
        return Err(OuroError::Validation(format!(
            "target binding mismatch: --node {node} does not match adopted machine {} — refused",
            initial_att.immutable.machine_id
        )));
    }

    // Recovery runs before a REAL write (§2.6), serialized by the same crash-releasing node lock.
    // A target-validated plan and managed reads do not mutate transaction state: they report
    // pending recovery/seal state but never reconcile it or create a persistent lock file.
    let journal = Journal::at(&tx_dir(&paths), &node);
    let seal = WriteSeal::at(&tx_dir(&paths), &node);
    let operation_is_read = crate::intent::lookup(&op)
        .is_some_and(|operation| operation.mutability == Mutability::Read);
    let read_only_gate = plan || operation_is_read;
    if seal.is_sealed() {
        return Err(OuroError::Validation(
            "writes are sealed by a prior failed rollback — operator recovery required (§2.6)"
                .into(),
        ));
    }
    if let Some(record) = journal.read()? {
        return Err(OuroError::Validation(format!(
            "target has pending transaction state {:?} for {} — ordinary plan/read/write commands \
             never auto-verify or auto-rollback it because that could bypass fresh fleet and human \
             authorization; use the explicit operator recovery procedure (§2.6)",
            record.state, record.operation_id
        )));
    }
    let att = initial_att;
    let active_allowlist = convention::Allowlist::load(&paths.home, false)?;
    let active_allowlist_digest = active_allowlist.signed_digest()?;
    if let Some(expected) = optional(args, "--expect-allowlist") {
        if expected != active_allowlist_digest {
            return Err(OuroError::Validation(format!(
                "control→target allowlist mismatch: expected {expected}, target has {active_allowlist_digest}"
            )));
        }
    }
    let (active_contract, active_image) = active_allowlist.contract_and_image_for(
        &att.immutable.image_config_digest,
        &att.immutable.platform,
    )?;
    if att.immutable.allowlist_version > active_allowlist.allowlist_version
        || (att.immutable.allowlist_version == active_allowlist.allowlist_version
            && att.immutable.allowlist_digest != active_allowlist_digest)
        || att.immutable.contract_id != active_contract.contract_id
        || att.immutable.convention_version != active_contract.convention_version
        || att.immutable.oci_index_digest != active_image.oci_index_digest
        || att.immutable.platform_manifest_digest != active_image.platform_manifest_digest
    {
        return Err(OuroError::Validation(
            "adoption attestation is not bound to the active signed allowlist/OCI identity — \
             re-adopt before operating"
                .into(),
        ));
    }

    // Build the stable, FINAL semantic intent before any short-lived fleet authorization exists.
    // The approved state binds the operator-selected pool spec identity and quorum policy. Upgrade
    // additionally binds the exact target-side recreate spec with an opaque HMAC, so secret env
    // values are commit-rechecked but never published in ToolOutput.
    let payload = collect_params(args)?;
    let registered = crate::intent::lookup(&op).ok_or_else(|| {
        OuroError::Validation(format!("operation {op:?} is not in the privileged registry"))
    })?;
    let fleet_sensitive = registered.touched.iter().any(|resource| {
        matches!(*resource, "container:restart" | "container:recreate")
    });
    let fleet_policy = if fleet_sensitive {
        let digest = flag(args, "--fleet-spec-digest")?.to_string();
        validate_digest_selector("--fleet-spec-digest", &digest)?;
        let pool_id = flag(args, "--fleet-pool-id")?.to_string();
        crate::intent::validate_machine_id(&pool_id)?;
        let min_online_relays = flag(args, "--fleet-min-online-relays")?
            .parse::<u32>()
            .map_err(|_| OuroError::Validation(
                "--fleet-min-online-relays must be an unsigned integer".into(),
            ))?;
        Some((digest, pool_id, min_online_relays))
    } else {
        if optional(args, "--fleet-spec-digest").is_some()
            || optional(args, "--fleet-pool-id").is_some()
            || optional(args, "--fleet-min-online-relays").is_some()
            || optional(args, "--fleet-permit").is_some()
        {
            return Err(OuroError::Validation(
                "fleet policy/permit arguments are accepted only for a disruptive operation"
                    .into(),
            ));
        }
        None
    };
    if op == "kes-rotation/install-opcert" && att.immutable.role != Role::Bp {
        return Err(OuroError::Validation(
            "kes-rotation/install-opcert is BP-only; a relay may never receive an opcert".into(),
        ));
    }
    let upgrade_snapshot = if op == "upgrade/step" {
        let target = payload.get("image").and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("upgrade/step lost target image".into()))?;
        let observation = read_observation(args)?;
        require_current_contract_observation(&att, active_contract, &observation)?;
        let transition = active_allowlist
            .transition_for(&att.immutable.image_config_digest, target)?
            .clone();
        crate::upgrade::validate_transition(&transition, &active_allowlist, &observation.live.platform)?;
        crate::executor::require_image_present(target)?;
        let recreate = observation.recreate.as_ref().ok_or_else(|| {
            OuroError::Validation(
                "upgrade plan unavailable: probe could not model the full container run-spec".into(),
            )
        })?;
        let binding = recreate_spec_binding(&paths, local, recreate)?;
        Some((observation, transition, binding))
    } else {
        None
    };
    let kes_candidate = if op == "kes-rotation/install-opcert" {
        let reference = payload.get("opcert").and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("KES intent lost its opcert reference".into()))?;
        Some(validate_kes_candidate(&att, &paths.home.join("inbox"), reference)?)
    } else {
        None
    };
    let preload_candidate = if op == "upgrade/preload-image" {
        let reference = payload.get("artifact").and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("preload intent lost its image artifact".into()))?;
        let target = payload.get("image").and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("preload intent lost its target digest".into()))?;
        active_allowlist.contract_and_image_for(target, &att.immutable.platform)?;
        let artifact = crate::inbox::resolve_typed(
            &paths.home.join("inbox"), reference, crate::inbox::ArtifactType::Image,
        )?;
        crate::inbox::require_single_docker_config(&artifact, target)?;
        crate::executor::require_image_absent(target)?;
        Some((reference.to_string(), target.to_string()))
    } else {
        None
    };
    let expected_post_state = serde_json::to_string(&json!({
        "fleet": fleet_policy.as_ref().map(|(digest, pool_id, min)| json!({
            "pool_spec_digest": digest,
            "pool_id": pool_id,
            "min_online_relays": min,
        })),
        "upgrade_recreate_binding": upgrade_snapshot.as_ref().map(|(_, _, binding)| binding),
    })).map_err(|e| OuroError::Validation(format!("cannot encode approved semantic state: {e}")))?;
    let fleet_permit_raw = optional(args, "--fleet-permit");
    let intent = Intent {
        schema_version: 1,
        operation_id: op.clone(),
        node_id: node.clone(),
        pre_state_generation: att.state.state_generation,
        pre_state_hash: att.closed_fingerprint(),
        expected_post_state,
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
    if op == "deploy/register-submit" {
        let requested_network = intent
            .payload
            .get("network")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("deploy intent lost its network binding".into()))?;
        if requested_network != att.immutable.network {
            return Err(OuroError::Validation(format!(
                "target binding mismatch: payload network {requested_network} != attested network {} — refused",
                att.immutable.network
            )));
        }
    }

    // Dispatched writes carry the control's complete security identity; local invocations still
    // validate the local identity structure but do not claim control↔target parity.
    let id = parity::SecurityIdentity::local();
    parity::require_parity(&id, &id)?;
    if let Some(expected) = optional(args, "--expect-embedded") {
        parity::require_expected_wire_digest(expected)?;
    }

    // Real mutations hold the crash-releasing node lock through terminal transaction state. A plan
    // and managed reads perform the same live identity comparison without writing a node lock;
    // the real operation always acquires the lock and repeats the comparison before commit.
    let canon = intent.canonical_hash();
    let audit_id = format!("op-{canon}");
    let probe = || {
        let observation = read_observation(args)?;
        require_current_contract_observation(&att, active_contract, &observation)?;
        Ok(observation.live.to_live())
    };
    let guard = if read_only_gate {
        att.require_matches_live(&probe()?)?;
        None
    } else {
        Some(crate::gate::require_attested_node(
            &att, &tx_dir(&paths).join("locks"), &node, &audit_id, &probe,
        )?)
    };

    // Internal fleet-authority read: the normal adoption, parity, allowlist and live identity gate
    // above has already run. Return a closed status projection; an unhealthy node is DATA
    // (`online:false`), not a reason to fabricate availability.
    if op == "fleet/status" {
        let observation = read_observation(args)?;
        require_current_contract_observation(&att, active_contract, &observation)?;
        let online = require_readiness(&att, &observation, false).is_ok();
        audit_emit(&paths, "live_preflight", &node, json!({
            "operation_id": op,
            "intent_hash": canon,
            "pre_state_generation": att.state.state_generation,
            "outcome": "managed_read_validated",
        }))?;
        audit_emit(&paths, "verified", &node, json!({
            "operation_id": op,
            "intent_hash": canon,
            "pre_state_generation": att.state.state_generation,
            "post_state_generation": att.state.state_generation,
            "outcome": "fleet_status_success",
        }))?;
        output::print_json(&ToolOutput::ok("ouro.op.read", false).with_data(json!({
            "op": op,
            "node": node,
            "intent_hash": canon,
            "result": {
                "node": node,
                "role": match att.immutable.role { Role::Bp => "bp", Role::Relay => "relay" },
                "network": att.immutable.network,
                "genesis_hash": att.immutable.genesis_hash,
                "host_key_sha256": att.immutable.host_key_sha256,
                "online": online,
                "image_config_digest": observation.live.image_config_digest,
                "state_generation": att.state.state_generation,
            },
        })))?;
        return Ok(());
    }
    let fleet_permit = if fleet_sensitive {
        let encoded = match fleet_permit_raw {
            Some(encoded) => encoded,
            None if plan => "",
            None => {
                return Err(OuroError::Validation(format!(
                    "{op} is disruptive and requires a signed --fleet-permit (§2.9)"
                )))
            }
        };
        if encoded.is_empty() {
            None
        } else {
            let permit: crate::fleet::StepPermit = serde_json::from_str(encoded)
                .map_err(|e| OuroError::Validation(format!("malformed fleet permit: {e}")))?;
            let secret = operation_secret(&paths, local)?;
            let (expected_spec_digest, expected_pool_id, expected_min) = fleet_policy.as_ref()
                .ok_or_else(|| OuroError::Validation("disruptive intent lost fleet policy".into()))?;
            permit.verify(
                &crate::fleet::PermitExpectation {
                    pool_id: expected_pool_id.clone(),
                    pool_spec_digest: expected_spec_digest.clone(),
                    node_id: node.clone(),
                    operation_id: op.clone(),
                    role: match att.immutable.role { Role::Bp => "bp", Role::Relay => "relay" }.into(),
                    target_image: if op == "upgrade/step" {
                        intent.payload.get("image").and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                    } else {
                        None
                    },
                    min_online_relays: *expected_min,
                    network: att.immutable.network.clone(),
                    genesis_hash: att.immutable.genesis_hash.clone(),
                    target_host_key_sha256: att.immutable.host_key_sha256.clone(),
                    intent_hash: canon.clone(),
                },
                secret.trim().as_bytes(),
                crate::s0019_confirmation::current_epoch()?,
            )?;
            Some(permit)
        }
    } else {
        None
    };
    // Upgrade safety is signed metadata, not an inference from "both images are allowlisted".
    let upgrade_transition = upgrade_snapshot.as_ref().map(|(_, transition, _)| transition);

    // Target-validated FINAL plan: registry/schema, adoption, allowlist, parity, live drift, stable
    // fleet policy and (for upgrade) the sealed run-spec have passed. A permit is deliberately not
    // accepted here; it is minted only after approval and cannot change this intent hash.
    if plan {
        let steps = if op == "upgrade/step" {
            let target = intent
                .payload
                .get("image")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("upgrade plan lost target image".into()))?;
            let recreate = upgrade_snapshot.as_ref()
                .and_then(|(observation, _, _)| observation.recreate.as_ref())
                .ok_or_else(|| OuroError::Validation("upgrade plan lost sealed recreate spec".into()))?;
            crate::executor::recreate_approval_argv(recreate, &att.state.container_id, target)?
        } else if matches!(
            op.as_str(),
            "kes-rotation/install-opcert" | "deploy/register-submit" | "upgrade/preload-image"
        ) {
            // An artifact-bearing plan validates the actual target inbox reference now; it never
            // asks the operator to approve a placeholder for missing/replaced bytes. KES also shows
            // the exact pre-cert backup step that the real recoverable transaction will prepend.
            let inbox = paths.home.join("inbox");
            let mut artifact_steps = crate::executor::build_plan(&intent, &att, Some(&inbox))?;
            if op == "kes-rotation/install-opcert" {
                let backup = tx_dir(&paths)
                    .join("rollback")
                    .join(&canon)
                    .join("node.cert.pre")
                    .display()
                    .to_string();
                artifact_steps.insert(0, vec![
                    "docker".into(),
                    "cp".into(),
                    format!("{}:/opt/cardano/config/keys/node.cert", att.state.container_id),
                    backup,
                ]);
            }
            artifact_steps
        } else {
            crate::executor::build_plan(&intent, &att, None)?
        };
        audit_emit(&paths, "live_preflight", &node, json!({
            "operation_id": op,
            "intent_hash": canon,
            "pre_state_generation": att.state.state_generation,
            "outcome": "target_plan_validated",
        }))?;
        output::print_json(&ToolOutput::ok("ouro.op.plan", false).with_data(json!({
            "op": op,
            "node": node,
            "mutability": format!("{:?}", spec.mutability),
            "intent_hash": canon,
            "intent_hash_final": true,
            "touched": spec.touched,
            "executor_plan": steps,
            "executor_plan_secret_values_redacted": op == "upgrade/step",
            "fleet_permit_required": fleet_sensitive,
            "fleet_permit_accepted_in_plan": false,
            "fleet_policy": fleet_policy.as_ref().map(|(digest, pool_id, min)| json!({
                "pool_spec_digest": digest,
                "pool_id": pool_id,
                "min_online_relays": min,
                "permit_freshness_seconds": 30,
            })),
            "confirmation_required": spec.mutability == Mutability::Dangerous,
            "commit_recheck_required": true,
            "upgrade_transition": upgrade_transition,
            "upgrade_failure_outcome": upgrade_transition
                .map(crate::upgrade::failure_outcome)
                .map(|outcome| format!("{outcome:?}")),
            "kes_candidate": kes_candidate.as_ref().map(|candidate| json!({
                "counter": candidate.counter,
                "kes_period": candidate.kes_period,
                "cold_key_signature_valid": true,
                "public_kes_vkey_matches": true,
                "live_protocol_window_valid": true,
                "artifact_replay": false,
            })),
            "preload_candidate": preload_candidate.as_ref().map(|(artifact, target)| json!({
                "artifact_ref": artifact,
                "target_image_config_digest": target,
                "archive_contains_exactly_one_target_config": true,
                "tag_changes": "none",
                "target_absent_before_load": true,
                "running_node_untouched": true,
            })),
            "note": if fleet_sensitive {
                "target-validated final plan; no node runtime/config/attestation/inbox/transaction \
                 mutation (audit and private temporary probe metadata may be written). Approve this \
                 intent_hash, mint confirmation, then mint a 30-second fleet permit last and execute \
                 immediately without replanning"
            } else {
                "target-validated final plan; no node runtime/config/attestation/inbox/transaction \
                 mutation (audit and private temporary probe metadata may be written)"
            },
        })))?;
        return Ok(());
    }

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

    audit_emit(&paths, "live_preflight", &node, json!({
        "operation_id": op,
        "intent_hash": canon,
        "pre_state_generation": att.state.state_generation,
        "outcome": "passed",
    }))?;
    if spec.mutability == Mutability::Dangerous {
        let token = optional(args, "--confirm-token").ok_or_else(|| {
            OuroError::Validation("dangerous operation lost verified confirmation".into())
        })?;
        let mut approval = serde_json::Map::new();
        approval.insert("operation_id".into(), json!(op));
        approval.insert("intent_hash".into(), json!(canon));
        approval.insert(
            "approval_evidence_hash".into(),
            json!(crate::intent::sha256_hex(token.as_bytes())),
        );
        if let Some(permit) = &fleet_permit {
            approval.insert("fencing_token".into(), json!(permit.fencing_token));
        }
        audit_emit(
            &paths,
            "intent_approval",
            &node,
            serde_json::Value::Object(approval),
        )?;
    }

    // A managed READ (e.g. observability/health) passes the attested gate but takes no confirm and
    // no write transaction. Plan mode returns the fixed argv; a real run executes that fixed argv
    // and returns its bounded, parsed result. No journal is touched.
    if spec.mutability == Mutability::Read {
        let executor_plan = crate::executor::build_plan(&intent, &att, None)?;
        let stdout = crate::executor::run_read_plan(&executor_plan)?;
        let result: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|error| {
            OuroError::Validation(format!(
                "managed health read returned malformed JSON: {error}"
            ))
        })?;
        audit_emit(&paths, "verified", &node, json!({
            "operation_id": op,
            "intent_hash": canon,
            "pre_state_generation": att.state.state_generation,
            "post_state_generation": att.state.state_generation,
            "outcome": "managed_read_success",
        }))?;
        output::print_json(&ToolOutput::ok("ouro.op.read", false).with_data(json!({
            "op": op, "node": node, "intent_hash": canon, "result": result,
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
        let transition = upgrade_transition.as_ref().ok_or_else(|| {
            OuroError::Validation("upgrade transition was not validated before planning".into())
        })?;
        let spec = upgrade_snapshot.as_ref()
            .and_then(|(observation, _, _)| observation.recreate.as_ref())
            .ok_or_else(|| OuroError::Validation("upgrade lost the approved recreate spec".into()))?;
        let commit = crate::executor::recreate_argv(spec, &att.state.container_id, to_digest)?;
        let rb = if crate::upgrade::rollback_possible(transition) {
            Some(crate::executor::upgrade_rollback_plan(&att, spec)?)
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
    let is_preload = op == "upgrade/preload-image";
    // These ops deliberately change managed CONTENT (opcert / config / topology); their post-commit
    // verify checks identity only, then ADVANCES the managed state (CAS gen bump) and persists it —
    // otherwise the very next op would drift-refuse against the stale attestation.
    let managed_changing = matches!(
        op.as_str(),
        "kes-rotation/install-opcert"
    );
    let to_digest_owned = intent
        .payload
        .get("image")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let approved_recreate_binding = upgrade_snapshot.as_ref()
        .map(|(_, _, binding)| binding.clone());
    // Repeat every mutable precondition while the attestation lock is held, but BEFORE entering
    // the transaction. A failed TOCTOU/recreate/KES check must not be interpreted as a failed
    // mutation and must therefore never invoke rollback against a node we did not change.
    guard
        .as_ref()
        .ok_or_else(|| {
            OuroError::Validation("real mutation lost its locked attestation guard".into())
        })?
        .recheck_before_commit()?;
    if is_upgrade {
        let fresh = read_observation(args)?;
        require_current_contract_observation(&att, active_contract, &fresh)?;
        let recreate = fresh.recreate.as_ref().ok_or_else(|| OuroError::Validation(
            "upgrade commit recheck could no longer model the full container run-spec".into(),
        ))?;
        let fresh_binding = recreate_spec_binding(&paths, local, recreate)?;
        if Some(&fresh_binding) != approved_recreate_binding.as_ref() {
            return Err(OuroError::Validation(
                "upgrade recreate spec changed after approval — mint a fresh plan/confirmation"
                    .into(),
            ));
        }
        crate::executor::require_image_present(&to_digest_owned)?;
    }
    if op == "kes-rotation/install-opcert" {
        let reference = intent.payload.get("opcert").and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("KES intent lost its opcert reference".into()))?;
        validate_kes_candidate(&att, &inbox, reference)?;
    }
    if is_preload {
        let reference = intent.payload.get("artifact").and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("preload intent lost its image artifact".into()))?;
        let target = intent.payload.get("image").and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("preload intent lost its target digest".into()))?;
        let artifact = crate::inbox::resolve_typed(
            &inbox, reference, crate::inbox::ArtifactType::Image,
        )?;
        crate::inbox::require_single_docker_config(&artifact, target)?;
        crate::executor::require_image_absent(target)?;
    }
    let commit = || crate::executor::run_plan(&commit_plan);
    let verify = || {
        let live = read_observation(args)?;
        if is_upgrade {
            let (target_contract, _) = active_allowlist
                .contract_and_image_for(&to_digest_owned, &live.live.platform)?;
            require_contract_shape_and_role(&att, target_contract, &live)?;
        } else {
            require_contract_shape_and_role(&att, active_contract, &live)?;
        }
        if !is_preload {
            require_readiness(&att, &live, is_upgrade)?;
        }
        if is_preload {
            let target = intent.payload.get("image").and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("preload intent lost its target digest".into()))?;
            crate::executor::require_image_present(target)?;
        }
        if is_upgrade {
            if live.live.image_config_digest != to_digest_owned {
                return Err(OuroError::Validation(
                    "upgrade did not land on the target image digest — rolling back (§2.10)".into(),
                ));
            }
            if live.live.container_id.is_empty() {
                return Err(OuroError::Validation("no node container after upgrade — rolling back".into()));
            }
            rotate_attestation_for_upgrade(&paths, &node, local, &att, &live, &to_digest_owned)?;
            audit_emit(&paths, "attestation_rotation", &node, json!({
                "operation_id": op,
                "intent_hash": canon,
                "pre_state_generation": att.state.state_generation,
                "post_state_generation": att.state.state_generation.saturating_add(1),
                "outcome": "upgrade_identity_rotated",
            }))
        } else if managed_changing {
            // Immutable identity must still hold (an image swap / recreate is still caught); the
            // content hashes are expected to have changed → snapshot them as the new baseline.
            att.require_identity_matches(&live.live.to_live())?;
            if op == "kes-rotation/install-opcert" {
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
            persist_attestation(&paths, &node, local, &advanced)?;
            audit_emit(&paths, "attestation_rotation", &node, json!({
                "operation_id": op,
                "intent_hash": canon,
                "pre_state_generation": att.state.state_generation,
                "post_state_generation": advanced.state.state_generation,
                "outcome": "managed_state_advanced",
            }))
        } else {
            att.require_matches_live(&live.live.to_live())
        }
    };
    let rollback = || {
        let plan = rb_plan.as_ref().ok_or_else(|| OuroError::Validation(format!(
            "{} has no safe automatic rollback; operator reconciliation required", op
        )))?;
        crate::executor::run_rollback_plan(&op, plan)?;
        if is_upgrade {
            let restored = read_observation(args)?;
            if restored.live.image_config_digest != att.immutable.image_config_digest {
                return Err(OuroError::Validation(
                    "upgrade rollback did not restore the prior image digest".into(),
                ));
            }
            let (prior_contract, _) = active_allowlist.contract_and_image_for(
                &att.immutable.image_config_digest,
                &restored.live.platform,
            )?;
            require_contract_shape_and_role(&att, prior_contract, &restored)?;
            require_readiness(&att, &restored, true)?;
            // Recreating N necessarily changes the container id/creation epoch. Persist a verified
            // prior-image attestation for that NEW identity; writing the stale pre-upgrade CID
            // would falsely report rollback success and make every later op drift-refuse.
            rotate_attestation_for_upgrade(
                &paths,
                &node,
                local,
                &att,
                &restored,
                &att.immutable.image_config_digest,
            )
        } else {
            persist_attestation(&paths, &node, local, &att)
        }
    };
    // Consume only after every fail-closed preflight/plan has succeeded, but before the transaction
    // can enter Committing. A crash or failed mutation then burns the approval permanently.
    if let Some(permit) = &fleet_permit {
        permit.require_live_relay_quorum()?;
        crate::fleet::TargetFence::at(&tx_dir(&paths).join("fleet-fence"), &node)
            .accept(permit, crate::s0019_confirmation::current_epoch()?)?;
    }
    if let Some(confirmation) = &verified_confirmation {
        crate::s0019_confirmation::consume(
            &tx_dir(&paths).join("confirm-used").join(format!("{node}.log")), confirmation,
        )?;
    }
    let ops = TxOps { commit: &commit, verify: &verify, rollback: &rollback };
    let post_generation = if is_upgrade || managed_changing {
        att.state.state_generation.saturating_add(1)
    } else {
        att.state.state_generation
    };
    let observe = |state: TxState| {
        let mut fields = serde_json::Map::new();
        fields.insert("operation_id".into(), json!(op));
        fields.insert("intent_hash".into(), json!(canon));
        fields.insert("pre_state_generation".into(), json!(att.state.state_generation));
        if matches!(state, TxState::Verified | TxState::RolledBack) {
            fields.insert("post_state_generation".into(), json!(
                if state == TxState::Verified || (state == TxState::RolledBack && is_upgrade) {
                    post_generation
                } else {
                    att.state.state_generation
                }
            ));
        }
        if let Some(permit) = &fleet_permit {
            fields.insert("fencing_token".into(), json!(permit.fencing_token));
        }
        fields.insert("outcome".into(), json!(format!("{state:?}")));
        audit_emit(
            &paths,
            tx_audit_event(state),
            &node,
            serde_json::Value::Object(fields),
        )
    };
    let outcome = transaction::run_observed(&journal, &seal, &base, &ops, &observe)?;
    output::print_json(&ToolOutput::ok("ouro.op.run", true).with_data(json!({
        "op": op, "node": node, "intent_hash": canon, "outcome": format!("{outcome:?}"),
    })))?;
    Ok(())
}

/// `ouro-ops confirm create --op <id> --node <id> --intent-hash <hash>` — mint a token bound to the
/// exact canonical intent + human diff (§2.5). Represents the OPERATOR'S approval.
pub fn run_confirm_create(args: &[String]) -> Result<()> {
    validate_closed_args(args, &["--op", "--node", "--intent-hash", "--ttl"], &[], &[])?;
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

/// Mint the operator approval for an exact adoption preview. The target verifies and durably
/// consumes it under the adoption lock before writing the attestation.
pub fn run_adopt_confirm_create(args: &[String]) -> Result<()> {
    validate_closed_args(
        args,
        &["--node", "--candidate-hash", "--host-key", "--ttl"],
        &[],
        &[],
    )?;
    let node = flag(args, "--node")?;
    crate::intent::validate_machine_id(node)?;
    let candidate = flag(args, "--candidate-hash")?;
    let host_key = flag(args, "--host-key")?;
    let valid_hash = |value: &str| {
        value.len() == 64
            && value.bytes().all(|byte| {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            })
    };
    if !valid_hash(candidate) || !valid_ssh_sha256_fingerprint(host_key) {
        return Err(OuroError::Validation(
            "adoption candidate must be lowercase SHA-256 and host key must be an OpenSSH SHA256 fingerprint"
                .into(),
        ));
    }
    let ttl = crate::confirm::parse_ttl(optional(args, "--ttl").unwrap_or("5m"))?;
    let ttl_seconds = u64::try_from(ttl.num_seconds())
        .map_err(|_| OuroError::Validation("adoption confirmation ttl must be positive".into()))?;
    let paths = ConfigPaths::discover();
    let secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let diff = format!("adopt {node} host {host_key}");
    let (token, expires_at) = crate::s0019_confirmation::mint(
        candidate,
        &diff,
        secret.as_bytes(),
        crate::s0019_confirmation::current_epoch()?,
        ttl_seconds,
    )?;
    output::print_json(&ToolOutput::ok("ouro.confirm.adopt.create", false).with_data(json!({
        "node": node,
        "candidate_hash": candidate,
        "host_key_sha256": host_key,
        "diff": diff,
        "approve_token": token,
        "expires_at_epoch": expires_at,
        "single_use": true,
    })))?;
    Ok(())
}

/// Build the SSH dispatch of an `op` to the target. Ordinary `--plan` is preserved and EXECUTED on
/// the target so registry, adoption, allowlist, parity and live-state validation really occur.
/// `--transport-plan` is the explicitly weaker SSH-argv-only inspection mode.
fn dispatch_op(
    host: &str,
    op: &str,
    node: &str,
    args: &[String],
    paths: &ConfigPaths,
    transport_plan: bool,
) -> Result<()> {
    if op == "observability/health" {
        return dispatch_stateless_observe(host, node, args, paths, transport_plan);
    }
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
            "--transport-plan" => {}
            other => remote.push(other.to_string()),
        }
        i += 1;
    }
    remote.push("--local".into());
    remote.push("--expect-allowlist".into());
    remote.push(convention::Allowlist::active_verified()?.signed_digest()?);
    let argv = crate::dispatch::op_dispatch_argv(
        host,
        22,
        &key,
        &paths.known_hosts,
        &remote,
        &parity::SecurityIdentity::local().wire_digest(),
    );
    if transport_plan {
        output::print_json(&ToolOutput::ok("ouro.op.dispatch.transport_plan", false).with_data(json!({
            "op": op, "node": node, "target": host, "principal": "ouro-op",
            "ssh_argv": argv,
            "target_validated": false,
            "note": "transport-only inspection: confined + host-key-pinned SSH argv; registry, \
                     adoption, allowlist, parity and live state have NOT been validated",
        })))?;
        return Ok(());
    }
    let out = crate::ssh::bounded_ssh(
        &argv,
        std::time::Duration::from_secs(15 * 60),
        256 * 1024,
        "managed operation SSH dispatch",
    ).map_err(|e| OuroError::Validation(format!("ssh dispatch failed: {e}")))?;
    finish_ssh_dispatch("ouro.op.dispatch", &out)
}

fn dispatch_stateless_observe(
    host: &str,
    node: &str,
    args: &[String],
    paths: &ConfigPaths,
    transport_plan: bool,
) -> Result<()> {
    for forbidden in [
        "--confirm-token",
        "--fleet-pool-id",
        "--fleet-spec-digest",
        "--fleet-min-online-relays",
        "--fleet-permit",
    ] {
        if args.iter().any(|arg| arg == forbidden) {
            return Err(OuroError::Validation(format!(
                "{forbidden} is not valid for stateless observability"
            )));
        }
    }
    let params = collect_params(args)?;
    if let Some(object) = params.as_object() {
        if object.keys().any(|key| key != "machine")
            || object
                .get("machine")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|machine| machine != node)
        {
            return Err(OuroError::Validation(
                "observability accepts only an optional machine=<node> parameter".into(),
            ));
        }
    }

    let default_key_ref = format!("creds://{node}");
    let key_ref = optional(args, "--ssh-key").unwrap_or(&default_key_ref);
    let key = crate::secrets::CredentialRef::parse(key_ref)?.resolve(&paths.credentials_dir)?;
    let runner = crate::runner::linux_x86_64()?;
    let target_args = vec![
        "target".to_string(),
        "observe".to_string(),
        "--node".to_string(),
        node.to_string(),
    ];
    let argv = crate::dispatch::ephemeral_runner_dispatch_argv(
        host,
        22,
        "cardano",
        &key,
        &paths.known_hosts,
        &runner.sha256,
        &target_args,
    )?;
    if transport_plan {
        output::print_json(
            &ToolOutput::ok("ouro.observe.dispatch.transport_plan", false).with_data(json!({
                "op": "observability/health",
                "node": node,
                "target": host,
                "principal": "cardano",
                "runner": {
                    "platform": runner.platform,
                    "sha256": runner.sha256,
                    "size_bytes": runner.bytes.len(),
                    "source": "control_build",
                },
                "ssh_argv": argv,
                "target_validated": false,
                "persistent_target_install": false,
                "note": "transport-only inspection; no SSH session or live observation ran",
            })),
        )?;
        return Ok(());
    }
    let out = crate::ssh::bounded_ssh_with_input(
        &argv,
        &runner.bytes,
        std::time::Duration::from_secs(5 * 60),
        256 * 1024,
        "ephemeral observability SSH dispatch",
    )
    .map_err(|error| OuroError::Validation(format!("ssh dispatch failed: {error}")))?;
    finish_ssh_dispatch("ouro.observe.dispatch", &out)
}

fn dispatch_stateless_plan(
    host: &str,
    op: &str,
    node: &str,
    args: &[String],
    paths: &ConfigPaths,
) -> Result<()> {
    if optional(args, "--confirm-token").is_some() || optional(args, "--fleet-permit").is_some() {
        return Err(OuroError::Validation(
            "stateless --plan never accepts confirmation or fleet capabilities".into(),
        ));
    }
    let spec_path = flag(args, "--spec")?;
    let spec = PoolSpec::from_file(Path::new(spec_path))?;
    let machine = spec
        .machines
        .iter()
        .find(|machine| machine.id == node)
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "target node {node:?} is not declared in pool spec {spec_path}"
            ))
        })?;
    if machine.ssh.host != host {
        return Err(OuroError::Validation(format!(
            "dispatch host {host:?} does not match pool-spec host {:?} for {node}",
            machine.ssh.host
        )));
    }
    if machine.ssh.user != "cardano" {
        return Err(OuroError::Validation(format!(
            "S0020 target user must be cardano; pool spec declares {:?} for {node}",
            machine.ssh.user
        )));
    }
    let supplied_key = optional(args, "--ssh-key")
        .map(crate::secrets::CredentialRef::parse)
        .transpose()?;
    if supplied_key
        .as_ref()
        .is_some_and(|credential| credential != &machine.ssh.key_ref)
    {
        return Err(OuroError::Validation(format!(
            "--ssh-key does not match the pool-spec credential reference for {node}"
        )));
    }
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let (pool_spec_digest, pool_id) = pool_spec_identity(&spec)?;
    for (name, expected) in [
        ("--fleet-pool-id", pool_id.as_str()),
        ("--fleet-spec-digest", pool_spec_digest.as_str()),
    ] {
        if optional(args, name).is_some_and(|supplied| supplied != expected) {
            return Err(OuroError::Validation(format!(
                "{name} conflicts with the value derived from {spec_path}"
            )));
        }
    }
    if optional(args, "--fleet-min-online-relays").is_some_and(|supplied| {
        supplied != spec.upgrade.min_online_relays.to_string()
    }) {
        return Err(OuroError::Validation(
            "--fleet-min-online-relays conflicts with pool-spec upgrade policy".into(),
        ));
    }
    let role = match machine.role {
        MachineRole::Bp => "bp",
        MachineRole::Relay => "relay",
    };
    let mut target_args = vec![
        "target".into(),
        "plan".into(),
        "--op".into(),
        op.to_string(),
        "--node".into(),
        node.to_string(),
        "--role".into(),
        role.into(),
        "--network".into(),
        spec.pool.network.as_str().into(),
        "--genesis".into(),
        spec.pool.genesis_hashes.shelley.clone(),
        "--pool-id".into(),
        pool_id,
        "--pool-spec-digest".into(),
        pool_spec_digest,
        "--min-online-relays".into(),
        spec.upgrade.min_online_relays.to_string(),
    ];
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--param" {
            target_args.push("--param".into());
            target_args.push(
                args.get(index + 1)
                    .ok_or_else(|| OuroError::InvalidArgs("missing --param value".into()))?
                    .clone(),
            );
            index += 2;
        } else {
            index += 1;
        }
    }
    let runner = crate::runner::linux_x86_64()?;
    let argv = crate::dispatch::ephemeral_runner_dispatch_argv(
        host,
        machine.ssh.port,
        "cardano",
        &key,
        &paths.known_hosts,
        &runner.sha256,
        &target_args,
    )?;
    let out = crate::ssh::bounded_ssh_with_input(
        &argv,
        &runner.bytes,
        std::time::Duration::from_secs(5 * 60),
        256 * 1024,
        "ephemeral stateless plan SSH dispatch",
    )
    .map_err(|error| OuroError::Validation(format!("ssh dispatch failed: {error}")))?;
    finish_ssh_dispatch("ouro.op.plan.dispatch", &out)
}

/// p6-3 — SSH-dispatch `adopt` to the target (as the bootstrap account), running `adopt --local`
/// there. Control-only flags are stripped; the target self-probes (p6-2).
fn dispatch_adopt(host: &str, node: &str, args: &[String], paths: &ConfigPaths, plan: bool) -> Result<()> {
    let spec_path = flag(args, "--spec")?;
    let spec = PoolSpec::from_file(std::path::Path::new(spec_path))?;
    let machine = spec.machines.iter().find(|machine| machine.id == node).ok_or_else(|| {
        OuroError::Validation(format!("adoption node {node} is not declared in the pool spec"))
    })?;
    if machine.runtime.as_ref().is_some_and(|runtime| runtime.mode != RuntimeMode::Docker) {
        return Err(OuroError::Validation(format!(
            "adoption runtime mismatch: pool spec declares {node} as non-Docker, but S0019 only \
             adopts the pinned Docker convention; correct the operator-owned spec after review"
        )));
    }
    let expected_role = match machine.role { MachineRole::Bp => "bp", MachineRole::Relay => "relay" };
    if flag(args, "--role")? != expected_role {
        return Err(OuroError::Validation(format!(
            "adoption role mismatch: pool spec declares {node} as {expected_role}"
        )));
    }
    let user = optional(args, "--bootstrap-user").unwrap_or("root");
    let key_ref = optional(args, "--ssh-key").unwrap_or("creds://bootstrap");
    let key = crate::secrets::CredentialRef::parse(key_ref)?.resolve(&paths.credentials_dir)?;
    let pinned_host_key = pinned_ed25519_host_key(host, 22, &paths.known_hosts)?;
    let mut remote: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dispatch" | "--ssh-key" | "--bootstrap-user" | "--observation" | "--spec" => i += 1,
            "--plan" => {}
            other => remote.push(other.to_string()),
        }
        i += 1;
    }
    remote.push("--expect-allowlist".into());
    remote.push(convention::Allowlist::active_verified()?.signed_digest()?);
    remote.push("--expected-role".into());
    remote.push(expected_role.into());
    remote.push("--expected-network".into());
    remote.push(spec.pool.network.as_str().into());
    remote.push("--expected-genesis".into());
    remote.push(spec.pool.genesis_hashes.shelley.clone());
    remote.push("--expected-host-key".into());
    remote.push(pinned_host_key);
    if let Some(container) = machine.runtime.as_ref().and_then(|runtime| runtime.container.as_ref()) {
        remote.push("--expected-container".into());
        remote.push(container.clone());
    }
    if let Some(image) = machine.runtime.as_ref().and_then(|runtime| runtime.image.as_ref()) {
        remote.push("--expected-image".into());
        remote.push(image.clone());
    }
    let expected_identity = parity::SecurityIdentity::local().wire_digest();
    let argv = crate::dispatch::adopt_dispatch_argv(
        host,
        22,
        user,
        &key,
        &paths.known_hosts,
        &remote,
        &expected_identity,
    )?;
    if plan {
        output::print_json(&ToolOutput::ok("ouro.adopt.dispatch.plan", false).with_data(json!({
            "node": node, "target": host, "principal": user, "ssh_argv": argv,
            "note": "dispatch plan — bootstrap account runs `ouro-ops adopt --local` on the target",
        })))?;
        return Ok(());
    }
    let identity_argv = crate::dispatch::adopt_dispatch_argv(
        host,
        22,
        user,
        &key,
        &paths.known_hosts,
        &["--identity-only".into()],
        &expected_identity,
    )?;
    let identity_out = crate::ssh::bounded_ssh(
        &identity_argv,
        std::time::Duration::from_secs(45),
        64 * 1024,
        "adoption identity SSH preflight",
    ).map_err(|e| OuroError::Validation(format!("adopt identity preflight failed: {e}")))?;
    let identity: serde_json::Value = serde_json::from_slice(identity_out.stdout.as_bytes()).map_err(|_| {
        OuroError::Validation(
            "target does not support the exact adoption security-identity preflight; update it before adoption"
                .into(),
        )
    })?;
    if identity_out.status != 0
        || identity.get("tool").and_then(serde_json::Value::as_str) != Some("ouro.adopt.identity")
        || identity.pointer("/data/security_identity").and_then(serde_json::Value::as_str)
            != Some(expected_identity.as_str())
    {
        return Err(OuroError::Validation(
            "target adoption security identity differs from the control binary; update it before adoption"
                .into(),
        ));
    }
    let out = crate::ssh::bounded_ssh(
        &argv,
        std::time::Duration::from_secs(5 * 60),
        256 * 1024,
        "adoption SSH dispatch",
    ).map_err(|e| OuroError::Validation(format!("ssh dispatch failed: {e}")))?;
    finish_ssh_dispatch("ouro.adopt.dispatch", &out)
}

/// Resolve the one Ed25519 key pinned for this SSH endpoint and express it exactly as OpenSSH does.
/// The target probe independently fingerprints `/etc/ssh/ssh_host_ed25519_key.pub`; adoption then
/// compares the two, binding the attestation/approval identity to the key that the control pins.
fn pinned_ed25519_host_key(host: &str, port: u16, known_hosts: &Path) -> Result<String> {
    let target = if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    };
    let output = std::process::Command::new("ssh-keygen")
        .args(["-F", &target, "-f"])
        .arg(known_hosts)
        .output()
        .map_err(|error| {
            OuroError::Validation(format!("cannot inspect pinned host key for {target}: {error}"))
        })?;
    let mut fingerprints = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .filter(|line| line.split_whitespace().nth(1) == Some("ssh-ed25519"))
        .filter_map(crate::cli::fingerprint_of)
        .collect::<Vec<_>>();
    fingerprints.sort();
    fingerprints.dedup();
    if fingerprints.len() != 1 || !valid_ssh_sha256_fingerprint(&fingerprints[0]) {
        return Err(OuroError::Validation(format!(
            "{target} must have exactly one pinned Ed25519 host key before adoption"
        )));
    }
    Ok(fingerprints.remove(0))
}

fn artifact_ref_digest(reference: &str) -> Option<&str> {
    reference.split_once("@sha256:").map(|(_, digest)| digest)
}

fn json_u64(value: &serde_json::Value, key: &str) -> Result<u64> {
    value.get(key).and_then(serde_json::Value::as_u64).ok_or_else(|| {
        OuroError::Validation(format!("cardano-cli KES validation omitted {key}"))
    })
}

/// `cardano-cli query kes-period-info --output-json` writes human ✓/✗ diagnostics before its JSON
/// object. Extract exactly one bounded terminal object; never accept a second/trailing structured
/// record that could make an agent and the validator approve different facts.
fn parse_cardano_cli_json(raw: &[u8], context: &str) -> Result<serde_json::Value> {
    if raw.len() > 65_536 {
        return Err(OuroError::Validation(format!("{context} output exceeds 64 KiB")));
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| OuroError::Validation(format!("{context} output is not UTF-8")))?;
    let start = text.find('{').ok_or_else(|| {
        OuroError::Validation(format!("{context} omitted its JSON object"))
    })?;
    serde_json::from_str(&text[start..]).map_err(|error| {
        OuroError::Validation(format!("{context} has malformed or trailing JSON: {error}"))
    })
}

/// Validate a staged PUBLIC opcert against the target's PUBLIC KES vkey and live protocol state.
/// The private KES key is never opened by ouro. A matching `kes.vkey` must already accompany the
/// operator-managed hot key, making key↔cert binding checkable without exposing secret material.
fn validate_kes_candidate(
    att: &AdoptionAttestation,
    inbox: &Path,
    reference: &str,
) -> Result<crate::kes::ParsedOperationalCertificate> {
    let digest = artifact_ref_digest(reference).ok_or_else(|| {
        OuroError::Validation("KES intent has a malformed artifact reference".into())
    })?;
    if digest == att.state.kes_opcert_id {
        return Err(OuroError::Validation(
            "KES artifact is identical to the currently attested opcert — replay refused".into(),
        ));
    }
    let path = crate::inbox::resolve_typed(
        inbox,
        reference,
        crate::inbox::ArtifactType::Opcert,
    )?;
    let bytes = std::fs::read(&path)?;
    let parsed = crate::kes::parse_operational_certificate(&bytes)?;

    let vkey_output = std::process::Command::new("docker")
        .args([
            "exec",
            &att.state.container_id,
            "sh",
            "-c",
            "test -f /opt/cardano/config/keys/kes.vkey && head -c 65537 /opt/cardano/config/keys/kes.vkey",
        ])
        .output()
        .map_err(|error| OuroError::Validation(format!("cannot read public KES vkey: {error}")))?;
    if !vkey_output.status.success() || vkey_output.stdout.len() > 65_536 {
        return Err(OuroError::Validation(
            "matching public /opt/cardano/config/keys/kes.vkey is required before KES planning"
                .into(),
        ));
    }
    let public_vkey = crate::kes::parse_kes_verification_key(&vkey_output.stdout)?;
    if public_vkey != parsed.hot_kes_verification_key {
        return Err(OuroError::Validation(
            "opcert hot KES key does not match the target's public kes.vkey — refused".into(),
        ));
    }

    let mut command = std::process::Command::new("docker");
    command.args([
        "exec", "-i", &att.state.container_id, "cardano-cli", "query", "kes-period-info",
        "--socket-path", "/ipc/node.socket", "--op-cert-file", "/dev/stdin", "--output-json",
    ]);
    match att.immutable.network.as_str() {
        "mainnet" => { command.arg("--mainnet"); }
        "preprod" => { command.args(["--testnet-magic", "1"]); }
        "preview" => { command.args(["--testnet-magic", "2"]); }
        network => return Err(OuroError::Validation(format!("unsupported KES network {network}"))),
    }
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| OuroError::Validation(format!("cannot run cardano-cli KES validation: {error}")))?;
    child.stdin.take().ok_or_else(|| OuroError::Validation("KES validator has no stdin".into()))?
        .write_all(&bytes)?;
    let output = child.wait_with_output()?;
    if output.stdout.len() > 65_536 || output.stderr.len() > 65_536 || !output.status.success() {
        return Err(OuroError::Validation(format!(
            "cardano-cli rejected the prospective opcert: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(2048).collect::<String>()
        )));
    }
    let facts = parse_cardano_cli_json(&output.stdout, "cardano-cli KES result")?;
    let current = json_u64(&facts, "qKesCurrentKesPeriod")?;
    let start = json_u64(&facts, "qKesStartKesInterval")?;
    let end = json_u64(&facts, "qKesEndKesInterval")?;
    let on_disk = json_u64(&facts, "qKesOnDiskOperationalCertificateNumber")?;
    let node_state = json_u64(&facts, "qKesNodeStateOperationalCertificateNumber")?;
    if parsed.counter != on_disk
        || parsed.kes_period != start
        || current < start
        || current >= end
        || on_disk < node_state
        || on_disk > node_state.saturating_add(1)
    {
        return Err(OuroError::Validation(format!(
            "prospective opcert is stale/out-of-period/inconsistent: counter={on_disk}, node_state={node_state}, period={start}..{end}, current={current}"
        )));
    }
    Ok(parsed)
}

fn load_attestation(paths: &ConfigPaths, node: &str, local: bool) -> Result<AdoptionAttestation> {
    let p = attestation_path_for(paths, node, local);
    let text = match attestation::read_document(&p) {
        Ok(text) => text,
        Err(OuroError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(OuroError::Validation(format!(
                "not_ouro_managed: node {node} has no adoption attestation — run `ouro-ops adopt` \
                 first; ops are refused, never adapted (§1.C)"
            )))
        }
        Err(error) => return Err(error),
    };
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
    let contract = attestation::read_document(&p)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("contract").cloned());
    let mut doc = serde_json::to_value(att).unwrap();
    if let Some(c) = contract {
        doc["contract"] = c;
    }
    attestation::write_document(&p, &doc)
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
    obs.supervisor.require_conformant()?;
    require_typed_mounts(&obs.live.mounts)?;
    let allow = convention::Allowlist::load(&paths.home, false)?;
    let (contract, allowed_image) = allow.contract_and_image_for(to_digest, &obs.live.platform)?;
    let role_rule = match old.immutable.role {
        Role::Bp => contract.role_rules.bp,
        Role::Relay => contract.role_rules.relay,
    };
    old.check_role(&role_rule, &obs.live.to_live())?;
    let immutable = ImmutableIdentity {
        role: old.immutable.role,
        contract_id: contract.contract_id.clone(),
        convention_version: contract.convention_version,
        allowlist_version: allow.allowlist_version,
        allowlist_digest: allow.signed_digest()?,
        host_key_sha256: old.immutable.host_key_sha256.clone(),
        machine_id: old.immutable.machine_id.clone(),
        oci_index_digest: allowed_image.oci_index_digest.clone(),
        platform_manifest_digest: allowed_image.platform_manifest_digest.clone(),
        image_config_digest: obs.live.image_config_digest.clone(),
        platform: obs.live.platform.clone(),
        container_creation_epoch: obs.live.container_creation_epoch,
        entrypoint: obs.live.entrypoint.clone(),
        args: obs.live.args.clone(),
        mounts: obs.live.mounts.clone(),
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
    let mut doc = serde_json::to_value(&att).unwrap();
    doc["contract"] = json!({ "in_container_paths": contract.in_container_paths });
    attestation::write_document(&p, &doc)
}

fn collect_params(args: &[String]) -> Result<serde_json::Value> {
    let mut obj = serde_json::Map::new();
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == "--param" {
            let (k, v) = args[i + 1].split_once('=').ok_or_else(|| {
                OuroError::InvalidArgs("--param must be key=value".into())
            })?;
            if k.is_empty() || obj.insert(k.to_string(), json!(v)).is_some() {
                return Err(OuroError::InvalidArgs(format!(
                    "duplicate or empty --param key {k:?}"
                )));
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    Ok(serde_json::Value::Object(obj))
}

fn validate_closed_args(
    args: &[String],
    value_flags: &[&str],
    bool_flags: &[&str],
    repeat_flags: &[&str],
) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        if bool_flags.contains(&name) {
            if !seen.insert(name) {
                return Err(OuroError::InvalidArgs(format!("duplicate flag {name}")));
            }
            index += 1;
            continue;
        }
        if !value_flags.contains(&name) {
            return Err(OuroError::InvalidArgs(format!("unexpected argument {name:?}")));
        }
        if !repeat_flags.contains(&name) && !seen.insert(name) {
            return Err(OuroError::InvalidArgs(format!("duplicate flag {name}")));
        }
        let value = args.get(index + 1).ok_or_else(|| {
            OuroError::InvalidArgs(format!("missing value for {name}"))
        })?;
        if value.starts_with("--") {
            return Err(OuroError::InvalidArgs(format!("missing value for {name}")));
        }
        index += 2;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{
        extract_embedded_probe, parse_cardano_cli_json, rotate_attestation_for_upgrade, ObsLive,
        Observation,
    };
    use crate::attestation::{AdoptionAttestation, ImmutableIdentity, ManagedState, Role, TypedMount};
    use crate::config::ConfigPaths;
    use crate::supervisor::SupervisorObservation;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn cardano_cli_kes_diagnostics_have_one_terminal_json_record() {
        let raw = b"\xe2\x9c\x93 Operational certificate's KES period is within the correct KES period interval\n\
                    \xe2\x9c\x93 The operational certificate counter agrees with the node protocol state\n\
                    {\"qKesCurrentKesPeriod\":10,\"qKesStartKesInterval\":9}";
        let parsed = parse_cardano_cli_json(raw, "test KES").unwrap();
        assert_eq!(parsed["qKesCurrentKesPeriod"], 10);

        let ambiguous = b"diagnostic\n{\"a\":1}\n{\"a\":2}";
        assert!(parse_cardano_cli_json(ambiguous, "test KES").is_err());
    }

    #[test]
    fn embedded_probe_extraction_does_not_follow_predictable_preplanted_symlink() {
        let root = std::env::temp_dir().join(format!(
            "ouro-probe-extract-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let victim = root.join("victim");
        std::fs::write(&victim, b"DO_NOT_EXECUTE_OR_REPLACE").unwrap();
        let old_predictable = root.join(format!("ouro-probe-{}", std::process::id()));
        std::fs::create_dir(&old_predictable).unwrap();
        std::os::unix::fs::symlink(&victim, old_predictable.join("ouro-probe.sh")).unwrap();

        let extracted = extract_embedded_probe(&root, b"SAFE_PROBE_BYTES").unwrap();
        assert_ne!(extracted.dir, old_predictable);
        assert_eq!(std::fs::read(&extracted.path).unwrap(), b"SAFE_PROBE_BYTES");
        assert_eq!(std::fs::read(&victim).unwrap(), b"DO_NOT_EXECUTE_OR_REPLACE");
        let metadata = std::fs::symlink_metadata(&extracted.path).unwrap();
        assert!(metadata.file_type().is_file() && !metadata.file_type().is_symlink());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        drop(extracted);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prior_image_recreate_rebases_attestation_to_the_new_container_identity() {
        let home = std::env::temp_dir().join(format!(
            "ouro-upgrade-rollback-attestation-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(home.join("attestations")).unwrap();
        let allow = crate::convention::Allowlist::load(&home, true).unwrap();
        let contract = &allow.contracts[0];
        let image = &contract.allowed[0];
        let mount = TypedMount {
            kind: "bind".into(), source_id: "8:1".into(), destination: "/data/db".into(),
            read_only: false, owner: "0:0".into(), mode: "0755".into(), no_symlink: true,
        };
        let old = AdoptionAttestation {
            immutable: ImmutableIdentity {
                role: Role::Bp,
                contract_id: contract.contract_id.clone(),
                convention_version: contract.convention_version,
                allowlist_version: allow.allowlist_version,
                allowlist_digest: allow.signed_digest().unwrap(),
                host_key_sha256: "a".repeat(64), machine_id: "bp1".into(),
                oci_index_digest: image.oci_index_digest.clone(),
                platform_manifest_digest: image.platform_manifest_digest.clone(),
                image_config_digest: image.image_config_digest.clone(),
                platform: image.platform.clone(), container_creation_epoch: 1000,
                entrypoint: vec!["cardano-node".into()], args: vec!["run".into()],
                mounts: vec![mount.clone()], network: "mainnet".into(), genesis_hash: "gh".into(),
                public_credential_ids: vec!["kes:5".into()], approval_evidence_hash: "approved".into(),
            },
            state: ManagedState {
                state_generation: 7, container_id: "old-cid".into(), topology_hash: "t".into(),
                config_hash: "c".into(), kes_opcert_id: "kes:5".into(),
            },
        };
        let path = home.join("attestations/bp1.json");
        let mut document = serde_json::to_value(&old).unwrap();
        document["contract"] = serde_json::json!({"in_container_paths": contract.in_container_paths});
        crate::attestation::write_document(&path, &document).unwrap();
        let observation = Observation {
            supervisor: SupervisorObservation {
                runtime: "docker".into(), rootful: true, rootless: false,
                node_container_count: 1, uses_bind_mounts: true,
                daemon_socket: "/var/run/docker.sock".into(), restart_policy: "unless-stopped".into(),
                orchestration: "run".into(),
            },
            live: ObsLive {
                image_config_digest: image.image_config_digest.clone(), platform: image.platform.clone(),
                container_id: "restored-new-cid".into(), container_creation_epoch: 2000,
                container_name: "cardano-node".into(), image_reference: "image:test".into(),
                entrypoint: vec!["cardano-node".into()], args: vec!["run".into()],
                image_entrypoint: vec!["cardano-node".into()], image_cmd: vec!["run".into()],
                mounts: vec![mount],
                topology_hash: "t".into(), config_hash: "c".into(), kes_opcert_id: "kes:5".into(),
                has_forging_keys: true, forging_key_permissions_safe: true,
                host_key_sha256: "a".repeat(64),
                genesis_hash: "gh".into(), network: "mainnet".into(),
            },
            readiness: None,
            recreate: None,
        };
        let paths = ConfigPaths {
            home: home.clone(), credentials_dir: home.join("credentials"),
            staging_dir: home.join("staging"), audit_db: home.join("audit.sqlite3"),
            confirmations: home.join("confirmations.json"), tool_run_secret: home.join("tool-run.secret"),
            known_hosts: home.join("known_hosts"), legacy_db: None,
        };
        rotate_attestation_for_upgrade(
            &paths, "bp1", false, &old, &observation, &image.image_config_digest,
        ).unwrap();
        let restored: AdoptionAttestation = serde_json::from_str(
            &crate::attestation::read_document(&path).unwrap(),
        ).unwrap();
        assert_eq!(restored.state.container_id, "restored-new-cid");
        assert_eq!(restored.immutable.container_creation_epoch, 2000);
        assert_eq!(restored.state.state_generation, 8);
        assert_ne!(restored.state.container_id, old.state.container_id);
        std::fs::remove_dir_all(&home).unwrap();
    }
}
