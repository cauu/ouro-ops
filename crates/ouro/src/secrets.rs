use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::{OuroError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialRef(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialStatus {
    pub name: String,
    pub registered: bool,
    pub usable: bool,
    pub entry_kind: String,
    pub owner_only_permissions: bool,
    pub owner_readable: bool,
    pub credential_contents_read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialRegistration {
    pub name: String,
    pub registered: bool,
    pub usable: bool,
    pub entry_kind: String,
    pub action: String,
    pub planned: bool,
    pub changed: bool,
    pub credential_contents_read: bool,
}

impl CredentialRef {
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.starts_with("creds://") && value.len() > "creds://".len() {
            Ok(Self(value))
        } else {
            Err(OuroError::Validation(
                "secret references must use creds:// and never inline plaintext".to_string(),
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The reference name — the part after `creds://` (e.g. `creds://bp1` → `bp1`).
    pub fn name(&self) -> &str {
        self.0.strip_prefix("creds://").unwrap_or(&self.0)
    }

    /// Resolve to the local credential file under the credentials dir (never inlines the
    /// secret; only yields a path). Rejects traversal / absolute names so a crafted spec
    /// (`creds://../../root/.ssh/id_rsa`) cannot escape the credentials directory. The
    /// file itself is provisioned by p1-4.
    pub fn resolve(&self, credentials_dir: &Path) -> Result<PathBuf> {
        let name = self.name();
        if name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name.contains("..")
            || name.starts_with('.')
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(OuroError::Validation(
                "invalid credential name (must be a single [A-Za-z0-9._-] segment)".into(),
            ));
        }
        Ok(credentials_dir.join(name))
    }
}

fn named_destination(credentials_dir: &Path, name: &str) -> Result<PathBuf> {
    CredentialRef::parse(format!("creds://{name}"))?.resolve(credentials_dir)
}

#[cfg(unix)]
fn owner_only(metadata: &fs::Metadata) -> bool {
    metadata.permissions().mode() & 0o077 == 0
}

#[cfg(unix)]
fn owner_readable(metadata: &fs::Metadata) -> bool {
    metadata.permissions().mode() & 0o400 != 0
}

#[cfg(not(unix))]
fn owner_only(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(not(unix))]
fn owner_readable(_metadata: &fs::Metadata) -> bool {
    false
}

/// Check exactly one operator-supplied credential name. This deliberately has no list variant and
/// never opens the target file; only path metadata is inspected.
pub fn credential_status(credentials_dir: &Path, name: &str) -> Result<CredentialStatus> {
    let destination = named_destination(credentials_dir, name)?;
    let entry = match fs::symlink_metadata(&destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(CredentialStatus {
                name: name.to_string(),
                registered: false,
                usable: false,
                entry_kind: "missing".into(),
                owner_only_permissions: false,
                owner_readable: false,
                credential_contents_read: false,
            })
        }
        Err(error) => return Err(error.into()),
    };

    let entry_kind = if entry.file_type().is_symlink() {
        "symlink"
    } else if entry.is_file() {
        "file"
    } else {
        "unsupported"
    };
    let target = fs::metadata(&destination);
    let (target_is_file, permissions_ok, readable) = match target {
        Ok(metadata) => (
            metadata.is_file(),
            owner_only(&metadata),
            owner_readable(&metadata),
        ),
        Err(_) => (false, false, false),
    };
    Ok(CredentialStatus {
        name: name.to_string(),
        registered: true,
        usable: target_is_file && permissions_ok && readable,
        entry_kind: if entry.file_type().is_symlink() && !target_is_file {
            "broken_symlink".into()
        } else {
            entry_kind.into()
        },
        owner_only_permissions: permissions_ok,
        owner_readable: readable,
        credential_contents_read: false,
    })
}

/// Register one operator-named existing private-key path as a symlink in the closed credential
/// namespace. The file contents are never opened or copied. Existing names are idempotent only
/// when they resolve to the same source; a conflict is never overwritten.
#[cfg(unix)]
pub fn register_existing_credential(
    credentials_dir: &Path,
    name: &str,
    source: &Path,
    dry_run: bool,
) -> Result<CredentialRegistration> {
    let destination = named_destination(credentials_dir, name)?;
    if !source.is_absolute() {
        return Err(OuroError::Validation(
            "--path must be the absolute path explicitly named by the operator".into(),
        ));
    }
    let source_metadata = fs::metadata(source).map_err(|_| {
        OuroError::Validation(
            "operator-supplied --path must be an existing regular file (contents were not read)"
                .into(),
        )
    })?;
    if !source_metadata.is_file() {
        return Err(OuroError::Validation(
            "operator-supplied --path must be a regular file (contents were not read)".into(),
        ));
    }
    if !owner_only(&source_metadata) {
        return Err(OuroError::Validation(
            "operator-supplied private key is group/world accessible; the operator must restrict its permissions before registration"
                .into(),
        ));
    }
    if !owner_readable(&source_metadata) {
        return Err(OuroError::Validation(
            "operator-supplied private key is not owner-readable (contents were not read)".into(),
        ));
    }
    let canonical_source = fs::canonicalize(source)?;

    if fs::symlink_metadata(&destination).is_ok() {
        let status = credential_status(credentials_dir, name)?;
        let same_source = fs::canonicalize(&destination)
            .map(|existing| existing == canonical_source)
            .unwrap_or(false);
        if !same_source || !status.usable {
            return Err(OuroError::Validation(format!(
                "credential name {name:?} already exists with a different or unusable entry; refusing to inspect, replace, or overwrite it"
            )));
        }
        return Ok(CredentialRegistration {
            name: name.into(),
            registered: true,
            usable: true,
            entry_kind: status.entry_kind,
            action: "already_registered_same_source".into(),
            planned: false,
            changed: false,
            credential_contents_read: false,
        });
    }

    if dry_run {
        return Ok(CredentialRegistration {
            name: name.into(),
            registered: false,
            usable: false,
            entry_kind: "planned_symlink".into(),
            action: "create_symlink".into(),
            planned: true,
            changed: false,
            credential_contents_read: false,
        });
    }

    match fs::symlink_metadata(credentials_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(OuroError::Validation(
                "credential namespace exists but is not a real directory; refusing registration"
                    .into(),
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => fs::create_dir_all(credentials_dir)?,
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(credentials_dir, fs::Permissions::from_mode(0o700))?;
    symlink(&canonical_source, &destination).map_err(|error| {
        if error.kind() == ErrorKind::AlreadyExists {
            OuroError::Validation(format!(
                "credential name {name:?} appeared concurrently; refusing to replace it"
            ))
        } else {
            error.into()
        }
    })?;
    let status = credential_status(credentials_dir, name)?;
    if !status.usable {
        return Err(OuroError::Validation(
            "credential registration did not produce a usable owner-only file".into(),
        ));
    }
    Ok(CredentialRegistration {
        name: name.into(),
        registered: true,
        usable: true,
        entry_kind: status.entry_kind,
        action: "created_symlink".into(),
        planned: false,
        changed: true,
        credential_contents_read: false,
    })
}

#[cfg(not(unix))]
pub fn register_existing_credential(
    _credentials_dir: &Path,
    _name: &str,
    _source: &Path,
    _dry_run: bool,
) -> Result<CredentialRegistration> {
    Err(OuroError::Validation(
        "credential symlink registration is supported on macOS and Linux only".into(),
    ))
}

impl<'de> Deserialize<'de> for CredentialRef {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        CredentialRef::parse(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{credential_status, register_existing_credential, CredentialRef};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    fn private_file(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"test-only-private-key-bytes").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    #[test]
    fn rejects_plain_secret_paths() {
        assert!(CredentialRef::parse("/home/operator/.ssh/id_ed25519").is_err());
        assert!(CredentialRef::parse("creds://relay1").is_ok());
    }

    #[test]
    fn resolves_to_credentials_dir_path() {
        let cred = CredentialRef::parse("creds://bp1").unwrap();
        assert_eq!(cred.name(), "bp1");
        let dir = std::path::Path::new("/home/op/.ouro/credentials");
        assert_eq!(
            cred.resolve(dir).unwrap(),
            std::path::Path::new("/home/op/.ouro/credentials/bp1")
        );
    }

    #[test]
    fn resolve_rejects_traversal_and_absolute() {
        let dir = std::path::Path::new("/home/op/.ouro/credentials");
        for bad in [
            "creds://../../root/.ssh/id_rsa",
            "creds:////etc/shadow",
            "creds://a/b",
            "creds://.hidden",
            "creds://a..b",
            "creds://white space",
            "creds://line\nfeed",
            "creds://密钥",
        ] {
            let cred = CredentialRef::parse(bad).unwrap();
            assert!(cred.resolve(dir).is_err(), "should reject {bad}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn named_registration_previews_then_symlinks_without_copying() {
        let root = std::env::temp_dir().join(format!("ouro-creds-{}", uuid::Uuid::new_v4()));
        let source_dir = root.join("operator");
        let credentials_dir = root.join("ouro/credentials");
        fs::create_dir_all(&source_dir).unwrap();
        let source = private_file(&source_dir, "id_ed25519");

        let missing = credential_status(&credentials_dir, "bp1").unwrap();
        assert!(!missing.registered && !missing.credential_contents_read);

        let preview =
            register_existing_credential(&credentials_dir, "bp1", &source, true).unwrap();
        assert!(preview.planned && !preview.changed && !preview.registered);
        assert!(!credentials_dir.exists(), "dry-run must not create the namespace");
        assert!(!preview.credential_contents_read);

        let registered =
            register_existing_credential(&credentials_dir, "bp1", &source, false).unwrap();
        assert!(registered.changed && registered.registered && registered.usable);
        let entry = credentials_dir.join("bp1");
        assert!(fs::symlink_metadata(&entry).unwrap().file_type().is_symlink());
        assert_eq!(fs::canonicalize(&entry).unwrap(), fs::canonicalize(&source).unwrap());
        assert_eq!(fs::read(&source).unwrap(), b"test-only-private-key-bytes");

        let idempotent =
            register_existing_credential(&credentials_dir, "bp1", &source, false).unwrap();
        assert!(!idempotent.changed && idempotent.registered);
        let status = credential_status(&credentials_dir, "bp1").unwrap();
        assert!(status.registered && status.usable && !status.credential_contents_read);
        assert_eq!(status.entry_kind, "symlink");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn named_registration_refuses_conflict_traversal_and_open_permissions() {
        let root = std::env::temp_dir().join(format!("ouro-creds-{}", uuid::Uuid::new_v4()));
        let source_dir = root.join("operator");
        let credentials_dir = root.join("ouro/credentials");
        fs::create_dir_all(&source_dir).unwrap();
        let first = private_file(&source_dir, "first");
        let second = private_file(&source_dir, "second");
        register_existing_credential(&credentials_dir, "bp1", &first, false).unwrap();
        assert!(register_existing_credential(&credentials_dir, "bp1", &second, false).is_err());
        assert!(register_existing_credential(&credentials_dir, "../bp1", &first, false).is_err());
        assert!(register_existing_credential(
            &credentials_dir,
            "relay1",
            std::path::Path::new("relative-key"),
            false,
        )
        .is_err());

        let open = private_file(&source_dir, "open-key");
        fs::set_permissions(&open, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(register_existing_credential(&credentials_dir, "relay1", &open, false).is_err());

        let unreadable = private_file(&source_dir, "unreadable-key");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(
            register_existing_credential(&credentials_dir, "relay1", &unreadable, false).is_err()
        );

        fs::remove_dir_all(root).unwrap();
    }
}
