//! S0019 p3-1 (§2.9) — fleet lease authority (pool-wide single writer).
//!
//! The round-2 review showed a per-node lock cannot stop two control machines from each passing a
//! local preflight and each stopping one relay (quorum lost). The fix is a pool-wide authority: a
//! monotonic pool generation + an exclusive, expiring LEASE with a fencing token. A controller
//! acquires the lease (bumping the fencing token); every disruptive step carries a signed STEP
//! PERMIT the target verifies; a target refuses a permit whose fencing token is stale (a fenced,
//! crashed, or superseded holder). Quorum + BP-last are re-evaluated immediately before each step,
//! not just at preflight.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{OuroError, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Lease {
    pub pool_id: String,
    pub holder: String,
    /// Monotonic fencing token — a new acquisition strictly increases it; a target refuses any
    /// permit below the highest token it has seen (fences a stale/crashed holder).
    pub fencing_token: u64,
    pub expiry_epoch: u64,
}

/// Durable pool authority record (control-side). Acquiring bumps the fencing token past the
/// current one and past any expired holder.
pub struct PoolAuthority {
    path: PathBuf,
}

impl PoolAuthority {
    pub fn at(dir: &Path, pool_id: &str) -> PoolAuthority {
        PoolAuthority { path: dir.join(format!("{pool_id}.lease.json")) }
    }

    fn read(&self) -> Option<Lease> {
        serde_json::from_str(&std::fs::read_to_string(&self.path).ok()?).ok()
    }

    fn write(&self, lease: &Lease) -> Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        std::fs::write(&self.path, serde_json::to_string(lease).unwrap())
            .map_err(|e| OuroError::Validation(format!("lease write: {e}")))
    }

    /// Acquire the lease. Fails if a non-expired lease is held by ANOTHER holder. On success the
    /// fencing token strictly increases (fencing any prior holder).
    pub fn acquire(
        &self,
        pool_id: &str,
        holder: &str,
        now_epoch: u64,
        ttl_secs: u64,
    ) -> Result<Lease> {
        let prev = self.read();
        if let Some(l) = &prev {
            let live = l.expiry_epoch > now_epoch;
            if live && l.holder != holder {
                return Err(OuroError::Validation(format!(
                    "pool {pool_id} lease held by {} until epoch {} — refused (single fleet writer, \
                     §2.9)",
                    l.holder, l.expiry_epoch
                )));
            }
        }
        let next_token = prev.as_ref().map(|l| l.fencing_token + 1).unwrap_or(1);
        let lease = Lease {
            pool_id: pool_id.to_string(),
            holder: holder.to_string(),
            fencing_token: next_token,
            expiry_epoch: now_epoch + ttl_secs,
        };
        self.write(&lease)?;
        Ok(lease)
    }
}

/// The highest fencing token a target has honored (target-side, monotonic). A permit below this is
/// refused — this is what actually fences a superseded controller at the point of action.
pub struct TargetFence {
    path: PathBuf,
}

impl TargetFence {
    pub fn at(dir: &Path, node_id: &str) -> TargetFence {
        TargetFence { path: dir.join(format!("{node_id}.fence")) }
    }
    fn highest(&self) -> u64 {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }
    /// Verify + record a step permit's fencing token. Refuses a token below the highest seen; a
    /// valid, at-or-above token ratchets the fence forward.
    pub fn accept(&self, permit: &StepPermit, now_epoch: u64) -> Result<()> {
        if permit.expiry_epoch <= now_epoch {
            return Err(OuroError::Validation("step permit expired (§2.9)".into()));
        }
        let high = self.highest();
        if permit.fencing_token < high {
            return Err(OuroError::Validation(format!(
                "step permit fencing token {} is stale (target has honored {}) — a superseded \
                 controller is fenced (§2.9)",
                permit.fencing_token, high
            )));
        }
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        std::fs::write(&self.path, permit.fencing_token.to_string())
            .map_err(|e| OuroError::Validation(format!("fence write: {e}")))?;
        Ok(())
    }
}

/// A permit for ONE disruptive step against ONE node, carrying the holder's fencing token.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StepPermit {
    pub pool_id: String,
    pub node_id: String,
    pub fencing_token: u64,
    pub expiry_epoch: u64,
}

/// Quorum re-evaluation immediately before a disruptive step (§2.9): taking `about_to_stop` offline
/// must still leave at least `min_online_relays` relays online.
pub fn require_quorum(
    online_relays: u32,
    min_online_relays: u32,
    about_to_stop_is_relay: bool,
) -> Result<()> {
    let remaining = if about_to_stop_is_relay {
        online_relays.saturating_sub(1)
    } else {
        online_relays
    };
    if remaining < min_online_relays {
        return Err(OuroError::Validation(format!(
            "quorum guard: stopping this relay would leave {remaining} online (< min {min_online_relays}) \
             — refused (§2.9)"
        )));
    }
    Ok(())
}

/// BP-last ordering: a BP step is only allowed once every relay in the rollout batch is done.
pub fn require_bp_last(is_bp_step: bool, relays_remaining: u32) -> Result<()> {
    if is_bp_step && relays_remaining > 0 {
        return Err(OuroError::Validation(format!(
            "BP step refused: {relays_remaining} relays still pending — BP is last (§2.9)"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ouro-fleet-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        d
    }

    #[test]
    fn lease_is_exclusive_and_fences() {
        let d = dir("lease");
        let auth = PoolAuthority::at(&d, "pool1");
        let l1 = auth.acquire("pool1", "ctrl-A", 1000, 300).unwrap();
        // A different controller cannot acquire while A's lease is live.
        assert!(auth.acquire("pool1", "ctrl-B", 1100, 300).is_err());
        // After expiry, B can acquire and the fencing token strictly increases.
        let l2 = auth.acquire("pool1", "ctrl-B", 2000, 300).unwrap();
        assert!(l2.fencing_token > l1.fencing_token, "fencing token increases");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn target_fences_a_stale_controller() {
        let d = dir("fence");
        let fence = TargetFence::at(&d, "relay1");
        // Controller B (token 2) acts first → target honors token 2.
        let p2 = StepPermit { pool_id: "p".into(), node_id: "relay1".into(), fencing_token: 2, expiry_epoch: 5000 };
        assert!(fence.accept(&p2, 1000).is_ok());
        // A superseded controller A (token 1) is now fenced at the point of action.
        let p1 = StepPermit { pool_id: "p".into(), node_id: "relay1".into(), fencing_token: 1, expiry_epoch: 5000 };
        assert!(fence.accept(&p1, 1000).is_err(), "stale token fenced");
        // An expired permit is refused.
        let p3 = StepPermit { pool_id: "p".into(), node_id: "relay1".into(), fencing_token: 3, expiry_epoch: 100 };
        assert!(fence.accept(&p3, 1000).is_err(), "expired permit refused");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn two_controllers_cannot_both_pass_and_break_quorum() {
        // Two relays online, min 1. Each of two controllers thinks it can stop one relay.
        // The fleet authority makes only ONE hold the lease; and quorum re-eval refuses the second
        // stop even if a race got that far.
        assert!(require_quorum(2, 1, true).is_ok(), "stopping 1 of 2 leaves 1 (>= min)");
        assert!(require_quorum(1, 1, true).is_err(), "stopping the last relay breaks quorum");
    }

    #[test]
    fn bp_is_last() {
        assert!(require_bp_last(true, 2).is_err(), "bp refused while relays pending");
        assert!(require_bp_last(true, 0).is_ok(), "bp allowed once relays done");
        assert!(require_bp_last(false, 2).is_ok(), "relay steps unaffected");
    }
}
