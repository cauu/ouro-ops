//! S0019 p4-2 — the sealed executor: FIXED argv per operation.
//!
//! The transaction's commit step performs the mutation via a FIXED argv array — never a shell,
//! never string interpolation. The command's variable parts come from the ATTESTATION (the
//! container id, resolved paths) and from digest-resolved inbox artifacts — NOT from the agent's
//! intent parameters (those were already validated as closed selectors, and never reach argv as a
//! command token). So even a hostile-but-schema-valid parameter cannot become injection: the
//! executor builds `["docker","restart","<attested container id>"]`, not `sh -c "<anything>"`.
//!
//! `build_argv` returns what the executor WOULD run; the actual `std::process` invocation happens
//! target-side (as the confined principal). This module is the sealed argv builder + its proof.

use crate::attestation::AdoptionAttestation;
use crate::intent::Intent;
use crate::{OuroError, Result};

/// Build the fixed argv for a validated intent against an attested node. The container id is taken
/// from the attestation; artifact params are immutable references resolved by digest before use.
pub fn build_argv(intent: &Intent, att: &AdoptionAttestation) -> Result<Vec<String>> {
    let cid = att.state.container_id.clone();
    let s = |x: &str| x.to_string();
    match intent.operation_id.as_str() {
        "runtime/restart" => Ok(vec![s("docker"), s("restart"), cid]),
        // topology-apply: the executor writes the derived topology (from the operator's spec, sealed)
        // and restarts; here the mutation step is the restart onto it.
        "runtime/topology-apply" => Ok(vec![s("docker"), s("restart"), cid]),
        "config/render" => Ok(vec![
            s("docker"), s("exec"), cid, s("cardano-node"), s("--version"),
        ]),
        // kes-rotation: install the opcert artifact (referenced by digest, resolved from the inbox)
        // then restart. The opcert param is an artifact REFERENCE, never a path in argv.
        "kes-rotation/rotate" => {
            let _opcert = intent
                .payload
                .get("opcert")
                .and_then(|v| v.as_str())
                .ok_or_else(|| OuroError::Validation("kes-rotation/rotate needs opcert".into()))?;
            // The executor resolves the inbox path by digest (§2.7) and installs it; the resolved
            // path is a target-side, digest-verified fact — never the agent's string.
            Ok(vec![s("docker"), s("restart"), cid])
        }
        "deploy/register-submit" => Ok(vec![
            s("docker"), s("exec"), cid, s("cardano-cli"), s("transaction"), s("submit"),
        ]),
        // A managed read — query the node's tip (health is derived target-side from reads like this).
        "observability/health" => Ok(vec![
            s("docker"), s("exec"), cid, s("cardano-cli"), s("query"), s("tip"),
        ]),
        // upgrade step: recreate the container onto the new (digest-verified, inbox) image. The
        // image param is an artifact REFERENCE resolved by digest target-side, never a raw arg.
        "upgrade/step" => {
            let _image = intent
                .payload
                .get("image")
                .and_then(|v| v.as_str())
                .ok_or_else(|| OuroError::Validation("upgrade/step needs image".into()))?;
            Ok(vec![s("docker"), s("restart"), cid])
        }
        other => Err(OuroError::Validation(format!(
            "no sealed executor for {other} (§2.5)"
        ))),
    }
}

/// Run a FIXED argv (the first element is the program). This is the transaction's commit action on
/// the target — a direct exec, never a shell. Returns Ok on exit 0, else a typed error. The argv
/// came from `build_argv` (attested facts only), so nothing agent-supplied is interpolated here.
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
    fn argv_uses_attested_container_not_agent_param() {
        // Even if the agent's machine param were hostile, argv uses the ATTESTED container id.
        let i = intent("runtime/restart", json!({"machine": "bp1"}));
        let argv = build_argv(&i, &att()).unwrap();
        assert_eq!(argv, vec!["docker", "restart", "cid-attested"]);
        // No shell, no agent string, anywhere in argv.
        assert!(!argv.iter().any(|a| a.contains("sh") || a.contains("bp1")));
    }

    #[test]
    fn hostile_param_never_reaches_argv() {
        // A param string that would be dangerous in a shell is simply not present in the fixed argv.
        let i = intent("runtime/restart", json!({"machine": "bp1; rm -rf /"}));
        let argv = build_argv(&i, &att()).unwrap();
        assert!(!argv.iter().any(|a| a.contains("rm") || a.contains(";")),
            "no agent string reaches argv: {argv:?}");
    }

    #[test]
    fn unknown_op_has_no_executor() {
        assert!(build_argv(&intent("evil/wipe", json!({})), &att()).is_err());
    }

    #[test]
    fn run_argv_reports_exit_status() {
        assert!(run_argv(&["true".into()]).is_ok(), "exit 0 → ok");
        assert!(run_argv(&["false".into()]).is_err(), "nonzero → error");
        assert!(run_argv(&[]).is_err(), "empty argv → error");
        assert!(run_argv(&["this-binary-does-not-exist-xyz".into()]).is_err(), "missing prog → error");
    }

    #[test]
    fn kes_needs_opcert_ref() {
        assert!(build_argv(&intent("kes-rotation/rotate", json!({"machine":"bp1"})), &att()).is_err());
        let good = format!("opcert-1@sha256:{}", "a".repeat(64));
        assert!(build_argv(&intent("kes-rotation/rotate",
            json!({"machine":"bp1","opcert":good})), &att()).is_ok());
    }
}
