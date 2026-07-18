//! Execution assets and generic digest/version helpers.
//!
//! Decision documents are website inputs and are deliberately absent. The generated `EMBEDDED`
//! slice contains only mechanism assets: the target probe and schemas. Hashing and semver parsing
//! live here without depending on Skill prose.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_documents_are_not_embedded() {
        assert!(EMBEDDED
            .iter()
            .all(|(path, _)| !path.ends_with("/SKILL.md")));
    }

    #[test]
    fn embedded_digest_is_stable_and_nonempty() {
        let d1 = embedded_digest();
        let d2 = embedded_digest();
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }
}
