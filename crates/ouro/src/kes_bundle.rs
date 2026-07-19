//! Build a public, platform-specific KES handoff for an air-gapped machine.
//!
//! Ouro does not publish or mirror `cardano-cli`. The online control machine fetches one exact
//! official Intersect release archive, verifies its published SHA256, extracts only the executable,
//! and atomically promotes the completed public bundle. The cold signing key and counter never
//! enter this module.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::{MachineRole, PoolSpec};
use crate::{OuroError, Result};

const RELEASE_BASE: &str = "https://github.com/IntersectMBO/cardano-cli/releases/download";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AirgapPlatform {
    #[serde(rename = "aarch64-darwin")]
    MacAppleSilicon,
    #[serde(rename = "x86_64-darwin")]
    MacIntel,
    #[serde(rename = "x86_64-linux")]
    LinuxIntelAmd,
    #[serde(rename = "aarch64-linux")]
    LinuxArm,
}

impl AirgapPlatform {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "mac-apple-silicon" | "aarch64-darwin" | "darwin-arm64" => Ok(Self::MacAppleSilicon),
            "mac-intel" | "x86_64-darwin" | "darwin-x86_64" => Ok(Self::MacIntel),
            "linux-intel-amd" | "x86_64-linux" | "linux-x86_64" | "linux-amd64" => {
                Ok(Self::LinuxIntelAmd)
            }
            "linux-arm" | "aarch64-linux" | "linux-aarch64" | "linux-arm64" => Ok(Self::LinuxArm),
            _ => Err(OuroError::InvalidArgs(format!(
                "unsupported air-gap device {value:?}; use mac-apple-silicon, mac-intel, \
                 linux-intel-amd, or linux-arm"
            ))),
        }
    }

    pub fn release_name(self) -> &'static str {
        match self {
            Self::MacAppleSilicon => "aarch64-darwin",
            Self::MacIntel => "x86_64-darwin",
            Self::LinuxIntelAmd => "x86_64-linux",
            Self::LinuxArm => "aarch64-linux",
        }
    }

    fn is_current_host(self) -> bool {
        matches!(
            (self, std::env::consts::OS, std::env::consts::ARCH),
            (Self::MacAppleSilicon, "macos", "aarch64")
                | (Self::MacIntel, "macos", "x86_64")
                | (Self::LinuxIntelAmd, "linux", "x86_64")
                | (Self::LinuxArm, "linux", "aarch64")
        )
    }
}

#[derive(Debug, Serialize)]
pub struct KesAirgapBundleReport {
    pub changed: bool,
    pub reused: bool,
    pub bundle_dir: String,
    pub node_cert_path: String,
    pub node_cert_present: bool,
    pub platform: AirgapPlatform,
    pub cardano_cli_version: String,
    pub cardano_cli_asset: String,
    pub cardano_cli_archive_sha256: String,
    pub cardano_cli_sha256: String,
    pub kes_vkey_sha256: String,
    pub manifest_sha256: String,
    pub files: Vec<&'static str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    schema_version: u8,
    kind: String,
    platform: AirgapPlatform,
    kes_period: u64,
    generated_at: String,
    kes_vkey_sha256: String,
    cardano_cli: CardanoCliManifest,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CardanoCliManifest {
    version: String,
    asset: String,
    release_url: String,
    archive_sha256: String,
    executable_sha256: String,
}

const BUNDLE_FILES: [&str; 5] = [
    "kes.vkey",
    "cold-sign.sh",
    "cardano-cli",
    "manifest.json",
    "SHA256SUMS",
];

pub fn pending_dir(spec_path: &Path, node: &str) -> Result<PathBuf> {
    crate::intent::validate_machine_id(node)?;
    let spec = PoolSpec::from_file(spec_path)?;
    let machine = spec
        .machines
        .iter()
        .find(|machine| machine.id == node)
        .ok_or_else(|| OuroError::Validation(format!("node {node:?} is not in the pool spec")))?;
    if machine.role != MachineRole::Bp {
        return Err(OuroError::Validation(format!(
            "KES air-gap handoff requires a BP, but {node:?} is a relay"
        )));
    }
    let canonical_spec = fs::canonicalize(spec_path).map_err(|error| {
        OuroError::Validation(format!(
            "cannot resolve pool spec {}: {error}",
            spec_path.display()
        ))
    })?;
    let parent = canonical_spec.parent().ok_or_else(|| {
        OuroError::Validation("pool spec must have a containing directory".into())
    })?;
    Ok(parent.join("ouro-kes-rotation").join(node).join("pending"))
}

pub fn create_airgap_bundle(
    kes_vkey_path: &Path,
    kes_period: u64,
    cardano_cli_version: &str,
    platform_value: &str,
    out: &Path,
) -> Result<KesAirgapBundleReport> {
    validate_version(cardano_cli_version)?;
    let platform = AirgapPlatform::parse(platform_value)?;
    if out.exists() {
        return validate_existing_bundle(
            kes_vkey_path,
            kes_period,
            cardano_cli_version,
            platform,
            out,
        );
    }
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = out
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            OuroError::Validation("air-gap bundle output must have a UTF-8 directory name".into())
        })?;
    let partial = parent.join(format!(".{name}.ouro-partial-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&partial)?;
    let result = build_bundle(
        kes_vkey_path,
        kes_period,
        cardano_cli_version,
        platform,
        out,
        &partial,
    );
    if result.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    result
}

fn build_bundle(
    kes_vkey_path: &Path,
    kes_period: u64,
    version: &str,
    platform: AirgapPlatform,
    out: &Path,
    partial: &Path,
) -> Result<KesAirgapBundleReport> {
    let vkey = fs::read(kes_vkey_path).map_err(|error| {
        OuroError::Validation(format!(
            "cannot read public KES verification key {}: {error}",
            kes_vkey_path.display()
        ))
    })?;
    crate::kes::parse_kes_verification_key(&vkey)?;
    let kes_vkey_sha256 = sha256(&vkey);

    let tag = format!("cardano-cli-{version}");
    let asset = format!("{tag}-{}.tar.gz", platform.release_name());
    let sums_name = format!("{tag}-sha256sums.txt");
    let release_url = format!("{RELEASE_BASE}/{tag}/{asset}");
    let archive_path = partial.join(&asset);
    let sums_path = partial.join(&sums_name);
    fetch_release_file(&tag, &asset, &archive_path, 128 * 1024 * 1024)?;
    fetch_release_file(&tag, &sums_name, &sums_path, 64 * 1024)?;
    let sums = fs::read_to_string(&sums_path).map_err(|error| {
        OuroError::Validation(format!(
            "official cardano-cli checksum file is not UTF-8: {error}"
        ))
    })?;
    let expected_archive_sha256 = expected_checksum(&sums, &asset)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    if archive_sha256 != expected_archive_sha256 {
        return Err(OuroError::Validation(format!(
            "official cardano-cli archive checksum mismatch for {asset}: expected \
             {expected_archive_sha256}, got {archive_sha256}"
        )));
    }

    let binary_path = partial.join("cardano-cli");
    let release_binary_name = format!("cardano-cli-{}", platform.release_name());
    extract_single_binary(&archive_path, &binary_path, &release_binary_name)?;
    set_executable(&binary_path)?;
    let cardano_cli_sha256 = sha256_file(&binary_path)?;
    if platform.is_current_host() {
        verify_reported_version(&binary_path, version)?;
    }

    fs::write(partial.join("kes.vkey"), &vkey)?;
    let generated_at = chrono::Utc::now().to_rfc3339();
    let manifest = BundleManifest {
        schema_version: 2,
        kind: "ouro-kes-airgap-bundle".into(),
        platform,
        kes_period,
        generated_at: generated_at.clone(),
        kes_vkey_sha256: kes_vkey_sha256.clone(),
        cardano_cli: CardanoCliManifest {
            version: version.into(),
            asset: asset.clone(),
            release_url,
            archive_sha256: archive_sha256.clone(),
            executable_sha256: cardano_cli_sha256.clone(),
        },
    };
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    fs::write(partial.join("manifest.json"), &manifest_bytes)?;
    let manifest_sha256 = sha256(&manifest_bytes);

    let script = crate::cold_sign::kes_bundle_cold_sign_script(
        kes_period,
        version,
        &cardano_cli_sha256,
        &kes_vkey_sha256,
        &manifest_sha256,
        &generated_at,
    );
    let script_path = partial.join("cold-sign.sh");
    fs::write(&script_path, script.as_bytes())?;
    set_executable(&script_path)?;

    let script_sha256 = sha256(script.as_bytes());
    let checksums = [
        ("cardano-cli", cardano_cli_sha256.as_str()),
        ("cold-sign.sh", script_sha256.as_str()),
        ("kes.vkey", kes_vkey_sha256.as_str()),
        ("manifest.json", manifest_sha256.as_str()),
    ];
    let checksums = checksums
        .iter()
        .map(|(name, digest)| format!("{digest}  {name}\n"))
        .collect::<String>();
    fs::write(partial.join("SHA256SUMS"), checksums)?;

    fs::remove_file(&archive_path)?;
    fs::remove_file(&sums_path)?;
    fs::rename(partial, out).map_err(|error| {
        OuroError::Validation(format!(
            "cannot atomically promote air-gap bundle to {}: {error}",
            out.display()
        ))
    })?;
    Ok(KesAirgapBundleReport {
        changed: true,
        reused: false,
        bundle_dir: out.display().to_string(),
        node_cert_path: out.join("node.cert").display().to_string(),
        node_cert_present: false,
        platform,
        cardano_cli_version: version.to_string(),
        cardano_cli_asset: asset,
        cardano_cli_archive_sha256: archive_sha256,
        cardano_cli_sha256,
        kes_vkey_sha256,
        manifest_sha256,
        files: vec![
            "kes.vkey",
            "cold-sign.sh",
            "cardano-cli",
            "manifest.json",
            "SHA256SUMS",
        ],
    })
}

fn validate_existing_bundle(
    kes_vkey_path: &Path,
    kes_period: u64,
    version: &str,
    platform: AirgapPlatform,
    out: &Path,
) -> Result<KesAirgapBundleReport> {
    validate_bundle_directory(out)?;
    let source_vkey = fs::read(kes_vkey_path).map_err(|error| {
        OuroError::Validation(format!(
            "cannot read public KES verification key {}: {error}",
            kes_vkey_path.display()
        ))
    })?;
    crate::kes::parse_kes_verification_key(&source_vkey)?;
    let expected_vkey_sha256 = sha256(&source_vkey);

    let manifest_bytes = fs::read(out.join("manifest.json"))?;
    let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        OuroError::Validation(format!("existing KES bundle manifest is invalid: {error}"))
    })?;
    if manifest.schema_version != 2
        || manifest.kind != "ouro-kes-airgap-bundle"
        || manifest.platform != platform
        || manifest.kes_period != kes_period
        || manifest.kes_vkey_sha256 != expected_vkey_sha256
        || manifest.cardano_cli.version != version
    {
        return Err(OuroError::Validation(
            "existing KES pending bundle does not match the staged key, period, platform and CLI version"
                .into(),
        ));
    }
    let expected_asset = format!("cardano-cli-{version}-{}.tar.gz", platform.release_name());
    let expected_url = format!("{RELEASE_BASE}/cardano-cli-{version}/{expected_asset}");
    if manifest.cardano_cli.asset != expected_asset
        || manifest.cardano_cli.release_url != expected_url
        || !valid_sha256(&manifest.cardano_cli.archive_sha256)
        || !valid_sha256(&manifest.cardano_cli.executable_sha256)
    {
        return Err(OuroError::Validation(
            "existing KES pending bundle has invalid cardano-cli provenance".into(),
        ));
    }
    let manifest_sha256 = sha256(&manifest_bytes);
    let cardano_cli_sha256 = sha256_file(&out.join("cardano-cli"))?;
    if cardano_cli_sha256 != manifest.cardano_cli.executable_sha256
        || sha256_file(&out.join("kes.vkey"))? != expected_vkey_sha256
        || fs::read(out.join("kes.vkey"))? != source_vkey
    {
        return Err(OuroError::Validation(
            "existing KES pending bundle file digest mismatch".into(),
        ));
    }
    let expected_script = crate::cold_sign::kes_bundle_cold_sign_script(
        kes_period,
        version,
        &cardano_cli_sha256,
        &expected_vkey_sha256,
        &manifest_sha256,
        &manifest.generated_at,
    );
    if fs::read(out.join("cold-sign.sh"))? != expected_script.as_bytes() {
        return Err(OuroError::Validation(
            "existing KES pending bundle signing script mismatch".into(),
        ));
    }
    let script_sha256 = sha256(expected_script.as_bytes());
    let expected_sums = format!(
        "{cardano_cli_sha256}  cardano-cli\n{script_sha256}  cold-sign.sh\n\
         {expected_vkey_sha256}  kes.vkey\n{manifest_sha256}  manifest.json\n"
    );
    if fs::read(out.join("SHA256SUMS"))? != expected_sums.as_bytes() {
        return Err(OuroError::Validation(
            "existing KES pending bundle checksum manifest mismatch".into(),
        ));
    }
    if platform.is_current_host() {
        verify_reported_version(&out.join("cardano-cli"), version)?;
    }
    Ok(KesAirgapBundleReport {
        changed: false,
        reused: true,
        bundle_dir: out.display().to_string(),
        node_cert_path: out.join("node.cert").display().to_string(),
        node_cert_present: out.join("node.cert").is_file(),
        platform,
        cardano_cli_version: version.to_string(),
        cardano_cli_asset: manifest.cardano_cli.asset,
        cardano_cli_archive_sha256: manifest.cardano_cli.archive_sha256,
        cardano_cli_sha256,
        kes_vkey_sha256: expected_vkey_sha256,
        manifest_sha256,
        files: BUNDLE_FILES.to_vec(),
    })
}

pub fn cleanup_pending_bundle(
    spec_path: &Path,
    node: &str,
    expected_vkey_sha256: &str,
) -> Result<bool> {
    if !valid_sha256(expected_vkey_sha256) {
        return Err(OuroError::InvalidArgs(
            "--expected-vkey-sha256 must be exactly 64 hexadecimal characters".into(),
        ));
    }
    let pending = pending_dir(spec_path, node)?;
    if !pending.exists() {
        return Ok(false);
    }
    validate_bundle_directory(&pending)?;
    let manifest: BundleManifest =
        serde_json::from_slice(&fs::read(pending.join("manifest.json"))?).map_err(|error| {
            OuroError::Validation(format!("KES pending bundle manifest is invalid: {error}"))
        })?;
    if manifest.schema_version != 2
        || manifest.kind != "ouro-kes-airgap-bundle"
        || manifest.kes_vkey_sha256 != expected_vkey_sha256
        || sha256_file(&pending.join("kes.vkey"))? != expected_vkey_sha256
    {
        return Err(OuroError::Validation(
            "refusing to clean a KES pending bundle not bound to the expected staged key".into(),
        ));
    }
    fs::remove_dir_all(&pending)?;
    remove_empty_parent(pending.parent())?;
    remove_empty_parent(pending.parent().and_then(Path::parent))?;
    Ok(true)
}

fn validate_bundle_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(OuroError::Validation(format!(
            "KES pending bundle {} must be a real directory",
            path.display()
        )));
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name().into_string().map_err(|_| {
            OuroError::Validation("KES pending bundle contains a non-UTF-8 entry".into())
        })?;
        if !file_type.is_file() || (!BUNDLE_FILES.contains(&name.as_str()) && name != "node.cert") {
            return Err(OuroError::Validation(format!(
                "KES pending bundle contains unexpected entry {name:?}"
            )));
        }
        names.push(name);
    }
    if BUNDLE_FILES
        .iter()
        .any(|required| !names.iter().any(|name| name == required))
    {
        return Err(OuroError::Validation(
            "KES pending bundle is incomplete".into(),
        ));
    }
    Ok(())
}

fn remove_empty_parent(path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if path.read_dir()?.next().is_none() {
            fs::remove_dir(path)?;
        }
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_version(version: &str) -> Result<()> {
    let components = version.split('.').collect::<Vec<_>>();
    if components.len() != 4
        || components
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(OuroError::InvalidArgs(
            "--cardano-cli-version must be four dot-separated numeric components".into(),
        ));
    }
    Ok(())
}

fn fetch_release_file(tag: &str, name: &str, destination: &Path, max_size: u64) -> Result<()> {
    let test_dir = if cfg!(debug_assertions) {
        std::env::var_os("OURO_CARDANO_CLI_RELEASE_DIR")
    } else {
        None
    };
    if let Some(test_dir) = test_dir {
        let source = PathBuf::from(test_dir).join(name);
        fs::copy(&source, destination).map_err(|error| {
            OuroError::Validation(format!(
                "cannot read test cardano-cli release {}: {error}",
                source.display()
            ))
        })?;
    } else {
        let url = format!("{RELEASE_BASE}/{tag}/{name}");
        let status = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--max-time",
                "300",
                "--retry",
                "2",
                "--retry-delay",
                "1",
                "--retry-all-errors",
                "--max-filesize",
                &max_size.to_string(),
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--output",
            ])
            .arg(destination)
            .arg(&url)
            .status()
            .map_err(|error| {
                OuroError::Validation(format!("cannot execute curl for {url}: {error}"))
            })?;
        if !status.success() {
            return Err(OuroError::Validation(format!(
                "cannot fetch official cardano-cli release file {url}"
            )));
        }
    }
    let size = fs::metadata(destination)?.len();
    if size == 0 || size > max_size {
        return Err(OuroError::Validation(format!(
            "cardano-cli release file {name} has invalid size {size}"
        )));
    }
    Ok(())
}

fn expected_checksum(text: &str, asset: &str) -> Result<String> {
    let matches = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let digest = fields.next()?;
            let name = fields.next()?;
            (name == asset && fields.next().is_none()).then(|| digest.to_ascii_lowercase())
        })
        .collect::<Vec<_>>();
    if matches.len() != 1
        || matches[0].len() != 64
        || !matches[0].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(OuroError::Validation(format!(
            "official cardano-cli checksum file must contain exactly one valid SHA256 for {asset}"
        )));
    }
    Ok(matches[0].clone())
}

fn extract_single_binary(
    archive_path: &Path,
    destination: &Path,
    expected_name: &str,
) -> Result<()> {
    let archive_file = File::open(archive_path)?;
    let mut archive = tar::Archive::new(GzDecoder::new(archive_file));
    let mut found = false;
    for entry in archive.entries().map_err(|error| {
        OuroError::Validation(format!(
            "cannot inspect cardano-cli release archive: {error}"
        ))
    })? {
        let mut entry = entry.map_err(|error| {
            OuroError::Validation(format!("cannot read cardano-cli release entry: {error}"))
        })?;
        let path = entry.path().map_err(|error| {
            OuroError::Validation(format!(
                "cardano-cli release contains an invalid path: {error}"
            ))
        })?;
        if path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(OuroError::Validation(
                "cardano-cli release archive contains an unsafe path".into(),
            ));
        }
        if entry.header().entry_type().is_file()
            && path.file_name().and_then(|name| name.to_str()) == Some(expected_name)
        {
            if found {
                return Err(OuroError::Validation(
                    "cardano-cli release archive contains multiple cardano-cli binaries".into(),
                ));
            }
            let size = entry.size();
            if size == 0 || size > 512 * 1024 * 1024 {
                return Err(OuroError::Validation(
                    "cardano-cli release executable has an invalid size".into(),
                ));
            }
            let mut file = File::create(destination)?;
            let copied = std::io::copy(&mut entry, &mut file)?;
            if copied != size {
                return Err(OuroError::Validation(
                    "cardano-cli release executable was truncated during extraction".into(),
                ));
            }
            file.sync_all()?;
            found = true;
        }
    }
    if !found {
        return Err(OuroError::Validation(format!(
            "cardano-cli release archive contains no {expected_name} binary"
        )));
    }
    Ok(())
}

fn verify_reported_version(binary: &Path, expected: &str) -> Result<()> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|error| {
            OuroError::Validation(format!("cannot execute downloaded cardano-cli: {error}"))
        })?;
    if !output.status.success() {
        return Err(OuroError::Validation(
            "downloaded cardano-cli --version failed".into(),
        ));
    }
    let report = String::from_utf8_lossy(&output.stdout);
    let prefix = format!("cardano-cli {expected}");
    if !report.lines().next().is_some_and(|line| {
        line == prefix
            || line
                .strip_prefix(&prefix)
                .is_some_and(|rest| rest.starts_with(' '))
    }) {
        return Err(OuroError::Validation(format!(
            "downloaded cardano-cli reported an unexpected version: {:?}",
            report.lines().next().unwrap_or("")
        )));
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_platform_names_are_closed() {
        assert_eq!(
            AirgapPlatform::parse("mac-apple-silicon")
                .unwrap()
                .release_name(),
            "aarch64-darwin"
        );
        assert_eq!(
            AirgapPlatform::parse("mac-intel").unwrap().release_name(),
            "x86_64-darwin"
        );
        assert_eq!(
            AirgapPlatform::parse("linux-intel-amd")
                .unwrap()
                .release_name(),
            "x86_64-linux"
        );
        assert_eq!(
            AirgapPlatform::parse("linux-arm").unwrap().release_name(),
            "aarch64-linux"
        );
        assert!(AirgapPlatform::parse("windows").is_err());
    }

    #[test]
    fn checksum_requires_one_exact_asset() {
        let digest = "a".repeat(64);
        assert_eq!(
            expected_checksum(&format!("{digest}  wanted.tar.gz\n"), "wanted.tar.gz").unwrap(),
            digest
        );
        assert!(expected_checksum(&format!("{digest}  other.tar.gz\n"), "wanted.tar.gz").is_err());
        assert!(expected_checksum(
            &format!("{digest}  wanted.tar.gz\n{digest}  wanted.tar.gz\n"),
            "wanted.tar.gz"
        )
        .is_err());
    }
}
