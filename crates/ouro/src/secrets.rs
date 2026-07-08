use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

use crate::{OuroError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialRef(String);

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

    /// Resolve to the local credential file under the credentials dir (never inlines
    /// the secret; only yields a path the caller reads at use-time). The file itself is
    /// provisioned by p1-4.
    pub fn resolve(&self, credentials_dir: &Path) -> PathBuf {
        credentials_dir.join(self.name())
    }
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
    use super::CredentialRef;

    #[test]
    fn rejects_plain_secret_paths() {
        assert!(CredentialRef::parse("/home/operator/.ssh/id_ed25519").is_err());
        assert!(CredentialRef::parse("creds://relay1").is_ok());
    }

    #[test]
    fn resolves_to_credentials_dir_path() {
        let cred = CredentialRef::parse("creds://bp1").unwrap();
        assert_eq!(cred.name(), "bp1");
        let path = cred.resolve(std::path::Path::new("/home/op/.ouro/credentials"));
        assert_eq!(path, std::path::Path::new("/home/op/.ouro/credentials/bp1"));
    }
}
