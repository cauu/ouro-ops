//! S0019 p4-2 / p7-3 — the sealed executor: a FIXED SEQUENCE of fixed argvs per operation.
//!
//! The transaction's commit step performs the mutation via fixed argv arrays — never a shell, never
//! string interpolation. The variable parts come from the ATTESTATION (the container id, the
//! attested network, resolved paths) and from digest-resolved inbox artifacts — NOT from the
//! agent's intent parameters (those were already validated as closed selectors, and never reach
//! argv as a command token). So even a hostile-but-schema-valid parameter cannot become injection:
//! the executor builds `["docker","restart","<attested container id>"]`, not `sh -c "<anything>"`.
//!
//! Artifact-bearing ops (KES opcert and Deploy signed-tx) consume a
//! content-addressed inbox artifact resolved BY DIGEST (`inbox::resolve` re-verifies the bytes
//! against the ref). The resolved path is a target-side, digest-verified fact — never the agent's
//! string. If a required artifact is not staged, the op is REFUSED (it never silently degrades to a
//! bare restart). The artifacts these ops install are PUBLIC (the opcert `node.cert` is a public
//! certificate; a signed tx is public). The current stateless KES executor generates the signing
//! key only inside its fixed BP-private stage and never reads or transports its contents; the
//! air-gapped cold key is NEVER touched, requested, or transported.
//!
//! `build_plan` returns what the executor WOULD run; the actual `std::process` invocation happens
//! target-side (as the confined principal). This module is the sealed argv builder + its proof.

use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::attestation::AdoptionAttestation;
use crate::intent::Intent;
use crate::{OuroError, Result};

/// The upgrade recreate spec (§2.10) — the target-side `docker inspect` facts needed to recreate the
/// container onto a new image WITHOUT losing anything the probe modeled. Fail-closed: the probe emits
/// `null` (→ refusal) for any shape it cannot faithfully reproduce (named volumes, tmpfs, etc.).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecreateSpec {
    pub name: String,
    pub restart_policy: String,
    pub network_mode: String,
    pub binds: Vec<Bind>,
    pub env: Vec<String>,
    pub ports: Vec<Port>,
    /// The resolved entrypoint executable (`.Path`) + its args (`.Args`) — the exact process.
    pub entrypoint: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Bind {
    pub source: String,
    pub destination: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Port {
    pub container: String,
    pub host_ip: String,
    pub host_port: String,
}

/// The converged container layout (§2.2): fixed destinations the sealed executor writes to.
const KEYS_DIR: &str = "/opt/cardano/config/keys";
pub const KES_SKEY_DEST: &str = "/opt/cardano/config/keys/kes.skey";
pub const KES_VKEY_DEST: &str = "/opt/cardano/config/keys/kes.vkey";
pub const VRF_SKEY_DEST: &str = "/opt/cardano/config/keys/vrf.skey";
pub const KES_STAGE_DIR: &str = "/opt/cardano/config/keys/.ouro-kes-stage";
pub const KES_STAGE_SKEY: &str = "/opt/cardano/config/keys/.ouro-kes-stage/kes.skey";
pub const KES_STAGE_VKEY: &str = "/opt/cardano/config/keys/.ouro-kes-stage/kes.vkey";
const OPCERT_DEST: &str = "/opt/cardano/config/keys/node.cert";
const KES_SKEY_PREVIOUS: &str = "/opt/cardano/config/keys/kes.skey.ouro-prev";
const KES_VKEY_PREVIOUS: &str = "/opt/cardano/config/keys/kes.vkey.ouro-prev";
const OPCERT_PREVIOUS: &str = "/opt/cardano/config/keys/node.cert.ouro-prev";
const SOCKET: &str = "/ipc/node.socket";
/// Where a signed tx artifact is staged INSIDE the container before submit (ephemeral, public tx).
const TX_STAGE: &str = "/tmp/ouro-tx.signed";

pub type ExecutionPlan = Vec<Vec<String>>;
pub type RecoverablePlans = (ExecutionPlan, Option<ExecutionPlan>);

/// Operation-scoped recovery state lives only in Cardano/Docker application objects. Commit keeps
/// the previous object, rollback restores it, and finalize removes it after live verification.
#[derive(Debug, Clone)]
pub struct StatelessRecoveryPlan {
    pub commit: ExecutionPlan,
    pub rollback: ExecutionPlan,
    pub finalize: ExecutionPlan,
}

fn s(x: &str) -> String {
    x.to_string()
}

/// Network → cardano-cli network selector, taken from the ATTESTATION (a sealed fact), never the
/// agent's param. preprod magic = 1, preview magic = 2 (well-known).
fn net_flags(network: &str) -> Result<Vec<String>> {
    match network {
        "mainnet" => Ok(vec![s("--mainnet")]),
        "preprod" => Ok(vec![s("--testnet-magic"), s("1")]),
        "preview" => Ok(vec![s("--testnet-magic"), s("2")]),
        other => Err(OuroError::Validation(format!(
            "attested network {other:?} has no known cardano-cli selector"
        ))),
    }
}

/// Resolve an artifact-ref intent param to a target-side, digest-verified path. In COMMIT mode
/// (`inbox = Some`) the bytes are re-hashed against the ref (`inbox::resolve`) and a missing/replaced
/// artifact is refused. In PREVIEW mode (`inbox = None`, plan/read display) a clearly-marked
/// placeholder stands in for the resolved path so the plan can be shown without a staged artifact.
/// A missing param is refused in both modes (the op cannot run without its artifact).
fn resolve_artifact(
    intent: &Intent,
    param: &str,
    expected_type: crate::inbox::ArtifactType,
    inbox: Option<&Path>,
) -> Result<String> {
    let art_ref = intent
        .payload
        .get(param)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "{} needs a staged inbox artifact `{param}` — refused (stage it via `ouro-ops inbox stage`)",
                intent.operation_id
            ))
        })?;
    match inbox {
        Some(dir) => Ok(crate::inbox::resolve_typed(dir, art_ref, expected_type)?
            .display()
            .to_string()),
        None => Ok(format!("<inbox:{art_ref}>")),
    }
}

/// Build the FIXED SEQUENCE of argvs for a validated intent against an attested node. `inbox` is the
/// target's content-addressed inbox (`Some` for a real commit — artifacts are digest-resolved;
/// `None` for a plan/read preview — a placeholder path is shown). The container id and the network
/// are taken from the attestation; artifact params are immutable references resolved by digest.
pub fn build_plan(
    intent: &Intent,
    att: &AdoptionAttestation,
    inbox: Option<&Path>,
) -> Result<Vec<Vec<String>>> {
    let cid = att.state.container_id.clone();
    let restart = || vec![s("docker"), s("restart"), cid.clone()];
    match intent.operation_id.as_str() {
        // Restart the attested container onto whatever is currently on disk.
        "runtime/restart" => Ok(vec![restart()]),
        // A managed READ — query the node's tip via the container's socket + attested network.
        "observability/health" => {
            let mut argv = vec![
                s("docker"), s("exec"), cid, s("cardano-cli"), s("query"), s("tip"),
                s("--socket-path"), s(SOCKET),
            ];
            argv.extend(net_flags(&att.immutable.network)?);
            Ok(vec![argv])
        }
        // Legacy resident-path projection. Current public KES flow uses the stateless staged-pair
        // executor below; this older transaction shape remains only for migration tests.
        "kes-rotation/install-opcert" => {
            let opcert = resolve_artifact(
                intent,
                "opcert",
                crate::inbox::ArtifactType::Opcert,
                inbox,
            )?;
            let _ = KEYS_DIR; // destination is the fixed OPCERT_DEST under the keys dir
            Ok(vec![
                vec![s("docker"), s("cp"), opcert, format!("{cid}:{OPCERT_DEST}")],
                restart(),
            ])
        }
        // deploy/register-submit: stage the digest-resolved SIGNED tx (public) into the container,
        // then submit via the node socket on the attested network. The tx is built + signed
        // air-gapped by the operator (cold key never here); this executor only submits the bytes.
        "deploy/register-submit" => {
            let tx = resolve_artifact(intent, "tx", crate::inbox::ArtifactType::Tx, inbox)?;
            let mut submit = vec![
                s("docker"), s("exec"), cid.clone(), s("cardano-cli"), s("transaction"), s("submit"),
                s("--tx-file"), s(TX_STAGE), s("--socket-path"), s(SOCKET),
            ];
            submit.extend(net_flags(&att.immutable.network)?);
            Ok(vec![
                vec![s("docker"), s("cp"), tx, format!("{cid}:{TX_STAGE}")],
                submit,
            ])
        }
        "upgrade/preload-image" => Err(OuroError::Validation(
            "upgrade image preparation is built from the fetched signed release catalog in the stateless target flow"
                .into(),
        )),
        // upgrade/step: the recreate is built by the op flow from the TARGET's own `docker inspect`
        // run-spec (§2.10, `recreate_argv`) + the allowlisted target digest, not from `build_plan`
        // (which has no observation here). This arm is only reached in plan/read PREVIEW, where the
        // recreate spec is not available — say so honestly.
        "upgrade/step" => Err(OuroError::Validation(
            "upgrade/step recreate is computed target-side from the observed run-spec (§2.10); \
             run without --plan to execute it"
                .into(),
        )),
        other => Err(OuroError::Validation(format!(
            "no sealed executor for {other} (§2.5)"
        ))),
    }
}

/// The rollback plan: restart the attested container onto its prior (pre-commit) on-disk state. The
/// commit steps that install an artifact are followed by a restart; rolling back is the same restart
/// after the prior artifact/config is what remains, so this is idempotent.
pub fn rollback_plan(att: &AdoptionAttestation) -> Vec<Vec<String>> {
    vec![vec![s("docker"), s("restart"), att.state.container_id.clone()]]
}

/// Build commit + honest rollback plans for a durable transaction. KES installation first copies
/// the previous public opcert into a root-only transaction directory, so rollback restores bytes
/// before restarting. Transaction submission is irreversible and therefore carries no fake plan.
pub fn recoverable_plans(
    intent: &Intent,
    att: &AdoptionAttestation,
    inbox: &Path,
    rollback_root: &Path,
) -> Result<RecoverablePlans> {
    let mut commit = build_plan(intent, att, Some(inbox))?;
    match intent.operation_id.as_str() {
        "kes-rotation/install-opcert" => {
            std::fs::create_dir_all(rollback_root)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(rollback_root, std::fs::Permissions::from_mode(0o700))?;
            }
            let backup = rollback_root.join("node.cert.pre").display().to_string();
            commit.insert(
                0,
                vec![
                    s("docker"), s("cp"),
                    format!("{}:{OPCERT_DEST}", att.state.container_id), backup.clone(),
                ],
            );
            let rollback = vec![
                vec![s("docker"), s("cp"), backup, format!("{}:{OPCERT_DEST}", att.state.container_id)],
                vec![s("docker"), s("restart"), att.state.container_id.clone()],
            ];
            Ok((commit, Some(rollback)))
        }
        "deploy/register-submit" => Ok((commit, None)),
        "upgrade/preload-image" => Err(OuroError::Validation(
            "upgrade image preparation is stateless and has no durable transaction plan".into(),
        )),
        _ => Ok((commit, Some(rollback_plan(att)))),
    }
}

fn recreate_run_argv(spec: &RecreateSpec, image_digest: &str) -> Result<Vec<String>> {
    if spec.name.is_empty() {
        return Err(OuroError::Validation(
            "upgrade: observed container has no name — refused (fail-closed)".into(),
        ));
    }
    if spec.binds.is_empty() {
        return Err(OuroError::Validation(
            "upgrade: no bind mounts observed — refusing to recreate a node without its volumes".into(),
        ));
    }
    if spec.entrypoint.is_empty() {
        return Err(OuroError::Validation(
            "upgrade: no resolved entrypoint observed — refused (fail-closed)".into(),
        ));
    }
    if image_digest.strip_prefix("sha256:").map(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())) != Some(true) {
        return Err(OuroError::Validation(format!(
            "upgrade: target image {image_digest:?} is not a sha256 digest — refused"
        )));
    }
    let mut run = vec![
        s("docker"), s("run"), s("-d"),
        s("--name"), spec.name.clone(),
        s("--restart"), if spec.restart_policy.is_empty() { s("unless-stopped") } else { spec.restart_policy.clone() },
    ];
    if !matches!(spec.network_mode.as_str(), "" | "default" | "bridge") {
        run.push(s("--network"));
        run.push(spec.network_mode.clone());
    }
    for p in &spec.ports {
        let mapping = if p.host_ip.is_empty() {
            format!("{}:{}", p.host_port, p.container)
        } else {
            format!("{}:{}:{}", p.host_ip, p.host_port, p.container)
        };
        run.push(s("-p"));
        run.push(mapping);
    }
    for e in &spec.env {
        run.push(s("-e"));
        run.push(e.clone());
    }
    for b in &spec.binds {
        let mut v = format!("{}:{}", b.source, b.destination);
        if b.read_only {
            v.push_str(":ro");
        }
        run.push(s("-v"));
        run.push(v);
    }
    run.push(s("--entrypoint"));
    run.push(spec.entrypoint.clone());
    run.push(image_digest.to_string());
    run.extend(spec.args.iter().cloned());
    Ok(run)
}

/// Build the legacy remove-first recreate sequence. Stateless S0020 apply uses the recovery plan
/// below so the previous live container survives an interrupted runner.
/// `docker run` faithfully reproduces the observed run-spec (name,
/// restart policy, network, published ports, env, bind mounts, entrypoint + args). FAIL-CLOSED: any
/// missing/ambiguous fact is refused — we never recreate a node with a partial spec. `image_digest`
/// is `sha256:<…>` (a target-present, allowlist-verified digest); nothing here is an agent string
/// except the digest, which was validated as a closed selector + allowlist membership.
pub fn recreate_argv(spec: &RecreateSpec, cid: &str, image_digest: &str) -> Result<Vec<Vec<String>>> {
    let run = recreate_run_argv(spec, image_digest)?;
    Ok(vec![vec![s("docker"), s("rm"), s("-f"), cid.to_string()], run])
}

/// Generate a new pair only inside the fixed BP-private stage; no active file is changed.
pub fn stateless_kes_stage_plan(cid: &str) -> ExecutionPlan {
    vec![
        vec![s("docker"), s("exec"), cid.to_string(), s("mkdir"), s("-m"), s("700"), s(KES_STAGE_DIR)],
        vec![
            s("docker"), s("exec"), cid.to_string(), s("cardano-cli"), s("node"),
            s("key-gen-KES"), s("--verification-key-file"), format!("{KES_STAGE_VKEY}.tmp"),
            s("--signing-key-file"), format!("{KES_STAGE_SKEY}.tmp"),
        ],
        vec![s("docker"), s("exec"), cid.to_string(), s("test"), s("-s"), format!("{KES_STAGE_VKEY}.tmp")],
        vec![s("docker"), s("exec"), cid.to_string(), s("test"), s("-s"), format!("{KES_STAGE_SKEY}.tmp")],
        vec![s("docker"), s("exec"), cid.to_string(), s("chmod"), s("600"), format!("{KES_STAGE_SKEY}.tmp")],
        vec![s("docker"), s("exec"), cid.to_string(), s("chmod"), s("644"), format!("{KES_STAGE_VKEY}.tmp")],
        vec![s("docker"), s("exec"), cid.to_string(), s("mv"), format!("{KES_STAGE_SKEY}.tmp"), s(KES_STAGE_SKEY)],
        vec![s("docker"), s("exec"), cid.to_string(), s("mv"), format!("{KES_STAGE_VKEY}.tmp"), s(KES_STAGE_VKEY)],
    ]
}

pub fn stateless_kes_stage_cleanup_plan(cid: &str) -> ExecutionPlan {
    vec![vec![s("docker"), s("exec"), cid.to_string(), s("rm"), s("-rf"), s(KES_STAGE_DIR)]]
}
/// Normalize the fixed active forging paths to the container node service identity. No caller can
/// select a path, mode, owner, or executable; the owner was read from `/proc/1` inside this same
/// container and shape-validated before this plan is constructed.
pub fn stateless_forging_permission_normalize_plan(
    cid: &str,
    service_owner: &str,
) -> ExecutionPlan {
    vec![
        vec![
            s("docker"),
            s("exec"),
            s("--user"),
            s("0"),
            cid.to_string(),
            s("chown"),
            s("--no-dereference"),
            service_owner.to_string(),
            s(KES_SKEY_DEST),
            s(VRF_SKEY_DEST),
        ],
        vec![
            s("docker"),
            s("exec"),
            s("--user"),
            s("0"),
            cid.to_string(),
            s("chmod"),
            s("700"),
            s(KEYS_DIR),
        ],
        vec![
            s("docker"),
            s("exec"),
            s("--user"),
            s("0"),
            cid.to_string(),
            s("chmod"),
            s("600"),
            s(KES_SKEY_DEST),
            s(VRF_SKEY_DEST),
        ],
    ]
}

/// Exact inverse for permission normalization. Every value is numeric metadata captured from the
/// same fixed paths during candidate construction; no key bytes are read or copied.
pub fn stateless_forging_permission_rollback_plan(
    cid: &str,
    keys_mode: &str,
    kes_owner: &str,
    kes_mode: &str,
    vrf_owner: &str,
    vrf_mode: &str,
) -> ExecutionPlan {
    vec![
        vec![
            s("docker"),
            s("exec"),
            s("--user"),
            s("0"),
            cid.to_string(),
            s("chmod"),
            keys_mode.to_string(),
            s(KEYS_DIR),
        ],
        vec![
            s("docker"),
            s("exec"),
            s("--user"),
            s("0"),
            cid.to_string(),
            s("chown"),
            s("--no-dereference"),
            kes_owner.to_string(),
            s(KES_SKEY_DEST),
        ],
        vec![
            s("docker"),
            s("exec"),
            s("--user"),
            s("0"),
            cid.to_string(),
            s("chmod"),
            kes_mode.to_string(),
            s(KES_SKEY_DEST),
        ],
        vec![
            s("docker"),
            s("exec"),
            s("--user"),
            s("0"),
            cid.to_string(),
            s("chown"),
            s("--no-dereference"),
            vrf_owner.to_string(),
            s(VRF_SKEY_DEST),
        ],
        vec![
            s("docker"),
            s("exec"),
            s("--user"),
            s("0"),
            cid.to_string(),
            s("chmod"),
            vrf_mode.to_string(),
            s(VRF_SKEY_DEST),
        ],
    ]
}


/// Preserve the previous active KES pair and public opcert until the new matched triple is live.
pub fn stateless_kes_recovery_plan(cid: &str, payload: &str) -> StatelessRecoveryPlan {
    StatelessRecoveryPlan {
        commit: vec![
            vec![s("docker"), s("exec"), cid.to_string(), s("test"), s("!"), s("-e"), s(KES_SKEY_PREVIOUS)],
            vec![s("docker"), s("exec"), cid.to_string(), s("test"), s("!"), s("-e"), s(KES_VKEY_PREVIOUS)],
            vec![s("docker"), s("exec"), cid.to_string(), s("test"), s("!"), s("-e"), s(OPCERT_PREVIOUS)],
            vec![s("docker"), s("exec"), cid.to_string(), s("cp"), s("-p"), s(KES_SKEY_DEST), s(KES_SKEY_PREVIOUS)],
            vec![s("docker"), s("exec"), cid.to_string(), s("cp"), s("-p"), s(KES_VKEY_DEST), s(KES_VKEY_PREVIOUS)],
            vec![s("docker"), s("exec"), cid.to_string(), s("cp"), s("-p"), s(OPCERT_DEST), s(OPCERT_PREVIOUS)],
            vec![s("docker"), s("exec"), cid.to_string(), s("cp"), s("-p"), s(KES_STAGE_SKEY), s(KES_SKEY_DEST)],
            vec![s("docker"), s("exec"), cid.to_string(), s("cp"), s("-p"), s(KES_STAGE_VKEY), s(KES_VKEY_DEST)],
            vec![s("docker"), s("cp"), payload.to_string(), format!("{cid}:{OPCERT_DEST}")],
            vec![s("docker"), s("restart"), cid.to_string()],
        ],
        rollback: vec![
            vec![s("docker"), s("exec"), cid.to_string(), s("cp"), s("-p"), s(KES_SKEY_PREVIOUS), s(KES_SKEY_DEST)],
            vec![s("docker"), s("exec"), cid.to_string(), s("cp"), s("-p"), s(KES_VKEY_PREVIOUS), s(KES_VKEY_DEST)],
            vec![s("docker"), s("exec"), cid.to_string(), s("cp"), s("-p"), s(OPCERT_PREVIOUS), s(OPCERT_DEST)],
            vec![s("docker"), s("restart"), cid.to_string()],
        ],
        finalize: vec![
            vec![s("docker"), s("exec"), cid.to_string(), s("rm"), s("-f"), s(KES_SKEY_PREVIOUS), s(KES_VKEY_PREVIOUS), s(OPCERT_PREVIOUS)],
            vec![s("docker"), s("exec"), cid.to_string(), s("rm"), s("-rf"), s(KES_STAGE_DIR)],
        ],
    }
}

pub fn stateless_kes_prepare_cleanup_plan(cid: &str) -> ExecutionPlan {
    vec![vec![
        s("docker"), s("exec"), cid.to_string(), s("rm"), s("-f"),
        s(KES_SKEY_PREVIOUS), s(KES_VKEY_PREVIOUS), s(OPCERT_PREVIOUS),
    ]]
}

/// Prove a completed KES transaction left neither a staged pair nor rollback backups behind.
pub fn stateless_kes_cleanup_verification_plan(cid: &str) -> ExecutionPlan {
    [
        KES_STAGE_DIR,
        KES_SKEY_PREVIOUS,
        KES_VKEY_PREVIOUS,
        OPCERT_PREVIOUS,
    ]
    .into_iter()
    .map(|path| {
        vec![
            s("docker"),
            s("exec"),
            cid.to_string(),
            s("test"),
            s("!"),
            s("-e"),
            s(path),
        ]
    })
    .collect()
}

/// Preserve the prior container as `<name>.ouro-prev` until the replacement is live-verified.
pub fn stateless_recreate_recovery_plan(
    spec: &RecreateSpec,
    cid: &str,
    image_digest: &str,
) -> Result<StatelessRecoveryPlan> {
    let run = recreate_run_argv(spec, image_digest)?;
    let previous = format!("{}.ouro-prev", spec.name);
    if previous.len() > 128 {
        return Err(OuroError::Validation(
            "upgrade: recovery container name exceeds Docker's supported length".into(),
        ));
    }
    Ok(StatelessRecoveryPlan {
        commit: vec![
            vec![s("docker"), s("rename"), cid.to_string(), previous.clone()],
            vec![s("docker"), s("stop"), previous.clone()],
            run,
        ],
        rollback: vec![
            vec![s("docker"), s("rm"), s("-f"), spec.name.clone()],
            vec![s("docker"), s("rename"), previous.clone(), spec.name.clone()],
            vec![s("docker"), s("start"), spec.name.clone()],
        ],
        finalize: vec![vec![s("docker"), s("rm"), s("-f"), previous]],
    })
}

fn redact_environment_values(plan: &mut ExecutionPlan) {
    for argv in plan {
        let mut index = 0;
        while index + 1 < argv.len() {
            if argv[index] == "-e" {
                let name = argv[index + 1].split_once('=').map(|(name, _)| name)
                    .unwrap_or(argv[index + 1].as_str());
                argv[index + 1] = format!("{name}=<redacted-target-value>");
                index += 2;
            } else {
                index += 1;
            }
        }
    }
}

/// Human-reviewable upgrade projection. Docker environment VALUES are deliberately absent from
/// ToolOutput: the target retains them in the root-only sealed commit plan, while approval sees the
/// variable names and an explicit redaction marker. The opaque HMAC run-spec binding in the intent
/// detects any value change before commit without publishing a password-verification oracle.
pub fn recreate_approval_argv(
    spec: &RecreateSpec,
    cid: &str,
    image_digest: &str,
) -> Result<Vec<Vec<String>>> {
    let mut plan = recreate_argv(spec, cid, image_digest)?;
    redact_environment_values(&mut plan);
    Ok(plan)
}

pub fn stateless_recreate_approval_argv(
    spec: &RecreateSpec,
    cid: &str,
    image_digest: &str,
) -> Result<Vec<Vec<String>>> {
    let recovery = stateless_recreate_recovery_plan(spec, cid, image_digest)?;
    let mut plan = recovery.commit;
    plan.extend(recovery.finalize);
    redact_environment_values(&mut plan);
    Ok(plan)
}

fn verify_image_inspect_output(target: &str, stdout: &[u8]) -> Result<()> {
    let actual = String::from_utf8_lossy(stdout).trim().to_string();
    if actual != target {
        return Err(OuroError::Validation(format!(
            "upgrade target image is not preloaded by exact config digest: requested {target}, \
             docker resolved {actual:?}"
        )));
    }
    Ok(())
}

/// Fixed target-local read proving the allowlisted config digest is already present. Pulling is a
/// separate, explicitly authorized workflow; upgrade never turns a plan into an implicit download.
pub fn require_image_present(target: &str) -> Result<()> {
    let output = std::process::Command::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", target])
        .output()
        .map_err(|e| OuroError::Validation(format!("cannot inspect preloaded upgrade image: {e}")))?;
    if !output.status.success() {
        return Err(OuroError::Validation(format!(
            "upgrade target image {target} is not preloaded — stage/pull it separately before planning"
        )));
    }
    verify_image_inspect_output(target, &output.stdout)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PulledImageEvidence {
    pub reference: String,
    pub repository: String,
    pub platform: String,
    pub platform_manifest_digest: String,
    pub image_config_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerImageInspect {
    id: String,
    repo_digests: Vec<String>,
    os: String,
    architecture: String,
}

/// Pull and verify one signed-catalog image tuple. The repository is a fixed Ouro convention and
/// the selector is an immutable platform-manifest digest; a tag or alternate registry can never
/// enter the executor argv.
pub fn pull_verified_image(
    repository: &str,
    platform_manifest_digest: &str,
    expected_config_digest: &str,
    expected_platform: &str,
) -> Result<PulledImageEvidence> {
    if repository != crate::convention::BLINKLABS_REPOSITORY {
        return Err(OuroError::Validation(format!(
            "upgrade image repository must be exactly {}",
            crate::convention::BLINKLABS_REPOSITORY
        )));
    }
    if expected_platform != "linux/amd64" {
        return Err(OuroError::Validation(format!(
            "upgrade image pull supports only linux/amd64, got {expected_platform:?}"
        )));
    }
    let valid_digest = |value: &str| {
        value.len() == 71
            && value.starts_with("sha256:")
            && value[7..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if !valid_digest(platform_manifest_digest) || !valid_digest(expected_config_digest) {
        return Err(OuroError::Validation(
            "upgrade pull requires lowercase sha256 platform-manifest and config digests".into(),
        ));
    }
    let reference = format!("{repository}@{platform_manifest_digest}");
    run_argv(&[
        s("docker"),
        s("pull"),
        s("--platform"),
        expected_platform.to_string(),
        reference.clone(),
    ])?;
    let output = std::process::Command::new("docker")
        .args(["image", "inspect", "--format", "{{json .}}", &reference])
        .output()
        .map_err(|error| {
            OuroError::Validation(format!("cannot inspect exact pulled image: {error}"))
        })?;
    if !output.status.success() {
        return Err(OuroError::Validation(
            "exact pulled image could not be inspected after pull".into(),
        ));
    }
    let inspected: DockerImageInspect = serde_json::from_slice(&output.stdout).map_err(|error| {
        OuroError::Validation(format!("docker returned malformed image inspection: {error}"))
    })?;
    let platform = format!("{}/{}", inspected.os, inspected.architecture);
    if inspected.id != expected_config_digest {
        return Err(OuroError::Validation(format!(
            "pulled image config digest mismatch: expected {expected_config_digest}, got {}",
            inspected.id
        )));
    }
    if platform != expected_platform {
        return Err(OuroError::Validation(format!(
            "pulled image platform mismatch: expected {expected_platform}, got {platform}"
        )));
    }
    if !inspected.repo_digests.iter().any(|value| value == &reference) {
        return Err(OuroError::Validation(format!(
            "pulled image is not bound to the approved repository manifest {reference}"
        )));
    }
    Ok(PulledImageEvidence {
        reference,
        repository: repository.to_string(),
        platform,
        platform_manifest_digest: platform_manifest_digest.to_string(),
        image_config_digest: expected_config_digest.to_string(),
    })
}

/// The upgrade rollback plan: recreate the container onto the PRIOR (attested) image digest with the
/// same observed run-spec — the honest inverse of a recreate. (Whether this restores service depends
/// on DB compatibility; the honest RollbackToN / ReSyncRequired classification is `upgrade.rs`.)
pub fn upgrade_rollback_plan(att: &AdoptionAttestation, spec: &RecreateSpec) -> Result<Vec<Vec<String>>> {
    // Remove by stable container name. The old immutable id may already be gone, while a partially
    // created replacement may now occupy the name.
    recreate_argv(spec, &spec.name, &att.immutable.image_config_digest)
}

/// Run a FIXED argv (the first element is the program) — a direct exec, never a shell. Returns Ok on
/// exit 0, else a typed error.
pub fn run_argv(argv: &[String]) -> Result<()> {
    let Some((prog, rest)) = argv.split_first() else {
        return Err(OuroError::Validation("empty executor argv".into()));
    };
    let out = std::process::Command::new(prog)
        .args(rest)
        .output()
        .map_err(|e| OuroError::Validation(format!("executor {prog} failed to start: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(OuroError::Validation(format!(
            "executor {prog} exited {}",
            out.status.code().unwrap_or(-1)
        )))
    }
}

const MAX_READ_OUTPUT_BYTES: usize = 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(30);

fn bounded_reader<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            let count = reader.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            let remaining = limit.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..count.min(remaining)]);
            if count > remaining {
                exceeded.store(true, Ordering::Release);
            }
            // Continue draining after the cap so the child cannot deadlock on a full pipe while
            // the parent observes the flag and terminates it.
        }
        Ok(kept)
    })
}

/// Execute the one fixed read argv and return bounded UTF-8 stdout. Unlike `run_argv`, this is a
/// data path: it must return the command's result, not merely claim that an argv would be run.
/// Both streams are drained concurrently, capped, and the child is terminated on timeout/cap.
pub fn run_read_plan(plan: &[Vec<String>]) -> Result<String> {
    if plan.len() != 1 {
        return Err(OuroError::Validation(
            "managed read executor requires exactly one fixed argv".into(),
        ));
    }
    let Some((prog, rest)) = plan[0].split_first() else {
        return Err(OuroError::Validation("empty managed read argv".into()));
    };
    let mut child = std::process::Command::new(prog)
        .args(rest)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            OuroError::Validation(format!("read executor {prog} failed to start: {error}"))
        })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        OuroError::Validation("read executor did not expose stdout".into())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        OuroError::Validation("read executor did not expose stderr".into())
    })?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_thread = bounded_reader(stdout, MAX_READ_OUTPUT_BYTES, Arc::clone(&exceeded));
    let stderr_thread = bounded_reader(stderr, MAX_READ_OUTPUT_BYTES, Arc::clone(&exceeded));
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if exceeded.load(Ordering::Acquire) {
            let _ = child.kill();
            break child.wait().map_err(|error| {
                OuroError::Validation(format!("read executor wait after output cap: {error}"))
            })?;
        }
        if started.elapsed() >= READ_TIMEOUT {
            timed_out = true;
            let _ = child.kill();
            break child.wait().map_err(|error| {
                OuroError::Validation(format!("read executor wait after timeout: {error}"))
            })?;
        }
        match child.try_wait().map_err(|error| {
            OuroError::Validation(format!("read executor status: {error}"))
        })? {
            Some(status) => break status,
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = stdout_thread
        .join()
        .map_err(|_| OuroError::Validation("read executor stdout reader panicked".into()))?
        .map_err(|error| OuroError::Validation(format!("read executor stdout: {error}")))?;
    let stderr = stderr_thread
        .join()
        .map_err(|_| OuroError::Validation("read executor stderr reader panicked".into()))?
        .map_err(|error| OuroError::Validation(format!("read executor stderr: {error}")))?;
    if timed_out {
        return Err(OuroError::Validation(
            "managed read exceeded the fixed 30s timeout".into(),
        ));
    }
    if exceeded.load(Ordering::Acquire) {
        return Err(OuroError::Validation(format!(
            "managed read output exceeded {MAX_READ_OUTPUT_BYTES} bytes"
        )));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr);
        return Err(OuroError::Validation(format!(
            "read executor {prog} exited {}: {}",
            status.code().unwrap_or(-1),
            detail.trim()
        )));
    }
    String::from_utf8(stdout)
        .map_err(|_| OuroError::Validation("managed read output was not UTF-8".into()))
}

/// Run a fixed SEQUENCE of argvs in order, stopping at the first failure. Each argv came from
/// `build_plan` (attested facts + digest-resolved artifacts only), so nothing agent-supplied is
/// interpolated here.
pub fn run_plan(plan: &[Vec<String>]) -> Result<()> {
    for argv in plan {
        run_argv(argv)?;
    }
    Ok(())
}

/// Execute a persisted rollback plan. Upgrade removal is deliberately best-effort: after a crash
/// the named container may not exist, and that expected condition must not prevent recreating N.
pub fn run_rollback_plan(operation_id: &str, plan: &[Vec<String>]) -> Result<()> {
    if operation_id == "upgrade/step" {
        let Some((remove, recreate)) = plan.split_first() else {
            return Err(OuroError::Validation("upgrade rollback plan is empty".into()));
        };
        if remove.first().map(String::as_str) != Some("docker")
            || remove.get(1).map(String::as_str) != Some("rm")
        {
            return Err(OuroError::Validation("upgrade rollback lacks fixed remove step".into()));
        }
        let _ = run_argv(remove); // absent container is expected after a partial recreate
        return run_plan(recreate);
    }
    run_plan(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{AdoptionAttestation, ImmutableIdentity, ManagedState, Role, TypedMount};
    use serde_json::json;

    fn att() -> AdoptionAttestation {
        AdoptionAttestation {
            immutable: ImmutableIdentity {
                role: Role::Bp, contract_id: "c".into(), convention_version: 1,
                allowlist_version: 1, allowlist_digest: "sha256:a".into(),
                host_key_sha256: "hk".into(), machine_id: "bp1".into(), oci_index_digest: "i".into(),
                platform_manifest_digest: "p".into(), image_config_digest: "cfg".into(),
                platform: "linux/amd64".into(),
                container_creation_epoch: 1, entrypoint: vec![], args: vec![],
                mounts: vec![TypedMount { kind: "bind".into(), source_id: "8:1:1".into(),
                    destination: "/data/db".into(), read_only: false, owner: "root".into(),
                    mode: "0755".into(), no_symlink: true }],
                network: "mainnet".into(), genesis_hash: "g".into(), public_credential_ids: vec![],
                approval_evidence_hash: "e".into(),
            },
            state: ManagedState { state_generation: 1, container_id: "cid-attested".into(),
                topology_hash: "t".into(), config_hash: "c".into(), kes_opcert_id: "k".into() },
        }
    }
    fn intent(op: &str, payload: serde_json::Value) -> Intent {
        Intent { schema_version: 1, operation_id: op.into(), node_id: "bp1".into(),
            pre_state_generation: 1, pre_state_hash: "h".into(), expected_post_state: "".into(),
            nonce: "n".into(), expiry_epoch: 0, payload }
    }

    #[test]
    fn restart_uses_attested_container_not_agent_param() {
        // Even if the agent's machine param were hostile, argv uses the ATTESTED container id.
        let plan = build_plan(&intent("runtime/restart", json!({"machine": "bp1"})), &att(), None).unwrap();
        assert_eq!(plan, vec![vec!["docker", "restart", "cid-attested"]]);
    }

    #[test]
    fn hostile_param_never_reaches_argv() {
        let plan = build_plan(&intent("runtime/restart", json!({"machine": "bp1; rm -rf /"})), &att(), None).unwrap();
        let flat: Vec<&String> = plan.iter().flatten().collect();
        assert!(!flat.iter().any(|a| a.contains("rm") || a.contains(";") || a.contains("bp1")),
            "no agent string reaches argv: {plan:?}");
    }

    #[test]
    fn retired_config_render_has_no_executor() {
        assert!(build_plan(&intent("config/render", json!({"machine": "bp1"})), &att(), None).is_err());
    }

    #[test]
    fn health_reads_tip_on_attested_network() {
        let plan = build_plan(&intent("observability/health", json!({"machine": "bp1"})), &att(), None).unwrap();
        let flat: Vec<String> = plan.into_iter().flatten().collect();
        assert!(flat.contains(&"tip".to_string()) && flat.contains(&"--mainnet".to_string()));
        assert!(flat.contains(&SOCKET.to_string()));
    }

    #[test]
    fn kes_installs_resolved_opcert_then_restarts() {
        // Refuse with no opcert.
        assert!(build_plan(&intent("kes-rotation/install-opcert", json!({"machine":"bp1"})), &att(), None).is_err());
        // Preview shows the cp+restart sequence with a placeholder path (no inbox needed).
        let good = format!("opcert-1@sha256:{}", "a".repeat(64));
        let plan = build_plan(&intent("kes-rotation/install-opcert", json!({"machine":"bp1","opcert":good})), &att(), None).unwrap();
        assert_eq!(plan.len(), 2, "cp then restart");
        assert_eq!(plan[0][0..2], ["docker".to_string(), "cp".to_string()]);
        assert_eq!(plan[0][2], "<inbox:opcert-1@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa>");
        assert_eq!(plan[0][3], format!("cid-attested:{OPCERT_DEST}"));
        assert_eq!(plan[1], vec!["docker", "restart", "cid-attested"]);
    }

    #[test]
    fn legacy_executor_cannot_accept_an_operator_image_archive() {
        let target = format!("sha256:{}", "b".repeat(64));
        let result = build_plan(
            &intent("upgrade/preload-image", json!({
                "machine": "bp1", "image": target,
            })),
            &att(),
            None,
        );
        assert!(result.is_err(), "only the signed-catalog stateless pull flow is valid");
    }

    #[test]
    fn kes_resolves_real_inbox_artifact_by_digest() {
        let dir = std::env::temp_dir().join(format!("ouro-exec-kes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let opcert_bytes = br#"{"type":"NodeOperationalCertificate","cborHex":"deadbeef"}"#;
        let art_ref = crate::inbox::stage(&dir, crate::inbox::ArtifactType::Opcert, opcert_bytes).unwrap();
        let plan = build_plan(
            &intent("kes-rotation/install-opcert", json!({"machine":"bp1","opcert":art_ref})),
            &att(), Some(&dir),
        ).unwrap();
        // The cp source is the REAL resolved inbox path (digest-verified), never the agent's string.
        assert!(plan[0][2].contains(dir.to_str().unwrap()), "resolved to the inbox path: {:?}", plan[0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kes_durable_plan_backs_up_and_restores_the_previous_opcert() {
        let dir = std::env::temp_dir().join(format!("ouro-exec-kes-rb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let inbox = dir.join("inbox");
        let rollback = dir.join("rollback");
        let bytes = br#"{"type":"NodeOperationalCertificate","cborHex":"deadbeef"}"#;
        let art_ref = crate::inbox::stage(&inbox, crate::inbox::ArtifactType::Opcert, bytes).unwrap();
        let (commit, rb) = recoverable_plans(
            &intent("kes-rotation/install-opcert", json!({"machine":"bp1","opcert":art_ref})),
            &att(), &inbox, &rollback,
        ).unwrap();
        assert_eq!(commit.len(), 3, "backup, install, restart");
        assert_eq!(commit[0][2], format!("cid-attested:{OPCERT_DEST}"));
        assert!(commit[0][3].ends_with("node.cert.pre"));
        let rb = rb.expect("KES has a real inverse");
        assert_eq!(rb[0][2], commit[0][3]);
        assert_eq!(rb[0][3], format!("cid-attested:{OPCERT_DEST}"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deploy_submits_resolved_tx() {
        assert!(build_plan(&intent("deploy/register-submit", json!({"machine":"bp1","network":"mainnet"})), &att(), None).is_err());
        let tx = format!("tx-1@sha256:{}", "b".repeat(64));
        let plan = build_plan(
            &intent("deploy/register-submit", json!({"machine":"bp1","tx":tx,"network":"mainnet"})),
            &att(), None,
        ).unwrap();
        assert_eq!(plan[0][0..2], ["docker".to_string(), "cp".to_string()]);
        let submit: Vec<String> = plan[1].clone();
        assert!(submit.contains(&"submit".to_string()) && submit.contains(&"--tx-file".to_string()));
        assert!(submit.contains(&TX_STAGE.to_string()) && submit.contains(&"--mainnet".to_string()));
    }

    #[test]
    fn deploy_has_no_fake_rollback() {
        let dir = std::env::temp_dir().join(format!("ouro-exec-tx-rb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let inbox = dir.join("inbox");
        let tx_bytes = br#"{"type":"Tx ConwayEra","cborHex":"deadbeef"}"#;
        let tx = crate::inbox::stage(&inbox, crate::inbox::ArtifactType::Tx, tx_bytes).unwrap();
        let (_, rollback) = recoverable_plans(
            &intent("deploy/register-submit", json!({"machine":"bp1","tx":tx,"network":"mainnet"})),
            &att(), &inbox, &dir.join("rollback"),
        ).unwrap();
        assert!(rollback.is_none(), "on-chain submit cannot be undone by docker restart");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrade_preview_defers_to_the_targetside_recreate() {
        // In preview (build_plan) upgrade has no observation, so it never fakes a restart — it says
        // the recreate is computed target-side. The real recreate is `recreate_argv` (tested below).
        let err = build_plan(&intent("upgrade/step", json!({"machine":"bp1"})), &att(), None).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("target-side"), "defers to the observed recreate: {msg}");
        assert!(!msg.contains("restart"), "must not pretend a restart is an upgrade");
    }

    fn recreate_spec() -> RecreateSpec {
        RecreateSpec {
            name: "bp1-node".into(),
            restart_policy: "unless-stopped".into(),
            network_mode: "bridge".into(),
            binds: vec![Bind { source: "/srv/db".into(), destination: "/data/db".into(), read_only: false }],
            env: vec!["NETWORK=mainnet".into()],
            ports: vec![Port { container: "3001/tcp".into(), host_ip: "".into(), host_port: "3001".into() }],
            entrypoint: "/usr/local/bin/cardano-node".into(),
            args: vec!["run".into(), "--socket-path".into(), "/ipc/node.socket".into()],
        }
    }

    #[test]
    fn recreate_reproduces_the_observed_runspec_onto_the_new_digest() {
        let new = format!("sha256:{}", "d".repeat(64));
        let seq = recreate_argv(&recreate_spec(), "cid-attested", &new).unwrap();
        assert_eq!(seq[0], vec!["docker", "rm", "-f", "cid-attested"], "removes the old container first");
        let run = &seq[1];
        let j = run.join(" ");
        assert!(j.contains("docker run -d --name bp1-node --restart unless-stopped"));
        assert!(j.contains("-p 3001:3001/tcp"), "published port preserved: {j}");
        assert!(j.contains("-e NETWORK=mainnet"), "env preserved: {j}");
        assert!(j.contains("-v /srv/db:/data/db"), "bind mount preserved: {j}");
        assert!(j.contains("--entrypoint /usr/local/bin/cardano-node"), "entrypoint preserved: {j}");
        // The NEW digest is the image ref; the args follow it.
        let img_pos = run.iter().position(|a| a == &new).unwrap();
        assert_eq!(run[img_pos + 1..], ["run".to_string(), "--socket-path".into(), "/ipc/node.socket".into()]);
    }

    #[test]
    fn approval_projection_never_exposes_container_environment_values() {
        let new = format!("sha256:{}", "d".repeat(64));
        let mut spec = recreate_spec();
        spec.env.push("COLD_KEY_SECRET=hunter2".into());
        let plan = recreate_approval_argv(&spec, "cid", &new).unwrap();
        let output = serde_json::to_string(&plan).unwrap();
        assert!(!output.contains("hunter2"));
        assert!(output.contains("COLD_KEY_SECRET=<redacted-target-value>"));
        let sealed = recreate_argv(&spec, "cid", &new).unwrap();
        assert!(serde_json::to_string(&sealed).unwrap().contains("hunter2"));
        let stateless = stateless_recreate_approval_argv(&spec, "cid", &new).unwrap();
        let stateless_output = serde_json::to_string(&stateless).unwrap();
        assert!(!stateless_output.contains("hunter2"));
        assert!(stateless_output.contains("COLD_KEY_SECRET=<redacted-target-value>"));
    }

    #[test]
    fn stateless_kes_keeps_previous_pair_and_cert_outside_ephemeral_payload_dir() {
        let plan = stateless_kes_recovery_plan("cid", "/tmp/ouro-run.123/public-payload");
        for previous in [KES_SKEY_PREVIOUS, KES_VKEY_PREVIOUS, OPCERT_PREVIOUS] {
            assert!(plan.commit.iter().flatten().any(|arg| arg == previous));
            assert!(plan.rollback.iter().flatten().any(|arg| arg == previous));
            assert!(plan.finalize.iter().flatten().any(|arg| arg == previous));
        }
        assert!(plan.commit.iter().skip(6).flatten()
            .any(|arg| arg == "/tmp/ouro-run.123/public-payload"));
        assert!(plan.commit.iter().flatten().any(|arg| arg == KES_STAGE_SKEY));
        assert!(plan.commit.iter().flatten().any(|arg| arg == KES_STAGE_VKEY));
        assert!(plan.commit.iter().chain(&plan.rollback).chain(&plan.finalize).flatten()
            .filter(|arg| arg.contains("ouro-run.123"))
            .all(|arg| arg.ends_with("public-payload")));
    }

    #[test]
    fn stateless_kes_stage_generates_only_in_the_fixed_private_stage() {
        let plan = stateless_kes_stage_plan("cid");
        let encoded = serde_json::to_string(&plan).unwrap();
        assert!(encoded.contains("key-gen-KES"));
        assert!(encoded.contains(KES_STAGE_SKEY));
        assert!(encoded.contains(KES_STAGE_VKEY));
        assert!(encoded.contains("600"));
        assert!(!encoded.contains("cold.skey"));
        assert!(!encoded.contains("/tmp/ouro-run"));
    }

    #[test]
    fn stateless_kes_completion_verifies_all_transaction_residue_absent() {
        let plan = stateless_kes_cleanup_verification_plan("cid");
        assert_eq!(plan.len(), 4);
        for path in [
            KES_STAGE_DIR,
            KES_SKEY_PREVIOUS,
            KES_VKEY_PREVIOUS,
            OPCERT_PREVIOUS,
        ] {
            assert!(plan
                .iter()
                .any(|argv| { argv == &vec!["docker", "exec", "cid", "test", "!", "-e", path] }));
        }
    }

    #[test]
    fn stateless_upgrade_preserves_previous_container_until_finalize() {
        let new = format!("sha256:{}", "d".repeat(64));
        let plan = stateless_recreate_recovery_plan(&recreate_spec(), "cid-old", &new).unwrap();
        assert_eq!(plan.commit[0], vec!["docker", "rename", "cid-old", "bp1-node.ouro-prev"]);
        assert_eq!(plan.commit[1], vec!["docker", "stop", "bp1-node.ouro-prev"]);
        assert_eq!(plan.commit[2][0..3], ["docker", "run", "-d"]);
        assert!(!plan.commit.iter().flatten().any(|arg| arg == "rm"));
        assert_eq!(plan.rollback, vec![
            vec!["docker", "rm", "-f", "bp1-node"],
            vec!["docker", "rename", "bp1-node.ouro-prev", "bp1-node"],
            vec!["docker", "start", "bp1-node"],
        ]);
        assert_eq!(plan.finalize, vec![
            vec!["docker", "rm", "-f", "bp1-node.ouro-prev"],
        ]);
    }

    #[test]
    fn image_presence_requires_exact_config_digest() {
        let target = format!("sha256:{}", "d".repeat(64));
        assert!(verify_image_inspect_output(&target, format!("{target}\n").as_bytes()).is_ok());
        assert!(verify_image_inspect_output(&target, format!("sha256:{}\n", "e".repeat(64)).as_bytes()).is_err());
    }

    #[test]
    fn recreate_is_fail_closed() {
        let new = format!("sha256:{}", "d".repeat(64));
        // No binds → refuse (never recreate a node without its volumes).
        let mut nb = recreate_spec(); nb.binds.clear();
        assert!(recreate_argv(&nb, "cid", &new).is_err());
        // No name → refuse.
        let mut nn = recreate_spec(); nn.name.clear();
        assert!(recreate_argv(&nn, "cid", &new).is_err());
        // Non-digest image → refuse.
        assert!(recreate_argv(&recreate_spec(), "cid", "blinklabs/cardano-node:latest").is_err());
    }

    #[test]
    fn upgrade_rollback_recreates_onto_the_prior_digest() {
        let mut a = att();
        let prior = format!("sha256:{}", "e".repeat(64));
        a.immutable.image_config_digest = prior.clone();
        let seq = upgrade_rollback_plan(&a, &recreate_spec()).unwrap();
        assert_eq!(seq[0][0..2], ["docker".to_string(), "rm".to_string()]);
        assert!(seq[1].iter().any(|arg| arg == &prior), "rollback recreates onto the PRIOR digest");
    }

    #[test]
    fn unknown_op_has_no_executor() {
        assert!(build_plan(&intent("evil/wipe", json!({})), &att(), None).is_err());
    }

    #[test]
    fn rollback_is_a_restart_onto_the_attested_container() {
        assert_eq!(rollback_plan(&att()), vec![vec!["docker", "restart", "cid-attested"]]);
    }

    #[test]
    fn run_argv_reports_exit_status() {
        assert!(run_argv(&["true".into()]).is_ok(), "exit 0 → ok");
        assert!(run_argv(&["false".into()]).is_err(), "nonzero → error");
        assert!(run_argv(&[]).is_err(), "empty argv → error");
        assert!(run_argv(&["this-binary-does-not-exist-xyz".into()]).is_err(), "missing prog → error");
    }

    #[test]
    fn managed_read_returns_data_instead_of_only_an_argv() {
        let plan = vec![vec!["printf".into(), r#"{"block":42}"#.into()]];
        assert_eq!(run_read_plan(&plan).unwrap(), r#"{"block":42}"#);
        assert!(run_read_plan(&[]).is_err());
        assert!(run_read_plan(&[vec!["false".into()]]).is_err());
    }

    #[test]
    fn run_plan_stops_at_first_failure() {
        assert!(run_plan(&[vec!["true".into()], vec!["true".into()]]).is_ok());
        assert!(run_plan(&[vec!["false".into()], vec!["true".into()]]).is_err());
    }
}
