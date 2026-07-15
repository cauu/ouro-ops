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

use hmac::{Hmac, Mac};
use sha2::Sha256;

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
    pool_id: String,
}

impl PoolAuthority {
    pub fn at(dir: &Path, pool_id: &str) -> PoolAuthority {
        PoolAuthority {
            path: dir.join(format!("{pool_id}.lease.json")),
            pool_id: pool_id.to_string(),
        }
    }

    fn read(&self) -> Result<Option<Lease>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| OuroError::Validation(format!("malformed fleet lease: {e}")))
    }

    fn write(&self, lease: &Lease) -> Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(lease)
            .map_err(|e| OuroError::Validation(format!("lease serialize: {e}")))?;
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.create(true).truncate(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        if let Some(parent) = self.path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
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
        if pool_id != self.pool_id {
            return Err(OuroError::Validation("fleet authority pool id mismatch".into()));
        }
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let _lock = crate::gate::NodeLock::acquire(
            &parent.join("locks"), pool_id, holder,
        )?;
        let prev = self.read()?;
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
    node_id: String,
}

#[derive(Deserialize, Serialize)]
struct FenceState {
    pool_id: String,
    node_id: String,
    highest_fencing_token: u64,
}

impl TargetFence {
    pub fn at(dir: &Path, node_id: &str) -> TargetFence {
        TargetFence {
            path: dir.join(format!("{node_id}.fence")),
            node_id: node_id.to_string(),
        }
    }
    fn state(&self) -> Result<Option<FenceState>> {
        std::fs::read_to_string(&self.path)
            .map(|text| serde_json::from_str(&text))
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    return Ok(Ok(FenceState {
                        pool_id: String::new(),
                        node_id: self.node_id.clone(),
                        highest_fencing_token: 0,
                    }));
                }
                Err(error)
            })
            .map_err(OuroError::from)?
            .map(Some)
            .map_err(|e| OuroError::Validation(format!("malformed target fence: {e}")))
    }
    /// Verify + record a step permit's fencing token. Refuses a token below the highest seen; a
    /// valid, at-or-above token ratchets the fence forward.
    pub fn accept(&self, permit: &StepPermit, now_epoch: u64) -> Result<()> {
        if permit.expiry_epoch <= now_epoch {
            return Err(OuroError::Validation("step permit expired (§2.9)".into()));
        }
        if permit.node_id != self.node_id {
            return Err(OuroError::Validation("step permit node does not match target fence".into()));
        }
        let state = self.state()?.unwrap();
        if !state.pool_id.is_empty() && state.pool_id != permit.pool_id {
            return Err(OuroError::Validation(format!(
                "target {} is already fenced to pool {}; permit for pool {} refused",
                self.node_id, state.pool_id, permit.pool_id
            )));
        }
        let high = state.highest_fencing_token;
        if permit.fencing_token <= high {
            return Err(OuroError::Validation(format!(
                "step permit fencing token {} is stale/replayed (target has honored {}) — a \
                 superseded or replaying controller is fenced (§2.9)",
                permit.fencing_token, high
            )));
        }
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = self.path.with_extension("fence.tmp");
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.create(true).truncate(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp)?;
            let state = FenceState {
                pool_id: permit.pool_id.clone(),
                node_id: permit.node_id.clone(),
                highest_fencing_token: permit.fencing_token,
            };
            let bytes = serde_json::to_vec(&state)
                .map_err(|e| OuroError::Validation(format!("fence serialize: {e}")))?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        if let Some(parent) = self.path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

/// A permit for ONE disruptive step against ONE node, carrying the holder's fencing token.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StepPermit {
    pub pool_id: String,
    pub node_id: String,
    pub operation_id: String,
    pub role: String,
    pub fencing_token: u64,
    pub expiry_epoch: u64,
    pub online_relays: u32,
    pub min_online_relays: u32,
    pub relays_remaining: u32,
    pub permit_id: String,
    pub signature: String,
}

impl StepPermit {
    fn signing_message(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.pool_id, self.node_id, self.operation_id, self.role, self.fencing_token,
            self.expiry_epoch, self.online_relays, self.min_online_relays,
            self.relays_remaining, self.permit_id
        )
    }

    pub fn sign(mut self, secret: &[u8]) -> Result<Self> {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret)
            .map_err(|_| OuroError::Validation("invalid fleet signing key".into()))?;
        mac.update(self.signing_message().as_bytes());
        self.signature = mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect();
        Ok(self)
    }

    pub fn verify(
        &self,
        expected_node: &str,
        expected_operation: &str,
        expected_role: &str,
        secret: &[u8],
        now_epoch: u64,
    ) -> Result<()> {
        if self.node_id != expected_node
            || self.operation_id != expected_operation
            || self.role != expected_role
        {
            return Err(OuroError::Validation("fleet permit target/operation/role mismatch".into()));
        }
        if self.expiry_epoch <= now_epoch {
            return Err(OuroError::Validation("fleet step permit expired (§2.9)".into()));
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(secret)
            .map_err(|_| OuroError::Validation("invalid fleet signing key".into()))?;
        mac.update(self.signing_message().as_bytes());
        let signature = decode_hex(&self.signature)?;
        mac.verify_slice(&signature)
            .map_err(|_| OuroError::Validation("fleet step permit signature mismatch".into()))?;
        require_quorum(
            self.online_relays,
            self.min_online_relays,
            self.role == "relay",
        )?;
        require_bp_last(self.role == "bp", self.relays_remaining)
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OuroError::Validation("malformed fleet permit signature".into()));
    }
    (0..value.len()).step_by(2).map(|index| {
        u8::from_str_radix(&value[index..index + 2], 16)
            .map_err(|_| OuroError::Validation("malformed fleet permit signature".into()))
    }).collect()
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

    fn permit(token: u64, expiry: u64) -> StepPermit {
        StepPermit {
            pool_id: "p".into(), node_id: "relay1".into(),
            operation_id: "runtime/restart".into(), role: "relay".into(),
            fencing_token: token, expiry_epoch: expiry,
            online_relays: 2, min_online_relays: 1, relays_remaining: 1,
            permit_id: format!("permit-{token}"), signature: String::new(),
        }.sign(b"secret").unwrap()
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
        let p2 = permit(2, 5000);
        p2.verify("relay1", "runtime/restart", "relay", b"secret", 1000).unwrap();
        assert!(fence.accept(&p2, 1000).is_ok());
        // A superseded controller A (token 1) is now fenced at the point of action.
        let p1 = permit(1, 5000);
        assert!(fence.accept(&p1, 1000).is_err(), "stale token fenced");
        assert!(fence.accept(&p2, 1000).is_err(), "equal-token replay fenced");
        let mut wrong_pool = permit(3, 5000);
        wrong_pool.pool_id = "other-pool".into();
        wrong_pool.signature.clear();
        wrong_pool = wrong_pool.sign(b"secret").unwrap();
        assert!(fence.accept(&wrong_pool, 1000).is_err(), "target pool binding enforced");
        // An expired permit is refused.
        let p3 = permit(3, 100);
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

    #[test]
    fn signed_permit_binds_policy_and_refuses_quorum_lie_after_tamper() {
        let signed = permit(1, 5000);
        assert!(signed.verify("relay1", "runtime/restart", "relay", b"secret", 1000).is_ok());
        let mut tampered = signed.clone();
        tampered.online_relays = 99;
        assert!(tampered.verify("relay1", "runtime/restart", "relay", b"secret", 1000).is_err());
        let mut unsafe_permit = permit(2, 5000);
        unsafe_permit.online_relays = 1;
        unsafe_permit.signature.clear();
        unsafe_permit = unsafe_permit.sign(b"secret").unwrap();
        assert!(unsafe_permit.verify("relay1", "runtime/restart", "relay", b"secret", 1000).is_err());
    }
}
