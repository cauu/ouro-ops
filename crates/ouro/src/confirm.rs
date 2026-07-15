use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{fs, path::Path};
use uuid::Uuid;

use crate::{OuroError, Result};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfirmationToken {
    pub token: String,
    pub action: String,
    pub machine: String,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
    /// S0017 p2-5b — optional target fingerprint the human approved (supervision
    /// mode + unit/container id + image digest, hashed by detect/runtime). When set,
    /// consumption requires the LIVE fingerprint to equal it, so a token approved for
    /// one target cannot drive an action on a different/changed one (review P1 + TOCTOU).
    /// Absent = legacy action+machine binding (rollback / kes-push), unchanged.
    #[serde(default)]
    pub evidence: Option<String>,
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
        evidence: Option<&str>,
    ) -> Result<ConfirmationToken> {
        let mut store = Self::load(path)?;
        let expires_at = Utc::now() + ttl;
        // Token entropy comes from a CSPRNG (uuid v4 is backed by getrandom), NOT a
        // hash of guessable inputs (action/machine/expiry/len) — otherwise an agent
        // could enumerate candidates offline and forge a valid token.
        let token = format!("tok_{}", Uuid::new_v4().simple());
        let entry = ConfirmationToken {
            token,
            action: action.to_string(),
            machine: machine.to_string(),
            expires_at,
            used: false,
            evidence: evidence.map(str::to_string),
        };
        store.tokens.push(entry.clone());
        store.save(path)?;
        Ok(entry)
    }

    /// Consume a token, binding it to `(action, machine)` and — when the token carries an
    /// approved `evidence` fingerprint — to the LIVE fingerprint supplied by the caller.
    /// A token WITH evidence requires a matching `evidence` argument (missing or different
    /// is refused); a token WITHOUT evidence keeps the legacy action+machine behavior.
    pub fn consume(
        path: &Path,
        token: &str,
        action: &str,
        machine: &str,
        evidence: Option<&str>,
    ) -> Result<()> {
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
        // Target-fingerprint binding (p2-5b): an evidence-bound token only fires against the
        // exact target the human approved, and only while it still matches (TOCTOU).
        if let Some(approved) = &entry.evidence {
            match evidence {
                Some(live) if live == approved => {}
                Some(_) => {
                    return Err(OuroError::Validation(
                        "confirmation token runtime evidence mismatch (target changed since approval)".to_string(),
                    ))
                }
                None => {
                    return Err(OuroError::Validation(
                        "confirmation token requires runtime evidence but none was supplied".to_string(),
                    ))
                }
            }
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

/// Load (or lazily create) the per-home signing secret used to bind an
/// `ouro-ops tool run` invocation to its audit context. The file is created `0600` so
/// only the executing user can read it; a diagnostic principal without read access
/// to it cannot forge a valid invocation token.
pub fn load_or_create_secret(path: &Path) -> Result<String> {
    if let Ok(existing) = fs::read_to_string(path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let secret = Uuid::new_v4().simple().to_string();
    fs::write(path, &secret)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(secret)
}

/// Signed token binding an audit invocation id to the local signing secret. It is a
/// keyed MAC (HMAC-SHA256, secret = key, audit id = message), injected into
/// `ouro-ops tool run` child scripts as `OURO_INVOCATION_TOKEN`; the scripts verify it
/// (via `ouro-ops tool verify-context`), so a fabricated `OURO_AUDIT_ID` env var alone
/// cannot satisfy the L2 write gate. Bound to the audit id only (not the tool name)
/// so an orchestrator batch that re-labels `OURO_TOOL_NAME` for sub-steps keeps a
/// valid token.
pub fn invocation_token(secret: &str, audit_id: &str) -> String {
    format!("inv_{}", hex_encode(&mac_bytes(secret, audit_id)))
}

/// Constant-time verification of an invocation token against `(secret, audit_id)`.
pub fn verify_invocation_token(secret: &str, audit_id: &str, token: &str) -> bool {
    let Some(hex) = token.strip_prefix("inv_") else {
        return false;
    };
    let Some(bytes) = hex_decode(hex) else {
        return false;
    };
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(audit_id.as_bytes());
    mac.verify_slice(&bytes).is_ok()
}

fn mac_bytes(secret: &str, audit_id: &str) -> Vec<u8> {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(audit_id.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() & 1 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{invocation_token, parse_ttl, verify_invocation_token, ConfirmationStore};

    #[test]
    fn invocation_token_is_a_verifiable_mac() {
        let token = invocation_token("secret-key", "audit-123");
        assert!(token.starts_with("inv_"));
        assert!(verify_invocation_token("secret-key", "audit-123", &token));
        // Wrong secret, wrong audit id, and a tampered token all fail.
        assert!(!verify_invocation_token("other-secret", "audit-123", &token));
        assert!(!verify_invocation_token("secret-key", "audit-999", &token));
        assert!(!verify_invocation_token("secret-key", "audit-123", "inv_deadbeef"));
        assert!(!verify_invocation_token("secret-key", "audit-123", "not-a-token"));
    }

    #[test]
    fn token_is_single_use() {
        let path = std::env::temp_dir().join(format!("ouro-confirm-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let token =
            ConfirmationStore::create(&path, "kes-push", "bp1", parse_ttl("60s").unwrap(), None)
                .unwrap();
        ConfirmationStore::consume(&path, &token.token, "kes-push", "bp1", None).unwrap();
        assert!(ConfirmationStore::consume(&path, &token.token, "kes-push", "bp1", None).is_err());
    }

    #[test]
    fn evidence_bound_token_requires_matching_live_fingerprint() {
        let path =
            std::env::temp_dir().join(format!("ouro-confirm-ev-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let token = ConfirmationStore::create(
            &path,
            "runtime/restart",
            "bp1",
            parse_ttl("60s").unwrap(),
            Some("fp_abc123"),
        )
        .unwrap();
        assert_eq!(token.evidence.as_deref(), Some("fp_abc123"));
        // Wrong live fingerprint (target changed since approval) is refused and does NOT
        // burn the token.
        assert!(ConfirmationStore::consume(
            &path, &token.token, "runtime/restart", "bp1", Some("fp_DIFFERENT")
        )
        .is_err());
        // Missing live fingerprint on an evidence-bound token is refused.
        assert!(
            ConfirmationStore::consume(&path, &token.token, "runtime/restart", "bp1", None).is_err()
        );
        // Matching fingerprint fires exactly once.
        ConfirmationStore::consume(&path, &token.token, "runtime/restart", "bp1", Some("fp_abc123"))
            .unwrap();
        assert!(ConfirmationStore::consume(
            &path, &token.token, "runtime/restart", "bp1", Some("fp_abc123")
        )
        .is_err());
    }
}
