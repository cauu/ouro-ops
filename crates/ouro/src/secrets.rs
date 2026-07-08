use serde::{Deserialize, Deserializer, Serialize};

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
}
