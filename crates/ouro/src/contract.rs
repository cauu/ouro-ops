//! Pure compatibility boundary between an external decision Skill and this CLI.

use serde::Serialize;

/// Exact public command/argument contract implemented by this release.
pub const CLI_CONTRACT: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Compatibility {
    pub ouro_version: &'static str,
    pub cli_contract: u64,
    pub requires_ouro: String,
    pub requires_contract: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub detail: String,
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn parse_requirement(value: &str) -> Option<(u64, u64, u64)> {
    value.strip_prefix(">=").and_then(parse_version)
}

/// Compare requirements using only compile-time constants and arguments. This function performs no
/// filesystem, credential, environment, audit, confirmation, candidate, network, or SSH access.
pub fn check(requires_ouro: &str, requires_contract: &str) -> Result<Compatibility, Refusal> {
    let required_version = parse_requirement(requires_ouro).ok_or_else(|| Refusal {
        code: "malformed_ouro_requirement",
        detail: "--requires-ouro must be exactly >=MAJOR.MINOR.PATCH (for example >=0.1.0)"
            .to_string(),
    })?;
    let required_contract = requires_contract
        .parse::<u64>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| Refusal {
            code: "malformed_contract_requirement",
            detail: "--requires-contract must be a positive integer".to_string(),
        })?;
    let current_version = parse_version(env!("CARGO_PKG_VERSION")).ok_or_else(|| Refusal {
        code: "invalid_cli_version",
        detail: "this CLI has an invalid package version and cannot establish compatibility"
            .to_string(),
    })?;

    if current_version < required_version {
        return Err(Refusal {
            code: "ouro_version_too_old",
            detail: format!(
                "Skill requires ouro-ops {requires_ouro}, but this CLI is {}; install a compatible ouro-ops release before continuing",
                env!("CARGO_PKG_VERSION")
            ),
        });
    }
    if required_contract != CLI_CONTRACT {
        return Err(Refusal {
            code: "cli_contract_mismatch",
            detail: format!(
                "Skill requires CLI contract {required_contract}, but this CLI implements exact contract {CLI_CONTRACT}; install the matching ouro-ops release before continuing"
            ),
        });
    }

    Ok(Compatibility {
        ouro_version: env!("CARGO_PKG_VERSION"),
        cli_contract: CLI_CONTRACT,
        requires_ouro: requires_ouro.to_string(),
        requires_contract: required_contract,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_current_floor_and_exact_contract() {
        let result = check(">=0.1.0", "1").unwrap();
        assert_eq!(result.cli_contract, 1);
    }

    #[test]
    fn refuses_old_cli_and_contract_mismatch() {
        assert_eq!(
            check(">=999.0.0", "1").unwrap_err().code,
            "ouro_version_too_old"
        );
        assert_eq!(
            check(">=0.1.0", "2").unwrap_err().code,
            "cli_contract_mismatch"
        );
    }

    #[test]
    fn requirement_syntax_is_closed() {
        for invalid in ["0.1.0", ">0.1.0", ">=0.1", ">=0.1.0-beta", ">= 0.1.0"] {
            assert_eq!(
                check(invalid, "1").unwrap_err().code,
                "malformed_ouro_requirement"
            );
        }
        for invalid in ["", "0", "1.0", "one"] {
            assert_eq!(
                check(">=0.1.0", invalid).unwrap_err().code,
                "malformed_contract_requirement"
            );
        }
    }
}
