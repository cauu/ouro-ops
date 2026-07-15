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

    /// Acquire a lease for exactly one disruptive permit window. ANY non-expired lease refuses,
    /// including the same caller-supplied holder: allowing same-holder reacquisition would let one
    /// controller mint permits for two relays from the same pre-stop snapshot and run them in
    /// parallel. On success the fencing token strictly increases (fencing every prior holder).
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
            if live {
                return Err(OuroError::Validation(format!(
                    "pool {pool_id} already has an active one-step lease held by {} until epoch {} \
                     — wait for expiry before authorizing another disruptive step (§2.9)",
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
#[serde(deny_unknown_fields)]
pub struct RelayHealthEndpoint {
    pub node_id: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StepPermit {
    pub pool_id: String,
    pub pool_spec_digest: String,
    pub network: String,
    pub genesis_hash: String,
    pub target_host_key_sha256: String,
    pub node_id: String,
    pub operation_id: String,
    /// Exact final target-validated intent approved by the operator before this live permit was
    /// minted. The permit is the last, short-lived authorization step; it never changes the plan.
    pub intent_hash: String,
    pub role: String,
    /// Present only for upgrade/step. Binds BP-last/relays-remaining facts to the exact image those
    /// facts were computed against, so a permit for image X cannot authorize image Y.
    pub target_image: Option<String>,
    pub fencing_token: u64,
    pub expiry_epoch: u64,
    /// Epoch when the control started collecting the signed live-facts snapshot. Targets reject
    /// stale snapshots even if the outer lease has a longer remaining TTL.
    pub facts_epoch: u64,
    pub online_relays: u32,
    pub min_online_relays: u32,
    pub relays_remaining: u32,
    /// Spec-derived public relay endpoints. The target probes these immediately before consuming
    /// the permit, narrowing (but not eliminating) the snapshot race for a relay crash/partition.
    pub relay_health_endpoints: Vec<RelayHealthEndpoint>,
    pub permit_id: String,
    pub signature: String,
}

/// Closed target-side expectation for one permit. Grouping these fields prevents positional
/// mix-ups between similarly shaped pool/host/intent digests at the authorization boundary.
pub struct PermitExpectation {
    pub pool_id: String,
    pub pool_spec_digest: String,
    pub node_id: String,
    pub operation_id: String,
    pub role: String,
    pub target_image: Option<String>,
    pub min_online_relays: u32,
    pub network: String,
    pub genesis_hash: String,
    pub target_host_key_sha256: String,
    pub intent_hash: String,
}

impl StepPermit {
    fn signing_message(&self) -> String {
        let endpoints = serde_json::to_string(&self.relay_health_endpoints)
            .expect("closed relay endpoint fields serialize");
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.pool_id, self.pool_spec_digest, self.network, self.genesis_hash,
            self.target_host_key_sha256, self.node_id,
            self.operation_id, self.intent_hash, self.role,
            self.target_image.as_deref().unwrap_or(""), self.fencing_token, self.expiry_epoch,
            self.facts_epoch, self.online_relays, self.min_online_relays, self.relays_remaining,
            endpoints, self.permit_id
        )
    }

    /// Immediately before a disruptive commit, independently require enough OTHER live relay
    /// endpoints. The original full readiness snapshot remains signed; this residual TCP probe
    /// narrows the crash/partition window without putting SSH credentials on the target. It proves
    /// only momentary endpoint liveness, not Cardano readiness or an atomic quorum guarantee.
    pub fn require_live_relay_quorum(&self) -> Result<()> {
        if self.min_online_relays == 0 {
            return Ok(());
        }
        let candidates = self.relay_health_endpoints.iter()
            .filter(|endpoint| self.role != "relay" || endpoint.node_id != self.node_id)
            .cloned()
            .collect::<Vec<_>>();
        let mut receivers = Vec::with_capacity(candidates.len());
        for endpoint in candidates {
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                use std::net::ToSocketAddrs;
                let live = (endpoint.host.as_str(), endpoint.port)
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut addresses| {
                        addresses.any(|address| {
                            std::net::TcpStream::connect_timeout(
                                &address,
                                std::time::Duration::from_secs(2),
                            )
                            .is_ok()
                        }).then_some(())
                    })
                    .is_some();
                let _ = sender.send(live);
            });
            receivers.push(receiver);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let online = receivers.into_iter().filter(|receiver| {
            receiver.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap_or(false)
        }).count() as u32;
        if online < self.min_online_relays {
            return Err(OuroError::Validation(format!(
                "immediate relay endpoint quorum failed: {online} reachable other relays < required {} — disruptive commit refused",
                self.min_online_relays
            )));
        }
        Ok(())
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
        expected: &PermitExpectation,
        secret: &[u8],
        now_epoch: u64,
    ) -> Result<()> {
        if self.pool_id != expected.pool_id
            || self.pool_spec_digest != expected.pool_spec_digest
            || self.node_id != expected.node_id
            || self.operation_id != expected.operation_id
            || self.role != expected.role
            || self.target_image != expected.target_image
            || self.min_online_relays != expected.min_online_relays
            || self.network != expected.network
            || self.genesis_hash != expected.genesis_hash
            || self.target_host_key_sha256 != expected.target_host_key_sha256
            || self.intent_hash != expected.intent_hash
        {
            return Err(OuroError::Validation(
                "fleet permit target/operation/role/image/policy/pool/intent mismatch".into(),
            ));
        }
        if self.expiry_epoch <= now_epoch {
            return Err(OuroError::Validation("fleet step permit expired (§2.9)".into()));
        }
        if self.facts_epoch > now_epoch.saturating_add(5)
            || now_epoch.saturating_sub(self.facts_epoch) > 30
        {
            return Err(OuroError::Validation(
                "fleet live-facts snapshot is stale or from the future — re-evaluate immediately \
                 before the disruptive step (§2.9)"
                    .into(),
            ));
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
            pool_id: "p".into(), pool_spec_digest: format!("sha256:{}", "a".repeat(64)),
            network: "mainnet".into(), genesis_hash: "genesis".into(),
            target_host_key_sha256: "h".repeat(64),
            node_id: "relay1".into(), operation_id: "runtime/restart".into(),
            intent_hash: "i".repeat(64), role: "relay".into(),
            target_image: None,
            fencing_token: token, expiry_epoch: expiry,
            facts_epoch: 1000,
            online_relays: 2, min_online_relays: 1, relays_remaining: 1,
            relay_health_endpoints: vec![
                RelayHealthEndpoint { node_id: "relay1".into(), host: "127.0.0.1".into(), port: 9 },
                RelayHealthEndpoint { node_id: "relay2".into(), host: "127.0.0.1".into(), port: 9 },
            ],
            permit_id: format!("permit-{token}"), signature: String::new(),
        }.sign(b"secret").unwrap()
    }

    fn expectation(operation: &str, target_image: Option<&str>, host: &str) -> PermitExpectation {
        PermitExpectation {
            pool_id: "p".into(),
            pool_spec_digest: format!("sha256:{}", "a".repeat(64)),
            node_id: "relay1".into(),
            operation_id: operation.into(),
            role: "relay".into(),
            target_image: target_image.map(str::to_string),
            min_online_relays: 1,
            network: "mainnet".into(),
            genesis_hash: "genesis".into(),
            target_host_key_sha256: host.into(),
            intent_hash: "i".repeat(64),
        }
    }

    #[test]
    fn lease_is_exclusive_and_fences() {
        let d = dir("lease");
        let auth = PoolAuthority::at(&d, "pool1");
        let l1 = auth.acquire("pool1", "ctrl-A", 1000, 300).unwrap();
        // Neither a different controller nor the same caller-supplied holder can acquire while the
        // one-step lease is live; otherwise two target-specific permits could execute concurrently.
        assert!(auth.acquire("pool1", "ctrl-B", 1100, 300).is_err());
        assert!(auth.acquire("pool1", "ctrl-A", 1100, 300).is_err());
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
        p2.verify(&expectation("runtime/restart", None, &"h".repeat(64)), b"secret", 1000).unwrap();
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
    fn same_holder_cannot_authorize_two_relays_from_one_snapshot_window() {
        let d = dir("same-holder-two-relays");
        let auth = PoolAuthority::at(&d, "pool1");
        let first = auth.acquire("pool1", "controller-a", 1000, 120).unwrap();
        assert_eq!(first.fencing_token, 1);
        assert!(
            auth.acquire("pool1", "controller-a", 1001, 120).is_err(),
            "same holder must not mint a second target permit while relay1's window is active"
        );
        std::fs::remove_dir_all(&d).ok();
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
        assert!(signed.verify(&expectation("runtime/restart", None, &"h".repeat(64)), b"secret", 1000).is_ok());
        let mut tampered = signed.clone();
        tampered.online_relays = 99;
        assert!(tampered.verify(&expectation("runtime/restart", None, &"h".repeat(64)), b"secret", 1000).is_err());
        assert!(signed.verify(&expectation("runtime/restart", None, &"b".repeat(64)), b"secret", 1000).is_err(), "permit snapshot for host B cannot execute on host A");
        let mut unsafe_permit = permit(2, 5000);
        unsafe_permit.online_relays = 1;
        unsafe_permit.signature.clear();
        unsafe_permit = unsafe_permit.sign(b"secret").unwrap();
        assert!(unsafe_permit.verify(&expectation("runtime/restart", None, &"h".repeat(64)), b"secret", 1000).is_err());
    }

    #[test]
    fn signed_permit_rejects_stale_live_facts() {
        let mut p = permit(1, 5000);
        p.facts_epoch = 969;
        p.signature.clear();
        p = p.sign(b"secret").unwrap();
        assert!(p.verify(&expectation("runtime/restart", None, &"h".repeat(64)), b"secret", 1000).is_err());
    }

    #[test]
    fn upgrade_permit_binds_exact_target_image() {
        let image = format!("sha256:{}", "a".repeat(64));
        let other = format!("sha256:{}", "b".repeat(64));
        let mut p = permit(1, 5000);
        p.operation_id = "upgrade/step".into();
        p.target_image = Some(image.clone());
        p.signature.clear();
        p = p.sign(b"secret").unwrap();
        assert!(p.verify(&expectation("upgrade/step", Some(&image), &"h".repeat(64)), b"secret", 1000).is_ok());
        assert!(p.verify(&expectation("upgrade/step", Some(&other), &"h".repeat(64)), b"secret", 1000).is_err());
    }

    #[test]
    fn permit_envelope_is_closed() {
        let mut value = serde_json::to_value(permit(1, 5000)).unwrap();
        value.as_object_mut().unwrap().insert("online_bps".into(), serde_json::json!(99));
        assert!(serde_json::from_value::<StepPermit>(value).is_err());
    }
}
