//! S0019 p3-2 (§2.10) — node-runtime N→N+1 upgrade, DB-compat honest.
//!
//! S0018 distributes only the ouro-ops binary; the NODE runtime upgrade is owned here. A transition
//! is gated by SIGNED metadata declaring node/cli/protocol and DB-format compatibility. The order:
//! upgrade ouro first, canary a relay, BP last, preserve volumes, verify, then atomically rotate
//! the attestation. Rollback restores runtime AND attestation ONLY if a tested backward-compatible
//! downgrade exists; otherwise the ONLY honest outcome is
//! forward-recovery / re-sync — and we say so rather than promise a rollback we cannot deliver.
//! Images arrive preloaded via the inbox (§2.7); no on-target fetch.

use serde::{Deserialize, Serialize};

use crate::{OuroError, Result};

/// Signed transition metadata for one adjacent runtime hop (verified against the release key like
/// the allowlist). Adjacency is the presence of this exact directed digest edge in signed policy;
/// layout convention versions remain independent because multiple node releases can share one
/// stable container contract.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransitionMeta {
    pub from_image_config_digest: String,
    pub to_image_config_digest: String,
    /// The new node can read the old chain DB unchanged (N data → N+1 runtime).
    pub db_forward_compatible: bool,
    /// The old node can read any DB writes made by N+1 (N+1 data → N runtime). This, not forward
    /// compatibility, is what makes a runtime downgrade safe.
    pub db_backward_compatible: bool,
    /// Release metadata may require/describe snapshots, but this static bit is NOT proof that a
    /// snapshot exists for this node/step. Automatic rollback ignores it until a future intent
    /// binds a concrete snapshot id, capacity check, creation evidence, and restore plan.
    pub snapshot_taken: bool,
}

/// One ordered step of a rollout. Relays (canary first) precede the BP; the BP is last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutStep {
    pub node_id: String,
    pub is_bp: bool,
}

/// Order a rollout: relays first (BP-last), preserving the caller's relay order (canary = first).
pub fn plan_rollout(relays: &[&str], bp: &str) -> Vec<RolloutStep> {
    let mut steps: Vec<RolloutStep> = relays
        .iter()
        .map(|r| RolloutStep { node_id: (*r).to_string(), is_bp: false })
        .collect();
    steps.push(RolloutStep { node_id: bp.to_string(), is_bp: true });
    steps
}

/// Whether a true rollback (restore runtime + attestation to N) is possible for this transition.
pub fn rollback_possible(meta: &TransitionMeta) -> bool {
    meta.db_backward_compatible
}

/// The honest outcome the spec/operator must be told when an upgrade step fails.
#[derive(Debug, PartialEq, Eq)]
pub enum FailureOutcome {
    /// Runtime + attestation restored to N.
    RollbackToN,
    /// The DB was migrated with no downgrade/snapshot — rollback is impossible; only a re-sync (or
    /// forward-recovery) restores service. We do NOT pretend a rollback will work.
    ReSyncRequired,
}

pub fn failure_outcome(meta: &TransitionMeta) -> FailureOutcome {
    if rollback_possible(meta) {
        FailureOutcome::RollbackToN
    } else {
        FailureOutcome::ReSyncRequired
    }
}

/// Validate a selected transition before starting. The exact directed edge is the signed N→N+1
/// authority; both endpoints must be allowlisted even when they share one layout convention.
pub fn validate_transition(
    meta: &TransitionMeta,
    allowlist: &crate::convention::Allowlist,
    platform: &str,
) -> Result<()> {
    if meta.from_image_config_digest == meta.to_image_config_digest {
        return Err(OuroError::Validation(
            "runtime transition must change the exact image config digest (§2.10)".into(),
        ));
    }
    let signed = allowlist.transition_for(
        &meta.from_image_config_digest,
        &meta.to_image_config_digest,
    )?;
    if signed != meta {
        return Err(OuroError::Validation(
            "runtime transition metadata does not match the signed directed edge (§2.10)".into(),
        ));
    }
    allowlist.contract_for(&meta.from_image_config_digest, platform)?;
    allowlist.contract_for(&meta.to_image_config_digest, platform)?;
    if !meta.db_forward_compatible {
        return Err(OuroError::Validation(
            "N+1 cannot read the existing chain DB; an in-place step is unsupported".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> TransitionMeta {
        TransitionMeta {
            from_image_config_digest: "sha256:old".into(),
            to_image_config_digest: "sha256:new".into(),
            db_forward_compatible: true,
            db_backward_compatible: false,
            snapshot_taken: false,
        }
    }

    #[test]
    fn rollout_is_bp_last() {
        let steps = plan_rollout(&["relay1", "relay2"], "bp1");
        assert_eq!(steps.len(), 3);
        assert!(!steps[0].is_bp && !steps[1].is_bp);
        assert!(steps[2].is_bp, "bp is last");
        assert_eq!(steps[0].node_id, "relay1", "canary order preserved");
    }

    #[test]
    fn rollback_only_when_recoverable_else_honest_resync() {
        // Forward-compatible alone does NOT prove that the old runtime can read N+1 writes.
        assert_eq!(failure_outcome(&meta()), FailureOutcome::ReSyncRequired);
        let mut m = meta();
        m.db_backward_compatible = true;
        assert_eq!(failure_outcome(&m), FailureOutcome::RollbackToN);
        // Static signed metadata is not per-node snapshot evidence and cannot restore rollback.
        m.db_backward_compatible = false;
        m.snapshot_taken = true;
        assert_eq!(failure_outcome(&m), FailureOutcome::ReSyncRequired);
    }

    #[test]
    fn exact_edge_must_change_digest_and_use_allowlisted_endpoints() {
        let mut m = meta();
        m.to_image_config_digest = m.from_image_config_digest.clone();
        let allow = crate::convention::Allowlist::embedded().unwrap();
        assert!(
            validate_transition(&m, &allow, "linux/amd64").is_err(),
            "self transition refused"
        );
        // An exact edge with a non-allowlisted target image is refused.
        let m2 = meta();
        assert!(
            validate_transition(&m2, &allow, "linux/amd64").is_err(),
            "unknown target image refused"
        );

        let mut signed = allow.transitions[0].clone();
        signed.db_backward_compatible = !signed.db_backward_compatible;
        assert!(
            validate_transition(&signed, &allow, "linux/amd64").is_err(),
            "metadata differing from the signed edge refused"
        );
    }
}
