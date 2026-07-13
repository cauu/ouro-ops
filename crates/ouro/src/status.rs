use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{fs, path::Path};

use crate::{
    domain::{PoolSpec, SyncMode},
    OuroError, Result,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusSnapshot {
    pub machines: Vec<MachineStatus>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MachineStatus {
    pub id: String,
    pub tip: TipStatus,
    pub slot_lag_s: u64,
    pub peers: PeerStatus,
    pub kes: KesStatus,
    pub network_magic: u64,
    pub genesis_hash: String,
    pub sync_mode: SyncMode,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TipStatus {
    pub slot: u64,
    pub block: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PeerStatus {
    pub inbound: u32,
    pub outbound: u32,
    pub local_roots: u32,
    pub public_roots: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KesStatus {
    pub remaining_periods: u32,
    pub remaining_days: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpecDiff {
    pub machine: String,
    pub field: String,
    pub expected: serde_json::Value,
    pub actual: serde_json::Value,
}

impl StatusSnapshot {
    pub fn from_file(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let snapshot: StatusSnapshot = serde_json::from_str(&text)?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<()> {
        if self.machines.is_empty() {
            return Err(OuroError::Validation(
                "status snapshot has no machines".to_string(),
            ));
        }
        for machine in &self.machines {
            if machine.genesis_hash.len() < 16
                || !machine
                    .genesis_hash
                    .chars()
                    .all(|ch| ch.is_ascii_hexdigit())
            {
                return Err(OuroError::Validation(format!(
                    "machine {} has invalid genesis_hash",
                    machine.id
                )));
            }
        }
        Ok(())
    }

    pub fn diff_spec(&self, spec: &PoolSpec) -> Vec<SpecDiff> {
        let mut diffs = Vec::new();
        for machine in &self.machines {
            if machine.network_magic != spec.pool.network_magic {
                diffs.push(SpecDiff {
                    machine: machine.id.clone(),
                    field: "network_magic".to_string(),
                    expected: json!(spec.pool.network_magic),
                    actual: json!(machine.network_magic),
                });
            }
            if machine.genesis_hash != spec.pool.genesis_hashes.shelley {
                diffs.push(SpecDiff {
                    machine: machine.id.clone(),
                    field: "genesis_hash".to_string(),
                    expected: json!(spec.pool.genesis_hashes.shelley),
                    actual: json!(machine.genesis_hash),
                });
            }
            // p5-12: sync is operation-scoped — a spec that omits it states nothing about
            // sync mode, so there is nothing to diff against (absent ≠ mismatch).
            if let Some(sync) = &spec.sync {
                if machine.sync_mode != sync.mode {
                    diffs.push(SpecDiff {
                        machine: machine.id.clone(),
                        field: "sync_mode".to_string(),
                        expected: json!(sync.mode),
                        actual: json!(machine.sync_mode),
                    });
                }
            }
        }
        diffs
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::domain::PoolSpec;

    use super::StatusSnapshot;

    #[test]
    fn reports_empty_diff_for_matching_snapshot() {
        let spec = PoolSpec::from_file(Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        let snapshot =
            StatusSnapshot::from_file(Path::new("tests/fixtures/status/healthy-preprod.json"))
                .unwrap();
        assert!(snapshot.diff_spec(&spec).is_empty());
    }
}
