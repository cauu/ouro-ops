//! S0019 p2-3 (§2.7) — the content-addressed artifact staging inbox.
//!
//! Operations that need a public blob (opcert or signed tx) do NOT pass a path or blob in the
//! intent (that would be traversal or a content parser in the schema). Instead the operator/agent
//! stages the artifact into a root-owned, content-addressed inbox; the artifact's identity is its
//! sha256. The intent then references only `<id>@sha256:<digest>` (validated by §2.5). At use time
//! the executor resolves the ref against the stored content and re-verifies the digest, so a
//! replaced/oversized/wrong-type/replayed artifact is refused. GC reclaims stale artifacts.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{OuroError, Result};

/// Per-artifact-type size ceilings (bytes). A blob over its ceiling is refused before hashing.
const MAX_OPCERT: usize = 64 * 1024;
const MAX_TX: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Opcert,
    Tx,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactPreview {
    pub artifact_ref: String,
    pub size_bytes: u64,
}

impl ArtifactType {
    pub fn max_bytes(self) -> usize {
        match self {
            ArtifactType::Opcert => MAX_OPCERT,
            ArtifactType::Tx => MAX_TX,
        }
    }
    pub fn prefix(self) -> &'static str {
        match self {
            ArtifactType::Opcert => "opcert",
            ArtifactType::Tx => "tx",
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
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(source)
        .map_err(|e| OuroError::Validation(format!("cannot open artifact safely: {e}")))?;
    let metadata = file.metadata()
        .map_err(|e| OuroError::Validation(format!("cannot inspect opened artifact: {e}")))?;
    if !metadata.file_type().is_file() {
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
    Ok(file)
}

/// Fully validate and identify the exact opened bytes without staging them. The returned file is
/// rewound for transport; a later caller can require the planned reference to defeat path swaps.
pub fn preview_source(kind: ArtifactType, source: &Path) -> Result<(fs::File, ArtifactPreview)> {
    let mut file = open_source(kind, source)?;
    let size_bytes = file.metadata()?.len();
    let mut bytes = Vec::with_capacity(size_bytes as usize);
    file.read_to_end(&mut bytes)?;
    validate_shape(kind, &bytes)?;
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hex_digest(hasher.finalize().as_slice());
    file.seek(SeekFrom::Start(0))?;
    Ok((file, ArtifactPreview {
        artifact_ref: format!("{}-{}@sha256:{}", kind.prefix(), &digest[..8], digest),
        size_bytes,
    }))
}

/// Bounded ingress used by the target-side SSH stdin wrapper. Reads at most `limit + 1`, so an
/// oversized sender never causes unbounded allocation before the size gate fires.
pub fn stage_reader<R: Read>(inbox: &Path, kind: ArtifactType, reader: R) -> Result<String> {
    stage_reader_expected(inbox, kind, reader, None)
}

pub fn stage_reader_expected<R: Read>(
    inbox: &Path,
    kind: ArtifactType,
    reader: R,
    expected_ref: Option<&str>,
) -> Result<String> {
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
    let reference = format!("{}-{}@sha256:{}", kind.prefix(), &digest[..8], digest);
    if expected_ref.is_some_and(|expected| expected != reference) {
        fs::remove_file(&tmp).ok();
        return Err(OuroError::Validation(
            "streamed artifact bytes do not match the reviewed reference — nothing finalized"
                .into(),
        ));
    }
    let path = inbox.join(&digest);
    match fs::hard_link(&tmp, &path) {
        Ok(()) => {
            fs::remove_file(&tmp)?;
            fs::File::open(inbox)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::remove_file(&tmp)?;
            resolve_typed(inbox, &reference, kind)?;
        }
        Err(error) => {
            fs::remove_file(&tmp).ok();
            return Err(error.into());
        }
    }
    Ok(reference)
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
    let bytes = fs::read(path)?;
    validate_shape(kind, &bytes)
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
            }
        }
    }
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
        if age > ttl_secs && std::fs::remove_file(e.path()).is_ok() {
            removed += 1;
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
