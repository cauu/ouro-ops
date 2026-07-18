//! Stateless package-version helpers.
//!
//! External Skill compatibility is checked by `contract.rs`. Release rollback/revocation policy
//! belongs to signed distribution metadata; this module deliberately keeps no local version floor.

pub type Ver = (u64, u64, u64);

pub fn parse(value: &str) -> Option<Ver> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub fn current() -> Ver {
    parse(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

pub fn fmt(version: Ver) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_exact_release_versions() {
        assert_eq!(parse("1.2.3"), Some((1, 2, 3)));
        for invalid in ["v1.2.3", "1.2", "1.2.3.4", "1.2.3-beta"] {
            assert_eq!(parse(invalid), None);
        }
    }

    #[test]
    fn current_matches_package_version() {
        assert_eq!(fmt(current()), env!("CARGO_PKG_VERSION"));
    }
}
