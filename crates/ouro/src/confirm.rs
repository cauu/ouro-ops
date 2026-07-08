use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::Path,
};

use crate::{OuroError, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfirmationToken {
    pub token: String,
    pub action: String,
    pub machine: String,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ConfirmationStore {
    pub tokens: Vec<ConfirmationToken>,
}

impl ConfirmationStore {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn create(
        path: &Path,
        action: &str,
        machine: &str,
        ttl: Duration,
    ) -> Result<ConfirmationToken> {
        let mut store = Self::load(path)?;
        let expires_at = Utc::now() + ttl;
        let token = format!(
            "tok_{}",
            stable_hash(&format!(
                "{action}:{machine}:{}:{}",
                expires_at.timestamp(),
                store.tokens.len()
            ))
        );
        let entry = ConfirmationToken {
            token,
            action: action.to_string(),
            machine: machine.to_string(),
            expires_at,
            used: false,
        };
        store.tokens.push(entry.clone());
        store.save(path)?;
        Ok(entry)
    }

    pub fn consume(path: &Path, token: &str, action: &str, machine: &str) -> Result<()> {
        let mut store = Self::load(path)?;
        let entry = store
            .tokens
            .iter_mut()
            .find(|candidate| candidate.token == token)
            .ok_or_else(|| OuroError::Validation("confirmation token not found".to_string()))?;
        if entry.used {
            return Err(OuroError::Validation(
                "confirmation token already used".to_string(),
            ));
        }
        if entry.action != action || entry.machine != machine {
            return Err(OuroError::Validation(
                "confirmation token action or machine mismatch".to_string(),
            ));
        }
        if entry.expires_at < Utc::now() {
            return Err(OuroError::Validation(
                "confirmation token expired".to_string(),
            ));
        }
        entry.used = true;
        store.save(path)?;
        Ok(())
    }
}

pub fn parse_ttl(value: &str) -> Result<Duration> {
    let (number, unit) = value.split_at(value.len().saturating_sub(1));
    let amount = number
        .parse::<i64>()
        .map_err(|_| OuroError::InvalidArgs(format!("invalid ttl {value}")))?;
    match unit {
        "s" => Ok(Duration::seconds(amount)),
        "m" => Ok(Duration::minutes(amount)),
        "h" => Ok(Duration::hours(amount)),
        _ => Err(OuroError::InvalidArgs(
            "ttl must end with s, m, or h".to_string(),
        )),
    }
}

pub fn readonly_invocation_token(audit_id: &str, tool: &str) -> String {
    format!("ro_{}", stable_hash(&format!("{audit_id}:{tool}:readonly")))
}

fn stable_hash(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::{parse_ttl, ConfirmationStore};

    #[test]
    fn token_is_single_use() {
        let path = std::env::temp_dir().join(format!("ouro-confirm-{}.json", std::process::id()));
        let token =
            ConfirmationStore::create(&path, "kes-push", "bp1", parse_ttl("60s").unwrap()).unwrap();
        ConfirmationStore::consume(&path, &token.token, "kes-push", "bp1").unwrap();
        assert!(ConfirmationStore::consume(&path, &token.token, "kes-push", "bp1").is_err());
    }
}
