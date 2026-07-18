//! S0016 p3-2/p3-3/p3-4 — the version gate + tamper-evident monotonic anti-rollback.
//!
//! Closes R2 P0-2 (a prompt-supplied floor must never LOWER the effective requirement) and
//! R2 N1 (the anti-rollback state must be tamper-evident; erasing it must NOT reopen a
//! downgrade — it falls back to the embedded floor, not "no floor", and the reset is auditable).
//!
//! `required = max(prompt_min, embedded_skill_floor, monotonic_rollback_floor, security_floor)`.
//! The prompt value can only ever RAISE `required`. If the running binary is below `required`
//! we fail closed (a real deployment self-updates via the signed channel first; that network
//! path is infra — p2-3 — so here the gate refuses rather than silently proceeding).

use std::path::Path;

use serde_json::json;

use crate::{assets, confirm, OuroError, Result};

pub type Ver = (u64, u64, u64);

pub fn current() -> Ver {
    assets::parse_floor(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

pub fn fmt(v: Ver) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// The strictest `requires_ouro` compiled into the binary (from the embedded SKILL.md headers).
fn embedded_floor() -> Ver {
    assets::parse_floor(&assets::required_ouro()).unwrap_or((0, 0, 0))
}

/// The signed security/revocation floor (from signed release metadata). That feed is release
/// infra (p2-3); until it is wired, the embedded floor stands in — NEVER a lower value.
fn security_floor() -> Ver {
    embedded_floor()
}

/// Outcome of a passing gate; `rollback_reset` is true when the monotonic floor state was
/// missing/tampered and had to be re-established (the caller records this in the audit).
pub struct GateOutcome {
    pub required: Ver,
    pub rollback_reset: bool,
}

fn floor_path(home: &Path) -> std::path::PathBuf {
    home.join("version-floor.json")
}

/// Read the monotonic anti-rollback floor. The stored value is MAC'd with the local
/// `tool_run_secret`; a missing OR MAC-invalid file is treated as a reset — we return the
/// **embedded floor** (not `(0,0,0)`) so erasing the state can never reopen a downgrade.
fn load_floor(home: &Path, secret: &str) -> (Ver, bool) {
    let path = floor_path(home);
    let Ok(bytes) = std::fs::read(&path) else {
        return (embedded_floor(), true);
    };
    let Ok(v): std::result::Result<serde_json::Value, _> = serde_json::from_slice(&bytes) else {
        return (embedded_floor(), true);
    };
    let (Some(version), Some(mac)) = (v["version"].as_str(), v["mac"].as_str()) else {
        return (embedded_floor(), true);
    };
    if !confirm::verify_invocation_token(secret, version, mac) {
        return (embedded_floor(), true); // tampered → reset to embedded floor, flag for audit
    }
    match assets::parse_floor(version) {
        Some(ver) => (std::cmp::max(ver, embedded_floor()), false),
        None => (embedded_floor(), true),
    }
}

/// Persist the monotonic floor at `max(floor, current)` — the floor only ever rises.
fn raise_floor(home: &Path, secret: &str, floor: Ver) -> Result<()> {
    let version = fmt(floor);
    let mac = confirm::invocation_token(secret, &version);
    std::fs::create_dir_all(home)?;
    std::fs::write(
        floor_path(home),
        serde_json::to_vec(&json!({ "version": version, "mac": mac }))
            .expect("floor serializes"),
    )?;
    Ok(())
}

/// The version gate. `prompt_min` is the (untrusted) `min_ouro_version` a pasted prompt may
/// carry; it can only RAISE `required`. Fails closed if the running binary is below `required`.
pub fn gate(home: &Path, prompt_min: Option<&str>) -> Result<GateOutcome> {
    let secret = confirm::load_or_create_secret(&home.join("tool-run.secret"))?;
    let (rollback_floor, rollback_reset) = load_floor(home, &secret);
    let prompt_floor = prompt_min
        .and_then(assets::parse_floor)
        .unwrap_or((0, 0, 0));

    let required = [
        prompt_floor,
        embedded_floor(),
        rollback_floor,
        security_floor(),
    ]
    .into_iter()
    .max()
    .unwrap();

    let cur = current();
    if cur < required {
        return Err(OuroError::Validation(format!(
            "ouro {} is below required {} (max of prompt/embedded/rollback/security floors); \
             self-update via the signed channel before running",
            fmt(cur),
            fmt(required)
        )));
    }
    // Monotonic: record the current version as the new floor if it advances the stored one.
    raise_floor(home, &secret, std::cmp::max(rollback_floor, cur))?;
    Ok(GateOutcome {
        required,
        rollback_reset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_home() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ouro-ver-{}-{:?}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn prompt_min_can_only_raise_not_lower() {
        let home = tmp_home();
        // A low prompt floor never lowers below the embedded floor.
        let out = gate(&home, Some("0.0.1")).expect("passes at embedded floor");
        assert!(out.required >= embedded_floor());
        // A prompt floor above the current binary fails closed.
        let err = gate(&home, Some("999.0.0"));
        assert!(err.is_err(), "prompt above current must fail closed");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn erasing_rollback_state_falls_back_to_embedded_floor_not_zero() {
        let home = tmp_home();
        gate(&home, None).unwrap(); // establishes the floor file
        std::fs::remove_file(home.join("version-floor.json")).unwrap(); // erase it
        let secret = confirm::load_or_create_secret(&home.join("tool-run.secret")).unwrap();
        let (floor, reset) = load_floor(&home, &secret);
        assert!(reset, "erased state is flagged as a reset");
        assert_eq!(floor, embedded_floor(), "reset falls back to embedded floor, not (0,0,0)");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn tampered_floor_mac_is_rejected() {
        let home = tmp_home();
        gate(&home, None).unwrap();
        // Forge a higher version with a bogus MAC — must be treated as reset, not trusted.
        std::fs::write(
            home.join("version-floor.json"),
            br#"{"version":"5.0.0","mac":"forged"}"#,
        )
        .unwrap();
        let secret = confirm::load_or_create_secret(&home.join("tool-run.secret")).unwrap();
        let (floor, reset) = load_floor(&home, &secret);
        assert!(reset);
        assert_eq!(floor, embedded_floor(), "forged high floor is not trusted");
        std::fs::remove_dir_all(&home).ok();
    }
}
