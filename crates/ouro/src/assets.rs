//! Execution assets and generic digest/version helpers.
//!
//! Decision documents are website inputs and are deliberately absent. The generated `EMBEDDED`
//! slice contains only mechanism assets: the target probe and schemas. Hashing and semver parsing
//! live here without depending on Skill prose.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));
// pub static EMBEDDED: &[(&str, &[u8])] = &[ ... ];

/// Bytes of an embedded execution asset by its repository-relative asset path.
pub fn asset(rel_path: &str) -> Option<&'static [u8]> {
    EMBEDDED
        .iter()
        .find(|(p, _)| *p == rel_path)
        .map(|(_, b)| *b)
}

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

/// A single digest over all embedded execution assets (path+content, deterministic order).
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

/// Digest over the subset of embedded assets whose path matches `pred` (deterministic order).
fn class_digest(pred: impl Fn(&str) -> bool) -> String {
    let mut h = Sha256::new();
    for (p, b) in EMBEDDED.iter().filter(|(p, _)| pred(p)) {
        h.update(p.as_bytes());
        h.update([0u8]);
        h.update(b);
        h.update([0u8]);
    }
    hex(&h.finalize())
}

/// Temporary package floor used by the legacy version gate until p3-2 removes that stateful path.
/// It depends only on package metadata and never on external Skill prose.
pub fn required_ouro() -> String {
    format!(">={}", env!("CARGO_PKG_VERSION"))
}

/// Parse a `>=x.y.z` (or bare `x.y.z`) constraint into a (major, minor, patch) tuple.
pub fn parse_floor(constraint: &str) -> Option<(u64, u64, u64)> {
    let v = constraint.trim_start_matches([' ', '>', '=', '^', '~', 'v']);
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it
        .next()
        .map(|p| p.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("0"))
        .unwrap_or("0")
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

/// Transitional execution-asset manifest. p3-2 replaces this legacy exhaustive taxonomy with the
/// compact public CLI contract/runner descriptor.
pub fn bundle_manifest() -> serde_json::Value {
    serde_json::json!({
        "manifest_version": 1,
        "ouro_version": env!("CARGO_PKG_VERSION"),
        "required_ouro": required_ouro(),
        "schema_hash": class_digest(|p| p.ends_with(".schema.json")),
        "embedded_digest": embedded_digest(),
        "assets": asset_hashes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_documents_are_not_embedded() {
        assert!(EMBEDDED.iter().all(|(path, _)| !path.ends_with("/SKILL.md")));
    }

    #[test]
    fn embedded_digest_is_stable_and_nonempty() {
        let d1 = embedded_digest();
        let d2 = embedded_digest();
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }

    #[test]
    fn committed_manifest_matches_embedded() {
        // p2-6/TC-4/TC-13 drift guard: editing any skill/schema without regenerating
        // packaging/bundle-manifest.json (via `ouro-ops manifest show`) fails CI here.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/bundle-manifest.json");
        let committed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("committed manifest present"))
                .expect("committed manifest is JSON");
        let actual = bundle_manifest();
        for key in ["schema_hash", "embedded_digest", "required_ouro"] {
            assert_eq!(
                committed.get(key),
                actual.get(key),
                "bundle manifest drift in {key}: run `ouro-ops manifest show > packaging/bundle-manifest.json`"
            );
        }
    }

    #[test]
    fn required_ouro_is_a_floor() {
        assert_eq!(super::parse_floor(">=1.2.3"), Some((1, 2, 3)));
        assert_eq!(super::parse_floor("0.1.0"), Some((0, 1, 0)));
        assert!(required_ouro().starts_with(">="));
    }

}
