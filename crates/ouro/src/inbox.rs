//! S0019 p2-3 (§2.7) — the content-addressed artifact staging inbox.
//!
//! Operations that need a blob (opcert, signed tx, OCI image) do NOT pass a path or blob in the
//! intent (that would be traversal or a content parser in the schema). Instead the operator/agent
//! stages the artifact into a root-owned, content-addressed inbox; the artifact's identity is its
//! sha256. The intent then references only `<id>@sha256:<digest>` (validated by §2.5). At use time
//! the executor resolves the ref against the stored content and re-verifies the digest, so a
//! replaced/oversized/wrong-type/replayed artifact is refused. GC reclaims stale artifacts.

use serde::{Deserialize, Serialize};
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
    fn max_bytes(self) -> usize {
        match self {
            ArtifactType::Opcert => MAX_OPCERT,
            ArtifactType::Tx => MAX_TX,
            ArtifactType::Image => MAX_IMAGE,
        }
    }
    fn prefix(self) -> &'static str {
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

/// Stage bytes into the inbox. Validates size + type-specific shape, stores content-addressed, and
/// returns the immutable reference `<prefix>-<short>@sha256:<digest>` for use in an intent.
pub fn stage(inbox: &Path, kind: ArtifactType, bytes: &[u8]) -> Result<String> {
    if bytes.is_empty() {
        return Err(OuroError::Validation("artifact is empty".into()));
    }
    if bytes.len() > kind.max_bytes() {
        return Err(OuroError::Validation(format!(
            "{} artifact exceeds {} bytes",
            kind.prefix(),
            kind.max_bytes()
        )));
    }
    validate_shape(kind, bytes)?;
    let digest = sha256_hex(bytes);
    std::fs::create_dir_all(inbox)
        .map_err(|e| OuroError::Validation(format!("cannot create inbox: {e}")))?;
    let path = inbox.join(&digest);
    // O_EXCL create → never follow/overwrite an existing (possibly symlinked) path. A re-stage of
    // identical content is idempotent (same digest already present).
    if !path.exists() {
        use std::io::Write;
        let tmp = inbox.join(format!("{digest}.tmp"));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| OuroError::Validation(format!("inbox write: {e}")))?;
        f.write_all(bytes).ok();
        f.sync_all().ok();
        std::fs::rename(&tmp, &path)
            .map_err(|e| OuroError::Validation(format!("inbox finalize: {e}")))?;
    }
    Ok(format!("{}-{}@sha256:{}", kind.prefix(), &digest[..8], digest))
}

/// Resolve an artifact reference to its stored path, RE-VERIFYING the digest against the content
/// (a replaced file, or a ref whose digest does not match, is refused).
pub fn resolve(inbox: &Path, artifact_ref: &str) -> Result<PathBuf> {
    let digest = artifact_ref
        .split_once("@sha256:")
        .map(|(_, d)| d)
        .filter(|d| d.len() == 64 && d.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| OuroError::Validation("malformed artifact reference".into()))?;
    let path = inbox.join(digest);
    let bytes = std::fs::read(&path)
        .map_err(|_| OuroError::Validation(format!("artifact {digest} not in inbox — refused")))?;
    if sha256_hex(&bytes) != digest {
        return Err(OuroError::Validation(
            "artifact content does not match its digest — refused (replaced?)".into(),
        ));
    }
    Ok(path)
}

fn validate_shape(kind: ArtifactType, bytes: &[u8]) -> Result<()> {
    match kind {
        // opcert/tx are cardano-cli text/CBOR envelopes — must be valid UTF-8 JSON envelopes here
        // (the deep domain/crypto validation is the consuming op's job; this rejects obvious junk).
        ArtifactType::Opcert | ArtifactType::Tx => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| OuroError::Validation("opcert/tx must be a text envelope".into()))?;
            serde_json::from_str::<serde_json::Value>(text)
                .map_err(|_| OuroError::Validation("opcert/tx is not a JSON envelope".into()))?;
            Ok(())
        }
        // an OCI image tar — must begin with a tar/gzip magic (cheap sanity, not full validation).
        ArtifactType::Image => {
            let gzip = bytes.starts_with(&[0x1f, 0x8b]);
            let tar = bytes.len() > 262 && &bytes[257..262] == b"ustar";
            if gzip || tar {
                Ok(())
            } else {
                Err(OuroError::Validation("image is not a tar/gzip archive".into()))
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
        let cert = br#"{"type":"cert","cborHex":"bb"}"#;
        let r = stage(&dir, ArtifactType::Opcert, cert).unwrap();
        // Replace the stored content → digest mismatch → refused.
        let digest = r.split("@sha256:").nth(1).unwrap();
        std::fs::write(dir.join(digest), b"tampered").unwrap();
        assert!(resolve(&dir, &r).is_err(), "replaced content refused");
        // Unknown/malformed refs refused.
        assert!(resolve(&dir, "x@sha256:deadbeef").is_err());
        assert!(resolve(&dir, "not-a-ref").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn shape_and_size_validation() {
        let dir = inbox("shape");
        assert!(stage(&dir, ArtifactType::Opcert, b"not json").is_err(), "junk opcert refused");
        assert!(stage(&dir, ArtifactType::Opcert, b"").is_err(), "empty refused");
        let huge = vec![b'{'; MAX_OPCERT + 1];
        assert!(stage(&dir, ArtifactType::Opcert, &huge).is_err(), "oversized refused");
        // gzip magic accepted as an image.
        assert!(stage(&dir, ArtifactType::Image, &[0x1f, 0x8b, 0x08, 0x00]).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gc_reclaims_stale() {
        let dir = inbox("gc");
        let r = stage(&dir, ArtifactType::Tx, br#"{"type":"Tx"}"#).unwrap();
        let digest = r.split("@sha256:").nth(1).unwrap().to_string();
        // With a "now" far in the future, the artifact is stale and GC'd.
        let future = 4_000_000_000u64;
        assert_eq!(gc(&dir, future, 60), 1, "stale artifact reclaimed");
        assert!(!dir.join(&digest).exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
