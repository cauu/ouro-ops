//! S0019 p1-3 (§2.3, §2.14) — the adoption attestation: immutable identity vs versioned managed
//! state, and the evidence-bound adoption approval.
//!
//! The round-2 review found the earlier "closed fingerprint" self-invalidated after a legitimate
//! write (topology/config/KES all change), forcing either a false drift-refuse or a defeated
//! fingerprint. The fix here: split the record into
//!   - IMMUTABLE identity, frozen at adopt (role, digests, container epoch, entrypoint/args, typed
//!     mounts, network/genesis, public credential ids), and
//!   - VERSIONED managed state, a monotonic `state_generation` + hashes of the fields a managed
//!     write may legitimately change, advanced by CAS INSIDE the write transaction (§2.6).
//! Live re-attestation (§2.4) then compares immutable identity AND the live mutable hashes against
//! the recorded state at the current generation: an out-of-band change (no generation bump) is
//! drift; a legitimate write bumped the generation and recorded the new hashes, so it is not.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::convention::RoleRule;
use crate::{OuroError, Result};

/// Target-side path of the attestation (written by the adopt ceremony; root-owned 0640).
pub const ATTESTATION_PATH: &str = "/var/lib/ouro/node-attestation.json";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AdoptionAttestation {
    pub immutable: ImmutableIdentity,
    pub state: ManagedState,
}

/// Frozen at adopt; never changes for the life of the adoption.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImmutableIdentity {
    pub role: Role,
    pub contract_id: String,
    pub convention_version: u32,
    pub host_key_sha256: String,
    pub machine_id: String,
    pub oci_index_digest: String,
    pub platform_manifest_digest: String,
    pub image_config_digest: String,
    pub container_creation_epoch: u64,
    pub entrypoint: Vec<String>,
    pub args: Vec<String>,
    pub mounts: Vec<TypedMount>,
    pub network: String,
    pub genesis_hash: String,
    /// Public credential identifiers only (e.g. opcert hash) — never secret material.
    pub public_credential_ids: Vec<String>,
    /// §2.14 — hash binding the operator's single-use approval to this exact candidate + host key.
    pub approval_evidence_hash: String,
}

/// Advances on every managed write, via CAS inside the transaction.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ManagedState {
    pub state_generation: u64,
    pub container_id: String,
    pub topology_hash: String,
    pub config_hash: String,
    /// KES period / opcert identifier the node is currently forging with (public).
    pub kes_opcert_id: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Bp,
    Relay,
}

/// A typed mount, distinguishing a host bind (with a stable device+inode we can re-verify) from a
/// named volume, so §2.4 can rebind to the exact source and reject a swapped mount.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TypedMount {
    /// `bind` | `volume`.
    pub kind: String,
    /// For a bind: `"<dev>:<inode>"`. For a volume: `"<volume-id>:<driver>"`.
    pub source_id: String,
    pub destination: String,
    pub read_only: bool,
    pub owner: String,
    pub mode: String,
    /// Always true: the executor opens beneath this with no symlink following (§2.4).
    #[serde(default = "yes")]
    pub no_symlink: bool,
}

fn yes() -> bool {
    true
}

/// A live observation of the running node, gathered by the re-attestation probe (§2.4). Compared
/// against the attestation; any mismatch is drift.
#[derive(Debug, Clone)]
pub struct LiveObservation {
    pub image_config_digest: String,
    pub container_id: String,
    pub container_creation_epoch: u64,
    pub entrypoint: Vec<String>,
    pub args: Vec<String>,
    pub mount_source_ids: Vec<String>,
    pub topology_hash: String,
    pub config_hash: String,
    pub kes_opcert_id: String,
    /// Whether forging keys (kes/vrf) are present in the node's key dir.
    pub has_forging_keys: bool,
}

pub fn stable_hash(value: &str) -> String {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    format!("{:016x}", h.finish())
}

impl AdoptionAttestation {
    /// §2.4 closed identity+generation anchor — cheap to compare, changes on identity drift or a
    /// generation change (an out-of-band mutation that did not go through the transaction).
    pub fn closed_fingerprint(&self) -> String {
        let canon = serde_json::to_string(&self.immutable).unwrap_or_default();
        stable_hash(&format!("{canon}|gen={}", self.state.state_generation))
    }

    /// §2.3 role rule check at adopt time: a relay MUST NOT bear forging keys; a BP MUST have an
    /// opcert. Rejects a mis-adopted node before it becomes a trust root.
    pub fn check_role(&self, rule: &RoleRule, live: &LiveObservation) -> Result<()> {
        match self.immutable.role {
            Role::Relay if rule.forbids_forging_keys && live.has_forging_keys => Err(
                OuroError::Validation(
                    "relay bears forging keys — refused (a relay must not hold KES/VRF; §2.3)"
                        .to_string(),
                ),
            ),
            Role::Bp if rule.requires_opcert && live.kes_opcert_id.is_empty() => Err(
                OuroError::Validation(
                    "bp has no operational certificate — refused (§2.3)".to_string(),
                ),
            ),
            _ => Ok(()),
        }
    }

    /// §2.4 — compare the live node against the attestation. Immutable identity must match exactly;
    /// mutable state must match the recorded state at the CURRENT generation (else out-of-band
    /// drift). Called under the per-node lock, before and after each irreversible commit.
    pub fn require_matches_live(&self, live: &LiveObservation) -> Result<()> {
        let id = &self.immutable;
        let drift = |what: &str| {
            Err(OuroError::Validation(format!(
                "node_drift: {what} changed since adoption — refused before mutation (§2.4)"
            )))
        };
        if live.image_config_digest != id.image_config_digest {
            return drift("image config digest");
        }
        if live.container_id != self.state.container_id {
            return drift("container id (recreated)");
        }
        if live.container_creation_epoch != id.container_creation_epoch {
            return drift("container creation epoch");
        }
        if live.entrypoint != id.entrypoint || live.args != id.args {
            return drift("entrypoint/args");
        }
        let mut want: Vec<&String> = id.mounts.iter().map(|m| &m.source_id).collect();
        let mut got: Vec<&String> = live.mount_source_ids.iter().collect();
        want.sort();
        got.sort();
        if want != got {
            return drift("mount source (swapped bind/volume)");
        }
        // Mutable state must equal the recorded state at this generation (no out-of-band change).
        if live.topology_hash != self.state.topology_hash {
            return drift("topology (out-of-band, no generation bump)");
        }
        if live.config_hash != self.state.config_hash {
            return drift("config (out-of-band, no generation bump)");
        }
        if live.kes_opcert_id != self.state.kes_opcert_id {
            return drift("KES/opcert (out-of-band, no generation bump)");
        }
        Ok(())
    }

    /// §2.6 — advance the managed state via CAS. Only succeeds if `expected_generation` matches the
    /// current one (so two writers cannot both advance from the same base). Returns the updated
    /// attestation to persist atomically with the mutation.
    pub fn advance_state(
        &self,
        expected_generation: u64,
        new_state: ManagedState,
    ) -> Result<AdoptionAttestation> {
        if expected_generation != self.state.state_generation {
            return Err(OuroError::Validation(format!(
                "state CAS failed: expected generation {expected_generation}, current is {} — a \
                 concurrent write advanced it (§2.6)",
                self.state.state_generation
            )));
        }
        let mut next = new_state;
        next.state_generation = self.state.state_generation + 1;
        Ok(AdoptionAttestation {
            immutable: self.immutable.clone(),
            state: next,
        })
    }
}

/// §2.14 — evidence-bound adoption approval. The adopt ceremony produces a canonical candidate
/// attestation; the operator's single-use token/signature is bound to that hash + the target host
/// key, and the resulting evidence hash is stored in the immutable identity. An agent may invoke
/// the path (P0-1), but only a token bound to THIS candidate + host key blesses THIS container as
/// the trust root.
pub fn candidate_hash(immutable_without_approval: &serde_json::Value) -> String {
    stable_hash(&serde_json::to_string(immutable_without_approval).unwrap_or_default())
}

pub fn bind_approval(candidate_hash: &str, operator_token: &str, host_key_sha256: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(operator_token.as_bytes())
        .expect("hmac accepts any key length");
    mac.update(candidate_hash.as_bytes());
    mac.update(b"|");
    mac.update(host_key_sha256.as_bytes());
    let tag = mac.finalize().into_bytes();
    tag.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verify an approval evidence hash matches the candidate + host key under the operator token.
pub fn verify_approval(
    evidence_hash: &str,
    candidate_hash: &str,
    operator_token: &str,
    host_key_sha256: &str,
) -> Result<()> {
    if bind_approval(candidate_hash, operator_token, host_key_sha256) == evidence_hash {
        Ok(())
    } else {
        Err(OuroError::Validation(
            "adoption approval evidence does not match this candidate + host key — refused (§2.14)"
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convention::RoleRule;

    fn ident() -> ImmutableIdentity {
        ImmutableIdentity {
            role: Role::Bp,
            contract_id: "blinklabs-cardano-node-v1".into(),
            convention_version: 1,
            host_key_sha256: "hk".into(),
            machine_id: "bp1".into(),
            oci_index_digest: "sha256:idx".into(),
            platform_manifest_digest: "sha256:pm".into(),
            image_config_digest: "sha256:cfg".into(),
            container_creation_epoch: 1000,
            entrypoint: vec!["cardano-node".into()],
            args: vec!["run".into()],
            mounts: vec![TypedMount {
                kind: "bind".into(),
                source_id: "8:1:12345".into(),
                destination: "/data/db".into(),
                read_only: false,
                owner: "root".into(),
                mode: "0755".into(),
                no_symlink: true,
            }],
            network: "mainnet".into(),
            genesis_hash: "gh".into(),
            public_credential_ids: vec!["opcert:abc".into()],
            approval_evidence_hash: "ev".into(),
        }
    }
    fn att() -> AdoptionAttestation {
        AdoptionAttestation {
            immutable: ident(),
            state: ManagedState {
                state_generation: 7,
                container_id: "cid123".into(),
                topology_hash: "t0".into(),
                config_hash: "c0".into(),
                kes_opcert_id: "kes:5".into(),
            },
        }
    }
    fn live() -> LiveObservation {
        LiveObservation {
            image_config_digest: "sha256:cfg".into(),
            container_id: "cid123".into(),
            container_creation_epoch: 1000,
            entrypoint: vec!["cardano-node".into()],
            args: vec!["run".into()],
            mount_source_ids: vec!["8:1:12345".into()],
            topology_hash: "t0".into(),
            config_hash: "c0".into(),
            kes_opcert_id: "kes:5".into(),
            has_forging_keys: true,
        }
    }

    #[test]
    fn live_match_and_drift() {
        let a = att();
        assert!(a.require_matches_live(&live()).is_ok(), "identical live matches");
        for mutate in [
            |l: &mut LiveObservation| l.image_config_digest = "sha256:evil".into(),
            |l: &mut LiveObservation| l.container_id = "cid999".into(),
            |l: &mut LiveObservation| l.args = vec!["run".into(), "--evil".into()],
            |l: &mut LiveObservation| l.mount_source_ids = vec!["9:9:99".into()],
            |l: &mut LiveObservation| l.topology_hash = "t-oob".into(),
            |l: &mut LiveObservation| l.kes_opcert_id = "kes:oob".into(),
        ] {
            let mut d = live();
            mutate(&mut d);
            assert!(a.require_matches_live(&d).is_err(), "drift must be refused");
        }
    }

    #[test]
    fn legitimate_write_advances_generation_not_drift() {
        let a = att();
        // A legit topology change: advance state (CAS) records the new hash + bumps generation.
        let advanced = a
            .advance_state(
                7,
                ManagedState {
                    state_generation: 0, // overwritten by advance_state
                    container_id: "cid123".into(),
                    topology_hash: "t1".into(),
                    config_hash: "c0".into(),
                    kes_opcert_id: "kes:5".into(),
                },
            )
            .unwrap();
        assert_eq!(advanced.state.state_generation, 8);
        // After the write, the live node shows the new topology → matches the advanced record.
        let mut post = live();
        post.topology_hash = "t1".into();
        assert!(advanced.require_matches_live(&post).is_ok(), "post-write is not drift");
        // Fingerprint changed because the generation advanced.
        assert_ne!(a.closed_fingerprint(), advanced.closed_fingerprint());
    }

    #[test]
    fn cas_rejects_stale_generation() {
        let a = att();
        assert!(a.advance_state(6, a.state.clone()).is_err(), "stale expected gen refused");
    }

    #[test]
    fn role_rule_relay_forbids_forging_keys() {
        let mut a = att();
        a.immutable.role = Role::Relay;
        let rule = RoleRule { requires_opcert: false, forbids_forging_keys: true };
        let mut l = live();
        l.has_forging_keys = true;
        assert!(a.check_role(&rule, &l).is_err(), "relay with forging keys refused");
        l.has_forging_keys = false;
        assert!(a.check_role(&rule, &l).is_ok());
    }

    #[test]
    fn approval_binding_is_candidate_and_hostkey_specific() {
        let ch = candidate_hash(&serde_json::json!({"machine_id":"bp1","image":"sha256:cfg"}));
        let ev = bind_approval(&ch, "op-token-xyz", "hk");
        assert!(verify_approval(&ev, &ch, "op-token-xyz", "hk").is_ok());
        // Wrong token, wrong host key, or a different candidate all fail.
        assert!(verify_approval(&ev, &ch, "op-token-xyz", "hk-other").is_err());
        assert!(verify_approval(&ev, &ch, "wrong-token", "hk").is_err());
        assert!(verify_approval(&ev, "different-candidate", "op-token-xyz", "hk").is_err());
    }
}
