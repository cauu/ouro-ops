//! S0019 p2-5 (§2.6a, §2.5) — role-specific readiness proxies and the dangerous-write confirm
//! binding.
//!
//! The write transaction's `verify` step (§2.6) must decide "is the node healthy after the write"
//! WITHOUT waiting for a freshly-forged block (a low-stake pool may not forge for days). So the
//! postcondition is a set of bounded readiness PROXIES, role-specific: a BP must be running the
//! attested container, answer on its socket on the expected network/genesis, have a valid
//! KES/opcert with credentials loaded, and its tip must be advancing; a relay drops the forging
//! requirements and needs advancing tip + peers.
//!
//! Dangerous writes additionally require a confirm-token bound to the EXACT canonical intent hash +
//! a human-readable diff, so the operator approves precisely what will run (not a category).

use crate::attestation::Role;
use crate::{OuroError, Result};

/// A bounded readiness snapshot gathered post-write (no "block produced" field — see module docs).
#[derive(Debug, Clone)]
pub struct Readiness {
    pub role: Role,
    pub node_running: bool,
    pub container_id_matches: bool,
    pub socket_answers: bool,
    pub network_ok: bool,
    pub genesis_ok: bool,
    /// Two tip samples; advancing = second strictly greater.
    pub tip_block: i64,
    pub tip_block_next: i64,
    pub kes_opcert_valid: bool,
    pub credential_loaded: bool,
    pub established_peers: u32,
}

impl Readiness {
    pub fn tip_advancing(&self) -> bool {
        self.tip_block_next > self.tip_block
    }

    /// Role-specific readiness verdict. Ok = the write's postcondition holds.
    pub fn evaluate(&self) -> Result<()> {
        let fail = |why: &str| {
            Err(OuroError::Validation(format!("readiness proxy failed: {why} (§2.6a)")))
        };
        if !self.node_running {
            return fail("node not running");
        }
        if !self.container_id_matches {
            return fail("running container is not the attested one");
        }
        if !self.socket_answers {
            return fail("node socket does not answer");
        }
        if !self.network_ok || !self.genesis_ok {
            return fail("node on unexpected network/genesis");
        }
        if !self.tip_advancing() {
            return fail("tip not advancing");
        }
        match self.role {
            Role::Bp => {
                if !self.kes_opcert_valid {
                    return fail("KES/opcert invalid (bp cannot forge)");
                }
                if !self.credential_loaded {
                    return fail("forging credentials not loaded");
                }
            }
            Role::Relay => {
                if self.established_peers == 0 {
                    return fail("relay has no established peers");
                }
            }
        }
        Ok(())
    }
}

/// §2.5 — bind a confirm-token to the EXACT canonical intent hash + a human-readable diff summary.
/// The operator approves this specific intent; a token for a different intent/diff is refused.
pub fn bind_confirm(canonical_intent_hash: &str, diff_summary: &str, secret: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac any key len");
    mac.update(canonical_intent_hash.as_bytes());
    mac.update(b"|");
    mac.update(diff_summary.as_bytes());
    mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn verify_confirm(
    token: &str,
    canonical_intent_hash: &str,
    diff_summary: &str,
    secret: &[u8],
) -> Result<()> {
    if bind_confirm(canonical_intent_hash, diff_summary, secret) == token {
        Ok(())
    } else {
        Err(OuroError::Validation(
            "confirm-token does not match this exact intent + diff — refused (§2.5)".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_bp() -> Readiness {
        Readiness {
            role: Role::Bp,
            node_running: true,
            container_id_matches: true,
            socket_answers: true,
            network_ok: true,
            genesis_ok: true,
            tip_block: 100,
            tip_block_next: 101,
            kes_opcert_valid: true,
            credential_loaded: true,
            established_peers: 5,
        }
    }

    #[test]
    fn bp_readiness_pass_and_fail_modes() {
        assert!(healthy_bp().evaluate().is_ok());
        for mutate in [
            |r: &mut Readiness| r.node_running = false,
            |r: &mut Readiness| r.container_id_matches = false,
            |r: &mut Readiness| r.socket_answers = false,
            |r: &mut Readiness| r.network_ok = false,
            |r: &mut Readiness| r.tip_block_next = 100, // not advancing
            |r: &mut Readiness| r.kes_opcert_valid = false,
            |r: &mut Readiness| r.credential_loaded = false,
        ] {
            let mut r = healthy_bp();
            mutate(&mut r);
            assert!(r.evaluate().is_err(), "unhealthy bp must fail readiness");
        }
    }

    #[test]
    fn relay_drops_forging_requirements_but_needs_peers() {
        let mut r = healthy_bp();
        r.role = Role::Relay;
        r.kes_opcert_valid = false; // fine for a relay
        r.credential_loaded = false;
        assert!(r.evaluate().is_ok(), "relay does not need forging creds");
        r.established_peers = 0;
        assert!(r.evaluate().is_err(), "relay with no peers fails");
    }

    #[test]
    fn confirm_binds_exact_intent() {
        let secret = b"tool-run-secret";
        let tok = bind_confirm("hash-abc", "restart bp1", secret);
        assert!(verify_confirm(&tok, "hash-abc", "restart bp1", secret).is_ok());
        // A different intent hash or diff → refused (approval is intent-specific).
        assert!(verify_confirm(&tok, "hash-xyz", "restart bp1", secret).is_err());
        assert!(verify_confirm(&tok, "hash-abc", "rotate kes bp1", secret).is_err());
    }
}
