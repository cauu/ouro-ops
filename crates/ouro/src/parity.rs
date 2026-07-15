//! S0019 p2-4 (§2.8) — executor identity / anti-downgrade parity, and legacy-write disablement.
//!
//! The attestation binds the node image; §2.8 additionally binds the SECURITY-DECIDING code: the
//! ouro-ops build id, the executor/registry/intent-schema digests, and a minimum security version.
//! `tool run` requires control↔target parity (same security identity family) BEFORE accepting an
//! intent, and refuses a target below the minimum security version — so an attested node cannot run
//! an older/mutable validator under weaker rules while the node fingerprint still matches. All
//! legacy S0017 write entry points are refused unless migrated into the deny-by-default registry.

use sha2::{Digest, Sha256};

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
            executor_digest: security_code_digest(),
            intent_schema_version: 1,
            registry_len: intent::registry().len(),
            min_security_version: crate::version::current(),
        }
    }

    /// Compact control→target identity covering every field checked by `require_parity`.
    pub fn wire_digest(&self) -> String {
        let material = format!(
            "{}\n{}\n{}\n{}\n{}.{}.{}",
            self.build_id, self.executor_digest, self.intent_schema_version, self.registry_len,
            self.min_security_version.0, self.min_security_version.1, self.min_security_version.2
        );
        sha256(material.as_bytes())
    }
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Digest security-deciding Rust source together with embedded Skills/assets. `include_str!` makes
/// builds from the same source portable across control/target platforms while detecting stale code.
fn security_sources() -> [(&'static str, &'static str); 38] {
    [
        ("attestation.rs", include_str!("attestation.rs")),
        ("audit.rs", include_str!("audit.rs")),
        ("bootstrap.rs", include_str!("bootstrap.rs")),
        ("cli.rs", include_str!("cli.rs")),
        ("cold_sign.rs", include_str!("cold_sign.rs")),
        ("config.rs", include_str!("config.rs")),
        ("confirm.rs", include_str!("confirm.rs")),
        ("convention.rs", include_str!("convention.rs")),
        ("dispatch.rs", include_str!("dispatch.rs")),
        ("domain.rs", include_str!("domain.rs")),
        ("error.rs", include_str!("error.rs")),
        ("executor.rs", include_str!("executor.rs")),
        ("fleet.rs", include_str!("fleet.rs")),
        ("gate.rs", include_str!("gate.rs")),
        ("inbox.rs", include_str!("inbox.rs")),
        ("intent.rs", include_str!("intent.rs")),
        ("kes.rs", include_str!("kes.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("main.rs", include_str!("main.rs")),
        ("migration.rs", include_str!("migration.rs")),
        ("onboard.rs", include_str!("onboard.rs")),
        ("output.rs", include_str!("output.rs")),
        ("parity.rs", include_str!("parity.rs")),
        ("pool.rs", include_str!("pool.rs")),
        ("provision.rs", include_str!("provision.rs")),
        ("readiness.rs", include_str!("readiness.rs")),
        ("render.rs", include_str!("render.rs")),
        ("s0019_cli.rs", include_str!("s0019_cli.rs")),
        ("s0019_confirmation.rs", include_str!("s0019_confirmation.rs")),
        ("secrets.rs", include_str!("secrets.rs")),
        ("skills.rs", include_str!("skills.rs")),
        ("ssh.rs", include_str!("ssh.rs")),
        ("state.rs", include_str!("state.rs")),
        ("status.rs", include_str!("status.rs")),
        ("supervisor.rs", include_str!("supervisor.rs")),
        ("transaction.rs", include_str!("transaction.rs")),
        ("upgrade.rs", include_str!("upgrade.rs")),
        ("version.rs", include_str!("version.rs")),
    ]
}

fn security_code_digest() -> String {
    let mut hasher = Sha256::new();
    for (_, component) in security_sources() {
        hasher.update((component.len() as u64).to_be_bytes());
        hasher.update(component.as_bytes());
    }
    let allowlist = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/allowlist.json"));
    hasher.update((allowlist.len() as u64).to_be_bytes());
    hasher.update(allowlist.as_bytes());
    hasher.update(skills::embedded_digest().as_bytes());
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod identity_tests {
    use super::security_sources;

    #[test]
    fn security_identity_source_set_is_explicit_and_complete() {
        let names = security_sources().map(|(name, _)| name);
        assert_eq!(names, [
            "attestation.rs", "audit.rs", "bootstrap.rs", "cli.rs", "cold_sign.rs", "config.rs",
            "confirm.rs", "convention.rs", "dispatch.rs", "domain.rs", "error.rs", "executor.rs",
            "fleet.rs", "gate.rs", "inbox.rs", "intent.rs", "kes.rs", "lib.rs", "main.rs",
            "migration.rs", "onboard.rs", "output.rs", "parity.rs", "pool.rs", "provision.rs",
            "readiness.rs", "render.rs", "s0019_cli.rs", "s0019_confirmation.rs", "secrets.rs",
            "skills.rs", "ssh.rs", "state.rs", "status.rs", "supervisor.rs", "transaction.rs",
            "upgrade.rs", "version.rs",
        ]);
    }
}

/// §2.8 — require control↔target parity before accepting an intent. The executor digest + intent
/// schema version must match exactly (a mismatch means one side runs different security-deciding
/// code), and the target must be at or above the control's minimum security version (anti-
/// downgrade). Actionable error routes to re-adopt/upgrade.
pub fn require_parity(control: &SecurityIdentity, target: &SecurityIdentity) -> Result<()> {
    if control.build_id != target.build_id {
        return Err(OuroError::Validation(format!(
            "ouro build mismatch: control {} vs target {} — update the target binary (§2.8)",
            control.build_id, target.build_id
        )));
    }
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
    if control.registry_len != target.registry_len {
        return Err(OuroError::Validation(format!(
            "intent registry mismatch: control has {} operations vs target {} (§2.8)",
            control.registry_len, target.registry_len
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

/// Target-side dispatch gate for the compact security identity supplied by the control binary.
pub fn require_expected_wire_digest(expected: &str) -> Result<()> {
    let actual = SecurityIdentity::local().wire_digest();
    if expected == actual {
        Ok(())
    } else {
        Err(OuroError::Validation(format!(
            "control→target security identity mismatch: expected {}… but target is {}… — update the target binary (§2.8)",
            &expected[..expected.len().min(12)], &actual[..actual.len().min(12)]
        )))
    }
}

/// Legacy S0017 write tools that are DISABLED in the S0019 world: a write may run only if it is in
/// the new deny-by-default registry (§2.5). A legacy tool name that is NOT registered is refused;
/// this closes the "run an old write entry point under weaker rules" gap during greenfield cutover.
const LEGACY_WRITE_TOOLS: &[&str] = &[
    "config/render",
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
    fn wire_digest_covers_all_parity_fields() {
        let id = SecurityIdentity::local();
        assert!(require_expected_wire_digest(&id.wire_digest()).is_ok());
        let mut changed = id.clone();
        changed.registry_len += 1;
        assert_ne!(id.wire_digest(), changed.wire_digest());
        assert!(require_parity(&id, &changed).is_err());
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
