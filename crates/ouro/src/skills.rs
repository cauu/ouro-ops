//! S0016 p2-1/p2-6/p2-7 — access to skill assets embedded into the binary at compile time.
//!
//! The generated `EMBEDDED` slice (build.rs → `$OUT_DIR/embedded_skills.rs`) is the single
//! in-binary source for BOTH the decision layer (`<skill>/SKILL.md`, served by
//! `ouro skill show`, R2 N3) and the mechanism layer (`<skill>/scripts/*.sh`, `lib/*.sh`,
//! schemas). Nothing is fetched from disk or network at runtime for the installed binary.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/embedded_skills.rs"));
// pub static EMBEDDED: &[(&str, &[u8])] = &[ ... ];

/// Bytes of an embedded asset by its `ouro-skills`-relative path (e.g. `deploy/SKILL.md`).
pub fn asset(rel_path: &str) -> Option<&'static [u8]> {
    EMBEDDED
        .iter()
        .find(|(p, _)| *p == rel_path)
        .map(|(_, b)| *b)
}

/// The decision-layer doc (`<skill>/SKILL.md`) for a skill, as text. This is the
/// authoritative decision source the agent consumes via `ouro skill show` — NOT the prompt.
pub fn skill_doc(skill: &str) -> Option<&'static str> {
    asset(&format!("{skill}/SKILL.md")).and_then(|b| std::str::from_utf8(b).ok())
}

/// Names of all embedded skills (those that carry a `SKILL.md`), sorted.
pub fn skill_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = EMBEDDED
        .iter()
        .filter_map(|(p, _)| p.strip_suffix("/SKILL.md"))
        .collect();
    names.sort_unstable();
    names
}

/// The embedded L2 script bytes for `<skill>/scripts/<script>.sh`.
pub fn script(skill: &str, script: &str) -> Option<&'static [u8]> {
    asset(&format!("{skill}/scripts/{script}.sh"))
}

/// Materialize a skill's shell assets (its `scripts/` + the shared `lib/`) under `dest`,
/// preserving the `ouro-skills`-relative layout so `source "$ROOT/ouro-skills/lib/..."`
/// still resolves. Files are written 0600, dirs 0700 (p2-1: per-run temp is caller-owned).
pub fn extract_shell_assets(skill: &str, dest: &Path) -> io::Result<()> {
    for (rel, bytes) in EMBEDDED.iter() {
        let is_skill_script = rel.starts_with(&format!("{skill}/scripts/")) && rel.ends_with(".sh");
        let is_lib = rel.starts_with("lib/") && rel.ends_with(".sh");
        if !(is_skill_script || is_lib) {
            continue;
        }
        let target = dest.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
            set_mode(parent, 0o700);
        }
        std::fs::write(&target, bytes)?;
        set_mode(&target, 0o600);
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}
#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

/// sha256 hex of arbitrary bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex(&h.finalize())
}

/// Per-asset sha256 map (path → hex), the raw material for the bundle manifest (p2-6).
pub fn asset_hashes() -> BTreeMap<String, String> {
    EMBEDDED
        .iter()
        .map(|(p, b)| (p.to_string(), sha256_hex(b)))
        .collect()
}

/// A single digest over ALL embedded assets (path+content, deterministic order) — the
/// binary's self-describing "embedded skills hash" used in the bundle manifest.
pub fn embedded_digest() -> String {
    let mut h = Sha256::new();
    for (p, b) in EMBEDDED.iter() {
        h.update(p.as_bytes());
        h.update([0u8]);
        h.update(b);
        h.update([0u8]);
    }
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_all_six_skill_docs() {
        let names = skill_names();
        for expected in [
            "deploy",
            "kes-rotation",
            "upgrade",
            "observability",
            "runtime",
            "troubleshooting",
        ] {
            assert!(names.contains(&expected), "missing embedded skill {expected}");
        }
    }

    #[test]
    fn skill_doc_carries_front_matter_and_red_lines() {
        let doc = skill_doc("kes-rotation").expect("kes-rotation SKILL.md embedded");
        assert!(doc.starts_with("---\n"), "front matter present");
        assert!(doc.contains("skill_version"));
        assert!(doc.contains("Red Lines"));
    }

    #[test]
    fn embedded_digest_is_stable_and_nonempty() {
        let d1 = embedded_digest();
        let d2 = embedded_digest();
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }

    #[test]
    fn extract_writes_scripts_and_lib() {
        let tmp = std::env::temp_dir().join(format!("ouro-skills-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        extract_shell_assets("deploy", &tmp).unwrap();
        assert!(tmp.join("deploy/scripts/status.sh").is_file());
        assert!(tmp.join("lib/ouro-lib.sh").is_file());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
