//! S0019 p9-1 — expiring, single-use confirmation tokens for dangerous intents.
//!
//! The control side signs an opaque token with the secret provisioned during onboarding. The target
//! verifies the MAC against the exact canonical intent hash + human diff and durably appends the
//! random token id to a target-local consumed ledger before mutation. Holding the per-node gate lock
//! serializes check+consume, so the same approval cannot race two writers or be replayed later.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::{OuroError, Result};

type HmacSha256 = Hmac<Sha256>;
const PREFIX: &str = "s19c1";

pub struct VerifiedConfirmation {
    id: String,
}

pub fn current_epoch() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| OuroError::Validation("system clock is before the Unix epoch".into()))
}

fn message(id: &str, expires_at: u64, intent_hash: &str, diff: &str) -> String {
    format!("{PREFIX}\n{id}\n{expires_at}\n{intent_hash}\n{diff}")
}

fn mac(secret: &[u8], message: &str) -> Result<HmacSha256> {
    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|_| OuroError::Validation("invalid confirmation signing key".into()))?;
    mac.update(message.as_bytes());
    Ok(mac)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OuroError::Validation(
            "malformed confirmation token MAC".into(),
        ));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| OuroError::Validation("malformed confirmation token MAC".into()))
        })
        .collect()
}

pub fn mint(
    intent_hash: &str,
    diff: &str,
    secret: &[u8],
    now_epoch: u64,
    ttl_seconds: u64,
) -> Result<(String, u64)> {
    if !(30..=900).contains(&ttl_seconds) {
        return Err(OuroError::Validation(
            "S0019 confirmation ttl must be between 30s and 15m".into(),
        ));
    }
    let expires_at = now_epoch
        .checked_add(ttl_seconds)
        .ok_or_else(|| OuroError::Validation("confirmation expiry overflow".into()))?;
    let id = Uuid::new_v4().simple().to_string();
    let signature = mac(secret, &message(&id, expires_at, intent_hash, diff))?.finalize();
    Ok((
        format!(
            "{PREFIX}.{id}.{expires_at}.{}",
            hex(signature.into_bytes().as_slice())
        ),
        expires_at,
    ))
}

pub fn verify(
    token: &str,
    intent_hash: &str,
    diff: &str,
    secret: &[u8],
    now_epoch: u64,
) -> Result<VerifiedConfirmation> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 4 || parts[0] != PREFIX {
        return Err(OuroError::Validation(
            "malformed S0019 confirmation token".into(),
        ));
    }
    let id = parts[1];
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OuroError::Validation(
            "malformed S0019 confirmation token id".into(),
        ));
    }
    let expires_at = parts[2]
        .parse::<u64>()
        .map_err(|_| OuroError::Validation("malformed S0019 confirmation expiry".into()))?;
    if now_epoch > expires_at {
        return Err(OuroError::Validation(
            "S0019 confirmation token expired".into(),
        ));
    }
    let expected = decode_hex(parts[3])?;
    mac(secret, &message(id, expires_at, intent_hash, diff))?
        .verify_slice(&expected)
        .map_err(|_| {
            OuroError::Validation(
                "confirmation token does not match this exact intent + diff — refused (§2.5)"
                    .into(),
            )
        })?;
    Ok(VerifiedConfirmation { id: id.to_string() })
}

/// Durably consume a previously verified token. The caller must hold the node gate lock, which
/// makes the read-check-append sequence single-writer for this node.
pub fn consume(ledger: &Path, verified: &VerifiedConfirmation) -> Result<()> {
    if let Some(parent) = ledger.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = match fs::read_to_string(ledger) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    if existing.lines().any(|line| line == verified.id) {
        return Err(OuroError::Validation(
            "S0019 confirmation token already used".into(),
        ));
    }

    let mut options = OpenOptions::new();
    options.create(true).append(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(ledger)?;
    writeln!(file, "{}", verified.id)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(ledger, fs::Permissions::from_mode(0o600))?;
    }
    if let Some(parent) = ledger.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_bound_expiring_and_single_use() {
        let dir = std::env::temp_dir().join(format!("ouro-s19-confirm-{}", Uuid::new_v4()));
        let ledger = dir.join("bp1.used");
        let (token, _) = mint("a".repeat(64).as_str(), "restart bp1", b"secret", 100, 60).unwrap();
        let verified = verify(
            &token,
            "a".repeat(64).as_str(),
            "restart bp1",
            b"secret",
            120,
        )
        .unwrap();
        consume(&ledger, &verified).unwrap();
        assert!(consume(&ledger, &verified).is_err(), "replay must fail");
        assert!(verify(
            &token,
            "b".repeat(64).as_str(),
            "restart bp1",
            b"secret",
            120
        )
        .is_err());
        assert!(verify(
            &token,
            "a".repeat(64).as_str(),
            "restart relay1",
            b"secret",
            120
        )
        .is_err());
        assert!(verify(
            &token,
            "a".repeat(64).as_str(),
            "restart bp1",
            b"secret",
            161
        )
        .is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
