//! S0019 p4-2 / p7-3 — the sealed executor: a FIXED SEQUENCE of fixed argvs per operation.
//!
//! The transaction's commit step performs the mutation via fixed argv arrays — never a shell, never
//! string interpolation. The variable parts come from the ATTESTATION (the container id, the
//! attested network, resolved paths) and from digest-resolved inbox artifacts — NOT from the
//! agent's intent parameters (those were already validated as closed selectors, and never reach
//! argv as a command token). So even a hostile-but-schema-valid parameter cannot become injection:
//! the executor builds `["docker","restart","<attested container id>"]`, not `sh -c "<anything>"`.
//!
//! Artifact-bearing ops (kes-rotation opcert, deploy signed-tx, upgrade image) consume a
//! content-addressed inbox artifact resolved BY DIGEST (`inbox::resolve` re-verifies the bytes
//! against the ref). The resolved path is a target-side, digest-verified fact — never the agent's
//! string. If a required artifact is not staged, the op is REFUSED (it never silently degrades to a
//! bare restart). The artifacts these ops install are PUBLIC (the opcert `node.cert` is a public
//! certificate; a signed tx is public) — the KES signing key and the air-gapped cold key are NEVER
//! touched, requested, or transported by this executor (§ category-3 stays sealed).
//!
//! `build_plan` returns what the executor WOULD run; the actual `std::process` invocation happens
//! target-side (as the confined principal). This module is the sealed argv builder + its proof.

use std::path::Path;

use serde::Deserialize;

use crate::attestation::AdoptionAttestation;
use crate::intent::Intent;
use crate::{OuroError, Result};

/// The upgrade recreate spec (§2.10) — the target-side `docker inspect` facts needed to recreate the
/// container onto a new image WITHOUT losing anything the probe modeled. Fail-closed: the probe emits
/// `null` (→ refusal) for any shape it cannot faithfully reproduce (named volumes, tmpfs, etc.).
#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct Bind {
    pub source: String,
    pub destination: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Port {
    pub container: String,
    pub host_ip: String,
    pub host_port: String,
}

/// The converged container layout (§2.2): fixed destinations the sealed executor writes to.
const KEYS_DIR: &str = "/opt/cardano/config/keys";
const OPCERT_DEST: &str = "/opt/cardano/config/keys/node.cert";
const SOCKET: &str = "/ipc/node.socket";
/// Where a signed tx artifact is staged INSIDE the container before submit (ephemeral, public tx).
const TX_STAGE: &str = "/tmp/ouro-tx.signed";

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
fn resolve_artifact(intent: &Intent, param: &str, inbox: Option<&Path>) -> Result<String> {
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
        Some(dir) => Ok(crate::inbox::resolve(dir, art_ref)?.display().to_string()),
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
        // Apply the on-disk topology by restarting onto it (the topology content itself is delivered
        // out-of-band into the mounted config dir; this op is the "apply" = restart).
        "runtime/topology-apply" => Ok(vec![restart()]),
        // Apply the on-disk rendered config by restarting onto it (was a bogus `--version` no-op).
        "config/render" => Ok(vec![restart()]),
        // A managed READ — query the node's tip via the container's socket + attested network.
        "observability/health" => {
            let mut argv = vec![
                s("docker"), s("exec"), cid, s("cardano-cli"), s("query"), s("tip"),
                s("--socket-path"), s(SOCKET),
            ];
            argv.extend(net_flags(&att.immutable.network)?);
            Ok(vec![argv])
        }
        // kes-rotation: install the digest-resolved opcert (public `node.cert`) into the keys mount,
        // then restart. NEVER touches the KES signing key or the air-gapped cold key — the operator
        // builds the opcert air-gapped and stages it; this executor only installs the public cert.
        "kes-rotation/rotate" => {
            let opcert = resolve_artifact(intent, "opcert", inbox)?;
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
            let tx = resolve_artifact(intent, "tx", inbox)?;
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

/// Build the recreate SEQUENCE for an upgrade (§2.10): remove the attested container, then
/// `docker run` a new one onto `image_digest`, faithfully reproducing the observed run-spec (name,
/// restart policy, network, published ports, env, bind mounts, entrypoint + args). FAIL-CLOSED: any
/// missing/ambiguous fact is refused — we never recreate a node with a partial spec. `image_digest`
/// is `sha256:<…>` (a target-present, allowlist-verified digest); nothing here is an agent string
/// except the digest, which was validated as a closed selector + allowlist membership.
pub fn recreate_argv(spec: &RecreateSpec, cid: &str, image_digest: &str) -> Result<Vec<Vec<String>>> {
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
    // Preserve a non-default network exactly (host / a named network); the docker default is elided.
    if !matches!(spec.network_mode.as_str(), "" | "default" | "bridge") {
        run.push(s("--network"));
        run.push(spec.network_mode.clone());
    }
    for p in &spec.ports {
        // hostip:hostport:container | hostport:container — reproduce the published mapping.
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
    Ok(vec![vec![s("docker"), s("rm"), s("-f"), cid.to_string()], run])
}

/// The upgrade rollback plan: recreate the container onto the PRIOR (attested) image digest with the
/// same observed run-spec — the honest inverse of a recreate. (Whether this restores service depends
/// on DB compatibility; the honest RollbackToN / ReSyncRequired classification is `upgrade.rs`.)
pub fn upgrade_rollback_plan(att: &AdoptionAttestation, spec: &RecreateSpec) -> Result<Vec<Vec<String>>> {
    recreate_argv(spec, &att.state.container_id, &att.immutable.image_config_digest)
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

/// Run a fixed SEQUENCE of argvs in order, stopping at the first failure. Each argv came from
/// `build_plan` (attested facts + digest-resolved artifacts only), so nothing agent-supplied is
/// interpolated here.
pub fn run_plan(plan: &[Vec<String>]) -> Result<()> {
    for argv in plan {
        run_argv(argv)?;
    }
    Ok(())
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
                host_key_sha256: "hk".into(), machine_id: "bp1".into(), oci_index_digest: "i".into(),
                platform_manifest_digest: "p".into(), image_config_digest: "cfg".into(),
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
    fn config_render_is_a_real_restart_not_a_version_probe() {
        let plan = build_plan(&intent("config/render", json!({"machine": "bp1"})), &att(), None).unwrap();
        assert_eq!(plan, vec![vec!["docker", "restart", "cid-attested"]]);
        assert!(!plan.iter().flatten().any(|a| a == "--version"), "no bogus --version");
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
        assert!(build_plan(&intent("kes-rotation/rotate", json!({"machine":"bp1"})), &att(), None).is_err());
        // Preview shows the cp+restart sequence with a placeholder path (no inbox needed).
        let good = format!("opcert-1@sha256:{}", "a".repeat(64));
        let plan = build_plan(&intent("kes-rotation/rotate", json!({"machine":"bp1","opcert":good})), &att(), None).unwrap();
        assert_eq!(plan.len(), 2, "cp then restart");
        assert_eq!(plan[0][0..2], ["docker".to_string(), "cp".to_string()]);
        assert_eq!(plan[0][2], "<inbox:opcert-1@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa>");
        assert_eq!(plan[0][3], format!("cid-attested:{OPCERT_DEST}"));
        assert_eq!(plan[1], vec!["docker", "restart", "cid-attested"]);
    }

    #[test]
    fn kes_resolves_real_inbox_artifact_by_digest() {
        let dir = std::env::temp_dir().join(format!("ouro-exec-kes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let opcert_bytes = br#"{"type":"NodeOperationalCertificate","cborHex":"deadbeef"}"#;
        let art_ref = crate::inbox::stage(&dir, crate::inbox::ArtifactType::Opcert, opcert_bytes).unwrap();
        let plan = build_plan(
            &intent("kes-rotation/rotate", json!({"machine":"bp1","opcert":art_ref})),
            &att(), Some(&dir),
        ).unwrap();
        // The cp source is the REAL resolved inbox path (digest-verified), never the agent's string.
        assert!(plan[0][2].contains(dir.to_str().unwrap()), "resolved to the inbox path: {:?}", plan[0]);
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
    fn run_plan_stops_at_first_failure() {
        assert!(run_plan(&[vec!["true".into()], vec!["true".into()]]).is_ok());
        assert!(run_plan(&[vec!["false".into()], vec!["true".into()]]).is_err());
    }
}
