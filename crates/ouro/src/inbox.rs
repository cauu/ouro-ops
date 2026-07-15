//! S0019 p2-3 (§2.7) — the content-addressed artifact staging inbox.
//!
//! Operations that need a blob (opcert, signed tx, OCI image) do NOT pass a path or blob in the
//! intent (that would be traversal or a content parser in the schema). Instead the operator/agent
//! stages the artifact into a root-owned, content-addressed inbox; the artifact's identity is its
//! sha256. The intent then references only `<id>@sha256:<digest>` (validated by §2.5). At use time
//! the executor resolves the ref against the stored content and re-verifies the digest, so a
//! replaced/oversized/wrong-type/replayed artifact is refused. GC reclaims stale artifacts.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{OuroError, Result};

/// Per-artifact-type size ceilings (bytes). A blob over its ceiling is refused before hashing.
const MAX_OPCERT: usize = 64 * 1024;
const MAX_TX: usize = 1 * 1024 * 1024;
const MAX_IMAGE: usize = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Opcert,
    Tx,
    Image,
}

impl ArtifactType {
    pub fn max_bytes(self) -> usize {
        match self {
            ArtifactType::Opcert => MAX_OPCERT,
            ArtifactType::Tx => MAX_TX,
            ArtifactType::Image => MAX_IMAGE,
        }
    }
    pub fn prefix(self) -> &'static str {
        match self {
            ArtifactType::Opcert => "opcert",
            ArtifactType::Tx => "tx",
            ArtifactType::Image => "image",
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Read a local artifact through a bounded stream. Symlinks, non-regular files and multiply-linked
/// files are refused before content is read, so a control-local path cannot swap into a device/FIFO
/// or alias a mutable file during staging.
pub fn stage_file(inbox: &Path, kind: ArtifactType, source: &Path) -> Result<String> {
    let file = open_source(kind, source)?;
    stage_reader(inbox, kind, file)
}

/// Open a bounded regular source for streaming transport. The target independently performs full
/// type/domain validation before finalizing it.
pub fn open_source(kind: ArtifactType, source: &Path) -> Result<fs::File> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|e| OuroError::Validation(format!("cannot inspect artifact: {e}")))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(OuroError::Validation(
            "artifact source must be a regular non-symlink file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(OuroError::Validation(
                "artifact source has multiple hard links — refused".into(),
            ));
        }
    }
    if metadata.len() > kind.max_bytes() as u64 {
        return Err(OuroError::Validation(format!(
            "{} artifact exceeds {} bytes",
            kind.prefix(),
            kind.max_bytes()
        )));
    }
    fs::File::open(source).map_err(Into::into)
}

/// Bounded ingress used by the target-side SSH stdin wrapper. Reads at most `limit + 1`, so an
/// oversized sender never causes unbounded allocation before the size gate fires.
pub fn stage_reader<R: Read>(inbox: &Path, kind: ArtifactType, reader: R) -> Result<String> {
    prepare_inbox(inbox)?;
    let tmp = inbox.join(format!(".incoming.{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(&tmp)?;
    let mut input = reader;
    let mut hasher = Sha256::new();
    let mut total = 0usize;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read).ok_or_else(|| {
            OuroError::Validation("artifact size overflow".into())
        })?;
        if total > kind.max_bytes() {
            drop(output);
            fs::remove_file(&tmp).ok();
            return Err(OuroError::Validation(format!(
                "{} artifact exceeds {} bytes",
                kind.prefix(),
                kind.max_bytes()
            )));
        }
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
    }
    if total == 0 {
        drop(output);
        fs::remove_file(&tmp).ok();
        return Err(OuroError::Validation("artifact is empty".into()));
    }
    output.sync_all()?;
    drop(output);
    if let Err(error) = validate_file(kind, &tmp) {
        fs::remove_file(&tmp).ok();
        return Err(error);
    }
    let digest = hex_digest(hasher.finalize().as_slice());
    let path = inbox.join(&digest);
    match fs::hard_link(&tmp, &path) {
        Ok(()) => {
            fs::remove_file(&tmp)?;
            fs::File::open(inbox)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::remove_file(&tmp)?;
            let reference = format!("{}-{}@sha256:{}", kind.prefix(), &digest[..8], digest);
            resolve_typed(inbox, &reference, kind)?;
        }
        Err(error) => {
            fs::remove_file(&tmp).ok();
            return Err(error.into());
        }
    }
    Ok(format!("{}-{}@sha256:{}", kind.prefix(), &digest[..8], digest))
}

/// Stage bytes into the inbox. Validates size + type-specific shape, stores content-addressed, and
/// returns the immutable reference `<prefix>-<short>@sha256:<digest>` for use in an intent.
pub fn stage(inbox: &Path, kind: ArtifactType, bytes: &[u8]) -> Result<String> {
    stage_reader(inbox, kind, std::io::Cursor::new(bytes))
}

fn prepare_inbox(inbox: &Path) -> Result<()> {
    fs::create_dir_all(inbox)
        .map_err(|e| OuroError::Validation(format!("cannot create inbox: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(inbox, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Resolve an artifact reference to its stored path, RE-VERIFYING the digest against the content
/// (a replaced file, or a ref whose digest does not match, is refused).
pub fn resolve(inbox: &Path, artifact_ref: &str) -> Result<PathBuf> {
    let (kind, digest) = parse_reference(artifact_ref)?;
    resolve_typed(inbox, artifact_ref, kind).map(|_| inbox.join(digest))
}

/// Resolve an artifact for a consuming operation, binding both the human id prefix and the
/// expected type to the revalidated bytes. `opcert-…` can no longer be relabeled as a tx/image.
pub fn resolve_typed(
    inbox: &Path,
    artifact_ref: &str,
    expected: ArtifactType,
) -> Result<PathBuf> {
    let (actual, digest) = parse_reference(artifact_ref)?;
    if actual != expected {
        return Err(OuroError::Validation(format!(
            "artifact type mismatch: {} operation received {} reference",
            expected.prefix(),
            actual.prefix()
        )));
    }
    let path = inbox.join(digest);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| OuroError::Validation(format!("artifact {digest} not in inbox — refused")))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(OuroError::Validation("inbox artifact is not a regular file".into()));
    }
    if metadata.len() > expected.max_bytes() as u64 {
        return Err(OuroError::Validation("stored artifact exceeds its type limit".into()));
    }
    if hash_file(&path, expected.max_bytes())? != digest {
        return Err(OuroError::Validation(
            "artifact content does not match its digest — refused (replaced?)".into(),
        ));
    }
    validate_file(expected, &path)?;
    Ok(path)
}

fn hash_file(path: &Path, max_bytes: usize) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut total = 0usize;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read);
        if total > max_bytes {
            return Err(OuroError::Validation("stored artifact exceeds its type limit".into()));
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_file(kind: ArtifactType, path: &Path) -> Result<()> {
    match kind {
        ArtifactType::Opcert | ArtifactType::Tx => {
            let bytes = fs::read(path)?;
            validate_shape(kind, &bytes)
        }
        ArtifactType::Image => validate_image_archive(path),
    }
}

fn parse_reference(artifact_ref: &str) -> Result<(ArtifactType, &str)> {
    let (id, digest) = artifact_ref
        .split_once("@sha256:")
        .ok_or_else(|| OuroError::Validation("malformed artifact reference".into()))?;
    if digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OuroError::Validation("malformed artifact digest".into()));
    }
    let kind = if id == format!("opcert-{}", &digest[..8]) {
        ArtifactType::Opcert
    } else if id == format!("tx-{}", &digest[..8]) {
        ArtifactType::Tx
    } else if id == format!("image-{}", &digest[..8]) {
        ArtifactType::Image
    } else {
        return Err(OuroError::Validation(
            "artifact id does not match its type + digest".into(),
        ));
    };
    Ok((kind, digest))
}

fn validate_shape(kind: ArtifactType, bytes: &[u8]) -> Result<()> {
    match kind {
        // opcert/tx are cardano-cli text/CBOR envelopes — must be valid UTF-8 JSON envelopes here
        // (the deep domain/crypto validation is the consuming op's job; this rejects obvious junk).
        ArtifactType::Opcert | ArtifactType::Tx => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| OuroError::Validation("opcert/tx must be a text envelope".into()))?;
            let envelope = serde_json::from_str::<serde_json::Value>(text)
                .map_err(|_| OuroError::Validation("opcert/tx is not a JSON envelope".into()))?;
            let object = envelope.as_object().ok_or_else(|| {
                OuroError::Validation("opcert/tx envelope must be a JSON object".into())
            })?;
            let envelope_type = object.get("type").and_then(|value| value.as_str()).unwrap_or("");
            let cbor = object.get("cborHex").and_then(|value| value.as_str()).unwrap_or("");
            if cbor.is_empty()
                || cbor.len() % 2 != 0
                || !cbor.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(OuroError::Validation(
                    "opcert/tx envelope has malformed cborHex".into(),
                ));
            }
            match kind {
                ArtifactType::Opcert if envelope_type == "NodeOperationalCertificate" => Ok(()),
                ArtifactType::Tx if envelope_type.contains("Tx")
                    && envelope_type != "NodeOperationalCertificate" => Ok(()),
                ArtifactType::Opcert => Err(OuroError::Validation(
                    "opcert envelope type must be NodeOperationalCertificate".into(),
                )),
                ArtifactType::Tx => Err(OuroError::Validation(
                    "tx envelope type is not a Cardano transaction".into(),
                )),
                ArtifactType::Image => unreachable!(),
            }
        }
        // an OCI image tar — must begin with a tar/gzip magic (cheap sanity, not full validation).
        ArtifactType::Image => {
            let gzip = bytes.len() >= 18 && bytes.starts_with(&[0x1f, 0x8b, 0x08]);
            let tar = bytes.len() > 262 && &bytes[257..262] == b"ustar";
            if gzip || tar {
                Ok(())
            } else {
                Err(OuroError::Validation("image is not a tar/gzip archive".into()))
            }
        }
    }
}

fn validate_image_archive(path: &Path) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let mut magic = [0u8; 3];
    file.read_exact(&mut magic)
        .map_err(|_| OuroError::Validation("image archive is truncated".into()))?;
    file.seek(SeekFrom::Start(0))?;
    let reader: Box<dyn Read> = if magic == [0x1f, 0x8b, 0x08] {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut archive = tar::Archive::new(reader);
    let mut names = HashSet::new();
    let mut small_json: HashMap<String, Vec<u8>> = HashMap::new();
    let mut verified_blobs = HashSet::new();
    for entry in archive.entries()
        .map_err(|e| OuroError::Validation(format!("image tar is invalid: {e}")))?
    {
        let mut entry = entry
            .map_err(|e| OuroError::Validation(format!("image tar entry is invalid: {e}")))?;
        let entry_path = entry.path()
            .map_err(|e| OuroError::Validation(format!("image tar path is invalid: {e}")))?
            .into_owned();
        if entry_path.is_absolute()
            || entry_path.components().any(|component| !matches!(
                component,
                std::path::Component::Normal(_)
            ))
        {
            return Err(OuroError::Validation(
                "image archive contains an unsafe path".into(),
            ));
        }
        let name = entry_path.to_string_lossy().into_owned();
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            names.insert(name);
            continue;
        }
        if !entry_type.is_file() {
            return Err(OuroError::Validation(
                "image archive contains a link/device entry — refused".into(),
            ));
        }
        names.insert(name.clone());
        if let Some(digest) = name.strip_prefix("blobs/sha256/") {
            if digest.len() != 64
                || !digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(OuroError::Validation("OCI blob path has malformed digest".into()));
            }
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = entry.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            if hex_digest(hasher.finalize().as_slice()) != digest {
                return Err(OuroError::Validation(
                    "OCI blob content does not match its digest".into(),
                ));
            }
            verified_blobs.insert(digest.to_string());
        } else if name == "manifest.json"
            || name == "index.json"
            || name == "oci-layout"
            || (name.ends_with(".json") && entry.size() <= 1024 * 1024)
        {
            let mut content = Vec::new();
            entry.take(1024 * 1024 + 1).read_to_end(&mut content)?;
            if content.len() > 1024 * 1024 {
                return Err(OuroError::Validation(
                    "image metadata JSON exceeds 1 MiB".into(),
                ));
            }
            small_json.insert(name, content);
        }
    }

    if let Some(manifest_bytes) = small_json.get("manifest.json") {
        let manifest: Vec<serde_json::Value> = serde_json::from_slice(manifest_bytes)
            .map_err(|_| OuroError::Validation("Docker image manifest.json is malformed".into()))?;
        if manifest.is_empty() {
            return Err(OuroError::Validation("Docker image manifest is empty".into()));
        }
        for image in manifest {
            let config = image.get("Config").and_then(|value| value.as_str())
                .ok_or_else(|| OuroError::Validation("Docker image manifest lacks Config".into()))?;
            let layers = image.get("Layers").and_then(|value| value.as_array())
                .ok_or_else(|| OuroError::Validation("Docker image manifest lacks Layers".into()))?;
            if !names.contains(config)
                || layers.is_empty()
                || layers.iter().any(|layer| {
                    layer.as_str().map(|name| !names.contains(name)).unwrap_or(true)
                })
            {
                return Err(OuroError::Validation(
                    "Docker image manifest references missing config/layers".into(),
                ));
            }
            if let Some(blob) = config.strip_prefix("blobs/sha256/") {
                if !verified_blobs.contains(blob) {
                    return Err(OuroError::Validation("Docker config blob was not verified".into()));
                }
            } else if let Some(stem) = config.strip_suffix(".json") {
                let content = small_json.get(config).ok_or_else(|| {
                    OuroError::Validation("Docker config JSON was not validated".into())
                })?;
                if stem.len() == 64 && sha256_hex(content) != stem {
                    return Err(OuroError::Validation(
                        "Docker config filename digest does not match content".into(),
                    ));
                }
            }
        }
        return Ok(());
    }

    let layout = small_json.get("oci-layout").ok_or_else(|| {
        OuroError::Validation("image archive has neither Docker manifest.json nor OCI layout".into())
    })?;
    let layout: serde_json::Value = serde_json::from_slice(layout)
        .map_err(|_| OuroError::Validation("OCI layout metadata is malformed".into()))?;
    if layout.get("imageLayoutVersion").and_then(|value| value.as_str()) != Some("1.0.0") {
        return Err(OuroError::Validation("unsupported OCI image layout version".into()));
    }
    let index: serde_json::Value = serde_json::from_slice(
        small_json.get("index.json")
            .ok_or_else(|| OuroError::Validation("OCI image archive lacks index.json".into()))?,
    ).map_err(|_| OuroError::Validation("OCI index.json is malformed".into()))?;
    let manifests = index.get("manifests").and_then(|value| value.as_array())
        .ok_or_else(|| OuroError::Validation("OCI index has no manifests".into()))?;
    if manifests.is_empty() || manifests.iter().any(|descriptor| {
        descriptor.get("digest").and_then(|value| value.as_str())
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .map(|digest| !verified_blobs.contains(digest))
            .unwrap_or(true)
    }) {
        return Err(OuroError::Validation(
            "OCI index references a missing or unverified manifest blob".into(),
        ));
    }
    Ok(())
}

/// GC: remove artifacts whose mtime is older than `ttl_secs` relative to `now_secs` (caller passes
/// the clock — no ambient time). Returns how many were reclaimed.
pub fn gc(inbox: &Path, now_secs: u64, ttl_secs: u64) -> usize {
    let mut removed = 0;
    let Ok(entries) = std::fs::read_dir(inbox) else {
        return 0;
    };
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        let Ok(modified) = meta.modified() else { continue };
        let age = modified
            .elapsed()
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .max(secs_between(modified, now_secs));
        if age > ttl_secs {
            if std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

fn secs_between(modified: std::time::SystemTime, now_secs: u64) -> u64 {
    let m = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now_secs.saturating_sub(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inbox(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ouro-inbox-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        d
    }

    fn valid_image_tar() -> Vec<u8> {
        fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, content: &[u8]) {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o600);
            header.set_cksum();
            builder.append_data(&mut header, path, content).unwrap();
        }
        let config = br#"{"rootfs":{"type":"layers","diff_ids":[]}}"#;
        let config_name = format!("{}.json", sha256_hex(config));
        let layer_name = "layer/layer.tar";
        let manifest = serde_json::to_vec(&serde_json::json!([{
            "Config": config_name.clone(),
            "RepoTags": [],
            "Layers": [layer_name],
        }])).unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        append(&mut builder, &config_name, config);
        append(&mut builder, layer_name, b"layer");
        append(&mut builder, "manifest.json", &manifest);
        builder.finish().unwrap();
        builder.into_inner().unwrap()
    }

    #[test]
    fn stage_and_resolve_roundtrip() {
        let dir = inbox("roundtrip");
        let cert = br#"{"type":"NodeOperationalCertificate","cborHex":"aa"}"#;
        let r = stage(&dir, ArtifactType::Opcert, cert).unwrap();
        assert!(r.contains("@sha256:"));
        let path = resolve(&dir, &r).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), cert);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tamper_and_wrong_ref_refused() {
        let dir = inbox("tamper");
        let cert = br#"{"type":"NodeOperationalCertificate","cborHex":"bb"}"#;
        let r = stage(&dir, ArtifactType::Opcert, cert).unwrap();
        // Replace the stored content → digest mismatch → refused.
        let digest = r.split("@sha256:").nth(1).unwrap();
        std::fs::write(dir.join(digest), b"tampered").unwrap();
        assert!(resolve(&dir, &r).is_err(), "replaced content refused");
        // Unknown/malformed refs refused.
        assert!(resolve(&dir, "x@sha256:deadbeef").is_err());
        assert!(resolve(&dir, "not-a-ref").is_err());
        let digest = r.split("@sha256:").nth(1).unwrap();
        let relabeled = format!("tx-{}@sha256:{digest}", &digest[..8]);
        assert!(resolve_typed(&dir, &relabeled, ArtifactType::Tx).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn shape_and_size_validation() {
        let dir = inbox("shape");
        assert!(stage(&dir, ArtifactType::Opcert, b"not json").is_err(), "junk opcert refused");
        assert!(stage(&dir, ArtifactType::Opcert, b"").is_err(), "empty refused");
        let huge = vec![b'{'; MAX_OPCERT + 1];
        assert!(stage(&dir, ArtifactType::Opcert, &huge).is_err(), "oversized refused");
        assert!(stage(&dir, ArtifactType::Tx, br#"{"type":"Tx BabbageEra","cborHex":"aa"}"#).is_ok());
        assert!(stage(&dir, ArtifactType::Tx, br#"{"type":"NodeOperationalCertificate","cborHex":"aa"}"#).is_err());
        // Magic-only junk is refused; a domain-valid Docker save archive is accepted.
        assert!(stage(&dir, ArtifactType::Image, &[0x1f, 0x8b, 0x08, 0x00]).is_err());
        assert!(stage(&dir, ArtifactType::Image, &valid_image_tar()).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gc_reclaims_stale() {
        let dir = inbox("gc");
        let r = stage(&dir, ArtifactType::Tx, br#"{"type":"Tx","cborHex":"aa"}"#).unwrap();
        let digest = r.split("@sha256:").nth(1).unwrap().to_string();
        // With a "now" far in the future, the artifact is stale and GC'd.
        let future = 4_000_000_000u64;
        assert_eq!(gc(&dir, future, 60), 1, "stale artifact reclaimed");
        assert!(!dir.join(&digest).exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
