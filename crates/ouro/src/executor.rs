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

use crate::attestation::AdoptionAttestation;
use crate::intent::Intent;
use crate::{OuroError, Result};

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
        // upgrade/step: HONEST REFUSAL. Recreating the container onto a new image must re-apply the
        // full run-spec, but the attestation stores mounts as device identity (major:minor:inode) for
        // verification — NOT as remountable host paths — so a faithful recreate cannot be built as a
        // fixed argv here. The multi-step, output-reading recreate belongs to the rollout flow
        // (`upgrade::plan_rollout`, §3 p3-2), not this per-container sealed executor. We validate the
        // image ref is staged + digest-verified, then refuse rather than fake a bare restart.
        "upgrade/step" => {
            let image = resolve_artifact(intent, "image", inbox)?;
            Err(OuroError::Validation(format!(
                "upgrade/step: image {image} is staged + digest-verified, but container recreate is \
                 not a fixed-argv sealed step (mounts are attested as device identity, not host \
                 paths); run the upgrade through the rollout flow (upgrade::plan_rollout, §3 p3-2)"
            )))
        }
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
    fn upgrade_refuses_recreate_but_validates_image() {
        // No image → refused for the missing artifact.
        assert!(build_plan(&intent("upgrade/step", json!({"machine":"bp1"})), &att(), None).is_err());
        // With an image → still refused, but the message names the rollout flow (not a fake restart).
        let img = format!("img-1@sha256:{}", "c".repeat(64));
        let err = build_plan(&intent("upgrade/step", json!({"machine":"bp1","image":img})), &att(), None).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("rollout"), "honest pointer to the rollout flow: {msg}");
        assert!(!msg.contains("restart"), "must not pretend a restart is an upgrade");
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
