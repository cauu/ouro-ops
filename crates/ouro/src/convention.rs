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
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path};
use std::process::Command;

use crate::{OuroError, Result};

/// The embedded, signed allowlist payload (release infra verifies the signature; see module docs).
const EMBEDDED_ALLOWLIST: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/allowlist.json"));

/// Raw 32-byte Ed25519 public key used to sign release-policy documents. The private release key
/// is intentionally not present in the repository or binary.
const RELEASE_VERIFY_KEY_HEX: &str =
    "3ceb1920f30d3768a7b979c563b4e1738dc7708e8ed6e91d6e32bd7a0df165dd";

/// One no-cache release source. HTTPS authenticates transport; the pinned Ed25519 key authenticates
/// the document. The branch URL becomes live when a reviewed release catalog lands on `main`.
pub const RELEASES_URL: &str =
    "https://raw.githubusercontent.com/cauu/ouro-ops/refs/heads/main/data/releases.json";
pub const BLINKLABS_REPOSITORY: &str = "ghcr.io/blinklabs-io/cardano-node";
const MAX_RELEASE_DOCUMENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Allowlist {
    pub allowlist_version: u32,
    /// Signature over the payload; verified against the pinned release key by S0018 infra. The
    /// literal `EMBEDDED-TRUSTED` marks the pre-infra state (embedded == trusted, never weaker).
    pub signature: String,
    /// One signed upstream image authority for every OCI tuple in a release catalog. The frozen
    /// embedded layout fixture predates remote pulls and therefore leaves this empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository: String,
    /// One current deployment recommendation per platform. Absent only in the frozen embedded
    /// layout fixture; a fetched release catalog must contain at least one entry.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub recommended: BTreeMap<String, String>,
    pub contracts: Vec<LayoutContract>,
    /// Emergency revocation: image_config digests refused even if a stale `allowed` still lists
    /// them. A denylist entry ALWAYS wins over an allow.
    #[serde(default)]
    pub denylist: Vec<String>,
    /// Optional signed runtime transitions. Upgrade admission targets `recommended`; an exact
    /// transition only describes whether automatic rollback to the source runtime is safe.
    #[serde(default)]
    pub transitions: Vec<crate::upgrade::TransitionMeta>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutContract {
    pub convention_version: u32,
    pub contract_id: String,
    pub in_container_paths: InContainerPaths,
    pub role_rules: RoleRules,
    /// Signed facts required only for a fresh S0027 Fleet Deploy. Historical runtime/Upgrade
    /// contracts may omit them; Deploy selection fails closed when they are absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy: Option<DeployBootstrapContract>,
    pub allowed: Vec<AllowedImage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeployBootstrapContract {
    pub required_binaries: Vec<String>,
    pub entrypoint: String,
    pub args: Vec<String>,
    pub database_marker: String,
    pub metrics: DeployMetricsContract,
    pub environment: DeployEnvironmentContract,
    pub networks: BTreeMap<String, DeployNetworkContract>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeployMetricsContract {
    pub container_port: u16,
    pub host_ip: String,
    pub host_port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeployEnvironmentContract {
    pub network: String,
    pub topology: String,
    pub database: String,
    pub socket: String,
    pub block_producer: String,
    pub restore_snapshot: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeployNetworkContract {
    pub config: String,
    pub config_sha256: String,
    pub topology: String,
    pub genesis: String,
    pub genesis_hash: String,
    pub genesis_vkey: String,
    pub ancillary_vkey: String,
    pub mithril_aggregator: String,
    pub min_memory_bytes: u64,
    pub min_free_disk_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InContainerPaths {
    pub socket: String,
    pub db: String,
    pub keys: String,
    pub config: String,
    pub topology: String,
    pub genesis: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleRules {
    pub bp: RoleRule,
    pub relay: RoleRule,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleRule {
    pub requires_opcert: bool,
    pub forbids_forging_keys: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedImage {
    /// Human release label for operator display. Image authority remains the immutable OCI tuple.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub release: String,
    pub platform: String,
    pub oci_index_digest: String,
    pub platform_manifest_digest: String,
    pub image_config_digest: String,
}

#[derive(Debug, Clone)]
pub struct VerifiedReleaseCatalog {
    pub policy: Allowlist,
    /// Compact public signed document transported to the ephemeral Upgrade runner.
    pub document: String,
    pub source: String,
}

impl Allowlist {
    /// Parse and cryptographically verify the binary's embedded allowlist.
    pub fn embedded() -> Result<Self> {
        parse_verified(EMBEDDED_ALLOWLIST)
    }

    /// Stable execution convention for ordinary non-Upgrade operations. Image release admission is
    /// deliberately not part of this lookup: supervisor, paths and role rules are the contract.
    pub fn stable_contract() -> Result<LayoutContract> {
        Self::embedded()?
            .contracts
            .into_iter()
            .find(|contract| contract.contract_id == "blinklabs-cardano-node-v1")
            .ok_or_else(|| {
                OuroError::Validation("embedded stable layout contract is missing".into())
            })
    }

    /// Verify one externally supplied release document. Used by the target after the control has
    /// transported the same public bytes; neither side trusts transport alone.
    pub fn release_document(text: &str) -> Result<Self> {
        if text.len() > MAX_RELEASE_DOCUMENT_BYTES {
            return Err(OuroError::Validation(
                "release document exceeds the 64 KiB bound".into(),
            ));
        }
        let policy = parse_verified(text)?;
        policy.validate_release_catalog()?;
        Ok(policy)
    }

    pub fn recommended_for(&self, platform: &str) -> Result<&AllowedImage> {
        let digest = self.recommended.get(platform).ok_or_else(|| {
            OuroError::Validation(format!(
                "signed release catalog has no deployment recommendation for {platform}"
            ))
        })?;
        self.contract_and_image_for(digest, platform)
            .map(|(_, image)| image)
    }

    /// Resolve the exact signed deploy tuple together with its image/bootstrap facts. Upgrade
    /// catalogs and historical embedded contracts intentionally need not carry this extension.
    pub fn recommended_deploy_for(
        &self,
        platform: &str,
    ) -> Result<(&LayoutContract, &AllowedImage, &DeployBootstrapContract)> {
        let image = self.recommended_for(platform)?;
        let (contract, _) = self.contract_and_image_for(&image.image_config_digest, platform)?;
        let deploy = contract.deploy.as_ref().ok_or_else(|| {
            OuroError::Validation(format!(
                "signed recommended image {} ({platform}) has no Fleet Deploy bootstrap contract",
                image.image_config_digest
            ))
        })?;
        Ok((contract, image, deploy))
    }

    pub fn recommended_upgrade_for(
        &self,
        current: &str,
        platform: &str,
    ) -> Result<(&AllowedImage, Option<&crate::upgrade::TransitionMeta>)> {
        self.contract_and_image_for(current, platform)?;
        let recommended = self.recommended_for(platform)?;
        if recommended.image_config_digest == current {
            return Err(OuroError::Validation(format!(
                "image {current} is already the signed recommended release for {platform}"
            )));
        }
        Ok((
            recommended,
            self.transition_for_optional(current, &recommended.image_config_digest),
        ))
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
        self.contracts
            .iter()
            .find_map(|contract| {
                contract
                    .allowed
                    .iter()
                    .find(|image| {
                        image.image_config_digest == image_config_digest
                            && image.platform == platform
                    })
                    .map(|image| (contract, image))
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
                    "no exact signed rollback metadata for {from_image_config_digest} → \
                     {to_image_config_digest}"
                ))
            })
    }

    pub fn transition_for_optional(
        &self,
        from_image_config_digest: &str,
        to_image_config_digest: &str,
    ) -> Option<&crate::upgrade::TransitionMeta> {
        self.transitions.iter().find(|transition| {
            transition.from_image_config_digest == from_image_config_digest
                && transition.to_image_config_digest == to_image_config_digest
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
    let (signature, canonical) = unsigned_payload(text)?;
    verify_signature(&signature, &canonical)?;

    let allowlist: Allowlist = serde_json::from_str(text)
        .map_err(|e| OuroError::Validation(format!("allowlist is malformed: {e}")))?;
    allowlist.validate_usability()?;
    Ok(allowlist)
}

/// Prepare one strict, semantically usable release candidate and return the exact bytes production
/// verification signs. This is intentionally shared with the macOS-only release signer so no
/// second language can invent a subtly different JSON canonicalization.
pub fn release_candidate(text: &str) -> Result<(Allowlist, Vec<u8>)> {
    let (_, canonical) = unsigned_payload(text)?;
    let allowlist: Allowlist = serde_json::from_str(text)
        .map_err(|e| OuroError::Validation(format!("allowlist is malformed: {e}")))?;
    allowlist.validate_usability()?;
    if !allowlist.recommended.is_empty() {
        allowlist.validate_release_catalog()?;
    }
    Ok((allowlist, canonical))
}

fn unsigned_payload(text: &str) -> Result<(String, Vec<u8>)> {
    let mut document: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| OuroError::Validation(format!("allowlist is malformed: {e}")))?;
    let signature = document
        .as_object_mut()
        .and_then(|object| object.remove("signature"))
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| OuroError::Validation("allowlist has no signature".into()))?;
    let canonical = serde_json::to_vec(&document)
        .map_err(|e| OuroError::Validation(format!("cannot canonicalize allowlist: {e}")))?;
    Ok((signature, canonical))
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
            .map_err(|_| {
                OuroError::Validation(
                    "allowlist Ed25519 signature is invalid — refused before contract use".into(),
                )
            });
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
        return mac
            .verify_slice(&expected)
            .map_err(|_| OuroError::Validation("test allowlist HMAC is invalid — refused".into()));
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
                    "allowlist contracts need unique nonempty ids/versions and allowed images"
                        .into(),
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
            if let Some(deploy) = &contract.deploy {
                deploy.validate()?;
            }
            for image in &contract.allowed {
                if !matches!(image.platform.as_str(), "linux/amd64" | "linux/arm64")
                    || !valid_digest(&image.oci_index_digest)
                    || !valid_digest(&image.platform_manifest_digest)
                    || !valid_digest(&image.image_config_digest)
                    || images
                        .insert(&image.image_config_digest, contract.convention_version)
                        .is_some()
                {
                    return Err(OuroError::Validation(
                        "allowlist image identities must be unique lowercase sha256 OCI tuples on a supported platform"
                            .into(),
                    ));
                }
            }
        }
        if self.denylist.iter().any(|digest| !valid_digest(digest)) {
            return Err(OuroError::Validation(
                "allowlist denylist has a malformed digest".into(),
            ));
        }
        let mut edges = HashSet::new();
        for transition in &self.transitions {
            if transition.from_image_config_digest == transition.to_image_config_digest
                || !images.contains_key(transition.from_image_config_digest.as_str())
                || !images.contains_key(transition.to_image_config_digest.as_str())
                || !edges.insert((
                    transition.from_image_config_digest.as_str(),
                    transition.to_image_config_digest.as_str(),
                ))
            {
                return Err(OuroError::Validation(
                    "allowlist transitions must be unique directed edges between distinct declared image config digests"
                        .into(),
                ));
            }
        }
        Ok(())
    }

    fn validate_release_catalog(&self) -> Result<()> {
        if self.repository != BLINKLABS_REPOSITORY {
            return Err(OuroError::Validation(format!(
                "signed release catalog repository must be exactly {BLINKLABS_REPOSITORY}"
            )));
        }
        if self.recommended.is_empty() {
            return Err(OuroError::Validation(
                "signed release catalog has no deployment recommendation".into(),
            ));
        }
        for (platform, digest) in &self.recommended {
            if self
                .contract_and_image_for(digest, platform)?
                .1
                .release
                .is_empty()
            {
                return Err(OuroError::Validation(format!(
                    "recommended release {digest} ({platform}) has no release label"
                )));
            }
        }
        if self
            .contracts
            .iter()
            .flat_map(|contract| &contract.allowed)
            .any(|image| image.release.is_empty())
        {
            return Err(OuroError::Validation(
                "signed release catalog image is missing its release label".into(),
            ));
        }
        Ok(())
    }
}

impl DeployBootstrapContract {
    fn validate(&self) -> Result<()> {
        let expected_binaries = [
            "cardano-cli",
            "cardano-node",
            "mithril-client",
            "nview",
            "txtop",
        ];
        let mut binaries = self
            .required_binaries
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        binaries.sort_unstable();
        binaries.dedup();
        if binaries != expected_binaries
            || self.entrypoint != "/usr/local/bin/entrypoint"
            || self.args != ["run"]
            || self.database_marker != "/data/db/protocolMagicId"
            || self.metrics.container_port != 12798
            || self.metrics.host_ip != "127.0.0.1"
            || self.metrics.host_port != 12798
        {
            return Err(OuroError::Validation(
                "Fleet Deploy bootstrap contract weakens the fixed image/runtime/metrics shape"
                    .into(),
            ));
        }
        let environment = &self.environment;
        if environment.network != "CARDANO_NETWORK"
            || environment.topology != "CARDANO_TOPOLOGY"
            || environment.database != "CARDANO_DATABASE_PATH"
            || environment.socket != "CARDANO_SOCKET_PATH"
            || environment.block_producer != "CARDANO_BLOCK_PRODUCER"
            || environment.restore_snapshot != "RESTORE_SNAPSHOT"
        {
            return Err(OuroError::Validation(
                "Fleet Deploy bootstrap contract has unexpected environment selectors".into(),
            ));
        }
        let expected_networks = ["mainnet", "preprod", "preview"];
        if self.networks.keys().map(String::as_str).collect::<Vec<_>>() != expected_networks {
            return Err(OuroError::Validation(
                "Fleet Deploy bootstrap contract must define mainnet, preprod and preview".into(),
            ));
        }
        for (network, facts) in &self.networks {
            for path in [
                &facts.config,
                &facts.topology,
                &facts.genesis,
                &facts.genesis_vkey,
                &facts.ancillary_vkey,
            ] {
                if !safe_absolute(path) || !path.contains(&format!("/{network}/")) {
                    return Err(OuroError::Validation(format!(
                        "Fleet Deploy {network} path {path:?} is outside its fixed image network directory"
                    )));
                }
            }
            if !valid_hex64(&facts.config_sha256)
                || !valid_hex64(&facts.genesis_hash)
                || !facts.mithril_aggregator.starts_with("https://aggregator.")
                || !facts
                    .mithril_aggregator
                    .ends_with(".api.mithril.network/aggregator")
                || facts.min_memory_bytes == 0
                || facts.min_free_disk_bytes == 0
            {
                return Err(OuroError::Validation(format!(
                    "Fleet Deploy {network} bootstrap facts are incomplete or malformed"
                )));
            }
        }
        Ok(())
    }
}

/// Fetch and verify the current release catalog without caching it. `OURO_RELEASES_FILE` is a
/// deterministic signed-file seam for tests; it does not bypass schema or signature verification.
pub fn fetch_release_catalog() -> Result<VerifiedReleaseCatalog> {
    let test_file = if cfg!(debug_assertions) {
        std::env::var_os("OURO_RELEASES_FILE")
    } else {
        None
    };
    let (text, source) = if let Some(path) = test_file {
        let text = std::fs::read_to_string(&path).map_err(|error| {
            OuroError::Validation(format!("cannot read OURO_RELEASES_FILE {path:?}: {error}"))
        })?;
        (text, format!("file:{}", Path::new(&path).display()))
    } else {
        let output = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--max-time",
                "15",
                "--max-filesize",
                "65536",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                RELEASES_URL,
            ])
            .output()
            .map_err(|error| {
                OuroError::Validation(format!(
                    "cannot execute curl for signed release catalog: {error}"
                ))
            })?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(OuroError::Validation(format!(
                "cannot fetch current signed release catalog from {RELEASES_URL}: {}",
                detail.trim()
            )));
        }
        let text = String::from_utf8(output.stdout).map_err(|_| {
            OuroError::Validation("signed release catalog is not UTF-8 JSON".into())
        })?;
        (text, RELEASES_URL.to_string())
    };
    let policy = Allowlist::release_document(&text)?;
    let document = serde_json::to_string(&policy).map_err(|error| {
        OuroError::Validation(format!("cannot compact release catalog: {error}"))
    })?;
    Ok(VerifiedReleaseCatalog {
        policy,
        document,
        source,
    })
}

fn safe_absolute(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        && path.components().all(|component| {
            !matches!(
                component,
                Component::ParentDir | Component::CurDir | Component::Prefix(_)
            )
        })
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").map(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) == Some(true)
}

fn valid_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    let next = incoming_version.max(floor);
    // A steady-state plan/read must not rewrite anti-rollback metadata merely for checking it.
    // Ratchet only when the floor is first established or actually advances.
    if reset || next > recorded {
        write_floor(&path, next, &secret)?;
    }
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
    let ver = v
        .get("allowlist_version")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| OuroError::Validation("allowlist floor version is malformed".into()))?;
    let mac = v
        .get("mac")
        .and_then(|value| value.as_str())
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
    }))
    .expect("floor serializes");
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&tmp)
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
    if value.len() & 1 != 0
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OuroError::Validation("hex value is malformed".into()));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| OuroError::Validation("hex value is malformed".into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/releases.json"));

    #[test]
    fn signed_release_catalog_selects_deploy_and_recommended_upgrade() {
        let catalog = Allowlist::release_document(RELEASES).expect("signed catalog verifies");
        assert_eq!(catalog.repository, BLINKLABS_REPOSITORY);
        let deploy = catalog.recommended_for("linux/amd64").unwrap();
        assert_eq!(deploy.release, "11.0.1-1");
        let (_, selected, bootstrap) = catalog.recommended_deploy_for("linux/amd64").unwrap();
        assert_eq!(selected.image_config_digest, deploy.image_config_digest);
        assert_eq!(bootstrap.database_marker, "/data/db/protocolMagicId");
        assert_eq!(bootstrap.required_binaries.len(), 5);
        assert_eq!(
            bootstrap.networks["mainnet"].genesis_hash,
            "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81"
        );
        let (target, transition) = catalog
            .recommended_upgrade_for(
                "sha256:a3223d93539d28e4f54e0b20dfc644a55387d5522a3d85b3b981eacff23c0c7a",
                "linux/amd64",
            )
            .unwrap();
        assert_eq!(target.release, "11.0.1-1");
        assert!(transition.is_none(), "direct rollback metadata is optional");
        let (_, direct_transition) = catalog
            .recommended_upgrade_for(
                "sha256:5fe0bf791a0af8884386479555996bf4ad7621493889625a2886039bf8734e51",
                "linux/amd64",
            )
            .unwrap();
        assert!(direct_transition.is_some());
        assert!(catalog
            .recommended_upgrade_for(&deploy.image_config_digest, "linux/amd64")
            .is_err());

        let tampered = RELEASES.replace("10.6.4-1", "10.6.4-evil");
        assert!(Allowlist::release_document(&tampered).is_err());
    }

    #[test]
    fn embedded_allowlist_parses_and_is_signed() {
        let a = Allowlist::embedded().expect("embedded allowlist parses");
        assert!(a.signature.starts_with("ed25519:"));
        assert!(!a.contracts.is_empty());
        assert_eq!(a.allowlist_version, 3);
        assert_eq!(a.transitions.len(), 3, "three exact rollback declarations");
        // The blinklabs baseline contract is present with the standard layout.
        let c = &a.contracts[0];
        assert_eq!(c.in_container_paths.socket, "/ipc/node.socket");
        assert!(
            c.role_rules.relay.forbids_forging_keys,
            "relay must forbid forging keys"
        );
        assert!(c.role_rules.bp.requires_opcert, "bp must require opcert");
        assert!(valid_digest(&c.allowed[0].oci_index_digest));
        assert!(valid_digest(&c.allowed[0].platform_manifest_digest));
        assert!(valid_digest(&c.allowed[0].image_config_digest));
        let (_, live) = a
            .contract_and_image_for(
                "sha256:a3223d93539d28e4f54e0b20dfc644a55387d5522a3d85b3b981eacff23c0c7a",
                "linux/amd64",
            )
            .expect("10.5.4-1 live baseline is admitted");
        assert_eq!(
            live.oci_index_digest,
            "sha256:6de965784be4134deccb94ca8d92c11dfb3e140a9d0616210f29a1836fdb13d7"
        );
        assert_eq!(
            live.platform_manifest_digest,
            "sha256:e4f7b5e761b0c739ebb4bd40359415817bfd782fcd4f427de0e1fa3109295983"
        );

        let v105 = "sha256:a3223d93539d28e4f54e0b20dfc644a55387d5522a3d85b3b981eacff23c0c7a";
        let v106 = "sha256:0fb74b5921860a6547ce5b6c669d59b71169d1c48b014f2fafcec2e4d382f1b3";
        let v107 = "sha256:5fe0bf791a0af8884386479555996bf4ad7621493889625a2886039bf8734e51";
        let v110 = "sha256:0bb21e45159327c4e6109704df256c3c297c725a4b2cdf6d0e1899e3a9df468f";
        for (config, index, manifest) in [
            (
                v106,
                "sha256:29154a16decd311c92f60219dca1a6b212e874b0e665b250f0e8d4ce945e7de8",
                "sha256:8efeca0ecc75c2b574436fd8c7e4f5c3411261b2330082d2f43d863ce8472c65",
            ),
            (
                v107,
                "sha256:ca12bcb51dece451eeb418c625fc0c5fd7b10c4d7bb46a1ecaf6fa80f4798aab",
                "sha256:f4f5b2dfadb89c4c64c2c120918fdf50edcf2b7555089c166e0a09473655cd9b",
            ),
            (
                v110,
                "sha256:d5ede07a890e9b6a0a5182cdba9dbaf73756336762235e0934a11690beedae02",
                "sha256:337e621185510eb7ca9ecbd33e2083d538b85d11373a19bec6a64f6b4325cee7",
            ),
        ] {
            let (_, image) = a
                .contract_and_image_for(config, "linux/amd64")
                .expect("reviewed OCI tuple is admitted");
            assert_eq!(image.oci_index_digest, index);
            assert_eq!(image.platform_manifest_digest, manifest);
        }
        for digest in [v105, v106, v107, v110] {
            let contract = a
                .contract_for(digest, "linux/amd64")
                .expect("reviewed release is allowlisted");
            assert_eq!(
                contract.convention_version, 1,
                "node releases share one stable Docker layout contract"
            );
        }
        assert!(!a.transition_for(v105, v106).unwrap().db_backward_compatible);
        assert!(a.transition_for(v106, v107).unwrap().db_backward_compatible);
        assert!(a.transition_for(v107, v110).unwrap().db_backward_compatible);
        assert!(
            a.transition_for(v105, v110).is_err(),
            "no exact direct rollback declaration"
        );
        assert!(
            a.transition_for(v110, v107).is_err(),
            "reverse edge refused"
        );
    }

    #[test]
    fn signature_tamper_and_placeholder_contract_refuse() {
        let tampered = EMBEDDED_ALLOWLIST.replace("/ipc/node.socket", "/ipc/evil.socket");
        assert!(
            parse_verified(&tampered).is_err(),
            "signed payload tamper refused"
        );

        let mut unsigned: serde_json::Value = serde_json::from_str(EMBEDDED_ALLOWLIST).unwrap();
        unsigned["signature"] = serde_json::Value::String("EMBEDDED-TRUSTED".into());
        assert!(parse_verified(&serde_json::to_string(&unsigned).unwrap()).is_err());

        // Trust-root rotation is real: reconstructing the exact v1 document/signature must not
        // verify under the new pinned authority.
        let mut old: serde_json::Value = serde_json::from_str(EMBEDDED_ALLOWLIST).unwrap();
        old["allowlist_version"] = serde_json::json!(1);
        old["signature"] = serde_json::json!(
            "ed25519:338fb06966c7cda4094f6cc27b63003d8877791bf68cee1f11d2d19bdeb40e5c1f229ca3bbdb36d9b2f66e0bb1dbf64fe6b1cf457f35eebc8f558605209c0a08"
        );
        old.pointer_mut("/contracts/0/allowed")
            .and_then(serde_json::Value::as_array_mut)
            .unwrap()
            .retain(|image| {
                image["image_config_digest"]
                    != "sha256:a3223d93539d28e4f54e0b20dfc644a55387d5522a3d85b3b981eacff23c0c7a"
            });
        assert!(parse_verified(&serde_json::to_string(&old).unwrap()).is_err());
    }

    #[test]
    fn release_candidate_rejects_ambiguous_or_unknown_fields() {
        let duplicate = EMBEDDED_ALLOWLIST.replacen(
            "\"allowlist_version\": 3,",
            "\"allowlist_version\": 3,\n  \"allowlist_version\": 3,",
            1,
        );
        assert!(
            release_candidate(&duplicate).is_err(),
            "duplicate field refused"
        );

        let mut unknown: serde_json::Value = serde_json::from_str(EMBEDDED_ALLOWLIST).unwrap();
        unknown["release_key_hint"] = serde_json::json!("untrusted");
        assert!(
            release_candidate(&serde_json::to_string(&unknown).unwrap()).is_err(),
            "unknown field refused"
        );

        let mut duplicate_edge: serde_json::Value =
            serde_json::from_str(EMBEDDED_ALLOWLIST).unwrap();
        let first = duplicate_edge["transitions"][0].clone();
        duplicate_edge["transitions"]
            .as_array_mut()
            .unwrap()
            .push(first);
        assert!(
            release_candidate(&serde_json::to_string(&duplicate_edge).unwrap()).is_err(),
            "duplicate directed edge refused"
        );

        let mut self_edge: serde_json::Value = serde_json::from_str(EMBEDDED_ALLOWLIST).unwrap();
        let from = self_edge["transitions"][0]["from_image_config_digest"].clone();
        self_edge["transitions"][0]["to_image_config_digest"] = from;
        assert!(
            release_candidate(&serde_json::to_string(&self_edge).unwrap()).is_err(),
            "self transition refused"
        );

        let mut wrong_repository: serde_json::Value = serde_json::from_str(RELEASES).unwrap();
        wrong_repository["signature"] = serde_json::json!("pending");
        wrong_repository["repository"] = serde_json::json!("docker.io/untrusted/cardano-node");
        assert!(
            release_candidate(&serde_json::to_string(&wrong_repository).unwrap()).is_err(),
            "alternate repository refused before signing"
        );

        let mut missing_repository: serde_json::Value = serde_json::from_str(RELEASES).unwrap();
        missing_repository["signature"] = serde_json::json!("pending");
        missing_repository
            .as_object_mut()
            .unwrap()
            .remove("repository");
        assert!(
            release_candidate(&serde_json::to_string(&missing_repository).unwrap()).is_err(),
            "missing repository refused before signing"
        );
    }

    #[test]
    fn allowed_digest_resolves_denylist_and_unknown_refuse() {
        let a = Allowlist::embedded().unwrap();
        let good = &a.contracts[0].allowed[0].image_config_digest.clone();
        let platform = &a.contracts[0].allowed[0].platform.clone();
        assert!(
            a.contract_for(good, platform).is_ok(),
            "allowlisted digest conforms"
        );
        // Unknown digest → refuse (no tag trust).
        assert!(a.contract_for("sha256:deadbeef", platform).is_err());
        // Wrong platform → refuse.
        assert!(a.contract_for(good, "linux/arm64").is_err());

        // Denylist wins over allow.
        let mut d = a.clone();
        d.denylist.push(good.clone());
        assert!(
            d.contract_for(good, platform).is_err(),
            "denylist overrides allow"
        );
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
        assert!(
            enforce_anti_rollback(&dir, embedded, false).is_err(),
            "rollback below floor refused"
        );
        // Erasing or corrupting the floor on a managed node fails closed instead of reopening v1.
        std::fs::remove_file(floor_path(&dir)).unwrap();
        assert!(enforce_anti_rollback(&dir, embedded, false).is_err());
        std::fs::write(
            floor_path(&dir),
            br#"{"allowlist_version":99,"mac":"forged"}"#,
        )
        .unwrap();
        assert!(enforce_anti_rollback(&dir, embedded + 100, false).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
