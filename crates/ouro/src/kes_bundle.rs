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
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{OuroError, Result};

const RELEASE_BASE: &str = "https://github.com/IntersectMBO/cardano-cli/releases/download";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    pub bundle_dir: String,
    pub platform: AirgapPlatform,
    pub cardano_cli_version: String,
    pub cardano_cli_asset: String,
    pub cardano_cli_archive_sha256: String,
    pub cardano_cli_sha256: String,
    pub kes_vkey_sha256: String,
    pub manifest_sha256: String,
    pub files: Vec<&'static str>,
}

#[derive(Serialize)]
struct BundleManifest<'a> {
    schema_version: u8,
    kind: &'static str,
    platform: AirgapPlatform,
    kes_period: u64,
    kes_vkey_sha256: &'a str,
    cardano_cli: CardanoCliManifest<'a>,
}

#[derive(Serialize)]
struct CardanoCliManifest<'a> {
    version: &'a str,
    asset: &'a str,
    release_url: &'a str,
    archive_sha256: &'a str,
    executable_sha256: &'a str,
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
        return Err(OuroError::Validation(format!(
            "refusing to replace existing air-gap bundle {}",
            out.display()
        )));
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
    let manifest = BundleManifest {
        schema_version: 1,
        kind: "ouro-kes-airgap-bundle",
        platform,
        kes_period,
        kes_vkey_sha256: &kes_vkey_sha256,
        cardano_cli: CardanoCliManifest {
            version,
            asset: &asset,
            release_url: &release_url,
            archive_sha256: &archive_sha256,
            executable_sha256: &cardano_cli_sha256,
        },
    };
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    fs::write(partial.join("manifest.json"), &manifest_bytes)?;
    let manifest_sha256 = sha256(&manifest_bytes);

    let generated_at = chrono::Utc::now().to_rfc3339();
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
        bundle_dir: out.display().to_string(),
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
