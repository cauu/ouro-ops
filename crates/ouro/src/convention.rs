//! S0019 p1-1 (§2.1) — the layout contract + SIGNED digest allowlist.
//!
//! The convention that makes the environment finite: a versioned table of layout contracts, each
//! naming the fixed in-container paths, role rules, and an allowlist of IMMUTABLE OCI digests
//! (never a tag). A node conforms only if its running image's config digest is `allowed` (and not
//! `denylisted`) for a platform; the returned contract is the ONLY layout source the skills read.
//!
//! Trust: the allowlist is embedded in the binary (like the skill pack) and carries a signature
//! field. Verifying that signature against a pinned release key is release infra (S0018) — until
//! that feed is wired, the EMBEDDED payload is trusted (mirrors `version::security_floor`, never a
//! weaker fallback). Anti-rollback: a target refuses an allowlist older than a locally recorded,
//! MAC-protected floor (mirrors `version.rs`), so erasing the floor cannot reopen a downgrade.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::{OuroError, Result};

/// The embedded, signed allowlist payload (release infra verifies the signature; see module docs).
const EMBEDDED_ALLOWLIST: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/allowlist.json"));

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Allowlist {
    pub allowlist_version: u32,
    /// Signature over the payload; verified against the pinned release key by S0018 infra. The
    /// literal `EMBEDDED-TRUSTED` marks the pre-infra state (embedded == trusted, never weaker).
    pub signature: String,
    pub contracts: Vec<LayoutContract>,
    /// Emergency revocation: image_config digests refused even if a stale `allowed` still lists
    /// them. A denylist entry ALWAYS wins over an allow.
    #[serde(default)]
    pub denylist: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LayoutContract {
    pub convention_version: u32,
    pub contract_id: String,
    pub in_container_paths: InContainerPaths,
    pub role_rules: RoleRules,
    pub allowed: Vec<AllowedImage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InContainerPaths {
    pub socket: String,
    pub db: String,
    pub keys: String,
    pub config: String,
    pub topology: String,
    pub genesis: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoleRules {
    pub bp: RoleRule,
    pub relay: RoleRule,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct RoleRule {
    pub requires_opcert: bool,
    pub forbids_forging_keys: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AllowedImage {
    pub platform: String,
    pub oci_index_digest: String,
    pub image_config_digest: String,
}

impl Allowlist {
    /// The embedded allowlist. A signed release feed replaces the embedded payload in production;
    /// the `OURO_ALLOWLIST_FILE` override is the seam for that feed (and for the container bed,
    /// which pins the bed image's real config digest). The override is trusted like the embedded
    /// payload — never a weaker fallback.
    pub fn embedded() -> Result<Self> {
        let text = match std::env::var_os("OURO_ALLOWLIST_FILE") {
            Some(p) => std::fs::read_to_string(&p).map_err(|e| {
                OuroError::Validation(format!("cannot read OURO_ALLOWLIST_FILE {p:?}: {e}"))
            })?,
            None => EMBEDDED_ALLOWLIST.to_string(),
        };
        let v: Allowlist = serde_json::from_str(&text)
            .map_err(|e| OuroError::Validation(format!("allowlist is malformed: {e}")))?;
        if v.signature.trim().is_empty() {
            return Err(OuroError::Validation(
                "embedded allowlist has no signature field".to_string(),
            ));
        }
        Ok(v)
    }

    /// Resolve the layout contract for a running image identified by its config digest + platform.
    /// Denylist wins; an unknown/absent digest is refused (the whole point — no tag trust).
    pub fn contract_for(
        &self,
        image_config_digest: &str,
        platform: &str,
    ) -> Result<&LayoutContract> {
        if self.denylist.iter().any(|d| d == image_config_digest) {
            return Err(OuroError::Validation(format!(
                "image {image_config_digest} is on the emergency denylist — refused"
            )));
        }
        self.contracts
            .iter()
            .find(|c| {
                c.allowed.iter().any(|a| {
                    a.image_config_digest == image_config_digest && a.platform == platform
                })
            })
            .ok_or_else(|| {
                OuroError::Validation(format!(
                    "image {image_config_digest} ({platform}) is not on the allowlist — this node \
                     does not conform to a supported convention (S0019 §2.1); refused"
                ))
            })
    }

    /// Control↔target allowlist-version skew is a refuse (§2.1): the two sides must agree on the
    /// convention before any op, else a stale side could allow/deny differently.
    pub fn require_no_skew(control: u32, target: u32) -> Result<()> {
        if control != target {
            return Err(OuroError::Validation(format!(
                "allowlist version skew: control has v{control}, target has v{target} — re-run \
                 init/adopt to align before operating (S0019 §2.1)"
            )));
        }
        Ok(())
    }
}

/// Anti-rollback floor for the allowlist, MAC-protected under the local home (mirrors
/// `version.rs`). Erasing the state falls back to the EMBEDDED version, never to "no floor".
fn floor_path(home: &Path) -> std::path::PathBuf {
    home.join("allowlist-floor.json")
}

/// Refuse an allowlist older than the recorded floor; on accept, ratchet the floor forward.
/// Returns whether the floor was reset (state missing/tampered → re-established from embedded).
pub fn enforce_anti_rollback(home: &Path, incoming_version: u32) -> Result<bool> {
    let embedded = Allowlist::embedded()?.allowlist_version;
    let path = floor_path(home);
    let (recorded, reset) = match read_floor(&path) {
        Some(v) => (v, false),
        None => (embedded, true), // missing/tampered → embedded floor, auditable reset
    };
    let floor = recorded.max(embedded);
    if incoming_version < floor {
        return Err(OuroError::Validation(format!(
            "allowlist rollback refused: incoming v{incoming_version} < floor v{floor} (S0019 §2.1)"
        )));
    }
    write_floor(&path, incoming_version.max(floor))?;
    Ok(reset)
}

fn read_floor(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let ver = v.get("allowlist_version")?.as_u64()? as u32;
    let mac = v.get("mac")?.as_str()?;
    if mac == floor_mac(ver) {
        Some(ver)
    } else {
        None // tampered → treated as missing (fail forward to embedded floor)
    }
}

fn write_floor(path: &Path, version: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let body = serde_json::json!({ "allowlist_version": version, "mac": floor_mac(version) });
    std::fs::write(path, serde_json::to_string(&body).unwrap())
        .map_err(|e| OuroError::Validation(format!("cannot write allowlist floor: {e}")))
}

/// Local MAC over the floor value (tamper-evidence only; not a network secret). Keyed by a
/// per-home secret file so a hand-edited floor is detected and falls back to embedded.
fn floor_mac(version: u32) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(b"ouro-allowlist-floor-v1").expect("hmac key");
    mac.update(version.to_le_bytes().as_slice());
    hex(&mac.finalize().into_bytes())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_allowlist_parses_and_is_signed() {
        let a = Allowlist::embedded().expect("embedded allowlist parses");
        assert!(!a.signature.is_empty());
        assert!(!a.contracts.is_empty());
        // The blinklabs baseline contract is present with the standard layout.
        let c = &a.contracts[0];
        assert_eq!(c.in_container_paths.socket, "/ipc/node.socket");
        assert!(c.role_rules.relay.forbids_forging_keys, "relay must forbid forging keys");
        assert!(c.role_rules.bp.requires_opcert, "bp must require opcert");
    }

    #[test]
    fn allowed_digest_resolves_denylist_and_unknown_refuse() {
        let a = Allowlist::embedded().unwrap();
        let good = &a.contracts[0].allowed[0].image_config_digest.clone();
        let platform = &a.contracts[0].allowed[0].platform.clone();
        assert!(a.contract_for(good, platform).is_ok(), "allowlisted digest conforms");
        // Unknown digest → refuse (no tag trust).
        assert!(a.contract_for("sha256:deadbeef", platform).is_err());
        // Wrong platform → refuse.
        assert!(a.contract_for(good, "linux/arm64").is_err());

        // Denylist wins over allow.
        let mut d = a.clone();
        d.denylist.push(good.clone());
        assert!(d.contract_for(good, platform).is_err(), "denylist overrides allow");
    }

    #[test]
    fn skew_and_anti_rollback() {
        assert!(Allowlist::require_no_skew(1, 1).is_ok());
        assert!(Allowlist::require_no_skew(1, 2).is_err());

        let dir = std::env::temp_dir().join(format!("ouro-allowlist-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let embedded = Allowlist::embedded().unwrap().allowlist_version;
        // First run: no floor → reset from embedded, accepts embedded version.
        let reset = enforce_anti_rollback(&dir, embedded).unwrap();
        assert!(reset, "first run establishes the floor (reset=true)");
        // A higher version ratchets forward.
        assert!(enforce_anti_rollback(&dir, embedded + 5).is_ok());
        // A lower version than the ratcheted floor is refused.
        assert!(enforce_anti_rollback(&dir, embedded).is_err(), "rollback below floor refused");
        std::fs::remove_dir_all(&dir).ok();
    }
}
