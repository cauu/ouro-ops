//! S0019 p1-1 (§2.1) — the layout contract + SIGNED digest allowlist.
//!
//! The convention that makes the environment finite: a versioned table of layout contracts, each
//! naming the fixed in-container paths, role rules, and an allowlist of IMMUTABLE OCI digests
//! (never a tag). A node conforms only if its running image's config digest is `allowed` (and not
//! `denylisted`) for a platform; the returned contract is the ONLY layout source the skills read.
//!
//! Trust: every embedded or external payload is Ed25519-verified against the pinned Ouro release
//! key before it is parsed into an executable contract. A debug-only HMAC seam exists for the
//! container bed; release builds cannot enable it. Anti-rollback is enforced on every adopt/op
//! load. Once a node is managed, a missing or tampered floor fails closed instead of silently
//! falling back to an older embedded version.

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path};

use crate::{OuroError, Result};

/// The embedded, signed allowlist payload (release infra verifies the signature; see module docs).
const EMBEDDED_ALLOWLIST: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/allowlist.json"));

/// Raw 32-byte Ed25519 public key used to sign `data/allowlist.json`. The private release key is
/// intentionally not present in the repository or binary.
const RELEASE_VERIFY_KEY_HEX: &str =
    "49b8291148d4ec505aaf7cf36ad359f4463fef85f4b2e72a353551bd29eed51f";

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
    /// Signed, explicit N→N+1 runtime transitions. Merely allowlisting two images does not prove DB
    /// compatibility in either direction.
    #[serde(default)]
    pub transitions: Vec<crate::upgrade::TransitionMeta>,
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
    pub platform_manifest_digest: String,
    pub image_config_digest: String,
}

impl Allowlist {
    /// Parse and cryptographically verify the binary's embedded allowlist.
    pub fn embedded() -> Result<Self> {
        parse_verified(EMBEDDED_ALLOWLIST)
    }

    /// Load the selected payload (embedded by default, or an externally delivered signed feed).
    /// `OURO_ALLOWLIST_FILE` is not a trust bypass: the same pinned signature is required.
    pub fn active_verified() -> Result<Self> {
        let text = match std::env::var_os("OURO_ALLOWLIST_FILE") {
            Some(p) => std::fs::read_to_string(&p).map_err(|e| {
                OuroError::Validation(format!("cannot read OURO_ALLOWLIST_FILE {p:?}: {e}"))
            })?,
            None => EMBEDDED_ALLOWLIST.to_string(),
        };
        parse_verified(&text)
    }

    /// Production load: signature + semantic validation followed by monotonic anti-rollback.
    /// `bootstrap_floor` may be true only before the first attestation is created.
    pub fn load(home: &Path, bootstrap_floor: bool) -> Result<Self> {
        let allowlist = Self::active_verified()?;
        enforce_anti_rollback(home, allowlist.allowlist_version, bootstrap_floor)?;
        Ok(allowlist)
    }

    /// Stable control↔target identity of the complete signed document.
    pub fn signed_digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self)
            .map_err(|e| OuroError::Validation(format!("cannot canonicalize allowlist: {e}")))?;
        Ok(format!("sha256:{}", hex(Sha256::digest(bytes).as_slice())))
    }

    /// Resolve the layout contract for a running image identified by its config digest + platform.
    /// Denylist wins; an unknown/absent digest is refused (the whole point — no tag trust).
    pub fn contract_for(
        &self,
        image_config_digest: &str,
        platform: &str,
    ) -> Result<&LayoutContract> {
        self.contract_and_image_for(image_config_digest, platform)
            .map(|(contract, _)| contract)
    }

    /// Resolve both the executable layout contract and the exact signed OCI identity tuple.
    pub fn contract_and_image_for(
        &self,
        image_config_digest: &str,
        platform: &str,
    ) -> Result<(&LayoutContract, &AllowedImage)> {
        if self.denylist.iter().any(|d| d == image_config_digest) {
            return Err(OuroError::Validation(format!(
                "image {image_config_digest} is on the emergency denylist — refused"
            )));
        }
        self.contracts.iter().find_map(|contract| {
            contract.allowed.iter().find(|image| {
                image.image_config_digest == image_config_digest && image.platform == platform
            }).map(|image| (contract, image))
        })
            .ok_or_else(|| {
                OuroError::Validation(format!(
                    "image {image_config_digest} ({platform}) is not on the allowlist — this node \
                     does not conform to a supported convention (S0019 §2.1); refused"
                ))
            })
    }

    pub fn transition_for(
        &self,
        from_image_config_digest: &str,
        to_image_config_digest: &str,
    ) -> Result<&crate::upgrade::TransitionMeta> {
        self.transitions
            .iter()
            .find(|transition| {
                transition.from_image_config_digest == from_image_config_digest
                    && transition.to_image_config_digest == to_image_config_digest
            })
            .ok_or_else(|| {
                OuroError::Validation(format!(
                    "no signed N→N+1 transition metadata for {from_image_config_digest} → \
                     {to_image_config_digest}; allowlisting images alone is insufficient"
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

fn parse_verified(text: &str) -> Result<Allowlist> {
    let mut document: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| OuroError::Validation(format!("allowlist is malformed: {e}")))?;
    let signature = document
        .as_object_mut()
        .and_then(|object| object.remove("signature"))
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| OuroError::Validation("allowlist has no signature".into()))?;
    let canonical = serde_json::to_vec(&document)
        .map_err(|e| OuroError::Validation(format!("cannot canonicalize allowlist: {e}")))?;
    verify_signature(&signature, &canonical)?;

    let allowlist: Allowlist = serde_json::from_str(text)
        .map_err(|e| OuroError::Validation(format!("allowlist is malformed: {e}")))?;
    allowlist.validate_usability()?;
    Ok(allowlist)
}

fn verify_signature(signature: &str, canonical: &[u8]) -> Result<()> {
    if let Some(encoded) = signature.strip_prefix("ed25519:") {
        let key: [u8; 32] = decode_hex(RELEASE_VERIFY_KEY_HEX)?
            .try_into()
            .map_err(|_| OuroError::Validation("pinned allowlist key has wrong length".into()))?;
        let signature: [u8; 64] = decode_hex(encoded)?
            .try_into()
            .map_err(|_| OuroError::Validation("allowlist signature has wrong length".into()))?;
        return VerifyingKey::from_bytes(&key)
            .map_err(|_| OuroError::Validation("pinned allowlist key is malformed".into()))?
            .verify_strict(canonical, &Signature::from_bytes(&signature))
            .map_err(|_| OuroError::Validation(
                "allowlist Ed25519 signature is invalid — refused before contract use".into(),
            ));
    }

    // A dynamic container image has no release private key. Debug/test builds may opt into a
    // separate HMAC trust root; this code is compile-time absent from production release behavior.
    #[cfg(debug_assertions)]
    if let Some(encoded) = signature.strip_prefix("test-hmac-sha256:") {
        use hmac::{Hmac, Mac};
        let secret = std::env::var("OURO_ALLOWLIST_TEST_KEY").map_err(|_| {
            OuroError::Validation("test allowlist signature needs OURO_ALLOWLIST_TEST_KEY".into())
        })?;
        let expected = decode_hex(encoded)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|_| OuroError::Validation("invalid test allowlist key".into()))?;
        mac.update(canonical);
        return mac.verify_slice(&expected).map_err(|_| {
            OuroError::Validation("test allowlist HMAC is invalid — refused".into())
        });
    }

    Err(OuroError::Validation(
        "allowlist signature scheme is unsupported (release requires ed25519)".into(),
    ))
}

impl Allowlist {
    fn validate_usability(&self) -> Result<()> {
        if self.allowlist_version == 0 || self.contracts.is_empty() {
            return Err(OuroError::Validation(
                "allowlist must have a nonzero version and at least one contract".into(),
            ));
        }
        let mut ids = HashSet::new();
        let mut versions = HashSet::new();
        let mut images: HashMap<&str, u32> = HashMap::new();
        for contract in &self.contracts {
            if contract.contract_id.is_empty()
                || !ids.insert(contract.contract_id.as_str())
                || contract.convention_version == 0
                || !versions.insert(contract.convention_version)
                || contract.allowed.is_empty()
            {
                return Err(OuroError::Validation(
                    "allowlist contracts need unique nonempty ids/versions and allowed images".into(),
                ));
            }
            for path in [
                &contract.in_container_paths.socket,
                &contract.in_container_paths.db,
                &contract.in_container_paths.keys,
                &contract.in_container_paths.config,
                &contract.in_container_paths.topology,
                &contract.in_container_paths.genesis,
            ] {
                if !safe_absolute(path) {
                    return Err(OuroError::Validation(format!(
                        "allowlist contract path {path:?} is not a safe absolute path"
                    )));
                }
            }
            if !contract.role_rules.bp.requires_opcert
                || !contract.role_rules.relay.forbids_forging_keys
            {
                return Err(OuroError::Validation(
                    "allowlist weakens mandatory BP/relay role rules".into(),
                ));
            }
            for image in &contract.allowed {
                if !matches!(image.platform.as_str(), "linux/amd64" | "linux/arm64")
                    || !valid_digest(&image.oci_index_digest)
                    || !valid_digest(&image.platform_manifest_digest)
                    || !valid_digest(&image.image_config_digest)
                    || images.insert(&image.image_config_digest, contract.convention_version).is_some()
                {
                    return Err(OuroError::Validation(
                        "allowlist image identities must be unique lowercase sha256 OCI tuples on a supported platform"
                            .into(),
                    ));
                }
            }
        }
        if self.denylist.iter().any(|digest| !valid_digest(digest)) {
            return Err(OuroError::Validation("allowlist denylist has a malformed digest".into()));
        }
        for transition in &self.transitions {
            if images.get(transition.from_image_config_digest.as_str())
                != Some(&transition.from_convention_version)
                || images.get(transition.to_image_config_digest.as_str())
                    != Some(&transition.to_convention_version)
            {
                return Err(OuroError::Validation(
                    "allowlist transition does not reference the declared contract versions".into(),
                ));
            }
        }
        Ok(())
    }
}

fn safe_absolute(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        && path.components().all(|component| {
            !matches!(component, Component::ParentDir | Component::CurDir | Component::Prefix(_))
        })
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").map(|digest| {
        digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) == Some(true)
}

/// Anti-rollback floor for the allowlist, MAC-protected under the local home (mirrors
/// `version.rs`). Erasing the state falls back to the EMBEDDED version, never to "no floor".
fn floor_path(home: &Path) -> std::path::PathBuf {
    home.join("allowlist-floor.json")
}

/// Refuse an allowlist older than the recorded floor; on accept, ratchet the floor forward.
/// Returns whether the floor was reset (state missing/tampered → re-established from embedded).
pub fn enforce_anti_rollback(
    home: &Path,
    incoming_version: u32,
    bootstrap_floor: bool,
) -> Result<bool> {
    let embedded = Allowlist::embedded()?.allowlist_version;
    let path = floor_path(home);
    let secret = crate::confirm::load_or_create_secret(&home.join("tool-run.secret"))?;
    let (recorded, reset) = match read_floor(&path, &secret)? {
        Some(v) => (v, false),
        None if bootstrap_floor => (embedded, true),
        None => {
            return Err(OuroError::Validation(
                "allowlist anti-rollback floor is missing on an adopted node — fail-closed; \
                 operator recovery/re-adoption is required"
                    .into(),
            ))
        }
    };
    let floor = recorded.max(embedded);
    if incoming_version < floor {
        return Err(OuroError::Validation(format!(
            "allowlist rollback refused: incoming v{incoming_version} < floor v{floor} (S0019 §2.1)"
        )));
    }
    write_floor(&path, incoming_version.max(floor), &secret)?;
    Ok(reset)
}

fn read_floor(path: &Path, secret: &str) -> Result<Option<u32>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
        OuroError::Validation("allowlist anti-rollback floor is malformed — fail-closed".into())
    })?;
    let ver = v.get("allowlist_version").and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| OuroError::Validation("allowlist floor version is malformed".into()))?;
    let mac = v.get("mac").and_then(|value| value.as_str())
        .ok_or_else(|| OuroError::Validation("allowlist floor MAC is missing".into()))?;
    if mac != floor_mac(secret, ver) {
        return Err(OuroError::Validation(
            "allowlist anti-rollback floor MAC is invalid — fail-closed".into(),
        ));
    }
    Ok(Some(ver))
}

fn write_floor(path: &Path, version: u32, secret: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec(&serde_json::json!({
        "allowlist_version": version,
        "mac": floor_mac(secret, version),
    })).expect("floor serializes");
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)
        .map_err(|e| OuroError::Validation(format!("cannot create allowlist floor: {e}")))?;
    file.write_all(&body)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// Local MAC over the floor value (tamper-evidence only; not a network secret). Keyed by a
/// per-home secret file so a hand-edited floor is detected and falls back to embedded.
fn floor_mac(secret: &str, version: u32) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(b"ouro-allowlist-floor-v2\0");
    mac.update(version.to_le_bytes().as_slice());
    hex(&mac.finalize().into_bytes())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0
        || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OuroError::Validation("hex value is malformed".into()));
    }
    (0..value.len()).step_by(2).map(|index| {
        u8::from_str_radix(&value[index..index + 2], 16)
            .map_err(|_| OuroError::Validation("hex value is malformed".into()))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_allowlist_parses_and_is_signed() {
        let a = Allowlist::embedded().expect("embedded allowlist parses");
        assert!(a.signature.starts_with("ed25519:"));
        assert!(!a.contracts.is_empty());
        // The blinklabs baseline contract is present with the standard layout.
        let c = &a.contracts[0];
        assert_eq!(c.in_container_paths.socket, "/ipc/node.socket");
        assert!(c.role_rules.relay.forbids_forging_keys, "relay must forbid forging keys");
        assert!(c.role_rules.bp.requires_opcert, "bp must require opcert");
        assert!(valid_digest(&c.allowed[0].oci_index_digest));
        assert!(valid_digest(&c.allowed[0].platform_manifest_digest));
        assert!(valid_digest(&c.allowed[0].image_config_digest));
    }

    #[test]
    fn signature_tamper_and_placeholder_contract_refuse() {
        let tampered = EMBEDDED_ALLOWLIST.replace("/ipc/node.socket", "/ipc/evil.socket");
        assert!(parse_verified(&tampered).is_err(), "signed payload tamper refused");

        let mut unsigned: serde_json::Value = serde_json::from_str(EMBEDDED_ALLOWLIST).unwrap();
        unsigned["signature"] = serde_json::Value::String("EMBEDDED-TRUSTED".into());
        assert!(parse_verified(&serde_json::to_string(&unsigned).unwrap()).is_err());
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
        let reset = enforce_anti_rollback(&dir, embedded, true).unwrap();
        assert!(reset, "first run establishes the floor (reset=true)");
        // A higher version ratchets forward.
        assert!(enforce_anti_rollback(&dir, embedded + 5, false).is_ok());
        // A lower version than the ratcheted floor is refused.
        assert!(enforce_anti_rollback(&dir, embedded, false).is_err(), "rollback below floor refused");
        // Erasing or corrupting the floor on a managed node fails closed instead of reopening v1.
        std::fs::remove_file(floor_path(&dir)).unwrap();
        assert!(enforce_anti_rollback(&dir, embedded, false).is_err());
        std::fs::write(floor_path(&dir), br#"{"allowlist_version":99,"mac":"forged"}"#).unwrap();
        assert!(enforce_anti_rollback(&dir, embedded + 100, false).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
