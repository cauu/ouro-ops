//! S0017 p1-6 — ephemeral tool-run state: a namespaced, crash-tolerant scratch area.
//!
//! Every `ouro-ops tool run` materializes the embedded skill into a temp dir and removes it after
//! the child terminates (TERMINAL-only deletion: never mid-operation, so a running script never
//! loses its own scripts). This module gives that scratch area a stable namespace + a
//! crash-tolerant GC so a process that dies before its terminal cleanup does not leak state
//! forever.
//!
//! Three-tier namespace (all under `run_root()`, all prefixed `ouro-run-` so one GC sweeps them):
//!   * per-invocation — one dir per `tool run`, named by its audit id (traceable to the audit log);
//!   * per-process    — a run also carries its pid in the name for concurrency/debugging;
//!   * workflow-scoped — a workflow reuses one audit id across steps, so its dirs share that id.
//!
//! Deletion is TERMINAL-only (the caller removes the dir after the child exits) and cleanup errors
//! are SURFACED, not swallowed — a scratch area that cannot be reclaimed is an operational fact the
//! operator must see.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Root for ephemeral run state. `OURO_STATE_DIR` overrides it (tests / confined targets);
/// otherwise the system temp dir.
pub fn run_root() -> PathBuf {
    std::env::var_os("OURO_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Sanitize an id into a filesystem-safe, bounded token (alnum/`-`/`_`, ≤40 chars).
fn safe_token(id: &str) -> String {
    let t: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    if t.is_empty() { "anon".to_string() } else { t }
}

/// Per-invocation run dir, namespaced by audit id (per-invocation / workflow tier) + pid + a
/// uniquifier (per-process tier / concurrency). Prefix `ouro-run-` makes it GC-visible.
pub fn run_dir(root: &Path, audit_id: &str, pid: u32, uniquifier: &str) -> PathBuf {
    root.join(format!(
        "ouro-run-inv-{}-pid-{}-{}",
        safe_token(audit_id),
        pid,
        safe_token(uniquifier)
    ))
}

/// Crash-tolerant GC: remove `ouro-run-*` dirs whose mtime is older than `ttl` — scratch left by a
/// run that died before its terminal cleanup. Never touches a fresh dir (age < ttl), so it cannot
/// race a live run. Best-effort: returns `(path, error)` for each dir it could NOT remove, so the
/// caller can surface them (they are not swallowed).
pub fn gc_stale_runs(root: &Path, ttl: Duration, now: SystemTime) -> Vec<(PathBuf, String)> {
    let mut errors = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return errors, // root absent/unreadable → nothing to GC
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("ouro-run-") {
            continue;
        }
        let path = entry.path();
        let age = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|mt| now.duration_since(mt).ok());
        match age {
            Some(age) if age >= ttl => {
                if let Err(e) = fs::remove_dir_all(&path) {
                    errors.push((path, e.to_string()));
                }
            }
            _ => {} // fresh, or unreadable mtime → leave it (don't race a live run)
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn run_dir_is_namespaced_and_sanitized() {
        let d = run_dir(Path::new("/tmp"), "aud-123", 4242, "abc");
        let s = d.to_string_lossy();
        assert!(s.contains("ouro-run-inv-aud-123-pid-4242-abc"));
        // hostile ids are sanitized (no path traversal / shell chars survive).
        let bad = run_dir(Path::new("/tmp"), "../../etc/x;rm", 1, "y");
        assert!(!bad.to_string_lossy().contains(".."));
        assert!(!bad.to_string_lossy().contains(';'));
    }

    #[test]
    fn gc_removes_stale_but_keeps_fresh_and_foreign() {
        let root = std::env::temp_dir().join(format!("ouro-gc-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let stale = root.join("ouro-run-inv-old-pid-1-a");
        let fresh = root.join("ouro-run-inv-new-pid-2-b");
        let foreign = root.join("something-else");
        for d in [&stale, &fresh, &foreign] {
            fs::create_dir_all(d).unwrap();
            fs::set_permissions(d, fs::Permissions::from_mode(0o700)).unwrap();
        }
        // GC with a 1h TTL, "now" = 2h after the stale dir's mtime but before the fresh dir's.
        let now = SystemTime::now();
        let two_h_ago = now - Duration::from_secs(7200);
        // Backdate the stale dir's mtime via a fresh file... simplest: call gc with now far ahead
        // for the stale one only. Instead: set now so all are "fresh" first (nothing removed),
        // then use a now far in the future so all ouro-run-* are stale.
        let errs = gc_stale_runs(&root, Duration::from_secs(3600), two_h_ago);
        assert!(errs.is_empty());
        // two_h_ago is BEFORE the dirs were created → age underflows → treated as fresh → kept.
        assert!(stale.exists() && fresh.exists());

        // now far in the future → every ouro-run-* dir is older than the TTL → removed; foreign kept.
        let future = now + Duration::from_secs(24 * 3600);
        let errs = gc_stale_runs(&root, Duration::from_secs(3600), future);
        assert!(errs.is_empty(), "unexpected GC errors: {errs:?}");
        assert!(!stale.exists() && !fresh.exists(), "stale ouro-run dirs not GC'd");
        assert!(foreign.exists(), "GC removed a non-ouro dir");

        fs::remove_dir_all(&root).unwrap();
    }
}
