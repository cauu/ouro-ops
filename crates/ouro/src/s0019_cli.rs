//! S0019 p4-1 — CLI wiring for the greenfield model: `ouro-ops adopt` (the adoption ceremony) and
//! `ouro-ops op` (the intent pipeline). These integrate the p1–p3 mechanism modules into the two
//! commands the greenfield skills call. The website/agent interaction is unchanged: the agent
//! supplies PARAMETERS to these commands, never raw commands; every gate fires in order and refuses
//! before any mutation.
//!
//! Decision (recorded in the spec): S0019 uses a NEW `op` command rather than overloading the
//! the retired S0017 script executor. The dispatched
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
const RUNTIME_RESTART_READINESS_TIMEOUT_SECONDS: u64 = 300;
const RUNTIME_RESTART_READINESS_POLL_SECONDS: u64 = 1;

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
        return if exit == 0 {
            Ok(())
        } else {
            Err(OuroError::Reported(exit))
        };
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
    let detail =
        if exit == 0 {
            format!(
                "target returned no typed ToolOutput (bounded stdout: {})",
                if stdout.is_empty() {
                    "<empty>"
                } else {
                    &stdout
                }
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
/// `ouro-ops inbox preview`: hash and type-check a public control-local artifact without staging
/// bytes anywhere. S0020 apply reopens the same operator-named file, verifies this reference, and
/// streams it immediately after the ephemeral runner in one private invocation.
///
/// `ouro-ops inbox stage` remains the S0019 migration/target-wrapper surface. It is not required by
/// the S0020 ordinary flow.
pub fn run_inbox(args: &[String]) -> Result<()> {
    let action = args.first().map(String::as_str);
    if !matches!(action, Some("preview" | "stage")) {
        return Err(OuroError::InvalidArgs(
            "expected: ouro-ops inbox preview --type <opcert|tx> --file <path> | \
             ouro-ops inbox stage --type <opcert|tx> \
             (--file <path> [--dispatch <host>] | --stdin --local)"
                .into(),
        ));
    }
    let args = &args[1..];
    if action == Some("preview") {
        validate_closed_args(args, &["--type", "--file"], &[], &[])?;
        let kind = match flag(args, "--type")? {
            "opcert" => crate::inbox::ArtifactType::Opcert,
            "tx" => crate::inbox::ArtifactType::Tx,
            other => {
                return Err(OuroError::Validation(format!(
                    "--type must be opcert|tx, got {other}"
                )))
            }
        };
        let file = flag(args, "--file")?;
        let (_, preview) = crate::inbox::preview_source(kind, Path::new(file))?;
        output::print_json(&ToolOutput::ok("ouro.inbox.preview", false).with_data(json!({
            "artifact_type": kind,
            "artifact_ref": preview.artifact_ref,
            "size_bytes": preview.size_bytes,
            "source": "operator_named_control_file",
            "staged": false,
            "note": "no bytes were copied; pass this reference to --plan and the same file to apply --artifact-file",
        })))?;
        return Ok(());
    }
    validate_closed_args(
        args,
        &[
            "--type",
            "--file",
            "--dispatch",
            "--ssh-key",
            "--expect-ref",
        ],
        &["--stdin", "--local", "--plan"],
        &[],
    )?;
    let kind = match flag(args, "--type")? {
        "opcert" => crate::inbox::ArtifactType::Opcert,
        "tx" => crate::inbox::ArtifactType::Tx,
        other => {
            return Err(OuroError::Validation(format!(
                "--type must be opcert|tx, got {other}"
            )))
        }
    };
    let paths = ConfigPaths::discover();
    if let Some(host) = optional(args, "--dispatch") {
        if args.iter().any(|arg| arg == "--stdin" || arg == "--local") {
            return Err(OuroError::Validation(
                "control dispatch requires --file; --stdin/--local are target-wrapper only".into(),
            ));
        }
        let file = flag(args, "--file")?;
        let (mut source, preview) = crate::inbox::preview_source(kind, std::path::Path::new(file))?;
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
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| OuroError::Validation("inbox SSH dispatch has no stdin pipe".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| OuroError::Validation("inbox SSH dispatch has no stdout pipe".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| OuroError::Validation("inbox SSH dispatch has no stderr pipe".into()))?;
        let copy = std::thread::spawn(move || -> std::io::Result<u64> {
            let copied = std::io::copy(&mut source, &mut stdin)?;
            drop(stdin);
            Ok(copied)
        });
        let drain = |mut pipe: Box<dyn Read + Send>| {
            std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
                let mut bounded = Vec::new();
                pipe.by_ref()
                    .take((INBOX_OUTPUT_CAP + 1) as u64)
                    .read_to_end(&mut bounded)?;
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
        let stdout = stdout_drain
            .join()
            .map_err(|_| OuroError::Validation("target inbox stdout drain panicked".into()))??;
        let stderr = stderr_drain
            .join()
            .map_err(|_| OuroError::Validation("target inbox stderr drain panicked".into()))??;
        if stdout.len() > INBOX_OUTPUT_CAP || stderr.len() > INBOX_OUTPUT_CAP {
            return Err(OuroError::Validation(
                "target inbox output exceeded the bounded protocol limit".into(),
            ));
        }
        let copied = copy
            .join()
            .map_err(|_| OuroError::Validation("artifact transport worker panicked".into()))?;
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
        let accepted_ref = response
            .pointer("/data/artifact_ref")
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
    kes_rotation_repair_ready: bool,
    kes_rotation_permissions: KesRotationPermissionEvidence,
    image_config_digest: String,
    state_generation: u64,
}

/// Read one target's closed fleet facts through the same release-embedded ephemeral runner used by
/// plans and applies. No target-installed CLI, adoption metadata or remote Ouro version participates.
fn fetch_fleet_status(
    machine: &Machine,
    paths: &ConfigPaths,
    allowlist_digest: &str,
    release_policy: Option<&str>,
    network: &str,
    genesis: &str,
) -> Result<FleetLiveStatus> {
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let role = match machine.role {
        MachineRole::Bp => "bp",
        MachineRole::Relay => "relay",
    };
    let mut remote = vec![
        "target".into(),
        "status".into(),
        "--node".into(),
        machine.id.clone(),
        "--role".into(),
        role.into(),
        "--network".into(),
        network.into(),
        "--genesis".into(),
        genesis.into(),
        "--expect-allowlist".into(),
        allowlist_digest.into(),
    ];
    if let Some(document) = release_policy {
        remote.push("--release-policy".into());
        remote.push(document.into());
    }
    let runner = crate::runner::linux_x86_64()?;
    let argv = crate::dispatch::ephemeral_runner_dispatch_argv(
        &machine.ssh.host,
        machine.ssh.port,
        &machine.ssh.user,
        &key,
        &paths.known_hosts,
        &runner.sha256,
        &remote,
    )?;
    let result = crate::ssh::bounded_ssh_with_input(
        &argv,
        &runner.bytes,
        std::time::Duration::from_secs(45),
        256 * 1024,
        "ephemeral fleet live-facts SSH",
    )
    .map_err(|e| {
        OuroError::Validation(format!(
            "fleet live-facts SSH failed for {}: {e}",
            machine.id
        ))
    })?;
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
            if detail.is_empty() {
                "<no diagnostic>"
            } else {
                &detail
            }
        )));
    }
    let value: serde_json::Value =
        serde_json::from_slice(result.stdout.as_bytes()).map_err(|error| {
            OuroError::Validation(format!(
                "fleet live-facts target {} returned malformed JSON: {error} (bounded DATA: {})",
                machine.id,
                bounded(result.stdout.as_bytes())
            ))
        })?;
    if value.get("tool").and_then(serde_json::Value::as_str) != Some("ouro.fleet.status")
        || value.get("status").and_then(serde_json::Value::as_str) != Some("ok")
        || value.get("changed").and_then(serde_json::Value::as_bool) != Some(false)
        || value
            .pointer("/data/node")
            .and_then(serde_json::Value::as_str)
            != Some(machine.id.as_str())
    {
        return Err(OuroError::Validation(format!(
            "fleet live-facts target {} returned an unexpected typed record (bounded DATA: {})",
            machine.id,
            bounded(result.stdout.as_bytes())
        )));
    }
    let result = value.pointer("/data").ok_or_else(|| {
        OuroError::Validation(format!(
            "fleet live-facts target {} omitted data",
            machine.id
        ))
    })?;
    let role = result
        .get("role")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "fleet live-facts target {} omitted role",
                machine.id
            ))
        })?;
    let node = result
        .get("node")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if node != machine.id
        || role
            != match machine.role {
                MachineRole::Bp => "bp",
                MachineRole::Relay => "relay",
            }
    {
        return Err(OuroError::Validation(format!(
            "fleet live-facts identity mismatch for {}: target reported node={node:?} role={role:?}",
            machine.id
        )));
    }
    let network = result
        .get("network")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "fleet live-facts target {} omitted network",
                machine.id
            ))
        })?;
    let genesis_hash = result
        .get("genesis_hash")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "fleet live-facts target {} omitted genesis_hash",
                machine.id
            ))
        })?;
    let host_key_sha256 = result
        .get("host_key_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "fleet live-facts target {} omitted host_key_sha256",
                machine.id
            ))
        })?;
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
    let valid_image = image
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.chars().all(|ch| ch.is_ascii_hexdigit()));
    if !valid_image {
        return Err(OuroError::Validation(format!(
            "fleet live-facts target {} returned an invalid image digest",
            machine.id
        )));
    }
    Ok(FleetLiveStatus {
        node: node.into(),
        role: role.into(),
        network: network.into(),
        genesis_hash: genesis_hash.into(),
        host_key_sha256: host_key_sha256.into(),
        online: result
            .get("online")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| {
                OuroError::Validation(format!(
                    "fleet live-facts target {} omitted online",
                    machine.id
                ))
            })?,
        // Only the operation-scoped KES permit consumes this additional qualification. Missing
        // evidence is false so a mixed or malformed runner can never weaken ordinary admission.
        kes_rotation_repair_ready: result
            .get("kes_rotation_repair_ready")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        kes_rotation_permissions: KesRotationPermissionEvidence {
            keys_directory_safe: result
                .get("keys_directory_safe")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            kes_skey_private: result
                .get("kes_skey_private")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            vrf_skey_private: result
                .get("vrf_skey_private")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        },
        image_config_digest: image.into(),
        state_generation: result
            .get("state_generation")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                OuroError::Validation(format!(
                    "fleet live-facts target {} omitted state_generation",
                    machine.id
                ))
            })?,
    })
}

fn fetch_kes_protocol_evidence(
    machine: &Machine,
    paths: &ConfigPaths,
    network: &str,
    genesis: &str,
    artifact_path: &Path,
) -> Result<crate::fleet::KesProtocolEvidence> {
    if machine.role != MachineRole::Relay {
        return Err(OuroError::Validation(
            "KES protocol evidence source is not a declared relay".into(),
        ));
    }
    let (file, preview) =
        crate::inbox::preview_source(crate::inbox::ArtifactType::Opcert, artifact_path)?;
    let digest = artifact_ref_digest(&preview.artifact_ref)
        .ok_or_else(|| OuroError::Validation("KES opcert reference lost its digest".into()))?;
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let remote = vec![
        "target".into(),
        "kes-protocol".into(),
        "--node".into(),
        machine.id.clone(),
        "--role".into(),
        "relay".into(),
        "--network".into(),
        network.into(),
        "--genesis".into(),
        genesis.into(),
    ];
    let runner = crate::runner::linux_x86_64()?;
    let argv = crate::dispatch::ephemeral_runner_payload_dispatch_argv(
        &machine.ssh.host,
        machine.ssh.port,
        &machine.ssh.user,
        &key,
        &paths.known_hosts,
        crate::dispatch::EphemeralPayloadInput {
            runner_sha256: &runner.sha256,
            runner_size: runner.bytes.len(),
            payload_sha256: digest,
            payload_size: preview.size_bytes,
        },
        &remote,
    )?;
    let result = crate::ssh::bounded_ssh_with_payload(
        &argv,
        &runner.bytes,
        file,
        std::time::Duration::from_secs(5 * 60),
        256 * 1024,
        "ephemeral relay KES protocol evidence SSH",
    )
    .map_err(|error| {
        OuroError::Validation(format!(
            "KES protocol evidence SSH failed for {}: {error}",
            machine.id
        ))
    })?;
    if result.status != 0 {
        return Err(OuroError::Validation(format!(
            "relay {} refused KES protocol evidence (exit {}): {}",
            machine.id,
            result.status,
            String::from_utf8_lossy(result.stderr.as_bytes())
                .chars()
                .take(DISPATCH_DIAGNOSTIC_CAP)
                .collect::<String>()
        )));
    }
    let value: serde_json::Value =
        serde_json::from_slice(result.stdout.as_bytes()).map_err(|error| {
            OuroError::Validation(format!(
                "relay {} returned malformed KES protocol evidence: {error}",
                machine.id
            ))
        })?;
    if value.get("tool").and_then(serde_json::Value::as_str) != Some("ouro.kes.protocol_evidence")
        || value.get("status").and_then(serde_json::Value::as_str) != Some("ok")
        || value.get("changed").and_then(serde_json::Value::as_bool) != Some(false)
    {
        return Err(OuroError::Validation(format!(
            "relay {} returned an unexpected KES protocol record",
            machine.id
        )));
    }
    let evidence: crate::fleet::KesProtocolEvidence = serde_json::from_value(
        value
            .pointer("/data/evidence")
            .cloned()
            .ok_or_else(|| OuroError::Validation("relay omitted KES protocol evidence".into()))?,
    )
    .map_err(|error| {
        OuroError::Validation(format!("relay KES protocol evidence is malformed: {error}"))
    })?;
    if evidence.relay_node != machine.id || evidence.artifact_sha256 != digest {
        return Err(OuroError::Validation(
            "relay KES protocol evidence does not bind the requested artifact/source".into(),
        ));
    }
    Ok(evidence)
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
        "--artifact-file",
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
    let bp = spec
        .machines
        .iter()
        .find(|machine| machine.role == MachineRole::Bp)
        .ok_or_else(|| OuroError::Validation("pool spec has no block producer identity".into()))?;
    // Stable v1 pool namespace: network/genesis + immutable logical BP id. Full spec digest is a
    // separate revision binding; changing node_version/metadata/SSH endpoints must not create an
    // independent lease namespace that could bypass single-writer quorum arbitration.
    let stable = format!(
        "{}\n{}\n{}",
        spec.pool.network.as_str(),
        spec.pool.genesis_hashes.shelley,
        bp.id
    );
    let stable_hash = crate::intent::sha256_hex(stable.as_bytes());
    let pool_id = format!("pool-{}", &stable_hash[..24]);
    Ok((digest, pool_id))
}

fn require_fleet_live_facts_fresh(facts_epoch: u64, now: u64) -> Result<()> {
    if facts_epoch > now.saturating_add(5)
        || now.saturating_sub(facts_epoch) > crate::fleet::LIVE_FACTS_VALIDITY_SECONDS
    {
        return Err(OuroError::Validation(format!(
            "fleet live-facts collection exceeded the shared {}-second authorization window — retry",
            crate::fleet::LIVE_FACTS_VALIDITY_SECONDS,
        )));
    }
    Ok(())
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
        output::print_json(
            &ToolOutput::ok("ouro.fleet.spec.identity", false).with_data(json!({
                "pool_spec_digest": pool_spec_digest,
                "pool_id": pool_id,
                "network": spec.pool.network.as_str(),
                "genesis_hash": spec.pool.genesis_hashes.shelley,
                "machines": spec.machines.iter().map(|machine| &machine.id).collect::<Vec<_>>(),
                "min_online_relays": spec.upgrade.min_online_relays,
            })),
        )?;
        return Ok(());
    }
    if args.first().map(String::as_str) != Some("permit")
        || args.get(1).map(String::as_str) != Some("create")
    {
        return Err(OuroError::InvalidArgs(
            "expected: ouro-ops fleet permit create --spec <pool-spec> --node <id> --op <id> \
             --intent-hash <final-plan-hash> --holder <id> \
             [--target-image sha256:<digest>] [--artifact-file <public-node.cert>]"
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
    let artifact_file = optional(args, "--artifact-file");
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
    if operation == "kes-rotation/install-opcert" && artifact_file.is_none() {
        return Err(OuroError::Validation(
            "KES rotation permit requires --artifact-file with the reviewed public node.cert"
                .into(),
        ));
    }
    if operation != "kes-rotation/install-opcert" && artifact_file.is_some() {
        return Err(OuroError::Validation(
            "--artifact-file is accepted only for kes-rotation/install-opcert fleet permits".into(),
        ));
    }
    if !fleet_operation
        .touched
        .iter()
        .any(|resource| matches!(*resource, "container:restart" | "container:recreate"))
    {
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
    let target = spec
        .machines
        .iter()
        .find(|machine| machine.id == node)
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "fleet target {node} is not declared in the pool spec"
            ))
        })?;
    let role = match target.role {
        MachineRole::Bp => "bp",
        MachineRole::Relay => "relay",
    };
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
        let valid = image
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == 64 && hex.chars().all(|ch| ch.is_ascii_hexdigit()));
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
    let release_catalog = if operation == "upgrade/step" {
        Some(convention::fetch_release_catalog()?)
    } else {
        None
    };
    let allowlist_digest = match &release_catalog {
        Some(catalog) => catalog.policy.signed_digest()?,
        None => convention::Allowlist::embedded()?.signed_digest()?,
    };
    let facts_epoch = crate::s0019_confirmation::current_epoch()?;
    let mut statuses = Vec::with_capacity(spec.machines.len());
    for machine in &spec.machines {
        statuses.push(fetch_fleet_status(
            machine,
            &paths,
            &allowlist_digest,
            release_catalog
                .as_ref()
                .map(|catalog| catalog.document.as_str()),
            spec.pool.network.as_str(),
            &spec.pool.genesis_hashes.shelley,
        )?);
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
    let target_status = statuses
        .iter()
        .find(|status| status.node == node)
        .ok_or_else(|| {
            OuroError::Validation(format!("fleet live-facts snapshot omitted target {node}"))
        })?;
    let target_qualified = if operation == "kes-rotation/install-opcert" {
        role == "bp"
            && target_status.kes_rotation_repair_ready
            && target_status.kes_rotation_permissions.ready()
    } else {
        target_status.online
    };
    if !target_qualified {
        let qualification = if operation == "kes-rotation/install-opcert" {
            "KES rotation repair readiness"
        } else {
            "full role readiness"
        };
        return Err(OuroError::Validation(format!(
            "fleet target {node} does not satisfy {qualification} — permit refused"
        )));
    }
    let online_relays = u32::try_from(
        statuses
            .iter()
            .filter(|status| status.role == "relay" && status.online)
            .count(),
    )
    .map_err(|_| OuroError::Validation("relay count exceeds supported range".into()))?;
    let relays_remaining = if let Some(image) = target_image {
        u32::try_from(
            statuses
                .iter()
                .filter(|status| status.role == "relay" && status.image_config_digest != image)
                .count(),
        )
        .map_err(|_| OuroError::Validation("relay count exceeds supported range".into()))?
    } else {
        0
    };
    crate::fleet::require_quorum(online_relays, min_online_relays, role == "relay")?;
    crate::fleet::require_bp_last(role == "bp", relays_remaining)?;
    let kes_protocol_evidence = if operation == "kes-rotation/install-opcert" {
        let relay = spec
            .machines
            .iter()
            .find(|machine| {
                machine.role == MachineRole::Relay
                    && statuses
                        .iter()
                        .any(|status| status.node == machine.id && status.online)
            })
            .ok_or_else(|| {
                OuroError::Validation(
                    "KES restart-loop repair requires one declared healthy relay for protocol evidence"
                        .into(),
                )
            })?;
        Some(fetch_kes_protocol_evidence(
            relay,
            &paths,
            expected_network,
            expected_genesis,
            Path::new(artifact_file.expect("KES artifact required above")),
        )?)
    } else {
        None
    };
    let ttl_seconds = crate::fleet::LIVE_FACTS_VALIDITY_SECONDS;
    let now = crate::s0019_confirmation::current_epoch()?;
    require_fleet_live_facts_fresh(facts_epoch, now)?;
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
        relay_health_endpoints: spec
            .machines
            .iter()
            .filter_map(|machine| {
                let ready = statuses.iter().any(|status| {
                    status.node == machine.id && status.role == "relay" && status.online
                });
                if machine.role != MachineRole::Relay || !ready {
                    return None;
                }
                machine
                    .public_endpoint
                    .as_ref()
                    .map(|endpoint| crate::fleet::RelayHealthEndpoint {
                        node_id: machine.id.clone(),
                        host: endpoint.host.clone(),
                        port: endpoint.port,
                    })
            })
            .collect(),
        kes_protocol_evidence: kes_protocol_evidence.clone(),
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
            "valid_until_epoch": facts_epoch.saturating_add(crate::fleet::LIVE_FACTS_VALIDITY_SECONDS),
            "online_relays": online_relays,
            "min_online_relays": min_online_relays,
            "relays_remaining": relays_remaining,
            "target_role": role,
            "target_online": target_status.online,
            "target_kes_rotation_repair_ready": target_status.kes_rotation_repair_ready,
            "target_kes_rotation_permissions": target_status.kes_rotation_permissions,
            "target_qualification": if operation == "kes-rotation/install-opcert" {
                "kes_rotation_repair_ready"
            } else {
                "full_role_readiness"
            },
            "target_state_generation": target_status.state_generation,
            "target_host_key_sha256": target_status.host_key_sha256,
            "target_image": target_status.image_config_digest,
            "kes_protocol_evidence": kes_protocol_evidence,
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
        "adopt",
        "live_preflight",
        "intent_approval",
        "prepared",
        "committing",
        "committed",
        "verifying",
        "verified",
        "rolling_back",
        "rolled_back",
        "sealed",
        "recovery",
        "attestation_rotation",
        "refusal",
        "apply_attempt",
        "apply_succeeded",
        "apply_failed",
        "apply_rolled_back",
        "apply_ambiguous",
    ];
    const EXTRA_FIELDS: &[&str] = &[
        "operation_id",
        "intent_hash",
        "approval_evidence_hash",
        "pre_state_generation",
        "post_state_generation",
        "fencing_token",
        "outcome",
        "refusal_code",
    ];
    if !EVENTS.contains(&event) {
        return Err(OuroError::Validation(format!(
            "unknown audit event {event:?}"
        )));
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
    ev.insert(
        "at_epoch".into(),
        json!(crate::s0019_confirmation::current_epoch()?),
    );
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
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options
        .open(&path)
        .map_err(|error| OuroError::Validation(format!("audit open: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| OuroError::Validation(format!("audit metadata: {error}")))?;
    if !metadata.file_type().is_file() {
        return Err(OuroError::Validation(
            "audit destination is not a regular file".into(),
        ));
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
    tx_dir(paths)
        .join("adoption-pending")
        .join(format!("{node}.pending"))
}

fn begin_adoption_commit(paths: &ConfigPaths, node: &str, candidate: &str) -> Result<PathBuf> {
    let path = adoption_pending_path(paths, node);
    let parent = path
        .parent()
        .ok_or_else(|| OuroError::Validation("invalid adoption journal path".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
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
#[derive(serde::Deserialize, Clone, PartialEq)]
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
    #[serde(default)]
    kes: Option<ObsKes>,
    #[serde(default)]
    block_producer_configured: bool,
    forging_credentials_ready: bool,
    established_peers: u32,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, PartialEq)]
struct ObsKes {
    #[serde(default)]
    source: Option<String>,
    current_period: i64,
    start_period: i64,
    end_period: i64,
    remaining_periods: i64,
    #[serde(default)]
    opcert_counter_on_disk: Option<i64>,
    #[serde(default)]
    opcert_counter_node_state: Option<i64>,
    #[serde(default)]
    counter_consistent: Option<bool>,
    #[serde(default)]
    counter_status: Option<String>,
    #[serde(default)]
    period_valid: Option<bool>,
    valid: bool,
}

/// The only S0020 target-side read entry point. It is deliberately closed and stateless: the
/// ephemeral runner gathers one live observation and does not consult attestation, Ouro home,
/// transaction journals, allowlist floors or an installed target version.
pub fn run_target(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("observe") => {
            let args = &args[1..];
            validate_closed_args(args, &["--node", "--op", "--role"], &[], &[])?;
            let node = flag(args, "--node")?;
            crate::intent::validate_machine_id(node)?;
            let observation = read_observation(&[])?;
            match optional(args, "--op").unwrap_or("observability/health") {
                "observability/health" => {
                    if optional(args, "--role").is_some() {
                        return Err(OuroError::Validation(
                            "observability target read does not accept a role".into(),
                        ));
                    }
                    output::print_json(&stateless_observation_output(node, &observation))
                }
                "troubleshooting/snapshot" => {
                    let role = parse_target_role(flag(args, "--role")?)?;
                    output::print_json(&stateless_troubleshooting_output(node, role, &observation))
                }
                op => Err(OuroError::Validation(format!(
                    "target observe does not support operation {op:?}"
                ))),
            }
        }
        Some("plan") => run_stateless_target_plan(&args[1..]),
        Some("preflight") => run_stateless_target_artifact_preflight(&args[1..]),
        Some("kes-protocol") => run_kes_protocol_evidence(&args[1..]),
        Some("apply") => run_stateless_target_apply(&args[1..]),
        Some("status") => run_stateless_target_status(&args[1..]),
        _ => Err(OuroError::InvalidArgs(
            "expected internal target observe|plan|preflight|kes-protocol|apply|status with closed arguments"
                .into(),
        )),
    }
}

fn run_kes_protocol_evidence(args: &[String]) -> Result<()> {
    validate_closed_args(
        args,
        &["--node", "--role", "--network", "--genesis"],
        &[],
        &[],
    )?;
    if flag(args, "--role")? != "relay" {
        return Err(OuroError::Validation(
            "KES protocol evidence must come from a declared relay".into(),
        ));
    }
    let node = flag(args, "--node")?;
    crate::intent::validate_machine_id(node)?;
    let network = flag(args, "--network")?;
    let genesis = flag(args, "--genesis")?;
    let observation = read_observation(&[])?;
    observation.supervisor.require_base_conformant()?;
    require_typed_mounts(&observation.live.mounts)?;
    let contract = convention::Allowlist::stable_contract()?;
    require_adoption_contract(&contract, &observation, network, genesis, None, None)?;
    if observation.live.has_forging_keys {
        return Err(OuroError::Validation(
            "KES protocol evidence relay unexpectedly bears forging keys".into(),
        ));
    }
    let readiness = observation.readiness.as_ref().ok_or_else(|| {
        OuroError::Validation("KES protocol relay omitted readiness evidence".into())
    })?;
    crate::readiness::Readiness {
        role: Role::Relay,
        node_running: readiness.node_running,
        container_id_matches: !observation.live.container_id.is_empty(),
        socket_answers: readiness.socket_answers,
        network_ok: observation.live.network == network,
        genesis_ok: observation.live.genesis_hash == genesis,
        tip_block: readiness.tip_block,
        tip_block_next: readiness.tip_block_next,
        tip_synced: readiness.tip_synced,
        kes_opcert_valid: false,
        forging_credentials_ready: false,
        established_peers: readiness.established_peers,
    }
    .evaluate()?;
    let path = std::env::var_os("OURO_EPHEMERAL_PAYLOAD")
        .map(PathBuf::from)
        .ok_or_else(|| {
            OuroError::Validation(
                "KES protocol evidence requires the sealed public opcert payload".into(),
            )
        })?;
    let (_file, preview) = crate::inbox::preview_source(crate::inbox::ArtifactType::Opcert, &path)?;
    let bytes = std::fs::read(&path)?;
    let parsed = crate::kes::parse_operational_certificate(&bytes)?;
    let mut query = vec![
        "query",
        "kes-period-info",
        "--socket-path",
        "/ipc/node.socket",
        "--op-cert-file",
        "/dev/stdin",
        "--output-json",
    ];
    match network {
        "mainnet" => query.push("--mainnet"),
        "preprod" => query.extend(["--testnet-magic", "1"]),
        "preview" => query.extend(["--testnet-magic", "2"]),
        _ => {
            return Err(OuroError::Validation(format!(
                "unsupported KES network {network}"
            )))
        }
    }
    let output = docker_cardano_cli_with_tx(
        &observation.live.container_id,
        &query,
        &bytes,
        "relay KES protocol query",
    )?;
    if !output.status.success() {
        return Err(OuroError::Validation(format!(
            "relay rejected KES protocol query: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2048)
                .collect::<String>()
        )));
    }
    let facts = parse_cardano_cli_json(&output.stdout, "relay cardano-cli KES result")?;
    let current_period = json_u64(&facts, "qKesCurrentKesPeriod")?;
    let start_period = json_u64(&facts, "qKesStartKesInterval")?;
    let end_period = json_u64(&facts, "qKesEndKesInterval")?;
    let on_disk_counter = json_u64(&facts, "qKesOnDiskOperationalCertificateNumber")?;
    let node_state = node_state_counter(&facts)?;
    if parsed.counter != on_disk_counter
        || parsed.kes_period != start_period
        || current_period < start_period
        || current_period >= end_period
    {
        return Err(OuroError::Validation(
            "relay protocol evidence rejects the candidate counter/KES window".into(),
        ));
    }
    if let NodeStateCounterEvidence::Present(value) = node_state {
        if on_disk_counter < value || on_disk_counter > value.saturating_add(1) {
            return Err(OuroError::Validation(
                "relay protocol evidence rejects the candidate node-state counter".into(),
            ));
        }
    }
    let evidence = crate::fleet::KesProtocolEvidence {
        artifact_sha256: crate::intent::sha256_hex(&bytes),
        relay_node: node.into(),
        current_period,
        start_period,
        end_period,
        on_disk_counter,
        node_state_counter: node_state.value(),
        node_state_counter_status: node_state.status().into(),
    };
    output::print_json(
        &ToolOutput::ok("ouro.kes.protocol_evidence", false).with_data(json!({
            "artifact_ref": preview.artifact_ref,
            "evidence": evidence,
            "source": "declared_healthy_relay_socket",
            "persistent_target_state_written": false,
        })),
    )
}

fn parse_target_role(value: &str) -> Result<Role> {
    match value {
        "bp" => Ok(Role::Bp),
        "relay" => Ok(Role::Relay),
        _ => Err(OuroError::Validation(format!(
            "target observe role must be bp|relay, got {value:?}"
        ))),
    }
}

struct StatelessTargetPlan {
    output: ToolOutput,
    op: String,
    node: String,
    role: Role,
    network: String,
    genesis: String,
    candidate_hash: String,
    intent: Intent,
    observation: Observation,
    policy: convention::Allowlist,
    kes_rotation: Option<KesRotationEvidence>,
}

#[derive(Clone, serde::Serialize)]
struct KesRotationEvidence {
    current_period: Option<u64>,
    cardano_cli_version: Option<String>,
    active_vkey_sha256: String,
    staged_vkey_sha256: Option<String>,
    staged_vkey: Option<serde_json::Value>,
    pending_existing: bool,
    preexisting_kes_opcert_valid: bool,
    preexisting_forging_credentials_ready: bool,
    preexisting_kes_evidence_sha256: String,
    permissions: KesRotationPermissionEvidence,
    activation_pending: bool,
    activation_promoted: bool,
    previous_vkey_sha256: Option<String>,
    previous_opcert_sha256: Option<String>,
    restart_loop_repair: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
struct KesRotationPermissionEvidence {
    keys_directory_safe: bool,
    kes_skey_private: bool,
    vrf_skey_private: bool,
}

impl KesRotationPermissionEvidence {
    fn from_live(live: &ObsLive) -> Self {
        Self {
            keys_directory_safe: live.keys_directory_safe,
            kes_skey_private: live.kes_skey_private,
            vrf_skey_private: live.vrf_skey_private,
        }
    }

    fn ready(&self) -> bool {
        self.keys_directory_safe && self.kes_skey_private && self.vrf_skey_private
    }
}

fn require_kes_rotation_permissions(live: &ObsLive) -> Result<KesRotationPermissionEvidence> {
    let evidence = KesRotationPermissionEvidence::from_live(live);
    if !evidence.ready() {
        let facts = serde_json::to_string(&evidence).map_err(|error| {
            OuroError::Validation(format!(
                "cannot serialize KES rotation permission evidence: {error}"
            ))
        })?;
        return Err(OuroError::Validation(format!(
            "KES rotation permission contract failed: {facts}"
        )));
    }
    Ok(evidence)
}

fn current_kes_period(observation: &Observation) -> Result<u64> {
    let current = observation
        .readiness
        .as_ref()
        .and_then(|readiness| readiness.kes.as_ref())
        .map(|kes| kes.current_period)
        .ok_or_else(|| {
            OuroError::Validation(
                "BP observation did not provide a typed current KES period".into(),
            )
        })?;
    u64::try_from(current).map_err(|_| {
        OuroError::Validation("BP observation returned a negative current KES period".into())
    })
}

fn cardano_cli_version(container: &str) -> Result<String> {
    let raw = crate::executor::run_read_plan(&[vec![
        "docker".into(),
        "exec".into(),
        container.into(),
        "cardano-cli".into(),
        "--version".into(),
    ]])
    .map_err(|error| {
        OuroError::Validation(format!(
            "cannot obtain the BP container cardano-cli version: {error}"
        ))
    })?;
    let first = raw.lines().next().unwrap_or("").trim();
    let mut words = first.split_whitespace();
    if words.next() != Some("cardano-cli") {
        return Err(OuroError::Validation(format!(
            "BP container returned an invalid cardano-cli version line: {first:?}"
        )));
    }
    let version = words.next().unwrap_or("");
    let components = version.split('.').collect::<Vec<_>>();
    if components.len() != 4
        || components
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(OuroError::Validation(format!(
            "BP container returned an unsupported cardano-cli version: {version:?}"
        )));
    }
    Ok(version.to_string())
}

fn require_kes_stage_base_availability(observation: &Observation) -> Result<()> {
    let readiness = observation.readiness.as_ref().ok_or_else(|| {
        OuroError::Validation("BP observation did not provide typed readiness evidence".into())
    })?;
    if !readiness.node_running || !readiness.socket_answers {
        return Err(OuroError::Validation(
            "KES staging requires the existing BP container and node socket to answer".into(),
        ));
    }
    Ok(())
}

fn kes_preexisting_evidence(observation: &Observation) -> Result<(bool, bool, String)> {
    let readiness = observation.readiness.as_ref().ok_or_else(|| {
        OuroError::Validation("BP observation did not provide typed readiness evidence".into())
    })?;
    let bytes = serde_json::to_vec(&readiness.kes).map_err(|error| {
        OuroError::Validation(format!(
            "cannot bind typed pre-existing KES evidence: {error}"
        ))
    })?;
    Ok((
        readiness.kes_opcert_valid,
        readiness.forging_credentials_ready,
        crate::intent::sha256_hex(&bytes),
    ))
}

/// Phase A intentionally does not require the old KES/opcert to be valid: expiry is a primary
/// reason to rotate it. It proves staging did not worsen or alter the bound active state. Moving
/// tip/peer counters are excluded; KES period/counter/validity and availability must be identical.
fn require_kes_stage_readiness_invariant(before: &Observation, after: &Observation) -> Result<()> {
    let before = before.readiness.as_ref().ok_or_else(|| {
        OuroError::Validation("KES stage candidate lost its pre-state readiness evidence".into())
    })?;
    let after = after.readiness.as_ref().ok_or_else(|| {
        OuroError::Validation("KES stage postcondition omitted readiness evidence".into())
    })?;
    if !after.node_running || !after.socket_answers {
        return Err(OuroError::Validation(
            "KES staging changed BP container/socket availability".into(),
        ));
    }
    if before.node_running != after.node_running
        || before.socket_answers != after.socket_answers
        || before.tip_synced != after.tip_synced
        || before.kes_opcert_valid != after.kes_opcert_valid
        || before.block_producer_configured != after.block_producer_configured
        || before.forging_credentials_ready != after.forging_credentials_ready
        || before.kes != after.kes
    {
        return Err(OuroError::Validation(
            "KES staging changed active BP readiness/KES evidence".into(),
        ));
    }
    Ok(())
}

fn pending_staged_kes_pair(container: &str) -> Result<Option<(serde_json::Value, String)>> {
    if let Ok(metadata) =
        crate::executor::fixed_path_metadata(container, crate::executor::KES_STAGE_DIR)
    {
        return match metadata {
            None => Ok(None),
            Some(metadata) if metadata.is_dir => inspect_staged_kes_pair(container).map(Some),
            Some(_) => Err(OuroError::Validation(
                "an existing staged KES path is not a real directory; it was not changed".into(),
            )),
        };
    }
    let absent = crate::executor::run_argv(&[
        "docker".into(),
        "exec".into(),
        container.into(),
        "test".into(),
        "!".into(),
        "-e".into(),
        crate::executor::KES_STAGE_DIR.into(),
    ])
    .is_ok();
    if absent {
        return Ok(None);
    }
    inspect_staged_kes_pair(container).map(Some).map_err(|error| {
        OuroError::Validation(format!(
            "an existing staged KES rotation is incomplete or unsafe to resume; it was not changed ({error})"
        ))
    })
}

fn pending_kes_activation(container: &str) -> Result<Option<(String, String)>> {
    let paths = [
        crate::executor::KES_SKEY_PREVIOUS,
        crate::executor::KES_VKEY_PREVIOUS,
        crate::executor::OPCERT_PREVIOUS,
    ];
    let static_present = paths.map(|path| {
        crate::executor::fixed_path_metadata(container, path)
            .ok()
            .flatten()
            .is_some_and(|metadata| metadata.is_file && metadata.size > 0)
    });
    let static_resolved = paths
        .iter()
        .all(|path| crate::executor::fixed_path_metadata(container, path).is_ok());
    let present = if static_resolved {
        static_present
    } else {
        paths.map(|path| {
            crate::executor::run_argv(&[
                "docker".into(),
                "exec".into(),
                container.into(),
                "test".into(),
                "-s".into(),
                path.into(),
            ])
            .is_ok()
        })
    };
    if present.iter().all(|value| !value) {
        return Ok(None);
    }
    if !present.iter().all(|value| *value) {
        return Err(OuroError::Validation(
            "incomplete KES activation recovery material exists; automatic activation/resume refused"
                .into(),
        ));
    }
    let (_, previous_vkey_sha256) = read_public_kes_vkey(
        container,
        crate::executor::KES_VKEY_PREVIOUS,
        "previous public KES verification key retained by KES activation",
    )?;
    let (_, previous_opcert_sha256) = read_public_opcert(
        container,
        crate::executor::OPCERT_PREVIOUS,
        "previous public node.cert retained by KES activation",
    )?;
    Ok(Some((previous_vkey_sha256, previous_opcert_sha256)))
}

fn read_public_kes_vkey(
    container: &str,
    path: &str,
    context: &str,
) -> Result<(serde_json::Value, String)> {
    let raw = crate::executor::run_read_plan(&[vec![
        "docker".into(),
        "exec".into(),
        container.into(),
        "head".into(),
        "-c".into(),
        "65537".into(),
        path.into(),
    ]])
    .map(|raw| raw.into_bytes())
    .or_else(|_| crate::executor::read_fixed_public_file(container, path, 65_536))
    .map_err(|error| OuroError::Validation(format!("cannot read {context}: {error}")))?;
    if raw.len() > 65_536 {
        return Err(OuroError::Validation(format!("{context} exceeds 64 KiB")));
    }
    let public_key = crate::kes::parse_kes_verification_key(&raw)?;
    let value: serde_json::Value = serde_json::from_slice(&raw).map_err(|error| {
        OuroError::Validation(format!("{context} is not a JSON text envelope: {error}"))
    })?;
    let digest = crate::intent::sha256_hex(&public_key);
    Ok((value, digest))
}

fn inspect_staged_kes_pair(container: &str) -> Result<(serde_json::Value, String)> {
    let exec_present = crate::executor::run_argv(&[
        "docker".into(),
        "exec".into(),
        container.into(),
        "test".into(),
        "-s".into(),
        crate::executor::KES_STAGE_SKEY.into(),
    ])
    .is_ok();
    let static_metadata = if exec_present {
        None
    } else {
        crate::executor::fixed_path_metadata(container, crate::executor::KES_STAGE_SKEY)?
    };
    if !exec_present
        && !static_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.is_file && metadata.size > 0)
    {
        return Err(OuroError::Validation(
            "no complete staged KES signing key exists on the BP; run kes-rotation/stage-key first"
                .into(),
        ));
    }
    let mode = crate::executor::run_read_plan(&[vec![
        "docker".into(),
        "exec".into(),
        container.into(),
        "stat".into(),
        "-c".into(),
        "%a".into(),
        crate::executor::KES_STAGE_SKEY.into(),
    ]])
    .ok()
    .map(|mode| mode.trim().to_string())
    .or_else(|| static_metadata.map(|metadata| format!("{:o}", metadata.mode)))
    .ok_or_else(|| {
        OuroError::Validation("cannot inspect staged KES signing-key permissions".into())
    })?;
    if mode != "600" {
        return Err(OuroError::Validation(format!(
            "staged KES signing key must have mode 600, got {:?}",
            mode
        )));
    }
    read_public_kes_vkey(
        container,
        crate::executor::KES_STAGE_VKEY,
        "staged public KES verification key",
    )
}

fn run_stateless_target_plan(args: &[String]) -> Result<()> {
    let plan = build_stateless_target_plan(args)?;
    output::print_json(&plan.output)
}

fn docker_cardano_cli_with_tx(
    container: &str,
    args: &[&str],
    bytes: &[u8],
    context: &str,
) -> Result<std::process::Output> {
    let mut child = std::process::Command::new("docker")
        .args(["exec", "-i", container, "cardano-cli"])
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| OuroError::Validation(format!("cannot start {context}: {error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| OuroError::Validation(format!("{context} has no stdin")))?
        .write_all(bytes)
        .map_err(|error| {
            OuroError::Validation(format!("cannot stream transaction to {context}: {error}"))
        })?;
    let output = child.wait_with_output()?;
    if output.stdout.len() > 256 * 1024 || output.stderr.len() > 256 * 1024 {
        return Err(OuroError::Validation(format!(
            "{context} output exceeds the 256 KiB review limit"
        )));
    }
    Ok(output)
}

fn run_stateless_target_artifact_preflight(args: &[String]) -> Result<()> {
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
            "--release-policy",
            "--registration-policy",
            "--candidate-hash",
            "--kes-protocol-evidence",
        ],
        &[],
        &["--param"],
    )?;
    let expected_candidate = flag(args, "--candidate-hash")?;
    let protocol_evidence = optional(args, "--kes-protocol-evidence")
        .map(|raw| {
            serde_json::from_str::<crate::fleet::KesProtocolEvidence>(raw).map_err(|error| {
                OuroError::Validation(format!("malformed KES protocol evidence: {error}"))
            })
        })
        .transpose()?;
    if expected_candidate.len() != 64
        || !expected_candidate
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OuroError::Validation(
            "--candidate-hash must be the 64 lowercase hex value from the target plan".into(),
        ));
    }
    let plan_args = without_value_flag(args, "--candidate-hash")?;
    let plan_args = without_value_flag(&plan_args, "--kes-protocol-evidence")?;
    let initial = build_stateless_target_plan(&plan_args)?;
    if initial.op != "kes-rotation/install-opcert" {
        return Err(OuroError::Validation(
            "artifact preflight currently supports only kes-rotation/install-opcert".into(),
        ));
    }
    if initial
        .kes_rotation
        .as_ref()
        .is_some_and(|evidence| evidence.restart_loop_repair)
        && protocol_evidence.is_none()
    {
        return Err(OuroError::Validation(
            "KES restart-loop artifact preflight requires healthy-relay protocol evidence".into(),
        ));
    }
    if initial.candidate_hash != expected_candidate {
        return Err(OuroError::Validation(format!(
            "preflight candidate does not match current live state: expected={expected_candidate}, current={}",
            initial.candidate_hash
        )));
    }
    let reference = initial
        .intent
        .payload
        .get("opcert")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| OuroError::Validation("KES preflight lost opcert reference".into()))?;
    let path = std::env::var_os("OURO_EPHEMERAL_PAYLOAD")
        .map(PathBuf::from)
        .ok_or_else(|| {
            OuroError::Validation(
                "KES artifact preflight requires the sealed ephemeral public opcert payload".into(),
            )
        })?;
    let (_file, preview) = crate::inbox::preview_source(crate::inbox::ArtifactType::Opcert, &path)?;
    if preview.artifact_ref != reference {
        return Err(OuroError::Validation(
            "ephemeral opcert bytes do not match the candidate-bound artifact reference".into(),
        ));
    }
    let validation =
        validate_ephemeral_kes_candidate(&initial, &path, reference, protocol_evidence.as_ref())?;

    // Deep validation invokes the live protocol query. Re-probe once more and require the exact
    // same capability-free candidate before returning a positive preflight report.
    let final_plan = build_stateless_target_plan(&plan_args)?;
    if final_plan.candidate_hash != expected_candidate {
        let changed = stateless_live_drift_components(
            &initial.observation,
            &final_plan.observation,
            &final_plan.op,
        );
        return Err(OuroError::Validation(format!(
            "live state changed during KES artifact preflight; no mutation executed; changed components: {}",
            if changed.is_empty() { "candidate binding".into() } else { changed.join(", ") },
        )));
    }
    let mut result = ToolOutput::ok("ouro.op.artifact_preflight", false).with_data(json!({
        "op": final_plan.op,
        "node": final_plan.node,
        "candidate_hash": final_plan.candidate_hash,
        "artifact_ref": reference,
        "artifact_type": "opcert",
        "validation": {
            "text_envelope": "valid",
            "cold_key_signature": "valid",
            "hot_kes_key_matches_target": true,
            "counter_and_live_kes_window": "valid",
            "counter": validation.parsed.counter,
            "kes_period": validation.parsed.kes_period,
            "node_state_counter": validation.node_state_counter.value(),
            "node_state_counter_status": validation.node_state_counter.status(),
            "active_opcert_counter": validation.active_opcert_counter,
            "cold_identity_bound": validation.cold_identity_bound,
            "hot_kes_key_sha256": crate::intent::sha256_hex(
                &validation.parsed.hot_kes_verification_key,
            ),
        },
        "confirmation_consumed": false,
        "fleet_permit_consumed": false,
        "executor_available": false,
        "persistent_target_state_written": false,
        "next": "show the unchanged final plan and wait for exact operator approval before apply",
    }));
    result.machine = Some(final_plan.node);
    output::print_json(&result)
}

fn require_upgrade_step_direct_run(supervisor: &SupervisorObservation) -> Result<()> {
    let (orchestration, reason, _) = supervisor.upgrade_routing();
    match orchestration.as_str() {
        "run" => supervisor.require_direct_run(),
        "compose" => Err(OuroError::Validation(
            "manual_compose_required: upgrade/step cannot recreate a Compose-managed container; \
             show the operator the signed Compose upgrade procedure and wait for completion"
                .into(),
        )),
        "unsupported" => Err(OuroError::Validation(format!(
            "unsupported_orchestration: upgrade/step refused before container mutation: {}",
            reason.unwrap_or_else(|| "unknown orchestration owner".into())
        ))),
        other => Err(OuroError::Validation(format!(
            "unsupported_orchestration: upgrade/step refused before container mutation: \
             unknown_orchestration:{other}"
        ))),
    }
}

fn build_stateless_target_plan(args: &[String]) -> Result<StatelessTargetPlan> {
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
            "--release-policy",
            "--registration-policy",
        ],
        &[],
        &["--param"],
    )?;
    let op = flag(args, "--op")?;
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
    if matches!(
        op,
        "kes-rotation/stage-key" | "kes-rotation/install-opcert" | "kes-rotation/discard-stage"
    ) && role != Role::Bp
    {
        return Err(OuroError::Validation(format!("{op} is BP-only")));
    }

    let mut observation = read_observation(&[])?;
    canonicalize_typed_mounts(&mut observation.live.mounts);
    if op == "upgrade/step" {
        require_upgrade_step_direct_run(&observation.supervisor)?;
    } else {
        observation.supervisor.require_base_conformant()?;
    }
    require_typed_mounts(&observation.live.mounts)?;
    let is_upgrade = matches!(op, "upgrade/preload-image" | "upgrade/step");
    let allowlist = if is_upgrade {
        stateless_release_policy(args)?
    } else {
        convention::Allowlist::embedded()?
    };
    let stable_contract;
    let (contract, image) = if is_upgrade {
        let (contract, image) = allowlist.contract_and_image_for(
            &observation.live.image_config_digest,
            &observation.live.platform,
        )?;
        (contract, Some(image))
    } else {
        stable_contract = convention::Allowlist::stable_contract()?;
        (&stable_contract, None)
    };
    require_adoption_contract(contract, &observation, network, genesis, None, None)?;
    match role {
        Role::Relay
            if contract.role_rules.relay.forbids_forging_keys
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

    let kes_rotation = match op {
        "kes-rotation/stage-key" => {
            let permissions = require_kes_rotation_permissions(&observation.live)?;
            let current_period = current_kes_period(&observation)?;
            require_kes_stage_base_availability(&observation)?;
            let cardano_cli_version = cardano_cli_version(&observation.live.container_id)?;
            let (
                preexisting_kes_opcert_valid,
                preexisting_forging_credentials_ready,
                preexisting_kes_evidence_sha256,
            ) = kes_preexisting_evidence(&observation)?;
            let staged = pending_staged_kes_pair(&observation.live.container_id)?;
            let (_, active_vkey_sha256) = read_public_kes_vkey(
                &observation.live.container_id,
                crate::executor::KES_VKEY_DEST,
                "active public KES verification key",
            )?;
            let pending_existing = staged.is_some();
            let (staged_vkey, staged_vkey_sha256) = staged
                .map(|(vkey, digest)| (Some(vkey), Some(digest)))
                .unwrap_or((None, None));
            Some(KesRotationEvidence {
                current_period: Some(current_period),
                cardano_cli_version: Some(cardano_cli_version),
                active_vkey_sha256,
                staged_vkey_sha256,
                staged_vkey,
                pending_existing,
                preexisting_kes_opcert_valid,
                preexisting_forging_credentials_ready,
                preexisting_kes_evidence_sha256,
                permissions,
                activation_pending: false,
                activation_promoted: false,
                previous_vkey_sha256: None,
                previous_opcert_sha256: None,
                restart_loop_repair: false,
            })
        }
        "kes-rotation/install-opcert" => {
            let permissions = require_kes_rotation_permissions(&observation.live)?;
            let restart_loop_repair =
                observation.live.container_restarting || !observation.live.container_running;
            let current_period = match current_kes_period(&observation) {
                Ok(period) => Some(period),
                Err(_) if restart_loop_repair => None,
                Err(error) => return Err(error),
            };
            let cardano_cli_version = match cardano_cli_version(&observation.live.container_id) {
                Ok(version) => Some(version),
                Err(_) if restart_loop_repair => None,
                Err(error) => return Err(error),
            };
            let (
                preexisting_kes_opcert_valid,
                preexisting_forging_credentials_ready,
                preexisting_kes_evidence_sha256,
            ) = kes_preexisting_evidence(&observation)?;
            let (_, active_vkey_sha256) = read_public_kes_vkey(
                &observation.live.container_id,
                crate::executor::KES_VKEY_DEST,
                "active public KES verification key",
            )?;
            let (_, staged_vkey_sha256) = inspect_staged_kes_pair(&observation.live.container_id)?;
            let pending_activation = pending_kes_activation(&observation.live.container_id)?;
            let activation_pending = pending_activation.is_some();
            let (previous_vkey_sha256, previous_opcert_sha256) = pending_activation
                .map(|(vkey, opcert)| (Some(vkey), Some(opcert)))
                .unwrap_or((None, None));
            let activation_promoted = if activation_pending {
                let expected = payload
                    .get("opcert")
                    .and_then(serde_json::Value::as_str)
                    .and_then(artifact_ref_digest)
                    .ok_or_else(|| {
                        OuroError::Validation("KES resume lost its artifact digest".into())
                    })?;
                let opcert_is_transaction_member = observation.live.kes_opcert_id == expected
                    || previous_opcert_sha256.as_deref()
                        == Some(observation.live.kes_opcert_id.as_str());
                let vkey_is_transaction_member = active_vkey_sha256 == staged_vkey_sha256
                    || previous_vkey_sha256.as_deref() == Some(active_vkey_sha256.as_str());
                if !opcert_is_transaction_member || !vkey_is_transaction_member {
                    return Err(OuroError::Validation(
                        "retained KES activation contains files outside the previous/staged candidate set; automatic resume refused"
                            .into(),
                    ));
                }
                observation.live.kes_opcert_id == expected
                    && active_vkey_sha256 == staged_vkey_sha256
            } else {
                false
            };
            Some(KesRotationEvidence {
                current_period,
                cardano_cli_version,
                active_vkey_sha256,
                staged_vkey_sha256: Some(staged_vkey_sha256),
                staged_vkey: None,
                pending_existing: false,
                preexisting_kes_opcert_valid,
                preexisting_forging_credentials_ready,
                preexisting_kes_evidence_sha256,
                permissions,
                activation_pending,
                activation_promoted,
                previous_vkey_sha256,
                previous_opcert_sha256,
                restart_loop_repair,
            })
        }
        "kes-rotation/discard-stage" => {
            let current_period = current_kes_period(&observation)?;
            require_kes_stage_base_availability(&observation)?;
            let cardano_cli_version = cardano_cli_version(&observation.live.container_id)?;
            let (
                preexisting_kes_opcert_valid,
                preexisting_forging_credentials_ready,
                preexisting_kes_evidence_sha256,
            ) = kes_preexisting_evidence(&observation)?;
            let (_, active_vkey_sha256) = read_public_kes_vkey(
                &observation.live.container_id,
                crate::executor::KES_VKEY_DEST,
                "active public KES verification key",
            )?;
            let (staged_vkey, staged_vkey_sha256) =
                inspect_staged_kes_pair(&observation.live.container_id)?;
            Some(KesRotationEvidence {
                current_period: Some(current_period),
                cardano_cli_version: Some(cardano_cli_version),
                active_vkey_sha256,
                staged_vkey_sha256: Some(staged_vkey_sha256),
                staged_vkey: Some(staged_vkey),
                pending_existing: true,
                preexisting_kes_opcert_valid,
                preexisting_forging_credentials_ready,
                preexisting_kes_evidence_sha256,
                permissions: KesRotationPermissionEvidence::from_live(&observation.live),
                activation_pending: false,
                activation_promoted: false,
                previous_vkey_sha256: None,
                previous_opcert_sha256: None,
                restart_loop_repair: false,
            })
        }
        _ => None,
    };
    let payload_machine = payload
        .get("machine")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| OuroError::Validation("intent payload is missing machine".into()))?;
    if payload_machine != node {
        return Err(OuroError::Validation(format!(
            "intent payload machine {payload_machine:?} does not match target node {node:?}"
        )));
    }
    if optional(args, "--registration-policy").is_some() {
        return Err(OuroError::Validation(
            "registration policy is not accepted by stateless target planning".into(),
        ));
    }
    let mut upgrade_transition = None;
    let mut target_upgrade_image = None;
    if let Some(target) = payload.get("image").and_then(serde_json::Value::as_str) {
        let (_, target_image) =
            allowlist.contract_and_image_for(target, &observation.live.platform)?;
        match op {
            "upgrade/preload-image" | "upgrade/step" => {
                let (recommended, transition) = allowlist.recommended_upgrade_for(
                    &observation.live.image_config_digest,
                    &observation.live.platform,
                )?;
                if target != recommended.image_config_digest {
                    return Err(OuroError::Validation(format!(
                        "upgrade target {target} is not the signed recommended image {} for {}; refused",
                        recommended.image_config_digest, observation.live.platform
                    )));
                }
                upgrade_transition = transition.cloned();
                target_upgrade_image = Some(target_image.clone());
                if op == "upgrade/step" {
                    crate::executor::require_image_present(target)?;
                }
            }
            _ => {}
        }
    }

    // Bind only operation-relevant target state. The recreate spec is consumed exclusively by
    // upgrade/step; binding it into restart/KES/preload candidates makes unrelated Docker inspect
    // details (including environment values) invalidate an otherwise identical operation.
    let live_binding = json!({
        "supervisor": observation.supervisor,
        "live": observation.live,
        "recreate": if op == "upgrade/step" {
            observation.recreate.as_ref()
        } else {
            None
        },
    });
    let live_bytes = serde_json::to_vec(&live_binding)
        .map_err(|error| OuroError::Validation(format!("cannot bind live state: {error}")))?;
    let live_state_hash = crate::intent::sha256_hex(&live_bytes);
    let recreate_binding = observation.recreate.as_ref().map(|spec| {
        serde_json::to_vec(&spec.canonicalized())
            .map(|bytes| crate::intent::sha256_hex(&bytes))
            .unwrap_or_default()
    });
    let signed_allowlist_digest = allowlist.signed_digest()?;
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
        "kes_rotation": kes_rotation,
        "recreate_binding": if op == "upgrade/step" { recreate_binding.as_deref() } else { None },
        "runtime_policy": {
            "signed_allowlist_digest": signed_allowlist_digest,
            "upgrade_transition": upgrade_transition,
            "repository": if is_upgrade { Some(allowlist.repository.as_str()) } else { None },
            "target_image": target_upgrade_image,
        },
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
        "kes-rotation/stage-key"
            if kes_rotation
                .as_ref()
                .is_some_and(|evidence| evidence.pending_existing) =>
        {
            Vec::new()
        }
        "kes-rotation/stage-key" => {
            crate::executor::stateless_kes_stage_plan(&observation.live.container_id)
        }
        "kes-rotation/discard-stage" => {
            crate::executor::stateless_kes_stage_cleanup_plan(&observation.live.container_id)
        }
        "kes-rotation/install-opcert" => {
            let reference = intent
                .payload
                .get("opcert")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("KES plan lost opcert reference".into()))?;
            let restart_loop_repair = kes_rotation
                .as_ref()
                .is_some_and(|evidence| evidence.restart_loop_repair);
            let recovery = if restart_loop_repair {
                crate::executor::stateless_kes_restart_loop_recovery_plan(
                    &observation.live.container_id,
                    &observation.live.image_config_digest,
                    &format!("<ephemeral-inbox:{reference}>"),
                )
            } else {
                crate::executor::stateless_kes_recovery_plan(
                    &observation.live.container_id,
                    &format!("<ephemeral-inbox:{reference}>"),
                )
            };
            let pending = kes_rotation.as_ref().is_some_and(|evidence| {
                evidence.activation_pending && evidence.activation_promoted
            });
            if pending {
                recovery.finalize
            } else if restart_loop_repair {
                let mut plan = recovery.commit;
                plan.extend(recovery.finalize);
                plan
            } else {
                let mut plan = if kes_rotation
                    .as_ref()
                    .is_some_and(|evidence| evidence.activation_pending)
                {
                    recovery.commit[6..].to_vec()
                } else {
                    recovery.commit
                };
                plan.extend(recovery.finalize);
                plan
            }
        }
        "upgrade/preload-image" => {
            let target_image = target_upgrade_image.as_ref().ok_or_else(|| {
                OuroError::Validation("preload plan lost signed target OCI tuple".into())
            })?;
            vec![vec![
                "docker".into(),
                "pull".into(),
                "--platform".into(),
                target_image.platform.clone(),
                format!(
                    "{}@{}",
                    allowlist.repository, target_image.platform_manifest_digest
                ),
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
            crate::executor::stateless_recreate_approval_argv(
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
    let upgrade_failure_outcome = if matches!(op, "upgrade/preload-image" | "upgrade/step") {
        Some(
            if upgrade_transition
                .as_ref()
                .is_some_and(crate::upgrade::rollback_possible)
            {
                "verified_rollback_to_N"
            } else {
                "forward_recovery_or_resync_required"
            },
        )
    } else {
        None
    };
    let rollback_executor_plan = if op == "upgrade/step"
        && upgrade_transition
            .as_ref()
            .is_some_and(crate::upgrade::rollback_possible)
    {
        let target = intent
            .payload
            .get("image")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("upgrade plan lost image".into()))?;
        let recreate = observation.recreate.as_ref().ok_or_else(|| {
            OuroError::Validation("upgrade plan lost recreate recovery specification".into())
        })?;
        Some(
            crate::executor::stateless_recreate_recovery_plan(
                recreate,
                &observation.live.container_id,
                target,
            )?
            .rollback,
        )
    } else {
        None
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
        "upgrade_transition": upgrade_transition,
        "upgrade_failure_outcome": upgrade_failure_outcome,
        "rollback_executor_plan": rollback_executor_plan,
        "kes_rotation": kes_rotation,
        "runtime_policy": {
            "allowlist_version": allowlist.allowlist_version,
            "signed_allowlist_digest": signed_allowlist_digest,
            "contract_id": contract.contract_id,
            "convention_version": contract.convention_version,
            "running_image_config_digest": observation.live.image_config_digest,
            "repository": if is_upgrade { Some(allowlist.repository.as_str()) } else { None },
            "release": image.map(|value| value.release.as_str()),
            "oci_index_digest": image.map(|value| value.oci_index_digest.as_str()),
            "platform_manifest_digest": image.map(|value| value.platform_manifest_digest.as_str()),
            "target_release": target_upgrade_image.as_ref().map(|value| value.release.as_str()),
            "target_oci_index_digest": target_upgrade_image.as_ref().map(|value| value.oci_index_digest.as_str()),
            "target_platform": target_upgrade_image.as_ref().map(|value| value.platform.as_str()),
            "target_platform_manifest_digest": target_upgrade_image.as_ref().map(|value| value.platform_manifest_digest.as_str()),
            "target_image_config_digest": target_upgrade_image.as_ref().map(|value| value.image_config_digest.as_str()),
            "target_pull_reference": target_upgrade_image.as_ref().map(|value| format!(
                "{}@{}", allowlist.repository, value.platform_manifest_digest
            )),
            "release_feed_required": is_upgrade,
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
        "confirmation_required": validated.mutability == Mutability::Dangerous
            && !(op == "kes-rotation/stage-key"
                && kes_rotation.as_ref().is_some_and(|evidence| evidence.pending_existing)),
        "apply_revalidation_required": true,
        "artifact_validation": if op == "kes-rotation/install-opcert" {
            "content digest is candidate-bound; public artifact shape/domain and live compatibility are revalidated before apply"
        } else if op == "upgrade/preload-image" {
            "not_applicable: target pulls and verifies the signed exact OCI tuple"
        } else {
            "not_applicable"
        },
        "persistent_target_state_written": false,
        "note": "final stateless candidate from current signed policy + live target facts; no mutation or durable Ouro ownership state",
    }));
    result.machine = Some(node.to_string());
    Ok(StatelessTargetPlan {
        output: result,
        op: op.to_string(),
        node: node.to_string(),
        role,
        network: network.to_string(),
        genesis: genesis.to_string(),
        candidate_hash,
        intent,
        observation,
        policy: allowlist,
        kes_rotation,
    })
}

fn without_value_flag(args: &[String], name: &str) -> Result<Vec<String>> {
    let mut stripped = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        if args[index] == name {
            if args.get(index + 1).is_none() {
                return Err(OuroError::InvalidArgs(format!("missing value for {name}")));
            }
            index += 2;
        } else {
            stripped.push(args[index].clone());
            index += 1;
        }
    }
    Ok(stripped)
}

fn stateless_readiness(
    plan: &StatelessTargetPlan,
    observation: &Observation,
    allow_rotated_container: bool,
) -> Result<()> {
    let evidence = observation.readiness.as_ref().ok_or_else(|| {
        OuroError::Validation(
            "target probe did not provide readiness evidence after stateless apply".into(),
        )
    })?;
    crate::readiness::Readiness {
        role: plan.role,
        node_running: evidence.node_running,
        container_id_matches: if allow_rotated_container {
            !observation.live.container_id.is_empty()
                && observation.live.container_id != plan.observation.live.container_id
        } else {
            observation.live.container_id == plan.observation.live.container_id
        },
        socket_answers: evidence.socket_answers,
        network_ok: observation.live.network == plan.network,
        genesis_ok: observation.live.genesis_hash == plan.genesis,
        tip_block: evidence.tip_block,
        tip_block_next: evidence.tip_block_next,
        tip_synced: evidence.tip_synced,
        kes_opcert_valid: evidence.kes_opcert_valid,
        forging_credentials_ready: evidence.forging_credentials_ready,
        established_peers: evidence.established_peers,
    }
    .evaluate()
}

fn stateless_kes_activation_readiness(
    plan: &StatelessTargetPlan,
    observation: &Observation,
    validation: &KesCandidateValidation,
) -> Result<()> {
    require_kes_rotation_permissions(&observation.live)?;
    if matches!(
        validation.node_state_counter,
        NodeStateCounterEvidence::Present(_)
    ) {
        return stateless_readiness(plan, observation, false);
    }
    let evidence = observation.readiness.as_ref().ok_or_else(|| {
        OuroError::Validation(
            "KES activation postcondition omitted typed readiness evidence".into(),
        )
    })?;
    let kes = evidence.kes.as_ref().ok_or_else(|| {
        OuroError::Validation(
            "KES activation postcondition omitted no-blocks-minted KES evidence".into(),
        )
    })?;
    let candidate_counter = i64::try_from(validation.parsed.counter).map_err(|_| {
        OuroError::Validation("KES candidate counter exceeds probe evidence range".into())
    })?;
    let candidate_period = i64::try_from(validation.parsed.kes_period).map_err(|_| {
        OuroError::Validation("KES candidate period exceeds probe evidence range".into())
    })?;
    if kes.source.as_deref() != Some("cardano_cli")
        || kes.counter_status.as_deref() != Some("no_blocks_minted_yet")
        || kes.opcert_counter_node_state.is_some()
        || kes.opcert_counter_on_disk != Some(candidate_counter)
        || kes.start_period != candidate_period
        || kes.current_period < kes.start_period
        || kes.current_period >= kes.end_period
        || kes.period_valid != Some(true)
    {
        return Err(OuroError::Validation(
            "KES activation no-blocks-minted postcondition does not match the candidate-bound preflight"
                .into(),
        ));
    }
    if !validation.cold_identity_bound
        || !evidence.block_producer_configured
        || !observation.live.has_forging_keys
    {
        return Err(OuroError::Validation(
            "KES activation no-blocks-minted postcondition lacks bound cold identity or safe forging credentials"
                .into(),
        ));
    }
    crate::readiness::Readiness {
        role: plan.role,
        node_running: evidence.node_running,
        container_id_matches: observation.live.container_id == plan.observation.live.container_id,
        socket_answers: evidence.socket_answers,
        network_ok: observation.live.network == plan.network,
        genesis_ok: observation.live.genesis_hash == plan.genesis,
        tip_block: evidence.tip_block,
        tip_block_next: evidence.tip_block_next,
        tip_synced: evidence.tip_synced,
        // Candidate-bound null-path evidence satisfies only this activation verification. The
        // probe deliberately keeps its global readiness booleans false until protocol state has a
        // counter for this cold key.
        kes_opcert_valid: true,
        forging_credentials_ready: true,
        established_peers: evidence.established_peers,
    }
    .evaluate()
}

fn wait_runtime_restart_readiness(
    plan: &StatelessTargetPlan,
    expected_image: &str,
) -> Result<Observation> {
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(RUNTIME_RESTART_READINESS_TIMEOUT_SECONDS);
    loop {
        let last_readiness_error = match read_observation(&[]) {
            Ok(post) => {
                // Identity/policy/layout drift is not a startup transient and must fail closed.
                require_stateless_post_contract(plan, &post, expected_image)?;
                match stateless_readiness(plan, &post, false) {
                    Ok(()) => return Ok(post),
                    Err(error) => error.to_string(),
                }
            }
            Err(error) => error.to_string(),
        };
        if std::time::Instant::now() >= deadline {
            return Err(OuroError::Validation(format!(
                "node did not return ready within {RUNTIME_RESTART_READINESS_TIMEOUT_SECONDS} \
                 seconds after restart: {last_readiness_error}"
            )));
        }
        std::thread::sleep(std::time::Duration::from_secs(
            RUNTIME_RESTART_READINESS_POLL_SECONDS,
        ));
    }
}

fn readiness_timeout_seconds() -> u64 {
    #[cfg(debug_assertions)]
    if let Ok(raw) = std::env::var("OURO_TEST_READINESS_TIMEOUT_SECONDS") {
        if let Ok(seconds) = raw.parse::<u64>() {
            return seconds.min(RUNTIME_RESTART_READINESS_TIMEOUT_SECONDS);
        }
    }
    RUNTIME_RESTART_READINESS_TIMEOUT_SECONDS
}

fn wait_kes_activation_readiness(
    plan: &StatelessTargetPlan,
    expected_image: &str,
    validation: &KesCandidateValidation,
) -> Result<Observation> {
    let timeout_seconds = readiness_timeout_seconds();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_seconds);
    loop {
        let last_readiness_error = match read_observation(&[]) {
            Ok(post) => {
                // Identity, policy and layout drift are never a startup transient.
                require_stateless_post_contract(plan, &post, expected_image)?;
                let expected_opcert = plan
                    .intent
                    .payload
                    .get("opcert")
                    .and_then(serde_json::Value::as_str)
                    .and_then(artifact_ref_digest)
                    .ok_or_else(|| OuroError::Validation("KES artifact digest was lost".into()))?;
                if post.live.kes_opcert_id != expected_opcert {
                    return Err(OuroError::Validation(
                        "installed opcert digest differs from the approved artifact".into(),
                    ));
                }
                match stateless_kes_activation_readiness(plan, &post, validation) {
                    Ok(()) => return Ok(post),
                    Err(error) => error.to_string(),
                }
            }
            Err(error) => error.to_string(),
        };
        if std::time::Instant::now() >= deadline {
            return Err(OuroError::Validation(format!(
                "KES activation did not return candidate-bound readiness within {timeout_seconds} seconds after restart: {last_readiness_error}"
            )));
        }
        std::thread::sleep(std::time::Duration::from_secs(
            RUNTIME_RESTART_READINESS_POLL_SECONDS,
        ));
    }
}

fn stateless_live_drift_components(
    before: &Observation,
    after: &Observation,
    operation: &str,
) -> Vec<&'static str> {
    let mut changed = Vec::new();
    if before.supervisor != after.supervisor {
        changed.push("supervisor");
    }
    macro_rules! changed_live {
        ($field:ident) => {
            if before.live.$field != after.live.$field {
                changed.push(stringify!($field));
            }
        };
    }
    changed_live!(image_config_digest);
    changed_live!(platform);
    changed_live!(container_id);
    changed_live!(container_running);
    changed_live!(container_restarting);
    changed_live!(container_status);
    changed_live!(container_name);
    changed_live!(image_reference);
    changed_live!(container_creation_epoch);
    changed_live!(entrypoint);
    changed_live!(args);
    changed_live!(image_entrypoint);
    changed_live!(image_cmd);
    changed_live!(mounts);
    changed_live!(topology_hash);
    changed_live!(config_hash);
    changed_live!(kes_opcert_id);
    changed_live!(has_forging_keys);
    changed_live!(forging_key_permissions_safe);
    changed_live!(keys_directory_safe);
    changed_live!(kes_skey_private);
    changed_live!(vrf_skey_private);
    changed_live!(host_key_sha256);
    changed_live!(genesis_hash);
    changed_live!(network);
    let recreate_matches = match (&before.recreate, &after.recreate) {
        (Some(approved), Some(observed)) => approved.semantically_eq(observed),
        (None, None) => true,
        _ => false,
    };
    if operation == "upgrade/step" && !recreate_matches {
        changed.push("upgrade_recreate_spec");
    }
    changed
}

fn require_stateless_post_contract(
    plan: &StatelessTargetPlan,
    observation: &Observation,
    expected_image: &str,
) -> Result<()> {
    if observation.live.image_config_digest != expected_image {
        return Err(OuroError::Validation(format!(
            "post-apply image drifted: expected {expected_image}, observed {}",
            observation.live.image_config_digest
        )));
    }
    observation.supervisor.require_base_conformant()?;
    require_typed_mounts(&observation.live.mounts)?;
    if plan.op == "upgrade/step" {
        let approved = plan.observation.recreate.as_ref().ok_or_else(|| {
            OuroError::Validation("approved upgrade lost its recreate specification".into())
        })?;
        let observed = observation.recreate.as_ref().ok_or_else(|| {
            OuroError::Validation(
                "post-apply container no longer has a fully modeled recreate specification".into(),
            )
        })?;
        if !approved.semantically_eq(observed) {
            return Err(OuroError::Validation(
                "post-apply container parameters differ from the approved recreate specification"
                    .into(),
            ));
        }
    }
    let stable_contract;
    let contract = if matches!(plan.op.as_str(), "upgrade/preload-image" | "upgrade/step") {
        plan.policy
            .contract_for(expected_image, &observation.live.platform)?
    } else {
        stable_contract = convention::Allowlist::stable_contract()?;
        &stable_contract
    };
    require_adoption_contract(
        contract,
        observation,
        &plan.network,
        &plan.genesis,
        None,
        None,
    )?;
    match plan.role {
        Role::Relay if observation.live.has_forging_keys => Err(OuroError::Validation(
            "relay gained forging keys during stateless apply".into(),
        )),
        Role::Bp if observation.live.kes_opcert_id.is_empty() => Err(OuroError::Validation(
            "BP lost its operational certificate during stateless apply".into(),
        )),
        _ => Ok(()),
    }
}

fn rollback_failure(operation: &str, primary: OuroError, rollback: Result<()>) -> OuroError {
    match rollback {
        Ok(()) => OuroError::Validation(format!(
            "{operation} failed after mutation ({primary}); live-state rollback completed"
        )),
        Err(rollback_error) => OuroError::Validation(format!(
            "{operation} failed after mutation ({primary}); rollback also failed ({rollback_error}) — operator reconciliation required"
        )),
    }
}

/// The control already authenticated this exact permit before constructing the closed target argv.
/// The target re-checks its bound fields, deadline and immediate public relay quorum after the final
/// live plan, so transport/artifact latency cannot turn an expired snapshot into a mutation.
fn require_stateless_target_fleet_gate(plan: &StatelessTargetPlan, args: &[String]) -> Result<()> {
    let registered = crate::intent::lookup(&plan.op).ok_or_else(|| {
        OuroError::Validation(format!("operation {:?} is not registered", plan.op))
    })?;
    let fleet_sensitive = registered
        .touched
        .iter()
        .any(|resource| matches!(*resource, "container:restart" | "container:recreate"));
    let raw = optional(args, "--verified-fleet-permit");
    if !fleet_sensitive {
        if raw.is_some() {
            return Err(OuroError::Validation(
                "non-disruptive target apply does not accept fleet evidence".into(),
            ));
        }
        return Ok(());
    }
    let raw = raw.ok_or_else(|| {
        OuroError::Validation(
            "disruptive target apply is missing control-verified fleet evidence".into(),
        )
    })?;
    let permit: crate::fleet::StepPermit = serde_json::from_str(raw).map_err(|error| {
        OuroError::Validation(format!(
            "malformed control-verified target fleet evidence: {error}"
        ))
    })?;
    let role = match plan.role {
        Role::Bp => "bp",
        Role::Relay => "relay",
    };
    let target_image = if plan.op == "upgrade/step" {
        plan.intent
            .payload
            .get("image")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    } else {
        None
    };
    if permit.pool_id != flag(args, "--pool-id")?
        || permit.pool_spec_digest != flag(args, "--pool-spec-digest")?
        || permit.node_id != plan.node
        || permit.operation_id != plan.op
        || permit.intent_hash != plan.candidate_hash
        || permit.role != role
        || permit.target_image != target_image
        || permit.min_online_relays
            != flag(args, "--min-online-relays")?
                .parse::<u32>()
                .map_err(|_| {
                    OuroError::Validation(
                        "target min-online-relays is not an unsigned integer".into(),
                    )
                })?
        || permit.network != plan.network
        || permit.genesis_hash != plan.genesis
    {
        return Err(OuroError::Validation(
            "control-verified fleet evidence does not bind this target/candidate/pool policy"
                .into(),
        ));
    }
    if plan.op == "kes-rotation/install-opcert"
        && plan
            .kes_rotation
            .as_ref()
            .is_some_and(|evidence| evidence.restart_loop_repair)
    {
        let expected_artifact = plan
            .intent
            .payload
            .get("opcert")
            .and_then(serde_json::Value::as_str)
            .and_then(artifact_ref_digest)
            .ok_or_else(|| OuroError::Validation("KES plan lost its artifact digest".into()))?;
        let evidence = permit.kes_protocol_evidence.as_ref().ok_or_else(|| {
            OuroError::Validation(
                "KES restart-loop repair permit lacks healthy-relay protocol evidence".into(),
            )
        })?;
        if evidence.artifact_sha256 != expected_artifact {
            return Err(OuroError::Validation(
                "KES restart-loop repair permit protocol evidence binds another artifact".into(),
            ));
        }
    }
    let check_time = |now: u64| {
        if permit.expiry_epoch <= now
            || permit.facts_epoch > now.saturating_add(5)
            || now.saturating_sub(permit.facts_epoch) > crate::fleet::LIVE_FACTS_VALIDITY_SECONDS
        {
            Err(OuroError::Validation(
                "fleet permit expired before target mutation — collect fresh facts and approve again"
                    .into(),
            ))
        } else {
            Ok(())
        }
    };
    check_time(crate::s0019_confirmation::current_epoch()?)?;
    crate::fleet::require_quorum(
        permit.online_relays,
        permit.min_online_relays,
        permit.role == "relay",
    )?;
    crate::fleet::require_bp_last(permit.role == "bp", permit.relays_remaining)?;
    permit.require_live_relay_quorum()?;
    check_time(crate::s0019_confirmation::current_epoch()?)
}

fn run_stateless_target_apply(args: &[String]) -> Result<()> {
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
            "--approved-candidate",
            "--verified-fleet-permit",
            "--release-policy",
            "--registration-policy",
        ],
        &[],
        &["--param"],
    )?;
    let approved = flag(args, "--approved-candidate")?;
    if approved.len() != 64
        || !approved
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OuroError::Validation(
            "--approved-candidate must be 64 lowercase hex characters".into(),
        ));
    }
    let plan_args = without_value_flag(args, "--approved-candidate")?;
    let plan_args = without_value_flag(&plan_args, "--verified-fleet-permit")?;
    let initial = build_stateless_target_plan(&plan_args)?;
    if initial.candidate_hash != approved {
        return Err(OuroError::Validation(format!(
            "approved candidate does not match current live state: approved={approved}, current={}",
            initial.candidate_hash
        )));
    }

    let payload_path = std::env::var_os("OURO_EPHEMERAL_PAYLOAD").map(PathBuf::from);
    let permit_kes_protocol_evidence = optional(args, "--verified-fleet-permit")
        .map(|raw| {
            serde_json::from_str::<crate::fleet::StepPermit>(raw).map_err(|error| {
                OuroError::Validation(format!(
                    "malformed control-verified fleet evidence: {error}"
                ))
            })
        })
        .transpose()?
        .and_then(|permit| permit.kes_protocol_evidence);
    let expected_artifact = match initial.op.as_str() {
        "kes-rotation/install-opcert" => Some((
            crate::inbox::ArtifactType::Opcert,
            initial
                .intent
                .payload
                .get("opcert")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("KES intent lost opcert reference".into()))?,
        )),
        _ => None,
    };
    let mut held_payload = None;
    let mut kes_candidate_validation = None;
    if let Some((kind, expected_ref)) = expected_artifact {
        let path = payload_path.as_deref().ok_or_else(|| {
            OuroError::Validation(format!(
                "{} requires its reviewed public artifact in the sealed ephemeral payload",
                initial.op
            ))
        })?;
        let (file, preview) = crate::inbox::preview_source(kind, path)?;
        if preview.artifact_ref != expected_ref {
            return Err(OuroError::Validation(
                "ephemeral artifact bytes do not match the candidate-bound reference".into(),
            ));
        }
        kes_candidate_validation = Some(validate_ephemeral_kes_candidate(
            &initial,
            path,
            expected_ref,
            permit_kes_protocol_evidence.as_ref(),
        )?);
        held_payload = Some((file, path.to_path_buf()));
    } else if payload_path.is_some() {
        return Err(OuroError::Validation(
            "this operation does not accept an ephemeral artifact".into(),
        ));
    }

    // Artifact/domain checks can be expensive. Re-probe immediately afterwards and require the
    // exact approved candidate again so all drift is detected before the first executor mutation.
    let final_plan = build_stateless_target_plan(&plan_args)?;
    if final_plan.candidate_hash != approved {
        let changed = stateless_live_drift_components(
            &initial.observation,
            &final_plan.observation,
            &final_plan.op,
        );
        return Err(OuroError::Validation(format!(
            "live state changed after approval/artifact validation; no mutation executed; changed components: {}",
            if changed.is_empty() { "candidate binding".into() } else { changed.join(", ") },
        )));
    }
    let current_container = final_plan.observation.live.container_id.clone();
    let current_image = final_plan.observation.live.image_config_digest.clone();
    require_stateless_target_fleet_gate(&final_plan, args)?;
    let live_postcondition: Option<serde_json::Value>;

    match final_plan.op.as_str() {
        "runtime/restart" => {
            crate::executor::run_argv(&["docker".into(), "restart".into(), current_container])?;
            let post = match wait_runtime_restart_readiness(&final_plan, &current_image) {
                Ok(post) => post,
                Err(error) => {
                    let mut failure = ToolOutput::failure(
                        "ouro.op.apply",
                        "postcondition_failed_after_mutation",
                        format!("restart executed but live postcondition did not pass: {error}"),
                    )
                    .with_data(json!({
                        "op": &final_plan.op,
                        "node": &final_plan.node,
                        "candidate_hash": &final_plan.candidate_hash,
                        "mutation_executed": true,
                        "live_postcondition": null,
                        "recovery": "do not retry restart; reconcile the live node with bounded reads",
                        "persistent_target_state_written": false,
                    }));
                    failure.changed = true;
                    failure.machine = Some(final_plan.node.clone());
                    output::print_json(&failure)?;
                    return Err(OuroError::Reported(10));
                }
            };
            let readiness = post.readiness.as_ref().expect(
                "stateless_readiness returned success only after requiring readiness evidence",
            );
            live_postcondition = Some(json!({
                "verification": "typed_role_readiness_passed",
                "container": {
                    "id": post.live.container_id,
                    "creation_epoch": post.live.container_creation_epoch,
                    "image_config_digest": post.live.image_config_digest,
                },
                "network": post.live.network,
                "genesis_hash": post.live.genesis_hash,
                "node_running": readiness.node_running,
                "socket_answers": readiness.socket_answers,
                "tip_block": readiness.tip_block_height,
                "tip_slot": readiness.tip_slot,
                "tip_era": readiness.tip_era,
                "sync_progress": readiness.sync_progress,
                "tip_synced": readiness.tip_synced,
            }));
        }
        "kes-rotation/stage-key" => {
            let resume_existing = final_plan
                .kes_rotation
                .as_ref()
                .is_some_and(|evidence| evidence.pending_existing);
            if !resume_existing {
                let stage = crate::executor::stateless_kes_stage_plan(&current_container);
                if let Err(error) = crate::executor::run_plan(&stage) {
                    let cleanup = crate::executor::run_plan(
                        &crate::executor::stateless_kes_stage_cleanup_plan(&current_container),
                    );
                    return Err(OuroError::Validation(format!(
                        "KES key staging failed before any live key/certificate change ({error}); cleanup={}",
                        if cleanup.is_ok() { "completed" } else { "failed; fixed staging residue may remain" }
                    )));
                }
            }
            let verified_stage = (|| -> Result<serde_json::Value> {
                let (public_vkey, public_vkey_sha256) =
                    inspect_staged_kes_pair(&current_container)?;
                let (_, active_vkey_sha256) = read_public_kes_vkey(
                    &current_container,
                    crate::executor::KES_VKEY_DEST,
                    "active public KES verification key",
                )?;
                let approved_active_vkey_sha256 = final_plan
                    .kes_rotation
                    .as_ref()
                    .map(|evidence| evidence.active_vkey_sha256.as_str())
                    .ok_or_else(|| {
                        OuroError::Validation(
                            "KES stage candidate lost its active-key binding".into(),
                        )
                    })?;
                if active_vkey_sha256 != approved_active_vkey_sha256 {
                    return Err(OuroError::Validation(
                        "active KES verification key changed during staging".into(),
                    ));
                }
                let mut post = read_observation(&[])?;
                canonicalize_typed_mounts(&mut post.live.mounts);
                require_stateless_post_contract(&final_plan, &post, &current_image)?;
                require_kes_stage_readiness_invariant(&final_plan.observation, &post)?;
                let changed =
                    stateless_live_drift_components(&final_plan.observation, &post, &final_plan.op);
                if !changed.is_empty() {
                    return Err(OuroError::Validation(format!(
                        "KES staging unexpectedly changed active node state: {}",
                        changed.join(", ")
                    )));
                }
                let current_period = final_plan
                    .kes_rotation
                    .as_ref()
                    .and_then(|evidence| evidence.current_period)
                    .ok_or_else(|| {
                        OuroError::Validation("KES stage candidate lost its current period".into())
                    })?;
                let approved_cardano_cli_version = final_plan
                    .kes_rotation
                    .as_ref()
                    .and_then(|evidence| evidence.cardano_cli_version.as_deref())
                    .ok_or_else(|| {
                        OuroError::Validation(
                            "KES stage candidate lost its cardano-cli version".into(),
                        )
                    })?;
                let post_cardano_cli_version = cardano_cli_version(&current_container)?;
                if post_cardano_cli_version != approved_cardano_cli_version {
                    return Err(OuroError::Validation(
                        "BP container cardano-cli version changed during KES staging".into(),
                    ));
                }
                Ok(json!({
                    "verification": if resume_existing {
                        "existing_kes_pair_resumed_without_mutation"
                    } else {
                        "new_kes_pair_staged_without_activation"
                    },
                    "kes_period": current_period,
                    "cardano_cli_version": post_cardano_cli_version,
                    "kes_vkey_sha256": public_vkey_sha256,
                    "kes_vkey": public_vkey,
                    "kes_skey_location": "target_private_stage_only",
                    "kes_skey_mode": "0600",
                    "active_container_unchanged": true,
                    "active_kes_key_unchanged": true,
                    "active_opcert_unchanged": true,
                    "preexisting_kes_opcert_valid": post.readiness.as_ref()
                        .map(|readiness| readiness.kes_opcert_valid),
                    "preexisting_forging_credentials_ready": post.readiness.as_ref()
                        .map(|readiness| readiness.forging_credentials_ready),
                    "restart_performed": false,
                }))
            })();
            live_postcondition = match verified_stage {
                Ok(postcondition) => Some(postcondition),
                Err(error) if resume_existing => {
                    return Err(OuroError::Validation(format!(
                        "existing staged KES pair changed or became unsafe while resuming; it was not cleaned ({error})"
                    )));
                }
                Err(error) => {
                    let cleanup = crate::executor::run_plan(
                        &crate::executor::stateless_kes_stage_cleanup_plan(&current_container),
                    );
                    return Err(OuroError::Validation(format!(
                        "KES pair was generated but its non-activation postcondition failed ({error}); cleanup={}",
                        if cleanup.is_ok() { "completed" } else { "failed; fixed staging residue may remain" }
                    )));
                }
            };
        }
        "kes-rotation/discard-stage" => {
            let expected_staged_vkey_sha256 = final_plan
                .kes_rotation
                .as_ref()
                .and_then(|evidence| evidence.staged_vkey_sha256.as_deref())
                .ok_or_else(|| {
                    OuroError::Validation(
                        "KES discard candidate lost its staged public-key binding".into(),
                    )
                })?;
            let (_, current_staged_vkey_sha256) = inspect_staged_kes_pair(&current_container)?;
            if current_staged_vkey_sha256 != expected_staged_vkey_sha256 {
                return Err(OuroError::Validation(
                    "staged KES pair changed before approved discard".into(),
                ));
            }
            crate::executor::run_plan(&crate::executor::stateless_kes_stage_cleanup_plan(
                &current_container,
            ))?;
            crate::executor::run_argv(&[
                "docker".into(),
                "exec".into(),
                current_container.clone(),
                "test".into(),
                "!".into(),
                "-e".into(),
                crate::executor::KES_STAGE_DIR.into(),
            ])
            .map_err(|_| {
                OuroError::Validation(
                    "approved KES stage discard did not remove the fixed staging directory".into(),
                )
            })?;
            let mut post = read_observation(&[])?;
            canonicalize_typed_mounts(&mut post.live.mounts);
            require_stateless_post_contract(&final_plan, &post, &current_image)?;
            require_kes_stage_readiness_invariant(&final_plan.observation, &post)?;
            let changed =
                stateless_live_drift_components(&final_plan.observation, &post, &final_plan.op);
            if !changed.is_empty() {
                return Err(OuroError::Validation(format!(
                    "KES stage discard unexpectedly changed active node state: {}",
                    changed.join(", ")
                )));
            }
            live_postcondition = Some(json!({
                "verification": "candidate_bound_staged_kes_pair_discarded",
                "discarded_kes_vkey_sha256": expected_staged_vkey_sha256,
                "staging_directory_absent": true,
                "active_container_unchanged": true,
                "active_kes_key_unchanged": true,
                "active_opcert_unchanged": true,
                "restart_performed": false,
            }));
        }
        "kes-rotation/install-opcert" => {
            let (_, payload) = held_payload.as_ref().ok_or_else(|| {
                OuroError::Validation("validated KES payload was not retained".into())
            })?;
            let candidate_validation = kes_candidate_validation.as_ref().ok_or_else(|| {
                OuroError::Validation(
                    "candidate-bound KES preflight evidence was not retained".into(),
                )
            })?;
            let restart_loop_repair = final_plan
                .kes_rotation
                .as_ref()
                .is_some_and(|evidence| evidence.restart_loop_repair);
            let recovery = if restart_loop_repair {
                crate::executor::stateless_kes_restart_loop_recovery_plan(
                    &current_container,
                    &current_image,
                    &payload.display().to_string(),
                )
            } else {
                crate::executor::stateless_kes_recovery_plan(
                    &current_container,
                    &payload.display().to_string(),
                )
            };
            let activation_pending = final_plan
                .kes_rotation
                .as_ref()
                .is_some_and(|evidence| evidence.activation_pending);
            let activation_promoted = final_plan
                .kes_rotation
                .as_ref()
                .is_some_and(|evidence| evidence.activation_promoted);
            if restart_loop_repair {
                if !activation_promoted {
                    if let Err(error) = crate::executor::run_argv(&recovery.commit[0]) {
                        let mut failure = ToolOutput::failure(
                            "ouro.op.apply",
                            "preparation_unverified",
                            format!(
                                "KES restart-loop repair preparation did not complete: {error}; active credentials were not intentionally promoted"
                            ),
                        )
                        .with_data(json!({
                            "op": &final_plan.op,
                            "node": &final_plan.node,
                            "candidate_hash": &final_plan.candidate_hash,
                            "mutation_executed": true,
                            "restart_performed": false,
                            "automatic_rollback_performed": false,
                            "recovery_material_retained": true,
                            "outcome": "preparation_unverified",
                            "recovery": "rerun the same Phase B with the same public artifact; do not restage or cold-sign",
                            "persistent_target_state_written": true,
                        }));
                        failure.changed = true;
                        failure.machine = Some(final_plan.node.clone());
                        output::print_json(&failure)?;
                        return Err(OuroError::Reported(10));
                    }
                    for (index, command) in recovery.commit[1..].iter().enumerate() {
                        if let Err(error) = crate::executor::run_argv(command) {
                            let mut failure = ToolOutput::failure(
                                "ouro.op.apply",
                                "activation_unverified",
                                format!(
                                    "KES restart-loop forward activation stopped at fixed step {}: {error}; previous credentials were not restored",
                                    index + 1
                                ),
                            )
                            .with_data(json!({
                                "op": &final_plan.op,
                                "node": &final_plan.node,
                                "candidate_hash": &final_plan.candidate_hash,
                                "mutation_executed": true,
                                "restart_performed": false,
                                "restart_attempted": index >= 2,
                                "restart_count_max": 1,
                                "automatic_rollback_performed": false,
                                "recovery_material_retained": true,
                                "outcome": "activation_unverified",
                                "recovery": "rerun the same Phase B with the same public artifact; do not restage, cold-sign, or advance the counter",
                                "persistent_target_state_written": true,
                            }));
                            failure.changed = true;
                            failure.machine = Some(final_plan.node.clone());
                            output::print_json(&failure)?;
                            return Err(OuroError::Reported(10));
                        }
                    }
                }
            } else if !activation_pending {
                crate::executor::run_plan(&recovery.commit[..3])?;
                if let Err(error) = crate::executor::run_plan(&recovery.commit[3..6]) {
                    let cleanup = crate::executor::run_plan(
                        &crate::executor::stateless_kes_prepare_cleanup_plan(&current_container),
                    );
                    return Err(OuroError::Validation(format!(
                        "KES recovery material could not be prepared; live key/certificate was not changed ({error}); cleanup={}",
                        if cleanup.is_ok() { "completed" } else { "failed; backup residue retained" }
                    )));
                }
            }
            if !restart_loop_repair && !activation_promoted {
                if let Err(error) = crate::executor::run_plan(&recovery.commit[6..9]) {
                    let mut failure = ToolOutput::failure(
                        "ouro.op.apply",
                        "activation_unverified",
                        format!(
                            "KES forward activation did not complete: {error}; automatic restoration/restart of the previous credentials is forbidden"
                        ),
                    )
                    .with_data(json!({
                        "op": &final_plan.op,
                        "node": &final_plan.node,
                        "candidate_hash": &final_plan.candidate_hash,
                        "mutation_executed": true,
                        "restart_count_max": 1,
                        "automatic_rollback_performed": false,
                        "recovery_material_retained": true,
                        "outcome": "activation_unverified",
                        "recovery": "rerun the same KES workflow with the same public artifact; do not restage, cold-sign, or advance the counter",
                        "persistent_target_state_written": true,
                    }));
                    failure.changed = true;
                    failure.machine = Some(final_plan.node.clone());
                    output::print_json(&failure)?;
                    return Err(OuroError::Reported(10));
                }
                if let Err(error) = crate::executor::run_argv(&recovery.commit[9]) {
                    let mut failure = ToolOutput::failure(
                        "ouro.op.apply",
                        "activation_unverified",
                        format!(
                            "KES candidate files were promoted but the single BP restart did not complete: {error}; automatic restoration/restart of the previous credentials is forbidden"
                        ),
                    )
                    .with_data(json!({
                        "op": &final_plan.op,
                        "node": &final_plan.node,
                        "candidate_hash": &final_plan.candidate_hash,
                        "mutation_executed": true,
                        "restart_performed": false,
                        "automatic_rollback_performed": false,
                        "recovery_material_retained": true,
                        "outcome": "activation_unverified",
                        "recovery": "rerun the same KES workflow with the same public artifact; do not restage, cold-sign, or advance the counter",
                        "persistent_target_state_written": true,
                    }));
                    failure.changed = true;
                    failure.machine = Some(final_plan.node.clone());
                    output::print_json(&failure)?;
                    return Err(OuroError::Reported(10));
                }
            }
            let verify = (|| -> Result<Observation> {
                let post = wait_kes_activation_readiness(
                    &final_plan,
                    &current_image,
                    candidate_validation,
                )?;
                let (_, active_vkey_sha256) = read_public_kes_vkey(
                    &current_container,
                    crate::executor::KES_VKEY_DEST,
                    "active public KES verification key",
                )?;
                let approved_vkey_sha256 = final_plan
                    .kes_rotation
                    .as_ref()
                    .and_then(|evidence| evidence.staged_vkey_sha256.as_deref())
                    .ok_or_else(|| {
                        OuroError::Validation("KES activation lost staged-key binding".into())
                    })?;
                if active_vkey_sha256 != approved_vkey_sha256 {
                    return Err(OuroError::Validation(
                        "activated KES verification key differs from the approved staged key"
                            .into(),
                    ));
                }
                Ok(post)
            })();
            let _post = match verify {
                Ok(post) => post,
                Err(error) => {
                    let mut failure = ToolOutput::failure(
                        "ouro.op.apply",
                        "activation_unverified",
                        if activation_promoted {
                            format!(
                                "retained KES activation is still not candidate-bound ready: {error}; no reinstall, restart or rollback was performed"
                            )
                        } else {
                            format!(
                                "KES credentials were promoted and the BP was restarted, but candidate-bound readiness was not verified: {error}; automatic restoration/restart of the previous credentials is forbidden"
                            )
                        },
                    )
                    .with_data(json!({
                        "op": &final_plan.op,
                        "node": &final_plan.node,
                        "candidate_hash": &final_plan.candidate_hash,
                        "mutation_executed": !activation_promoted,
                        "restart_performed": !activation_promoted,
                        "activation_resumed": activation_pending,
                        "automatic_rollback_performed": false,
                        "recovery_material_retained": true,
                        "outcome": "activation_unverified",
                        "recovery": "rerun the same KES workflow with the same public artifact; do not restage, cold-sign, or advance the counter",
                        "persistent_target_state_written": true,
                    }));
                    failure.changed = !activation_promoted;
                    failure.machine = Some(final_plan.node.clone());
                    output::print_json(&failure)?;
                    return Err(OuroError::Reported(10));
                }
            };
            crate::executor::run_plan(&recovery.finalize).map_err(|error| {
                OuroError::Validation(format!(
                    "KES apply verified but previous-key/certificate cleanup failed; recovery residue retained: {error}"
                ))
            })?;
            crate::executor::run_plan(&crate::executor::stateless_kes_cleanup_verification_plan(
                &current_container,
            ))
            .map_err(|error| {
                OuroError::Validation(format!(
                    "KES apply verified but transaction residue remains after cleanup: {error}"
                ))
            })?;
            live_postcondition = Some(json!({
                "verification": "staged_kes_pair_and_bound_opcert_activated",
                "kes_vkey_sha256": final_plan.kes_rotation.as_ref()
                    .and_then(|evidence| evidence.staged_vkey_sha256.as_deref()),
                "opcert_sha256": final_plan.intent.payload.get("opcert")
                    .and_then(serde_json::Value::as_str)
                    .and_then(artifact_ref_digest),
                "typed_role_readiness": "passed",
                "node_state_counter_status": candidate_validation.node_state_counter.status(),
                "cold_identity_bound": candidate_validation.cold_identity_bound,
                "restart_performed": !activation_promoted,
                "activation_resumed": activation_pending,
                "rollback_residue_removed": true,
                "staging_residue_removed": true,
            }));
        }
        "upgrade/preload-image" => {
            let target = final_plan
                .intent
                .payload
                .get("image")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("preload image digest was lost".into()))?;
            let (_, target_image) = final_plan
                .policy
                .contract_and_image_for(target, &final_plan.observation.live.platform)?;
            stateless_readiness(&final_plan, &final_plan.observation, false)?;
            let pulled = crate::executor::pull_verified_image(
                &final_plan.policy.repository,
                &target_image.platform_manifest_digest,
                &target_image.image_config_digest,
                &target_image.platform,
            )?;
            let mut post = read_observation(&[])?;
            canonicalize_typed_mounts(&mut post.live.mounts);
            require_stateless_post_contract(&final_plan, &post, &current_image)?;
            stateless_readiness(&final_plan, &post, false)?;
            let changed =
                stateless_live_drift_components(&final_plan.observation, &post, &final_plan.op);
            if !changed.is_empty() {
                return Err(OuroError::Validation(format!(
                    "exact image pull changed active-container state unexpectedly: {}",
                    changed.join(", ")
                )));
            }
            live_postcondition = Some(json!({
                "verification": "signed_exact_oci_tuple_present",
                "pulled_image": pulled,
                "running_container_unchanged": {
                    "id": post.live.container_id,
                    "creation_epoch": post.live.container_creation_epoch,
                    "image_config_digest": post.live.image_config_digest,
                    "entrypoint": post.live.entrypoint,
                    "args": post.live.args,
                    "mounts": post.live.mounts,
                    "network": post.live.network,
                    "readiness": "passed_before_and_after_pull",
                },
            }));
        }
        "upgrade/step" => {
            let target = final_plan
                .intent
                .payload
                .get("image")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("upgrade image digest was lost".into()))?;
            let recreate = final_plan.observation.recreate.as_ref().ok_or_else(|| {
                OuroError::Validation("upgrade lost its live recreate specification".into())
            })?;
            let recovery = crate::executor::stateless_recreate_recovery_plan(
                recreate,
                &current_container,
                target,
            )?;
            let (recommended, transition) = final_plan
                .policy
                .recommended_upgrade_for(&current_image, &final_plan.observation.live.platform)?;
            if target != recommended.image_config_digest {
                return Err(OuroError::Validation(
                    "upgrade target is no longer the signed recommended image".into(),
                ));
            }
            let automatic_rollback_allowed =
                transition.is_some_and(crate::upgrade::rollback_possible);
            let rollback = || {
                crate::executor::run_rollback_plan("upgrade/step", &recovery.rollback)?;
                let restored = read_observation(&[])?;
                require_stateless_post_contract(&final_plan, &restored, &current_image)?;
                stateless_readiness(&final_plan, &restored, false)
            };
            crate::executor::run_argv(&recovery.commit[0])?;
            if let Err(error) = crate::executor::run_argv(&recovery.commit[1]) {
                return Err(rollback_failure(&final_plan.op, error, rollback()));
            }
            if let Err(error) = crate::executor::run_argv(&recovery.commit[2]) {
                if automatic_rollback_allowed {
                    return Err(rollback_failure(&final_plan.op, error, rollback()));
                }
                return Err(OuroError::Validation(format!(
                    "upgrade/step activation may have begun and the signed transition is not backward-compatible ({error}); automatic rollback refused — forward recovery or re-sync required; prior container retained"
                )));
            }
            let verify = (|| -> Result<Observation> {
                let post = read_observation(&[])?;
                if post.live.image_config_digest != target {
                    return Err(OuroError::Validation(
                        "upgrade did not land on the approved image digest".into(),
                    ));
                }
                require_stateless_post_contract(&final_plan, &post, target)?;
                stateless_readiness(&final_plan, &post, true)?;
                Ok(post)
            })();
            let post = match verify {
                Ok(post) => post,
                Err(error) if automatic_rollback_allowed => {
                    return Err(rollback_failure(&final_plan.op, error, rollback()));
                }
                Err(error) => {
                    return Err(OuroError::Validation(format!(
                        "upgrade/step reached N+1 but postcondition failed and the signed transition is not backward-compatible ({error}); automatic rollback refused — forward recovery or re-sync required; prior container retained"
                    )));
                }
            };
            crate::executor::run_plan(&recovery.finalize).map_err(|error| {
                OuroError::Validation(format!(
                    "upgrade verified but previous-container cleanup failed; recovery residue retained: {error}"
                ))
            })?;
            let readiness = post.readiness.as_ref().ok_or_else(|| {
                OuroError::Validation(
                    "verified upgrade postcondition lost readiness evidence".into(),
                )
            })?;
            live_postcondition = Some(json!({
                "verification": "typed_role_readiness_passed",
                "container": {
                    "id": post.live.container_id,
                    "creation_epoch": post.live.container_creation_epoch,
                    "image_config_digest": post.live.image_config_digest,
                },
                "network": post.live.network,
                "genesis_hash": post.live.genesis_hash,
                "node_running": readiness.node_running,
                "socket_answers": readiness.socket_answers,
                "tip_block": readiness.tip_block_height,
                "tip_slot": readiness.tip_slot,
                "tip_era": readiness.tip_era,
                "sync_progress": readiness.sync_progress,
                "tip_synced": readiness.tip_synced,
                "recreate_spec": "matched_approved_supported_fields",
                "upgrade_failure_outcome": if automatic_rollback_allowed {
                    "verified_rollback_to_N"
                } else {
                    "forward_recovery_or_resync_required"
                },
            }));
        }
        other => {
            return Err(OuroError::Validation(format!(
                "operation {other} has no stateless apply executor"
            )))
        }
    }

    let resumed_kes_stage = final_plan.op == "kes-rotation/stage-key"
        && final_plan
            .kes_rotation
            .as_ref()
            .is_some_and(|evidence| evidence.pending_existing);
    let mut result = ToolOutput::ok("ouro.op.apply", !resumed_kes_stage).with_data(json!({
        "op": final_plan.op,
        "node": final_plan.node,
        "candidate_hash": final_plan.candidate_hash,
        "assurance": "approved_candidate_revalidated",
        "recovery_model": if final_plan.op == "kes-rotation/install-opcert" {
            "forward-only activation; never automatically restore/restart previous credentials"
        } else {
            "live postcondition verification with in-invocation rollback where an inverse exists"
        },
        "live_postcondition": live_postcondition,
        "persistent_target_state_written": false,
        "operational_kes_stage_written": final_plan.op == "kes-rotation/stage-key"
            && !resumed_kes_stage,
        "ephemeral_artifact_removed_by_transport": held_payload.is_some(),
    }));
    result.machine = Some(final_plan.node);
    output::print_json(&result)
}

fn run_stateless_target_status(args: &[String]) -> Result<()> {
    validate_closed_args(
        args,
        &[
            "--node",
            "--role",
            "--network",
            "--genesis",
            "--expect-allowlist",
            "--release-policy",
        ],
        &[],
        &[],
    )?;
    let node = flag(args, "--node")?;
    crate::intent::validate_machine_id(node)?;
    let role = match flag(args, "--role")? {
        "bp" => Role::Bp,
        "relay" => Role::Relay,
        other => {
            return Err(OuroError::Validation(format!(
                "target status role must be bp|relay, got {other:?}"
            )))
        }
    };
    let network = flag(args, "--network")?;
    let genesis = flag(args, "--genesis")?;
    validate_digest_selector("--genesis", &format!("sha256:{genesis}"))?;
    let observation = read_observation(&[])?;
    observation.supervisor.require_base_conformant()?;
    require_typed_mounts(&observation.live.mounts)?;
    let release_policy = optional(args, "--release-policy");
    let allowlist = match release_policy {
        Some(document) => convention::Allowlist::release_document(document)?,
        None => convention::Allowlist::embedded()?,
    };
    if allowlist.signed_digest()? != flag(args, "--expect-allowlist")? {
        return Err(OuroError::Validation(
            "fleet status runner policy differs from the control release".into(),
        ));
    }
    let stable_contract;
    let contract = if release_policy.is_some() {
        allowlist.contract_for(
            &observation.live.image_config_digest,
            &observation.live.platform,
        )?
    } else {
        stable_contract = convention::Allowlist::stable_contract()?;
        &stable_contract
    };
    require_adoption_contract(contract, &observation, network, genesis, None, None)?;
    match role {
        Role::Relay
            if contract.role_rules.relay.forbids_forging_keys
                && observation.live.has_forging_keys =>
        {
            return Err(OuroError::Validation(
                "pool spec declares relay but live node bears forging keys".into(),
            ))
        }
        // A non-target BP is part of the signed fleet snapshot, but its forging readiness does not
        // contribute to relay quorum. Preserve an unhealthy BP as `online: false` below instead of
        // making an unrelated relay restart impossible. A BP selected for a write still fails its
        // own target plan/readiness gates before and after mutation.
        _ => {}
    }
    let online = observation.readiness.as_ref().is_some_and(|evidence| {
        crate::readiness::Readiness {
            role,
            node_running: evidence.node_running,
            container_id_matches: !observation.live.container_id.is_empty(),
            socket_answers: evidence.socket_answers,
            network_ok: observation.live.network == network,
            genesis_ok: observation.live.genesis_hash == genesis,
            tip_block: evidence.tip_block,
            tip_block_next: evidence.tip_block_next,
            tip_synced: evidence.tip_synced,
            kes_opcert_valid: evidence.kes_opcert_valid,
            forging_credentials_ready: evidence.forging_credentials_ready,
            established_peers: evidence.established_peers,
        }
        .evaluate()
        .is_ok()
    });
    // KES rotation is a repair operation. Its Phase-B qualification is intentionally static on the
    // BP: a credential-caused restart loop cannot satisfy a socket/sync precondition. The control
    // permit separately requires a healthy declared relay and candidate-bound protocol evidence.
    // Supervisor, typed mounts and runtime contract were already checked above.
    let kes_rotation_repair_ready = role == Role::Bp
        && observation.readiness.as_ref().is_some_and(|evidence| {
            !observation.live.container_id.is_empty()
                && (evidence.node_running
                    || observation.live.container_running
                    || observation.live.container_restarting)
                && observation.live.network == network
                && observation.live.genesis_hash == genesis
                && evidence.block_producer_configured
                && observation.live.has_forging_keys
                && KesRotationPermissionEvidence::from_live(&observation.live).ready()
                && !observation.live.kes_opcert_id.is_empty()
        });
    let role_name = match role {
        Role::Bp => "bp",
        Role::Relay => "relay",
    };
    output::print_json(
        &ToolOutput::ok("ouro.fleet.status", false).with_data(json!({
            "node": node,
            "role": role_name,
            "network": observation.live.network,
            "genesis_hash": observation.live.genesis_hash,
            "host_key_sha256": observation.live.host_key_sha256,
            "online": online,
            "kes_rotation_repair_ready": kes_rotation_repair_ready,
            "keys_directory_safe": observation.live.keys_directory_safe,
            "kes_skey_private": observation.live.kes_skey_private,
            "vrf_skey_private": observation.live.vrf_skey_private,
            "image_config_digest": observation.live.image_config_digest,
            "state_generation": observation.live.container_creation_epoch,
            "container_running": observation.live.container_running,
            "container_restarting": observation.live.container_restarting,
            "container_status": observation.live.container_status,
            "assurance": "live_observation",
            "management_state": "not_required",
        })),
    )
}

fn stateless_observation_output(node: &str, observation: &Observation) -> ToolOutput {
    let (orchestration, orchestration_reason, compose) = observation.supervisor.upgrade_routing();
    let readiness = observation.readiness.as_ref();
    let runtime_policy = match convention::Allowlist::stable_contract() {
        Ok(contract) => {
            let conformity = observation
                .supervisor
                .require_base_conformant()
                .and_then(|_| require_typed_mounts(&observation.live.mounts))
                .and_then(|_| {
                    require_adoption_contract(
                        &contract,
                        observation,
                        &observation.live.network,
                        &observation.live.genesis_hash,
                        None,
                        None,
                    )
                });
            match conformity {
                Ok(()) => json!({
                    "supported": true,
                    "contract_id": contract.contract_id,
                    "convention_version": contract.convention_version,
                    "image_release_admission": "not_required_for_read",
                }),
                Err(error) => json!({
                    "supported": false,
                    "contract_id": contract.contract_id,
                    "convention_version": contract.convention_version,
                    "detail": error.to_string(),
                    "effect": "live layout does not conform to the stable contract",
                }),
            }
        }
        Err(error) => json!({
            "supported": null,
            "detail": error.to_string(),
            "effect": "stable layout contract unavailable; live read evidence is still returned",
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
                "orchestration": orchestration,
                "orchestration_reason": orchestration_reason,
                "compose": compose,
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

fn stateless_troubleshooting_output(
    node: &str,
    role: Role,
    observation: &Observation,
) -> ToolOutput {
    let readiness = observation.readiness.as_ref();
    let liveness_ready = readiness
        .is_some_and(|value| value.node_running && value.socket_answers && value.tip_synced);
    let role_name = match role {
        Role::Bp => "bp",
        Role::Relay => "relay",
    };

    let forging = match role {
        Role::Relay => json!({
            "applicable": false,
            "status": "not_applicable",
            "block_production_ready": null,
            "reason": "relay role does not forge blocks",
        }),
        Role::Bp => match readiness {
            None => json!({
                "applicable": true,
                "status": "evidence_unavailable",
                "block_production_ready": false,
                "reason": "target probe returned no readiness evidence",
                "opcert_present": !observation.live.kes_opcert_id.is_empty(),
                "keys_present": observation.live.has_forging_keys,
                "key_permissions_safe": observation.live.forging_key_permissions_safe,
            }),
            Some(value) => {
                let status = match value.kes.as_ref() {
                    None => "kes_evidence_unavailable",
                    Some(kes) if kes.current_period < kes.start_period => "opcert_not_yet_valid",
                    Some(kes) if kes.current_period >= kes.end_period => "opcert_expired",
                    Some(kes) if kes.counter_consistent == Some(false) => {
                        "opcert_counter_inconsistent"
                    }
                    Some(kes) if kes.counter_consistent.is_none() => {
                        "opcert_counter_evidence_unavailable"
                    }
                    Some(kes) if !kes.valid => "opcert_invalid",
                    Some(_) if !value.block_producer_configured => "forging_not_configured",
                    Some(_)
                        if observation.live.kes_opcert_id.is_empty()
                            || !observation.live.has_forging_keys =>
                    {
                        "credentials_missing"
                    }
                    Some(_) if !observation.live.forging_key_permissions_safe => {
                        "key_permissions_unsafe"
                    }
                    Some(_) if !value.forging_credentials_ready => "credentials_not_ready",
                    Some(_) => "ready",
                };
                json!({
                    "applicable": true,
                    "status": status,
                    "block_production_ready": status == "ready",
                    "configured": value.block_producer_configured,
                    "opcert_present": !observation.live.kes_opcert_id.is_empty(),
                    "keys_present": observation.live.has_forging_keys,
                    "key_permissions_safe": observation.live.forging_key_permissions_safe,
                    "credentials_ready": value.forging_credentials_ready,
                    "kes_opcert_valid": value.kes_opcert_valid,
                    "kes": value.kes.as_ref().map(|kes| json!({
                        "source": kes.source,
                        "current_period": kes.current_period,
                        "start_period": kes.start_period,
                        "end_period": kes.end_period,
                        "remaining_periods": kes.remaining_periods,
                        "opcert_counter_on_disk": kes.opcert_counter_on_disk,
                        "opcert_counter_node_state": kes.opcert_counter_node_state,
                        "counter_consistent": kes.counter_consistent,
                        "valid": kes.valid,
                    })),
                })
            }
        },
    };
    let role_ready = match role {
        Role::Bp => forging
            .get("block_production_ready")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        Role::Relay => readiness.is_some_and(|value| value.established_peers > 0),
    };
    let evidence_complete = readiness.is_some()
        && match role {
            Role::Bp => readiness
                .and_then(|value| value.kes.as_ref())
                .is_some_and(|kes| {
                    kes.current_period < kes.start_period
                        || kes.current_period >= kes.end_period
                        || kes.counter_consistent.is_some()
                }),
            Role::Relay => true,
        };
    let role_readiness = if !evidence_complete {
        "insufficient_evidence"
    } else if liveness_ready && role_ready {
        "ready"
    } else {
        "not_ready"
    };

    let mut result = ToolOutput::ok("ouro.troubleshooting.snapshot", false).with_data(json!({
        "op": "troubleshooting/snapshot",
        "node": node,
        "role": role_name,
        "assurance": "live_observation",
        "management_state": "not_required",
        "role_readiness": {
            "status": role_readiness,
            "scope": "current liveness, sync and role-specific evidence",
            "overall_health_claimed": false,
        },
        "result": {
            "liveness": {
                "node_running": readiness.map(|value| value.node_running).unwrap_or(false),
                "socket_answers": readiness.map(|value| value.socket_answers).unwrap_or(false),
                "tip_synced": readiness.map(|value| value.tip_synced).unwrap_or(false),
                "tip": {
                    "block": readiness.and_then(|value| value.tip_block_height),
                    "slot": readiness.and_then(|value| value.tip_slot),
                    "era": readiness.and_then(|value| value.tip_era.as_deref()),
                    "sync_progress": readiness.and_then(|value| value.sync_progress.as_deref()),
                },
            },
            "network": {
                "established_peers": readiness.map(|value| value.established_peers),
            },
            "forging": forging,
            "container": {
                "running_count": observation.supervisor.node_container_count,
                "runtime": observation.supervisor.runtime,
                "id": observation.live.container_id,
                "name": observation.live.container_name,
                "image_reference": observation.live.image_reference,
                "image_config_digest": observation.live.image_config_digest,
                "platform": observation.live.platform,
            },
        },
        "evidence_gaps": [
            "host resource saturation and storage growth",
            "recent log failures",
            "block-fetch latency and mempool pressure",
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
    container_running: bool,
    #[serde(default)]
    container_restarting: bool,
    #[serde(default)]
    container_status: String,
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
    #[serde(default)]
    keys_directory_safe: bool,
    #[serde(default)]
    kes_skey_private: bool,
    #[serde(default)]
    vrf_skey_private: bool,
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
            && destination.components().all(|component| {
                !matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            });
        let owner_ok = mount.owner.split_once(':').map(|(uid, gid)| {
            !uid.is_empty()
                && !gid.is_empty()
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

fn canonicalize_typed_mounts(mounts: &mut [TypedMount]) {
    mounts.sort_by(|left, right| {
        left.destination
            .cmp(&right.destination)
            .then_with(|| left.source_id.cmp(&right.source_id))
    });
}

fn require_adoption_contract(
    contract: &crate::convention::LayoutContract,
    observation: &Observation,
    expected_network: &str,
    expected_genesis: &str,
    expected_container: Option<&str>,
    expected_image: Option<&str>,
) -> Result<()> {
    if expected_network.is_empty() || expected_genesis.is_empty() {
        return Err(OuroError::Validation(
            "expected pool binding is incomplete".into(),
        ));
    }
    if observation.live.network.is_empty() || observation.live.genesis_hash.is_empty() {
        return Err(OuroError::Validation(format!(
            "adoption pool binding evidence unavailable: observed network={:?} genesis={:?}; no mismatch was established",
            observation.live.network, observation.live.genesis_hash
        )));
    }
    if observation.live.network != expected_network
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
        "blinklabs-cardano-node-v1" | "blinklabs-cardano-node-v2" => {
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
                || required
                    .strip_prefix(destination)
                    .is_some_and(|tail| tail.starts_with('/'))
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
    let mut observation: Observation = serde_json::from_str(&text)
        .map_err(|e| OuroError::Validation(format!("malformed observation: {e}")))?;
    if let Some(recreate) = observation.recreate.as_mut() {
        recreate.normalize_order();
    }
    Ok(observation)
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
        let dir = temp_root.join(format!("ouro-probe-{}", uuid::Uuid::new_v4().simple()));
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
            let bytes = crate::assets::asset("lib/ouro-probe.sh").ok_or_else(|| {
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
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn audit_refusal(args: &[String], operation: &str, error: &OuroError) -> Result<()> {
    let node = optional(args, "--node").unwrap_or("unknown");
    let paths = ConfigPaths::discover();
    audit_emit(
        &paths,
        "refusal",
        node,
        json!({
            "operation_id": operation,
            "outcome": "refused",
            "refusal_code": format!("exit_{}", error.exit_code()),
        }),
    )
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
    let bytes = serde_json::to_vec(&spec.canonicalized())
        .map_err(|e| OuroError::Validation(format!("cannot bind upgrade recreate spec: {e}")))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.trim().as_bytes())
        .map_err(|_| OuroError::Validation("invalid operation secret".into()))?;
    mac.update(&bytes);
    Ok(format!(
        "hmac-sha256:{}",
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn validate_digest_selector(name: &str, value: &str) -> Result<()> {
    let valid = value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if valid {
        Ok(())
    } else {
        Err(OuroError::Validation(format!(
            "{name} must be sha256:<64hex>"
        )))
    }
}

fn valid_ssh_sha256_fingerprint(value: &str) -> bool {
    value.strip_prefix("SHA256:").is_some_and(|encoded| {
        encoded.len() == 43
            && encoded
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
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
            "--node",
            "--role",
            "--observation",
            "--dispatch",
            "--bootstrap-user",
            "--ssh-key",
            "--spec",
            "--approve-token",
            "--expect-embedded",
            "--expect-allowlist",
            "--expected-role",
            "--expected-network",
            "--expected-genesis",
            "--expected-container",
            "--expected-image",
            "--expected-host-key",
        ],
        &["--local", "--preview", "--plan", "--identity-only"],
        &[],
    )?;
    if args.iter().any(|arg| arg == "--identity-only") {
        let allowed = ["--local", "--identity-only", "--expect-embedded"];
        if args.len() != 4
            || args
                .iter()
                .any(|arg| arg.starts_with("--") && !allowed.contains(&arg.as_str()))
        {
            return Err(OuroError::InvalidArgs(
                "--identity-only is target-internal and accepts only --local --expect-embedded <digest>"
                    .into(),
            ));
        }
        let expected = flag(args, "--expect-embedded")?;
        parity::require_expected_wire_digest(expected)?;
        output::print_json(
            &ToolOutput::ok("ouro.adopt.identity", false).with_data(json!({
                "security_identity": parity::SecurityIdentity::local().wire_digest(),
            })),
        )?;
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
            "--local",
            "--observation",
            "--expect-embedded",
            "--expect-allowlist",
            "--expected-role",
            "--expected-network",
            "--expected-genesis",
            "--expected-container",
            "--expected-image",
            "--expected-host-key",
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
        other => {
            return Err(OuroError::Validation(format!(
                "--role must be bp|relay, got {other}"
            )))
        }
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
        contract,
        &obs,
        expected_network,
        expected_genesis,
        expected_container,
        expected_image,
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
        std::fs::read_to_string(shared).map_err(|e| {
            OuroError::Validation(format!("cannot read shared adoption secret: {e}"))
        })?
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
        &tx_dir(&paths)
            .join("adopt-confirm-used")
            .join(format!("{node}.log")),
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
    audit_emit(
        &paths,
        "adopt",
        &node,
        json!({
            "approval_evidence_hash": att.immutable.approval_evidence_hash,
            "post_state_generation": 0,
            "outcome": "adopted",
        }),
    )?;
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
        Err(error) if error.is_reported() || error.is_audited() => Err(error),
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
            "--op",
            "--node",
            "--param",
            "--confirm-token",
            "--dispatch",
            "--ssh-key",
            "--spec",
            "--candidate-hash",
            "--artifact-file",
            "--observation",
            "--expect-embedded",
            "--expect-allowlist",
            "--fleet-pool-id",
            "--fleet-spec-digest",
            "--fleet-min-online-relays",
            "--fleet-permit",
        ],
        &[
            "--plan",
            "--artifact-preflight",
            "--transport-plan",
            "--local",
        ],
        &["--param"],
    )?;
    let op = flag(args, "--op")?.to_string();
    let node = flag(args, "--node")?.to_string();
    crate::intent::validate_machine_id(&node)?;
    let plan = args.iter().any(|a| a == "--plan");
    let artifact_preflight = args.iter().any(|a| a == "--artifact-preflight");
    let transport_plan = args.iter().any(|a| a == "--transport-plan");
    if usize::from(plan) + usize::from(artifact_preflight) + usize::from(transport_plan) > 1 {
        return Err(OuroError::InvalidArgs(
            "--plan, --artifact-preflight and --transport-plan are mutually exclusive".into(),
        ));
    }
    if (plan || artifact_preflight || transport_plan) && optional(args, "--confirm-token").is_some()
    {
        return Err(OuroError::Validation(
            "do not pass a confirm-token to plan/preflight/transport inspection; review the \
             target-validated plan and artifact preflight first, then mint approval"
                .into(),
        ));
    }
    if (plan || artifact_preflight || transport_plan) && optional(args, "--fleet-permit").is_some()
    {
        return Err(OuroError::Validation(
            "do not pass a fleet permit to plan/preflight/transport inspection; mint the \
             short-lived permit last only after exact operator approval"
                .into(),
        ));
    }
    if plan && optional(args, "--candidate-hash").is_some() {
        return Err(OuroError::Validation(
            "--plan derives a fresh candidate and never accepts an apply candidate".into(),
        ));
    }
    if plan && optional(args, "--artifact-file").is_some() {
        return Err(OuroError::Validation(
            "--artifact-file is not accepted during stateless planning".into(),
        ));
    }
    if artifact_preflight
        && (optional(args, "--candidate-hash").is_none()
            || optional(args, "--artifact-file").is_none())
    {
        return Err(OuroError::Validation(
            "--artifact-preflight requires --candidate-hash from the final plan and \
             --artifact-file containing the exact public opcert"
                .into(),
        ));
    }
    let paths = ConfigPaths::discover();

    // S0020 p1-2: reads do not depend on prior ownership metadata. A dispatched read streams the
    // control-selected runner; a local/debug read directly runs the same sealed live probe.
    if matches!(
        op.as_str(),
        "observability/health" | "troubleshooting/snapshot"
    ) {
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
        return if op == "observability/health" {
            output::print_json(&stateless_observation_output(&node, &observation))
        } else {
            let spec_path = flag(args, "--spec")?;
            let spec = PoolSpec::from_file(Path::new(spec_path))?;
            let machine = spec
                .machines
                .iter()
                .find(|machine| machine.id == node)
                .ok_or_else(|| {
                    OuroError::Validation(format!(
                        "troubleshooting node {node:?} is not declared in pool spec {spec_path}"
                    ))
                })?;
            let role = match machine.role {
                MachineRole::Bp => Role::Bp,
                MachineRole::Relay => Role::Relay,
            };
            output::print_json(&stateless_troubleshooting_output(&node, role, &observation))
        };
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

    if artifact_preflight {
        if op != "kes-rotation/install-opcert" {
            return Err(OuroError::Validation(
                "--artifact-preflight currently supports only kes-rotation/install-opcert".into(),
            ));
        }
        let host = optional(args, "--dispatch").ok_or_else(|| {
            OuroError::Validation(
                "--artifact-preflight requires --dispatch and the ephemeral target runner".into(),
            )
        })?;
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
        return dispatch_stateless_artifact_preflight(host, &op, &node, args, &paths);
    }

    // S0021 p3-1: every dispatched write is control-authorized and executed by the
    // ephemeral runner. It never falls through to the installed-CLI/attestation transaction path.
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
        return dispatch_stateless_apply(host, &op, &node, args, &paths, transport_plan);
    }
    if !(cfg!(debug_assertions) && optional(args, "--observation").is_some()) {
        return Err(OuroError::Validation(
            "current operations require --dispatch and the ephemeral target runner; use the internal `target` command only in target-side tests"
                .into(),
        ));
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
    let (active_contract, active_image) = active_allowlist
        .contract_and_image_for(&att.immutable.image_config_digest, &att.immutable.platform)?;
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
        OuroError::Validation(format!(
            "operation {op:?} is not in the privileged registry"
        ))
    })?;
    let fleet_sensitive = registered
        .touched
        .iter()
        .any(|resource| matches!(*resource, "container:restart" | "container:recreate"));
    let fleet_policy = if fleet_sensitive {
        let digest = flag(args, "--fleet-spec-digest")?.to_string();
        validate_digest_selector("--fleet-spec-digest", &digest)?;
        let pool_id = flag(args, "--fleet-pool-id")?.to_string();
        crate::intent::validate_machine_id(&pool_id)?;
        let min_online_relays = flag(args, "--fleet-min-online-relays")?
            .parse::<u32>()
            .map_err(|_| {
                OuroError::Validation(
                    "--fleet-min-online-relays must be an unsigned integer".into(),
                )
            })?;
        Some((digest, pool_id, min_online_relays))
    } else {
        if optional(args, "--fleet-spec-digest").is_some()
            || optional(args, "--fleet-pool-id").is_some()
            || optional(args, "--fleet-min-online-relays").is_some()
            || optional(args, "--fleet-permit").is_some()
        {
            return Err(OuroError::Validation(
                "fleet policy/permit arguments are accepted only for a disruptive operation".into(),
            ));
        }
        None
    };
    if op == "kes-rotation/install-opcert" && att.immutable.role != Role::Bp {
        return Err(OuroError::Validation(
            "kes-rotation/install-opcert is BP-only; a relay may never receive an opcert".into(),
        ));
    }
    if op == "upgrade/preload-image" {
        let target = payload
            .get("image")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                OuroError::Validation("upgrade/preload-image lost target image".into())
            })?;
        let observation = read_observation(args)?;
        require_current_contract_observation(&att, active_contract, &observation)?;
        let (recommended, _) = active_allowlist.recommended_upgrade_for(
            &att.immutable.image_config_digest,
            &observation.live.platform,
        )?;
        if target != recommended.image_config_digest {
            return Err(OuroError::Validation(format!(
                "upgrade target {target} is not the signed recommended image {} for {}; refused",
                recommended.image_config_digest, observation.live.platform
            )));
        }
    }
    let upgrade_snapshot = if op == "upgrade/step" {
        let target = payload
            .get("image")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("upgrade/step lost target image".into()))?;
        let observation = read_observation(args)?;
        require_current_contract_observation(&att, active_contract, &observation)?;
        let (recommended, transition) = active_allowlist.recommended_upgrade_for(
            &att.immutable.image_config_digest,
            &observation.live.platform,
        )?;
        if target != recommended.image_config_digest {
            return Err(OuroError::Validation(format!(
                "upgrade target {target} is not the signed recommended image {} for {}; refused",
                recommended.image_config_digest, observation.live.platform
            )));
        }
        crate::executor::require_image_present(target)?;
        let recreate = observation.recreate.as_ref().ok_or_else(|| {
            OuroError::Validation(
                "upgrade plan unavailable: probe could not model the full container run-spec"
                    .into(),
            )
        })?;
        let binding = recreate_spec_binding(&paths, local, recreate)?;
        Some((observation, transition.cloned(), binding))
    } else {
        None
    };
    let kes_candidate = if op == "kes-rotation/install-opcert" {
        let reference = payload
            .get("opcert")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("KES intent lost its opcert reference".into()))?;
        Some(validate_kes_candidate(
            &att,
            &paths.home.join("inbox"),
            reference,
        )?)
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
    }))
    .map_err(|e| OuroError::Validation(format!("cannot encode approved semantic state: {e}")))?;
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
    let payload_machine = intent
        .payload
        .get("machine")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            OuroError::Validation("intent payload is missing its machine binding".into())
        })?;
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
            &att,
            &tx_dir(&paths).join("locks"),
            &node,
            &audit_id,
            &probe,
        )?)
    };

    // Internal fleet-authority read: the normal adoption, parity, allowlist and live identity gate
    // above has already run. Return a closed status projection; an unhealthy node is DATA
    // (`online:false`), not a reason to fabricate availability.
    if op == "fleet/status" {
        let observation = read_observation(args)?;
        require_current_contract_observation(&att, active_contract, &observation)?;
        let online = require_readiness(&att, &observation, false).is_ok();
        audit_emit(
            &paths,
            "live_preflight",
            &node,
            json!({
                "operation_id": op,
                "intent_hash": canon,
                "pre_state_generation": att.state.state_generation,
                "outcome": "managed_read_validated",
            }),
        )?;
        audit_emit(
            &paths,
            "verified",
            &node,
            json!({
                "operation_id": op,
                "intent_hash": canon,
                "pre_state_generation": att.state.state_generation,
                "post_state_generation": att.state.state_generation,
                "outcome": "fleet_status_success",
            }),
        )?;
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
            let (expected_spec_digest, expected_pool_id, expected_min) =
                fleet_policy.as_ref().ok_or_else(|| {
                    OuroError::Validation("disruptive intent lost fleet policy".into())
                })?;
            permit.verify(
                &crate::fleet::PermitExpectation {
                    pool_id: expected_pool_id.clone(),
                    pool_spec_digest: expected_spec_digest.clone(),
                    node_id: node.clone(),
                    operation_id: op.clone(),
                    role: match att.immutable.role {
                        Role::Bp => "bp",
                        Role::Relay => "relay",
                    }
                    .into(),
                    target_image: if op == "upgrade/step" {
                        intent
                            .payload
                            .get("image")
                            .and_then(serde_json::Value::as_str)
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
    // Upgrade admission targets the signed recommendation. Exact transition metadata is optional
    // and grants automatic rollback only when it explicitly declares backward compatibility.
    let upgrade_transition = upgrade_snapshot
        .as_ref()
        .and_then(|(_, transition, _)| transition.as_ref());

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
            let recreate = upgrade_snapshot
                .as_ref()
                .and_then(|(observation, _, _)| observation.recreate.as_ref())
                .ok_or_else(|| {
                    OuroError::Validation("upgrade plan lost sealed recreate spec".into())
                })?;
            crate::executor::recreate_approval_argv(recreate, &att.state.container_id, target)?
        } else if matches!(
            op.as_str(),
            "kes-rotation/install-opcert" | "upgrade/preload-image"
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
                artifact_steps.insert(
                    0,
                    vec![
                        "docker".into(),
                        "cp".into(),
                        format!(
                            "{}:/opt/cardano/config/keys/node.cert",
                            att.state.container_id
                        ),
                        backup,
                    ],
                );
            }
            artifact_steps
        } else {
            crate::executor::build_plan(&intent, &att, None)?
        };
        audit_emit(
            &paths,
            "live_preflight",
            &node,
            json!({
                "operation_id": op,
                "intent_hash": canon,
                "pre_state_generation": att.state.state_generation,
                "outcome": "target_plan_validated",
            }),
        )?;
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
                "permit_freshness_seconds": crate::fleet::LIVE_FACTS_VALIDITY_SECONDS,
            })),
            "confirmation_required": spec.mutability == Mutability::Dangerous,
            "commit_recheck_required": true,
            "upgrade_transition": upgrade_transition,
            "upgrade_failure_outcome": if op == "upgrade/step" {
                Some(if upgrade_transition.is_some_and(crate::upgrade::rollback_possible) {
                    "RollbackToN"
                } else {
                    "ReSyncRequired"
                })
            } else {
                None
            },
            "kes_candidate": kes_candidate.as_ref().map(|candidate| json!({
                "counter": candidate.counter,
                "kes_period": candidate.kes_period,
                "cold_key_signature_valid": true,
                "public_kes_vkey_matches": true,
                "live_protocol_window_valid": true,
                "artifact_replay": false,
            })),
            "note": if fleet_sensitive {
                "target-validated final plan; no node runtime/config/attestation/inbox/transaction \
                 mutation (audit and private temporary probe metadata may be written). Approve this \
                 intent_hash, mint confirmation, then mint a short-lived 180-second fleet permit last and execute \
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
            std::fs::read_to_string(shared).map_err(|e| {
                OuroError::Validation(format!("cannot read shared confirm secret: {e}"))
            })?
        } else {
            crate::confirm::load_or_create_secret(&paths.tool_run_secret)?
        };
        let diff = format!("{op} on {node}");
        Some(crate::s0019_confirmation::verify(
            token,
            &canon,
            &diff,
            secret.trim().as_bytes(),
            crate::s0019_confirmation::current_epoch()?,
        )?)
    } else {
        None
    };

    audit_emit(
        &paths,
        "live_preflight",
        &node,
        json!({
            "operation_id": op,
            "intent_hash": canon,
            "pre_state_generation": att.state.state_generation,
            "outcome": "passed",
        }),
    )?;
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
        audit_emit(
            &paths,
            "verified",
            &node,
            json!({
                "operation_id": op,
                "intent_hash": canon,
                "pre_state_generation": att.state.state_generation,
                "post_state_generation": att.state.state_generation,
                "outcome": "managed_read_success",
            }),
        )?;
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
            .ok_or_else(|| {
                OuroError::Validation("upgrade/step needs image (an allowlisted digest)".into())
            })?;
        let spec = upgrade_snapshot
            .as_ref()
            .and_then(|(observation, _, _)| observation.recreate.as_ref())
            .ok_or_else(|| {
                OuroError::Validation("upgrade lost the approved recreate spec".into())
            })?;
        let commit = crate::executor::recreate_argv(spec, &att.state.container_id, to_digest)?;
        let rb = if upgrade_transition.is_some_and(crate::upgrade::rollback_possible) {
            Some(crate::executor::upgrade_rollback_plan(&att, spec)?)
        } else {
            None
        };
        (commit, rb)
    } else {
        crate::executor::recoverable_plans(
            &intent,
            &att,
            &inbox,
            &tx_dir(&paths).join("rollback").join(&canon),
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
    let managed_changing = matches!(op.as_str(), "kes-rotation/install-opcert");
    let to_digest_owned = intent
        .payload
        .get("image")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let approved_recreate_binding = upgrade_snapshot
        .as_ref()
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
        let recreate = fresh.recreate.as_ref().ok_or_else(|| {
            OuroError::Validation(
                "upgrade commit recheck could no longer model the full container run-spec".into(),
            )
        })?;
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
        let reference = intent
            .payload
            .get("opcert")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| OuroError::Validation("KES intent lost its opcert reference".into()))?;
        validate_kes_candidate(&att, &inbox, reference)?;
    }
    let commit = || crate::executor::run_plan(&commit_plan);
    let verify = || {
        let live = read_observation(args)?;
        if is_upgrade {
            let (target_contract, _) =
                active_allowlist.contract_and_image_for(&to_digest_owned, &live.live.platform)?;
            require_contract_shape_and_role(&att, target_contract, &live)?;
        } else {
            require_contract_shape_and_role(&att, active_contract, &live)?;
        }
        if !is_preload {
            require_readiness(&att, &live, is_upgrade)?;
        }
        if is_preload {
            let target = intent
                .payload
                .get("image")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    OuroError::Validation("preload intent lost its target digest".into())
                })?;
            crate::executor::require_image_present(target)?;
        }
        if is_upgrade {
            if live.live.image_config_digest != to_digest_owned {
                return Err(OuroError::Validation(
                    "upgrade did not land on the target image digest — rolling back (§2.10)".into(),
                ));
            }
            if live.live.container_id.is_empty() {
                return Err(OuroError::Validation(
                    "no node container after upgrade — rolling back".into(),
                ));
            }
            rotate_attestation_for_upgrade(&paths, &node, local, &att, &live, &to_digest_owned)?;
            audit_emit(
                &paths,
                "attestation_rotation",
                &node,
                json!({
                    "operation_id": op,
                    "intent_hash": canon,
                    "pre_state_generation": att.state.state_generation,
                    "post_state_generation": att.state.state_generation.saturating_add(1),
                    "outcome": "upgrade_identity_rotated",
                }),
            )
        } else if managed_changing {
            // Immutable identity must still hold (an image swap / recreate is still caught); the
            // content hashes are expected to have changed → snapshot them as the new baseline.
            att.require_identity_matches(&live.live.to_live())?;
            if op == "kes-rotation/install-opcert" {
                let expected = intent
                    .payload
                    .get("opcert")
                    .and_then(|value| value.as_str())
                    .and_then(artifact_ref_digest)
                    .ok_or_else(|| {
                        OuroError::Validation("KES intent lost its artifact digest".into())
                    })?;
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
            audit_emit(
                &paths,
                "attestation_rotation",
                &node,
                json!({
                    "operation_id": op,
                    "intent_hash": canon,
                    "pre_state_generation": att.state.state_generation,
                    "post_state_generation": advanced.state.state_generation,
                    "outcome": "managed_state_advanced",
                }),
            )
        } else {
            att.require_matches_live(&live.live.to_live())
        }
    };
    let rollback = || {
        let plan = rb_plan.as_ref().ok_or_else(|| {
            OuroError::Validation(format!(
                "{} has no safe automatic rollback; operator reconciliation required",
                op
            ))
        })?;
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
            &tx_dir(&paths)
                .join("confirm-used")
                .join(format!("{node}.log")),
            confirmation,
        )?;
    }
    let ops = TxOps {
        commit: &commit,
        verify: &verify,
        rollback: &rollback,
    };
    let post_generation = if is_upgrade || managed_changing {
        att.state.state_generation.saturating_add(1)
    } else {
        att.state.state_generation
    };
    let observe = |state: TxState| {
        let mut fields = serde_json::Map::new();
        fields.insert("operation_id".into(), json!(op));
        fields.insert("intent_hash".into(), json!(canon));
        fields.insert(
            "pre_state_generation".into(),
            json!(att.state.state_generation),
        );
        if matches!(state, TxState::Verified | TxState::RolledBack) {
            fields.insert(
                "post_state_generation".into(),
                json!(
                    if state == TxState::Verified || (state == TxState::RolledBack && is_upgrade) {
                        post_generation
                    } else {
                        att.state.state_generation
                    }
                ),
            );
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
    validate_closed_args(
        args,
        &["--op", "--node", "--intent-hash", "--ttl"],
        &[],
        &[],
    )?;
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
        hash,
        &diff,
        secret.as_bytes(),
        crate::s0019_confirmation::current_epoch()?,
        ttl_seconds,
    )?;
    output::print_json(
        &ToolOutput::ok("ouro.confirm.create", false).with_data(json!({
            "op": op, "node": node, "intent_hash": hash, "diff": diff,
            "confirm_token": token, "expires_at_epoch": expires_at, "single_use": true,
        })),
    )?;
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
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    output::print_json(
        &ToolOutput::ok("ouro.confirm.adopt.create", false).with_data(json!({
            "node": node,
            "candidate_hash": candidate,
            "host_key_sha256": host_key,
            "diff": diff,
            "approve_token": token,
            "expires_at_epoch": expires_at,
            "single_use": true,
        })),
    )?;
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
    if matches!(op, "observability/health" | "troubleshooting/snapshot") {
        return dispatch_stateless_observe(host, op, node, args, paths, transport_plan);
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
        output::print_json(
            &ToolOutput::ok("ouro.op.dispatch.transport_plan", false).with_data(json!({
                "op": op, "node": node, "target": host, "principal": "ouro-op",
                "ssh_argv": argv,
                "target_validated": false,
                "note": "transport-only inspection: confined + host-key-pinned SSH argv; registry, \
                         adoption, allowlist, parity and live state have NOT been validated",
            })),
        )?;
        return Ok(());
    }
    let out = crate::ssh::bounded_ssh(
        &argv,
        std::time::Duration::from_secs(15 * 60),
        256 * 1024,
        "managed operation SSH dispatch",
    )
    .map_err(|e| OuroError::Validation(format!("ssh dispatch failed: {e}")))?;
    finish_ssh_dispatch("ouro.op.dispatch", &out)
}

fn dispatch_stateless_observe(
    host: &str,
    op: &str,
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
                "{forbidden} is not valid for a stateless read"
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
                "stateless reads accept only an optional machine=<node> parameter".into(),
            ));
        }
    }

    let spec_path = flag(args, "--spec")?;
    let spec = PoolSpec::from_file(Path::new(spec_path))?;
    let machine = spec
        .machines
        .iter()
        .find(|machine| machine.id == node)
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "stateless read node {node:?} is not declared in pool spec {spec_path}"
            ))
        })?;
    if machine.ssh.host != host {
        return Err(OuroError::Validation(format!(
            "stateless read dispatch host {host:?} does not match pool-spec host {:?} for {node}",
            machine.ssh.host
        )));
    }
    let supplied_key = optional(args, "--ssh-key")
        .map(crate::secrets::CredentialRef::parse)
        .transpose()?;
    if supplied_key
        .as_ref()
        .is_some_and(|key| key != &machine.ssh.key_ref)
    {
        return Err(OuroError::Validation(format!(
            "stateless read --ssh-key does not match the pool-spec credential for {node}"
        )));
    }
    let key = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    let runner = crate::runner::linux_x86_64()?;
    let mut target_args = vec![
        "target".to_string(),
        "observe".to_string(),
        "--node".to_string(),
        node.to_string(),
    ];
    if op == "troubleshooting/snapshot" {
        target_args.extend([
            "--op".to_string(),
            op.to_string(),
            "--role".to_string(),
            match machine.role {
                MachineRole::Bp => "bp".to_string(),
                MachineRole::Relay => "relay".to_string(),
            },
        ]);
    }
    let argv = crate::dispatch::ephemeral_runner_dispatch_argv(
        host,
        machine.ssh.port,
        &machine.ssh.user,
        &key,
        &paths.known_hosts,
        &runner.sha256,
        &target_args,
    )?;
    if transport_plan {
        let tool = if op == "troubleshooting/snapshot" {
            "ouro.troubleshooting.snapshot.dispatch.transport_plan"
        } else {
            "ouro.observe.dispatch.transport_plan"
        };
        output::print_json(&ToolOutput::ok(tool, false).with_data(json!({
            "op": op,
            "node": node,
            "target": host,
            "principal": machine.ssh.user,
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
        })))?;
        return Ok(());
    }
    let out = crate::ssh::bounded_ssh_with_input(
        &argv,
        &runner.bytes,
        std::time::Duration::from_secs(5 * 60),
        256 * 1024,
        "ephemeral stateless read SSH dispatch",
    )
    .map_err(|error| OuroError::Validation(format!("ssh dispatch failed: {error}")))?;
    finish_ssh_dispatch(
        if op == "troubleshooting/snapshot" {
            "ouro.troubleshooting.snapshot.dispatch"
        } else {
            "ouro.observe.dispatch"
        },
        &out,
    )
}

struct StatelessDispatchContext {
    spec: PoolSpec,
    machine: Machine,
    pool_spec_digest: String,
    pool_id: String,
    target_args: Vec<String>,
    key: PathBuf,
}

fn stateless_dispatch_context(
    host: &str,
    op: &str,
    node: &str,
    args: &[String],
    paths: &ConfigPaths,
    target_action: &str,
) -> Result<StatelessDispatchContext> {
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
        })?
        .clone();
    if machine.ssh.host != host {
        return Err(OuroError::Validation(format!(
            "dispatch host {host:?} does not match pool-spec host {:?} for {node}",
            machine.ssh.host
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
    if optional(args, "--fleet-min-online-relays")
        .is_some_and(|supplied| supplied != spec.upgrade.min_online_relays.to_string())
    {
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
        target_action.into(),
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
        pool_id.clone(),
        "--pool-spec-digest".into(),
        pool_spec_digest.clone(),
        "--min-online-relays".into(),
        spec.upgrade.min_online_relays.to_string(),
    ];
    if matches!(op, "upgrade/preload-image" | "upgrade/step") {
        let catalog = convention::fetch_release_catalog()?;
        target_args.push("--release-policy".into());
        target_args.push(catalog.document);
    }
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
    Ok(StatelessDispatchContext {
        spec,
        machine,
        pool_spec_digest,
        pool_id,
        target_args,
        key,
    })
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
    let context = stateless_dispatch_context(host, op, node, args, paths, "plan")?;
    let runner = crate::runner::linux_x86_64()?;
    let argv = crate::dispatch::ephemeral_runner_dispatch_argv(
        host,
        context.machine.ssh.port,
        &context.machine.ssh.user,
        &context.key,
        &paths.known_hosts,
        &runner.sha256,
        &context.target_args,
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

fn stateless_apply_terminal(result: &crate::ssh::SshOutcome) -> (&'static str, &'static str) {
    let typed = serde_json::from_slice::<serde_json::Value>(result.stdout.as_bytes())
        .ok()
        .filter(|value| {
            value.is_object() && value.get("tool").is_some() && value.get("status").is_some()
        });
    if result.status == 0
        && typed.as_ref().is_some_and(|value| {
            value.get("status").and_then(serde_json::Value::as_str) == Some("ok")
        })
    {
        return ("apply_succeeded", "verified_success");
    }
    let error_code = typed
        .as_ref()
        .and_then(|value| value.pointer("/error/code"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if error_code == "submission_ambiguous" {
        return ("apply_ambiguous", "submission_outcome_unknown_no_retry");
    }
    let detail = typed
        .as_ref()
        .and_then(|value| value.pointer("/error/detail"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if detail.contains("live-state rollback completed") {
        return ("apply_rolled_back", "mutation_rolled_back_and_verified");
    }
    if typed.is_some() {
        return ("apply_failed", "typed_target_failure");
    }
    ("apply_ambiguous", "untyped_or_transport_outcome")
}

fn dispatch_stateless_artifact_preflight(
    host: &str,
    op: &str,
    node: &str,
    args: &[String],
    paths: &ConfigPaths,
) -> Result<()> {
    let candidate = flag(args, "--candidate-hash")?;
    if candidate.len() != 64
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OuroError::Validation(
            "--candidate-hash must be the 64 lowercase hex value from the target plan".into(),
        ));
    }
    let reference = collect_params(args)?
        .get("opcert")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| OuroError::Validation("KES preflight lost opcert reference".into()))?
        .to_string();
    let artifact_path = flag(args, "--artifact-file")?;
    let (file, preview) =
        crate::inbox::preview_source(crate::inbox::ArtifactType::Opcert, Path::new(artifact_path))?;
    if preview.artifact_ref != reference {
        return Err(OuroError::Validation(
            "--artifact-file bytes do not match the candidate-bound opcert reference".into(),
        ));
    }
    let digest = artifact_ref_digest(&reference)
        .ok_or_else(|| OuroError::Validation("artifact reference lost its digest".into()))?;
    let mut context = stateless_dispatch_context(host, op, node, args, paths, "preflight")?;
    let relay = context
        .spec
        .machines
        .iter()
        .find(|machine| machine.role == MachineRole::Relay)
        .ok_or_else(|| {
            OuroError::Validation(
                "KES preflight requires one declared relay for protocol evidence".into(),
            )
        })?;
    let protocol_evidence = fetch_kes_protocol_evidence(
        relay,
        paths,
        context.spec.pool.network.as_str(),
        &context.spec.pool.genesis_hashes.shelley,
        Path::new(artifact_path),
    )?;
    context.target_args.push("--candidate-hash".into());
    context.target_args.push(candidate.to_string());
    context.target_args.push("--kes-protocol-evidence".into());
    context
        .target_args
        .push(serde_json::to_string(&protocol_evidence).map_err(|error| {
            OuroError::Validation(format!("cannot encode KES protocol evidence: {error}"))
        })?);
    let runner = crate::runner::linux_x86_64()?;
    let argv = crate::dispatch::ephemeral_runner_payload_dispatch_argv(
        host,
        context.machine.ssh.port,
        &context.machine.ssh.user,
        &context.key,
        &paths.known_hosts,
        crate::dispatch::EphemeralPayloadInput {
            runner_sha256: &runner.sha256,
            runner_size: runner.bytes.len(),
            payload_sha256: digest,
            payload_size: preview.size_bytes,
        },
        &context.target_args,
    )?;
    let out = crate::ssh::bounded_ssh_with_payload(
        &argv,
        &runner.bytes,
        file,
        std::time::Duration::from_secs(10 * 60),
        256 * 1024,
        "ephemeral stateless KES artifact preflight SSH dispatch",
    )
    .map_err(|error| OuroError::Validation(format!("ssh dispatch failed: {error}")))?;
    finish_ssh_dispatch("ouro.op.artifact_preflight.dispatch", &out)
}

fn dispatch_stateless_apply(
    host: &str,
    op: &str,
    node: &str,
    args: &[String],
    paths: &ConfigPaths,
    transport_plan: bool,
) -> Result<()> {
    let registered = crate::intent::lookup(op).ok_or_else(|| {
        OuroError::Validation(format!("operation {op:?} is not in the typed registry"))
    })?;
    if registered.mutability == Mutability::Read {
        return Err(OuroError::Validation(format!(
            "operation {op:?} is not a stateless apply"
        )));
    }
    let candidate = flag(args, "--candidate-hash")?;
    if candidate.len() != 64
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OuroError::Validation(
            "--candidate-hash must be the 64 lowercase hex value from the target plan".into(),
        ));
    }
    let mut context = stateless_dispatch_context(host, op, node, args, paths, "apply")?;
    context.target_args.push("--approved-candidate".into());
    context.target_args.push(candidate.to_string());

    let expected_artifact = match op {
        "kes-rotation/install-opcert" => Some((
            crate::inbox::ArtifactType::Opcert,
            collect_params(args)?
                .get("opcert")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| OuroError::Validation("KES intent lost opcert reference".into()))?
                .to_string(),
        )),
        _ => None,
    };
    let payload = if let Some((kind, expected_ref)) = expected_artifact {
        let artifact_path = flag(args, "--artifact-file")?;
        let (file, preview) = crate::inbox::preview_source(kind, Path::new(artifact_path))?;
        if preview.artifact_ref != expected_ref {
            return Err(OuroError::Validation(
                "--artifact-file bytes do not match the candidate-bound artifact reference".into(),
            ));
        }
        let digest = artifact_ref_digest(&expected_ref)
            .ok_or_else(|| OuroError::Validation("artifact reference lost its digest".into()))?
            .to_string();
        Some((file, preview, digest))
    } else {
        if optional(args, "--artifact-file").is_some() {
            return Err(OuroError::Validation(
                "--artifact-file is accepted only for KES install".into(),
            ));
        }
        None
    };

    let runner = crate::runner::linux_x86_64()?;
    let build_argv = |target_args: &[String]| -> Result<Vec<String>> {
        if let Some((_, preview, digest)) = &payload {
            crate::dispatch::ephemeral_runner_payload_dispatch_argv(
                host,
                context.machine.ssh.port,
                &context.machine.ssh.user,
                &context.key,
                &paths.known_hosts,
                crate::dispatch::EphemeralPayloadInput {
                    runner_sha256: &runner.sha256,
                    runner_size: runner.bytes.len(),
                    payload_sha256: digest,
                    payload_size: preview.size_bytes,
                },
                target_args,
            )
        } else {
            crate::dispatch::ephemeral_runner_dispatch_argv(
                host,
                context.machine.ssh.port,
                &context.machine.ssh.user,
                &context.key,
                &paths.known_hosts,
                &runner.sha256,
                target_args,
            )
        }
    };
    if transport_plan {
        let argv = build_argv(&context.target_args)?;
        output::print_json(
            &ToolOutput::ok("ouro.op.apply.dispatch.transport_plan", false).with_data(json!({
                "op": op,
                "node": node,
                "target": host,
                "principal": context.machine.ssh.user,
                "candidate_hash": candidate,
                "runner": {
                    "platform": runner.platform,
                    "sha256": runner.sha256,
                    "size_bytes": runner.bytes.len(),
                    "source": "control_build",
                },
                "artifact": payload.as_ref().map(|(_, preview, _)| json!({
                    "artifact_ref": preview.artifact_ref,
                    "size_bytes": preview.size_bytes,
                    "transport": "same one-shot private stream after runner bytes",
                })),
                "ssh_argv": argv,
                "target_validated": false,
                "approval_consumed": false,
                "persistent_target_install": false,
                "note": "transport-only inspection; no SSH session or mutation ran",
            })),
        )?;
        return Ok(());
    }

    let secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let token = flag(args, "--confirm-token")?;
    let diff = format!("{op} on {node}");
    let confirmation = crate::s0019_confirmation::verify(
        token,
        candidate,
        &diff,
        secret.trim().as_bytes(),
        crate::s0019_confirmation::current_epoch()?,
    )?;

    let fleet_sensitive = registered
        .touched
        .iter()
        .any(|resource| matches!(*resource, "container:restart" | "container:recreate"));
    let mut audit_fencing_token = None;
    if fleet_sensitive {
        let raw = flag(args, "--fleet-permit")?;
        let permit: crate::fleet::StepPermit = serde_json::from_str(raw)
            .map_err(|error| OuroError::Validation(format!("malformed fleet permit: {error}")))?;
        let role = match context.machine.role {
            MachineRole::Bp => "bp",
            MachineRole::Relay => "relay",
        };
        let target_image = if op == "upgrade/step" {
            collect_params(args)?
                .get("image")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        } else {
            None
        };
        let expected = crate::fleet::PermitExpectation {
            pool_id: context.pool_id.clone(),
            pool_spec_digest: context.pool_spec_digest.clone(),
            node_id: node.to_string(),
            operation_id: op.to_string(),
            role: role.into(),
            target_image,
            min_online_relays: context.spec.upgrade.min_online_relays,
            network: context.spec.pool.network.as_str().into(),
            genesis_hash: context.spec.pool.genesis_hashes.shelley.clone(),
            target_host_key_sha256: pinned_ed25519_host_key(
                host,
                context.machine.ssh.port,
                &paths.known_hosts,
            )?,
            intent_hash: candidate.to_string(),
        };
        permit.verify(
            &expected,
            secret.trim().as_bytes(),
            crate::s0019_confirmation::current_epoch()?,
        )?;
        permit.require_live_relay_quorum()?;
        audit_fencing_token = Some(permit.fencing_token);
        // Only the exact permit authenticated above enters the internal target argv. The target
        // cannot accept an independently supplied public selector for this field.
        context.target_args.push("--verified-fleet-permit".into());
        context.target_args.push(raw.to_string());
    } else if optional(args, "--fleet-permit").is_some() {
        return Err(OuroError::Validation(
            "this non-disruptive operation does not accept a fleet permit".into(),
        ));
    }
    let argv = build_argv(&context.target_args)?;

    // The local lock serializes verification + durable single-use consumption + the SSH apply.
    // Target state remains only Docker/filesystem truth; no remote Ouro lock/journal is created.
    let _control_lock = crate::gate::NodeLock::acquire(
        &paths.home.join("stateless-control/locks"),
        node,
        &format!("apply-{candidate}"),
    )?;
    crate::s0019_confirmation::consume(
        &paths
            .home
            .join("stateless-control/confirm-used")
            .join(format!("{node}.log")),
        &confirmation,
    )?;
    let audit_fields = |outcome: &str| {
        let mut fields = serde_json::Map::new();
        fields.insert("operation_id".into(), json!(op));
        fields.insert("intent_hash".into(), json!(candidate));
        fields.insert("outcome".into(), json!(outcome));
        if let Some(fencing_token) = audit_fencing_token {
            fields.insert("fencing_token".into(), json!(fencing_token));
        }
        serde_json::Value::Object(fields)
    };
    audit_emit(
        paths,
        "apply_attempt",
        node,
        audit_fields("dispatch_pending"),
    )?;

    let dispatched = if let Some((file, _, _)) = payload {
        crate::ssh::bounded_ssh_with_payload(
            &argv,
            &runner.bytes,
            file,
            std::time::Duration::from_secs(15 * 60),
            256 * 1024,
            "ephemeral stateless artifact apply SSH dispatch",
        )
    } else {
        crate::ssh::bounded_ssh_with_input(
            &argv,
            &runner.bytes,
            std::time::Duration::from_secs(15 * 60),
            256 * 1024,
            "ephemeral stateless apply SSH dispatch",
        )
    };
    let out = match dispatched {
        Ok(out) => out,
        Err(error) => {
            audit_emit(paths, "apply_ambiguous", node, audit_fields("transport_error_after_dispatch"))
                .map_err(|audit_error| OuroError::Audited {
                    message: format!(
                        "stateless apply transport and terminal audit both failed; live reconciliation required: transport={error}; audit={audit_error}"
                    ),
                    exit_code: 20,
                })?;
            return Err(OuroError::Audited {
                message: format!(
                    "stateless apply transport failed after approval consumption; outcome is ambiguous and must be reconciled from live state: {error}"
                ),
                exit_code: 20,
            });
        }
    };
    let (event, outcome) = stateless_apply_terminal(&out);
    audit_emit(paths, event, node, audit_fields(outcome)).map_err(|audit_error| {
        OuroError::Audited {
            message: format!(
                "stateless apply returned a terminal target result but control audit failed; live reconciliation required: {audit_error}"
            ),
            exit_code: 20,
        }
    })?;
    finish_ssh_dispatch("ouro.op.apply.dispatch", &out)
}

/// p6-3 — SSH-dispatch `adopt` to the target (as the bootstrap account), running `adopt --local`
/// there. Control-only flags are stripped; the target self-probes (p6-2).
fn dispatch_adopt(
    host: &str,
    node: &str,
    args: &[String],
    paths: &ConfigPaths,
    plan: bool,
) -> Result<()> {
    let spec_path = flag(args, "--spec")?;
    let spec = PoolSpec::from_file(std::path::Path::new(spec_path))?;
    let machine = spec
        .machines
        .iter()
        .find(|machine| machine.id == node)
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "adoption node {node} is not declared in the pool spec"
            ))
        })?;
    if machine
        .runtime
        .as_ref()
        .is_some_and(|runtime| runtime.mode != RuntimeMode::Docker)
    {
        return Err(OuroError::Validation(format!(
            "adoption runtime mismatch: pool spec declares {node} as non-Docker, but S0019 only \
             adopts the pinned Docker convention; correct the operator-owned spec after review"
        )));
    }
    let expected_role = match machine.role {
        MachineRole::Bp => "bp",
        MachineRole::Relay => "relay",
    };
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
    if let Some(container) = machine
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.container.as_ref())
    {
        remote.push("--expected-container".into());
        remote.push(container.clone());
    }
    if let Some(image) = machine
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.image.as_ref())
    {
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
    )
    .map_err(|e| OuroError::Validation(format!("adopt identity preflight failed: {e}")))?;
    let identity: serde_json::Value = serde_json::from_slice(identity_out.stdout.as_bytes()).map_err(|_| {
        OuroError::Validation(
            "target does not support the exact adoption security-identity preflight; update it before adoption"
                .into(),
        )
    })?;
    if identity_out.status != 0
        || identity.get("tool").and_then(serde_json::Value::as_str) != Some("ouro.adopt.identity")
        || identity
            .pointer("/data/security_identity")
            .and_then(serde_json::Value::as_str)
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
    )
    .map_err(|e| OuroError::Validation(format!("ssh dispatch failed: {e}")))?;
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
            OuroError::Validation(format!(
                "cannot inspect pinned host key for {target}: {error}"
            ))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeStateCounterEvidence {
    Present(u64),
    NoBlocksMintedYet,
}

impl NodeStateCounterEvidence {
    fn status(self) -> &'static str {
        match self {
            Self::Present(_) => "present",
            Self::NoBlocksMintedYet => "no_blocks_minted_yet",
        }
    }

    fn value(self) -> Option<u64> {
        match self {
            Self::Present(value) => Some(value),
            Self::NoBlocksMintedYet => None,
        }
    }
}

#[derive(Clone, Debug)]
struct KesCandidateValidation {
    parsed: crate::kes::ParsedOperationalCertificate,
    node_state_counter: NodeStateCounterEvidence,
    active_opcert_counter: Option<u64>,
    cold_identity_bound: bool,
}

fn json_u64(value: &serde_json::Value, key: &str) -> Result<u64> {
    match value.get(key) {
        None => Err(OuroError::Validation(format!(
            "cardano-cli KES schema incompatible: omitted {key}"
        ))),
        Some(raw) => raw.as_u64().ok_or_else(|| {
            OuroError::Validation(format!(
                "cardano-cli KES output malformed: {key} must be an unsigned integer"
            ))
        }),
    }
}

fn node_state_counter(value: &serde_json::Value) -> Result<NodeStateCounterEvidence> {
    const KEY: &str = "qKesNodeStateOperationalCertificateNumber";
    match value.get(KEY) {
        None => Err(OuroError::Validation(format!(
            "cardano-cli KES schema incompatible: omitted {KEY}"
        ))),
        Some(serde_json::Value::Null) => Ok(NodeStateCounterEvidence::NoBlocksMintedYet),
        Some(raw) => raw
            .as_u64()
            .map(NodeStateCounterEvidence::Present)
            .ok_or_else(|| {
                OuroError::Validation(format!(
                    "cardano-cli KES output malformed: {KEY} must be an unsigned integer or null"
                ))
            }),
    }
}

fn read_public_opcert(
    container: &str,
    path: &str,
    context: &str,
) -> Result<(crate::kes::ParsedOperationalCertificate, String)> {
    let raw = crate::executor::run_read_plan(&[vec![
        "docker".into(),
        "exec".into(),
        container.into(),
        "head".into(),
        "-c".into(),
        "65537".into(),
        path.into(),
    ]])
    .map(|raw| raw.into_bytes())
    .or_else(|_| crate::executor::read_fixed_public_file(container, path, 65_536))
    .map_err(|error| OuroError::Validation(format!("cannot read {context}: {error}")))?;
    if raw.len() > 65_536 {
        return Err(OuroError::Validation(format!("{context} exceeds 64 KiB")));
    }
    let parsed = crate::kes::parse_operational_certificate(&raw).map_err(|error| {
        OuroError::Validation(format!("{context} cannot establish cold identity: {error}"))
    })?;
    Ok((parsed, crate::intent::sha256_hex(&raw)))
}

fn validate_kes_protocol_facts(
    facts: &serde_json::Value,
    parsed: crate::kes::ParsedOperationalCertificate,
    container: &str,
    identity_opcert_path: &str,
    expected_identity_opcert_digest: &str,
) -> Result<KesCandidateValidation> {
    let current = json_u64(facts, "qKesCurrentKesPeriod")?;
    let start = json_u64(facts, "qKesStartKesInterval")?;
    let end = json_u64(facts, "qKesEndKesInterval")?;
    let on_disk = json_u64(facts, "qKesOnDiskOperationalCertificateNumber")?;
    let node_state = node_state_counter(facts)?;
    if parsed.counter != on_disk || parsed.kes_period != start || current < start || current >= end
    {
        return Err(OuroError::Validation(format!(
            "prospective opcert is stale/out-of-period/inconsistent: counter={on_disk}, node_state={}, period={start}..{end}, current={current}",
            node_state
                .value()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".into())
        )));
    }
    match node_state {
        NodeStateCounterEvidence::Present(value)
            if on_disk < value || on_disk > value.saturating_add(1) =>
        {
            Err(OuroError::Validation(format!(
                "prospective opcert is stale/out-of-period/inconsistent: counter={on_disk}, node_state={value}, period={start}..{end}, current={current}"
            )))
        }
        NodeStateCounterEvidence::Present(_) => Ok(KesCandidateValidation {
            parsed,
            node_state_counter: node_state,
            active_opcert_counter: None,
            cold_identity_bound: true,
        }),
        NodeStateCounterEvidence::NoBlocksMintedYet => {
            let (active, active_digest) = read_public_opcert(
                container,
                identity_opcert_path,
                "fixed public identity node.cert",
            )?;
            if active_digest != expected_identity_opcert_digest {
                return Err(OuroError::Validation(
                    "fixed public identity node.cert changed after the candidate-bound observation"
                        .into(),
                ));
            }
            if active.cold_verification_key != parsed.cold_verification_key {
                return Err(OuroError::Validation(
                    "prospective opcert cold key does not match the verified active opcert while protocol state has no counter"
                        .into(),
                ));
            }
            if parsed.counter <= active.counter {
                return Err(OuroError::Validation(format!(
                    "prospective opcert counter {} must be greater than verified active opcert counter {} while protocol state has no counter",
                    parsed.counter, active.counter
                )));
            }
            Ok(KesCandidateValidation {
                parsed,
                node_state_counter: node_state,
                active_opcert_counter: Some(active.counter),
                cold_identity_bound: true,
            })
        }
    }
}

/// `cardano-cli query kes-period-info --output-json` writes human ✓/✗ diagnostics before its JSON
/// object. Extract exactly one bounded terminal object; never accept a second/trailing structured
/// record that could make an agent and the validator approve different facts.
fn parse_cardano_cli_json(raw: &[u8], context: &str) -> Result<serde_json::Value> {
    if raw.len() > 65_536 {
        return Err(OuroError::Validation(format!(
            "{context} output exceeds 64 KiB"
        )));
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| OuroError::Validation(format!("{context} output is not UTF-8")))?;
    let start = text
        .find('{')
        .ok_or_else(|| OuroError::Validation(format!("{context} omitted its JSON object")))?;
    serde_json::from_str(&text[start..]).map_err(|error| {
        OuroError::Validation(format!("{context} has malformed or trailing JSON: {error}"))
    })
}

/// Deep validation for the public opcert carried only by the current ephemeral invocation. It
/// binds the candidate bytes to the BP's exact staged public KES verification key and protocol window;
/// neither the KES signing key nor cold key is opened or transported.
fn validate_ephemeral_kes_candidate(
    plan: &StatelessTargetPlan,
    path: &Path,
    reference: &str,
    protocol_evidence: Option<&crate::fleet::KesProtocolEvidence>,
) -> Result<KesCandidateValidation> {
    let digest = artifact_ref_digest(reference).ok_or_else(|| {
        OuroError::Validation("KES intent has a malformed artifact reference".into())
    })?;
    let activation_pending = plan
        .kes_rotation
        .as_ref()
        .is_some_and(|evidence| evidence.activation_pending);
    if digest == plan.observation.live.kes_opcert_id && !activation_pending {
        return Err(OuroError::Validation(
            "KES artifact is identical to the currently running opcert — replay refused".into(),
        ));
    }
    let bytes = std::fs::read(path)?;
    let parsed = crate::kes::parse_operational_certificate(&bytes)?;
    let container = plan.observation.live.container_id.as_str();
    let (public_envelope, staged_digest) = inspect_staged_kes_pair(container)?;
    let public_bytes = serde_json::to_vec(&public_envelope).map_err(|error| {
        OuroError::Validation(format!(
            "cannot canonicalize staged public KES vkey: {error}"
        ))
    })?;
    let public_vkey = crate::kes::parse_kes_verification_key(&public_bytes)?;
    let approved_staged_digest = plan
        .kes_rotation
        .as_ref()
        .and_then(|evidence| evidence.staged_vkey_sha256.as_deref())
        .ok_or_else(|| {
            OuroError::Validation("KES candidate lost its staged public-key binding".into())
        })?;
    if staged_digest != approved_staged_digest {
        return Err(OuroError::Validation(
            "staged public KES verification key changed after planning — refused".into(),
        ));
    }
    if public_vkey != parsed.hot_kes_verification_key {
        return Err(OuroError::Validation(
            "opcert hot KES key does not match the target's staged public KES vkey — refused"
                .into(),
        ));
    }
    let (identity_path, identity_digest) = if activation_pending {
        (
            crate::executor::OPCERT_PREVIOUS,
            plan.kes_rotation
                .as_ref()
                .and_then(|evidence| evidence.previous_opcert_sha256.as_deref())
                .ok_or_else(|| {
                    OuroError::Validation(
                        "KES activation resume lost its previous cold-identity binding".into(),
                    )
                })?,
        )
    } else {
        (
            crate::executor::OPCERT_DEST,
            plan.observation.live.kes_opcert_id.as_str(),
        )
    };
    if let Some(evidence) = protocol_evidence {
        if evidence.artifact_sha256 != digest {
            return Err(OuroError::Validation(
                "relay KES protocol evidence does not bind the candidate artifact".into(),
            ));
        }
        let counter_shape_is_valid = matches!(
            (
                evidence.node_state_counter,
                evidence.node_state_counter_status.as_str()
            ),
            (Some(_), "present") | (None, "no_blocks_minted_yet")
        );
        if !counter_shape_is_valid {
            return Err(OuroError::Validation(
                "relay KES protocol evidence has an inconsistent node-state counter status".into(),
            ));
        }
        let facts = json!({
            "qKesCurrentKesPeriod": evidence.current_period,
            "qKesStartKesInterval": evidence.start_period,
            "qKesEndKesInterval": evidence.end_period,
            "qKesOnDiskOperationalCertificateNumber": evidence.on_disk_counter,
            "qKesNodeStateOperationalCertificateNumber": evidence.node_state_counter,
        });
        return validate_kes_protocol_facts(
            &facts,
            parsed,
            container,
            identity_path,
            identity_digest,
        );
    }
    let mut command = std::process::Command::new("docker");
    command.args([
        "exec",
        "-i",
        container,
        "cardano-cli",
        "query",
        "kes-period-info",
        "--socket-path",
        "/ipc/node.socket",
        "--op-cert-file",
        "/dev/stdin",
        "--output-json",
    ]);
    match plan.network.as_str() {
        "mainnet" => {
            command.arg("--mainnet");
        }
        "preprod" => {
            command.args(["--testnet-magic", "1"]);
        }
        "preview" => {
            command.args(["--testnet-magic", "2"]);
        }
        network => {
            return Err(OuroError::Validation(format!(
                "unsupported KES network {network}"
            )))
        }
    }
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| {
            OuroError::Validation(format!("cannot run cardano-cli KES validation: {error}"))
        })?;
    child
        .stdin
        .take()
        .ok_or_else(|| OuroError::Validation("KES validator has no stdin".into()))?
        .write_all(&bytes)?;
    let output = child.wait_with_output()?;
    if output.stdout.len() > 65_536 || output.stderr.len() > 65_536 || !output.status.success() {
        return Err(OuroError::Validation(format!(
            "cardano-cli rejected the prospective opcert: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2048)
                .collect::<String>()
        )));
    }
    let facts = parse_cardano_cli_json(&output.stdout, "cardano-cli KES result")?;
    validate_kes_protocol_facts(&facts, parsed, container, identity_path, identity_digest)
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
    let path = crate::inbox::resolve_typed(inbox, reference, crate::inbox::ArtifactType::Opcert)?;
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
        "exec",
        "-i",
        &att.state.container_id,
        "cardano-cli",
        "query",
        "kes-period-info",
        "--socket-path",
        "/ipc/node.socket",
        "--op-cert-file",
        "/dev/stdin",
        "--output-json",
    ]);
    match att.immutable.network.as_str() {
        "mainnet" => {
            command.arg("--mainnet");
        }
        "preprod" => {
            command.args(["--testnet-magic", "1"]);
        }
        "preview" => {
            command.args(["--testnet-magic", "2"]);
        }
        network => {
            return Err(OuroError::Validation(format!(
                "unsupported KES network {network}"
            )))
        }
    }
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| {
            OuroError::Validation(format!("cannot run cardano-cli KES validation: {error}"))
        })?;
    child
        .stdin
        .take()
        .ok_or_else(|| OuroError::Validation("KES validator has no stdin".into()))?
        .write_all(&bytes)?;
    let output = child.wait_with_output()?;
    if output.stdout.len() > 65_536 || output.stderr.len() > 65_536 || !output.status.success() {
        return Err(OuroError::Validation(format!(
            "cardano-cli rejected the prospective opcert: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2048)
                .collect::<String>()
        )));
    }
    let facts = parse_cardano_cli_json(&output.stdout, "cardano-cli KES result")?;
    Ok(validate_kes_protocol_facts(
        &facts,
        parsed,
        &att.state.container_id,
        crate::executor::OPCERT_DEST,
        &att.state.kes_opcert_id,
    )?
    .parsed)
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
            let (k, v) = args[i + 1]
                .split_once('=')
                .ok_or_else(|| OuroError::InvalidArgs("--param must be key=value".into()))?;
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
            return Err(OuroError::InvalidArgs(format!(
                "unexpected argument {name:?}"
            )));
        }
        if !repeat_flags.contains(&name) && !seen.insert(name) {
            return Err(OuroError::InvalidArgs(format!("duplicate flag {name}")));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| OuroError::InvalidArgs(format!("missing value for {name}")))?;
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
    args.windows(2)
        .find(|p| p[0] == name)
        .map(|p| p[1].as_str())
}

fn stateless_release_policy(args: &[String]) -> Result<convention::Allowlist> {
    if let Some(document) = optional(args, "--release-policy") {
        return convention::Allowlist::release_document(document);
    }
    if cfg!(debug_assertions) {
        if let Some(path) = std::env::var_os("OURO_RELEASES_FILE") {
            let document = std::fs::read_to_string(&path).map_err(|error| {
                OuroError::Validation(format!("cannot read OURO_RELEASES_FILE {path:?}: {error}"))
            })?;
            return convention::Allowlist::release_document(&document);
        }
    }
    Err(OuroError::Validation(
        "Upgrade requires the current signed release document from the control CLI".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        extract_embedded_probe, parse_cardano_cli_json, require_fleet_live_facts_fresh,
        rotate_attestation_for_upgrade, stateless_apply_terminal, ObsLive, Observation,
    };
    use crate::attestation::{
        AdoptionAttestation, ImmutableIdentity, ManagedState, Role, TypedMount,
    };
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
    fn fleet_collection_uses_the_same_live_facts_window_as_the_signed_permit() {
        assert!(require_fleet_live_facts_fresh(969, 1000).is_ok());
        assert!(require_fleet_live_facts_fresh(819, 1000).is_err());
        assert!(require_fleet_live_facts_fresh(1006, 1000).is_err());
    }

    #[test]
    fn stateless_apply_audit_distinguishes_rollback_from_transport_ambiguity() {
        let rolled_back = crate::ssh::SshOutcome {
            status: 30,
            stdout: serde_json::json!({
                "tool": "ouro",
                "status": "error",
                "error": {
                    "detail": "upgrade failed after mutation; live-state rollback completed"
                }
            })
            .to_string(),
            stderr: String::new(),
        };
        assert_eq!(
            stateless_apply_terminal(&rolled_back),
            ("apply_rolled_back", "mutation_rolled_back_and_verified")
        );
        let ambiguous = crate::ssh::SshOutcome {
            status: 255,
            stdout: String::new(),
            stderr: "connection reset".into(),
        };
        assert_eq!(
            stateless_apply_terminal(&ambiguous),
            ("apply_ambiguous", "untyped_or_transport_outcome")
        );
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
        assert_eq!(
            std::fs::read(&victim).unwrap(),
            b"DO_NOT_EXECUTE_OR_REPLACE"
        );
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
            kind: "bind".into(),
            source_id: "8:1".into(),
            destination: "/data/db".into(),
            read_only: false,
            owner: "0:0".into(),
            mode: "0755".into(),
            no_symlink: true,
        };
        let old = AdoptionAttestation {
            immutable: ImmutableIdentity {
                role: Role::Bp,
                contract_id: contract.contract_id.clone(),
                convention_version: contract.convention_version,
                allowlist_version: allow.allowlist_version,
                allowlist_digest: allow.signed_digest().unwrap(),
                host_key_sha256: "a".repeat(64),
                machine_id: "bp1".into(),
                oci_index_digest: image.oci_index_digest.clone(),
                platform_manifest_digest: image.platform_manifest_digest.clone(),
                image_config_digest: image.image_config_digest.clone(),
                platform: image.platform.clone(),
                container_creation_epoch: 1000,
                entrypoint: vec!["cardano-node".into()],
                args: vec!["run".into()],
                mounts: vec![mount.clone()],
                network: "mainnet".into(),
                genesis_hash: "gh".into(),
                public_credential_ids: vec!["kes:5".into()],
                approval_evidence_hash: "approved".into(),
            },
            state: ManagedState {
                state_generation: 7,
                container_id: "old-cid".into(),
                topology_hash: "t".into(),
                config_hash: "c".into(),
                kes_opcert_id: "kes:5".into(),
            },
        };
        let path = home.join("attestations/bp1.json");
        let mut document = serde_json::to_value(&old).unwrap();
        document["contract"] =
            serde_json::json!({"in_container_paths": contract.in_container_paths});
        crate::attestation::write_document(&path, &document).unwrap();
        let observation = Observation {
            supervisor: SupervisorObservation {
                runtime: "docker".into(),
                rootful: true,
                rootless: false,
                node_container_count: 1,
                uses_bind_mounts: true,
                daemon_socket: "/var/run/docker.sock".into(),
                restart_policy: "unless-stopped".into(),
                orchestration: "run".into(),
                orchestration_reason: None,
                compose: None,
            },
            live: ObsLive {
                image_config_digest: image.image_config_digest.clone(),
                platform: image.platform.clone(),
                container_id: "restored-new-cid".into(),
                container_running: true,
                container_restarting: false,
                container_status: "running".into(),
                container_creation_epoch: 2000,
                container_name: "cardano-node".into(),
                image_reference: "image:test".into(),
                entrypoint: vec!["cardano-node".into()],
                args: vec!["run".into()],
                image_entrypoint: vec!["cardano-node".into()],
                image_cmd: vec!["run".into()],
                mounts: vec![mount],
                topology_hash: "t".into(),
                config_hash: "c".into(),
                kes_opcert_id: "kes:5".into(),
                has_forging_keys: true,
                forging_key_permissions_safe: true,
                keys_directory_safe: true,
                kes_skey_private: true,
                vrf_skey_private: true,
                host_key_sha256: "a".repeat(64),
                genesis_hash: "gh".into(),
                network: "mainnet".into(),
            },
            readiness: None,
            recreate: None,
        };
        let paths = ConfigPaths {
            home: home.clone(),
            credentials_dir: home.join("credentials"),
            staging_dir: home.join("staging"),
            audit_db: home.join("audit.sqlite3"),
            confirmations: home.join("confirmations.json"),
            tool_run_secret: home.join("tool-run.secret"),
            known_hosts: home.join("known_hosts"),
            legacy_db: None,
        };
        rotate_attestation_for_upgrade(
            &paths,
            "bp1",
            false,
            &old,
            &observation,
            &image.image_config_digest,
        )
        .unwrap();
        let restored: AdoptionAttestation =
            serde_json::from_str(&crate::attestation::read_document(&path).unwrap()).unwrap();
        assert_eq!(restored.state.container_id, "restored-new-cid");
        assert_eq!(restored.immutable.container_creation_epoch, 2000);
        assert_eq!(restored.state.state_generation, 8);
        assert_ne!(restored.state.container_id, old.state.container_id);
        std::fs::remove_dir_all(&home).unwrap();
    }
}
