//! SSH agent / keychain related helpers.

use std::process::Command;

use crate::error::AppError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SshKeyInfo {
    pub bits: Option<u32>,
    pub fingerprint: String,
    pub comment: String,
    pub key_type: String,
}

/// Check whether a fingerprint exists in ssh-agent.
pub fn verify_ssh_agent_key(fingerprint: &str) -> Result<bool, AppError> {
    let keys = ssh_agent_list_keys()?;
    Ok(keys.iter().any(|k| k.fingerprint == fingerprint))
}

/// Prompt user to add key into ssh-agent / keychain.
pub fn prompt_add_key(key_path: &str) -> Result<(), AppError> {
    let status = Command::new("ssh-add")
        .arg("--apple-use-keychain")
        .arg(key_path)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Internal(format!(
            "ssh-add failed with status: {status}"
        )))
    }
}

/// List loaded keys from ssh-agent.
pub fn ssh_agent_list_keys() -> Result<Vec<SshKeyInfo>, AppError> {
    let output = Command::new("ssh-add").arg("-l").output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let text = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };

    if !output.status.success() {
        if text.contains("The agent has no identities") {
            return Ok(Vec::new());
        }
        return Err(AppError::Internal(format!(
            "ssh-add -l failed: {}",
            text.trim()
        )));
    }

    Ok(parse_ssh_add_list_output(&text))
}

fn parse_ssh_add_list_output(output: &str) -> Vec<SshKeyInfo> {
    let mut result = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.contains("The agent has no identities") {
            continue;
        }
        if let Some(info) = parse_key_line(line) {
            result.push(info);
        }
    }
    result
}

fn parse_key_line(line: &str) -> Option<SshKeyInfo> {
    let mut parts = line.split_whitespace();
    let bits = parts.next()?.parse::<u32>().ok();
    let fingerprint = parts.next()?.to_string();
    let rest = parts.collect::<Vec<_>>().join(" ");
    if rest.is_empty() {
        return Some(SshKeyInfo {
            bits,
            fingerprint,
            comment: String::new(),
            key_type: String::new(),
        });
    }

    let (comment, key_type) = split_comment_and_type(&rest);
    Some(SshKeyInfo {
        bits,
        fingerprint,
        comment,
        key_type,
    })
}

fn split_comment_and_type(rest: &str) -> (String, String) {
    if let Some(start) = rest.rfind(" (") {
        if rest.ends_with(')') && start + 3 <= rest.len() {
            let comment = rest[..start].trim().to_string();
            let key_type = rest[start + 2..rest.len() - 1].trim().to_string();
            return (comment, key_type);
        }
    }
    (rest.trim().to_string(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ssh_add_output() {
        let output = "3072 SHA256:abc123 user@host (RSA)\n256 SHA256:def456 /Users/me/.ssh/id_ed25519 (ED25519)";
        let parsed = parse_ssh_add_list_output(output);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].bits, Some(3072));
        assert_eq!(parsed[0].fingerprint, "SHA256:abc123");
        assert_eq!(parsed[0].comment, "user@host");
        assert_eq!(parsed[0].key_type, "RSA");
        assert_eq!(parsed[1].key_type, "ED25519");
    }

    #[test]
    fn tc_mch_010_ssh_agent_list_keys_returns_fingerprints() {
        let output =
            "3072 SHA256:fp1 user@host (RSA)\n256 SHA256:fp2 /Users/me/.ssh/id_ed25519 (ED25519)";
        let parsed = parse_ssh_add_list_output(output);
        let fingerprints: Vec<String> = parsed.into_iter().map(|k| k.fingerprint).collect();
        assert_eq!(
            fingerprints,
            vec!["SHA256:fp1".to_string(), "SHA256:fp2".to_string()]
        );
    }

    #[test]
    fn parse_empty_agent_output() {
        let output = "The agent has no identities.";
        let parsed = parse_ssh_add_list_output(output);
        assert!(parsed.is_empty());
    }
}
