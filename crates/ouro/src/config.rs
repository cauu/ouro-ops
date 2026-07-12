use std::{
    env,
    path::{Path, PathBuf},
};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ConfigPaths {
    pub home: PathBuf,
    pub credentials_dir: PathBuf,
    pub staging_dir: PathBuf,
    pub audit_db: PathBuf,
    pub confirmations: PathBuf,
    pub tool_run_secret: PathBuf,
    /// S0017 p3-2/p3-3 — pinned target host keys (a known_hosts file). `ouro-ops init` pins the
    /// target's key here on first connect; per-op dispatch enforces it with
    /// StrictHostKeyChecking=yes, so a swapped host key is rejected (no accept-new TOFU).
    pub known_hosts: PathBuf,
    pub legacy_db: Option<PathBuf>,
}

impl ConfigPaths {
    pub fn discover() -> Self {
        let home = env::var_os("OURO_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".ouro")))
            .unwrap_or_else(|| PathBuf::from(".ouro"));
        let legacy_db = env::var_os("OURO_LEGACY_DB").map(PathBuf::from);
        Self {
            credentials_dir: home.join("credentials"),
            staging_dir: home.join("staging"),
            audit_db: home.join("audit.sqlite3"),
            confirmations: home.join("confirmations.json"),
            tool_run_secret: home.join("tool-run.secret"),
            known_hosts: home.join("known_hosts"),
            home,
            legacy_db,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigPaths;

    #[test]
    fn defaults_to_ouro_home_without_desktop_runtime_paths() {
        let paths = ConfigPaths::discover();
        let joined = paths.home.to_string_lossy();
        assert!(joined.contains(".ouro") || joined == ".ouro");
    }
}
