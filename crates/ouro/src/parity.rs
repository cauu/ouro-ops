//! S0019 p2-4 (§2.8) — executor identity / anti-downgrade parity, and legacy-write disablement.
//!
//! The attestation binds the node image; §2.8 additionally binds the SECURITY-DECIDING code: the
//! ouro-ops build id, the executor/registry/intent-schema digests, and a minimum security version.
//! `tool run` requires control↔target parity (same security identity family) BEFORE accepting an
//! intent, and refuses a target below the minimum security version — so an attested node cannot run
//! an older/mutable validator under weaker rules while the node fingerprint still matches. All
//! legacy S0017 write entry points are refused unless migrated into the deny-by-default registry.

use crate::{intent, skills, OuroError, Result};

/// The security identity a side presents. Derived from embedded, root-owned, content-addressed
/// facts (no mutable on-disk override in production).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityIdentity {
    pub build_id: String,
    /// Digest over the embedded skill/executor assets (the registry + schemas live here).
    pub executor_digest: String,
    pub intent_schema_version: u32,
    pub registry_len: usize,
    pub min_security_version: (u64, u64, u64),
}

impl SecurityIdentity {
    /// This binary's identity.
    pub fn local() -> SecurityIdentity {
        SecurityIdentity {
            build_id: env!("CARGO_PKG_VERSION").to_string(),
            executor_digest: skills::embedded_digest(),
            intent_schema_version: 1,
            registry_len: intent::registry().len(),
            min_security_version: crate::version::current(),
        }
    }
}

/// §2.8 — require control↔target parity before accepting an intent. The executor digest + intent
/// schema version must match exactly (a mismatch means one side runs different security-deciding
/// code), and the target must be at or above the control's minimum security version (anti-
/// downgrade). Actionable error routes to re-adopt/upgrade.
pub fn require_parity(control: &SecurityIdentity, target: &SecurityIdentity) -> Result<()> {
    if control.executor_digest != target.executor_digest {
        return Err(OuroError::Validation(format!(
            "executor parity mismatch: control executor {}… vs target {}… — the target runs \
             different security-deciding code; re-run adopt with a matching binary (§2.8)",
            &control.executor_digest[..control.executor_digest.len().min(12)],
            &target.executor_digest[..target.executor_digest.len().min(12)]
        )));
    }
    if control.intent_schema_version != target.intent_schema_version {
        return Err(OuroError::Validation(format!(
            "intent schema version mismatch: control v{} vs target v{} (§2.8)",
            control.intent_schema_version, target.intent_schema_version
        )));
    }
    if target.min_security_version < control.min_security_version {
        return Err(OuroError::Validation(format!(
            "target security version {:?} is below the control minimum {:?} — anti-downgrade \
             refused; upgrade the target binary (§2.8)",
            target.min_security_version, control.min_security_version
        )));
    }
    Ok(())
}

/// Legacy S0017 write tools that are DISABLED in the S0019 world: a write may run only if it is in
/// the new deny-by-default registry (§2.5). A legacy tool name that is NOT registered is refused;
/// this closes the "run an old write entry point under weaker rules" gap during greenfield cutover.
const LEGACY_WRITE_TOOLS: &[&str] = &[
    "runtime/topology-apply",
    "runtime/restart",
    "kes-rotation/rotate",
    "kes-rotation/generate-offline",
    "kes-rotation/push-offline",
    "deploy/register-build",
    "deploy/register-submit",
    "deploy/provision",
    "deploy/sync",
    "deploy/start",
    "deploy/takeover",
    "observability/install-gateway",
    "upgrade/rollout",
    "upgrade/upgrade-one",
];

/// A write operation is admissible only if it is in the new registry. A legacy write name that has
/// not been migrated into the registry is refused (not silently accepted through an old path).
pub fn require_registered_write(operation_id: &str) -> Result<()> {
    if intent::lookup(operation_id).is_some() {
        return Ok(()); // migrated into the deny-by-default registry
    }
    if LEGACY_WRITE_TOOLS.contains(&operation_id) {
        return Err(OuroError::Validation(format!(
            "legacy write tool {operation_id} is disabled in S0019 — it has not been migrated into \
             the intent registry (§2.8); refused"
        )));
    }
    Err(OuroError::Validation(format!(
        "unknown write operation {operation_id} — refused (deny-by-default, §2.5)"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_matches_self() {
        let id = SecurityIdentity::local();
        assert!(require_parity(&id, &id).is_ok());
    }

    #[test]
    fn executor_digest_mismatch_refused() {
        let control = SecurityIdentity::local();
        let mut target = control.clone();
        target.executor_digest = "sha256:different".into();
        assert!(require_parity(&control, &target).is_err());
    }

    #[test]
    fn downgrade_refused() {
        let control = SecurityIdentity::local();
        let mut target = control.clone();
        // A target one patch version below the control minimum is refused.
        let (a, b, c) = target.min_security_version;
        target.min_security_version = (a, b, c.saturating_sub(1).min(c));
        if target.min_security_version < control.min_security_version {
            assert!(require_parity(&control, &target).is_err());
        }
        // schema skew also refused.
        let mut t2 = control.clone();
        t2.intent_schema_version = 2;
        assert!(require_parity(&control, &t2).is_err());
    }

    #[test]
    fn legacy_write_disabled_unless_registered() {
        // A registered op passes.
        assert!(require_registered_write("runtime/restart").is_ok());
        // A legacy write NOT in the registry is refused as disabled.
        assert!(require_registered_write("deploy/takeover").is_err());
        // An entirely unknown op is refused deny-by-default.
        assert!(require_registered_write("evil/wipe").is_err());
    }
}
