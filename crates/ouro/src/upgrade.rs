//! S0019 p3-2 (§2.10) — node-runtime N→N+1 upgrade, DB-compat honest.
//!
//! S0018 distributes only the ouro-ops binary; the NODE runtime upgrade is owned here. A transition
//! is gated by SIGNED metadata declaring node/cli/protocol and DB-format compatibility. The order:
//! upgrade ouro first, canary a relay, BP last, preserve volumes, verify, then atomically rotate
//! the attestation. Rollback restores runtime AND attestation ONLY if a tested backward-compatible
//! downgrade or a crash-consistent volume snapshot exists; otherwise the ONLY honest outcome is
//! forward-recovery / re-sync — and we say so rather than promise a rollback we cannot deliver.
//! Images arrive preloaded via the inbox (§2.7); no on-target fetch.

use serde::{Deserialize, Serialize};

use crate::{OuroError, Result};

/// Signed transition metadata for one N→N+1 hop (verified against the release key like the
/// allowlist; embedded-trusted until the S0018 feed exists).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TransitionMeta {
    pub from_convention_version: u32,
    pub to_convention_version: u32,
    pub from_image_config_digest: String,
    pub to_image_config_digest: String,
    /// The new node can read the old chain DB unchanged (N data → N+1 runtime).
    pub db_forward_compatible: bool,
    /// The old node can read any DB writes made by N+1 (N+1 data → N runtime). This, not forward
    /// compatibility, is what makes a runtime downgrade safe.
    pub db_backward_compatible: bool,
    /// A crash-consistent volume snapshot was taken before the upgrade (with capacity checked).
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
    meta.db_backward_compatible || meta.snapshot_taken
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

/// Validate a transition before starting: it must be exactly N→N+1, and the target image must be
/// on the allowlist (§2.1). Returns an error a rollout must not proceed past.
pub fn validate_transition(
    meta: &TransitionMeta,
    allowlist: &crate::convention::Allowlist,
    platform: &str,
) -> Result<()> {
    if meta.to_convention_version != meta.from_convention_version + 1 {
        return Err(OuroError::Validation(format!(
            "only N→N+1 transitions are supported (got {}→{}) (§2.10)",
            meta.from_convention_version, meta.to_convention_version
        )));
    }
    let from = allowlist.contract_for(&meta.from_image_config_digest, platform)?;
    let to = allowlist.contract_for(&meta.to_image_config_digest, platform)?;
    if from.convention_version != meta.from_convention_version
        || to.convention_version != meta.to_convention_version
    {
        return Err(OuroError::Validation(
            "upgrade transition versions do not match the signed layout contracts".into(),
        ));
    }
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
            from_convention_version: 1,
            to_convention_version: 2,
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
        // A snapshot restores the rollback path.
        m.db_backward_compatible = false;
        m.snapshot_taken = true;
        assert_eq!(failure_outcome(&m), FailureOutcome::RollbackToN);
    }

    #[test]
    fn only_n_to_n_plus_1_and_allowlisted_target() {
        let mut m = meta();
        m.to_convention_version = 3; // skips a version
        let allow = crate::convention::Allowlist::embedded().unwrap();
        assert!(validate_transition(&m, &allow, "linux/amd64").is_err(), "N→N+2 refused");
        // N→N+1 but a non-allowlisted target image → refused.
        let m2 = meta();
        assert!(validate_transition(&m2, &allow, "linux/amd64").is_err(), "unknown target image refused");
    }
}
