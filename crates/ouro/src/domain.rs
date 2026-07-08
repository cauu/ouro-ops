use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

use crate::{secrets::CredentialRef, OuroError, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PoolSpec {
    pub spec_version: u8,
    pub pool: Pool,
    pub topology_mode: TopologyMode,
    pub node_version: String,
    pub machines: Vec<Machine>,
    pub sync: Sync,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pool {
    pub ticker: String,
    pub network: Network,
    pub network_magic: u64,
    pub genesis_hashes: GenesisHashes,
    pub metadata_url: String,
    pub pledge_lovelace: u64,
    pub margin: f64,
    pub cost_lovelace: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Preprod,
    Preview,
}

impl Network {
    pub fn magic(self) -> u64 {
        match self {
            Network::Mainnet => 764_824_073,
            Network::Preprod => 1,
            Network::Preview => 2,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GenesisHashes {
    pub byron: Option<String>,
    pub shelley: String,
    pub alonzo: Option<String>,
    pub conway: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TopologyMode {
    P2p,
    Legacy,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Machine {
    pub id: String,
    pub role: MachineRole,
    pub public_endpoint: Option<Endpoint>,
    pub ssh: SshTarget,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MachineRole {
    Bp,
    Relay,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_ref: CredentialRef,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sync {
    pub mode: SyncMode,
    pub mithril: Option<MithrilSync>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    Mithril,
    Genesis,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MithrilSync {
    pub aggregator_endpoint: String,
    pub genesis_verification_key_ref: CredentialRef,
    pub verify_snapshot_digest: bool,
}

impl PoolSpec {
    pub fn from_json_file(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let spec: PoolSpec = serde_json::from_str(&text)?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<()> {
        if self.spec_version != 1 {
            return Err(OuroError::Validation("spec_version must be 1".to_string()));
        }
        if self.pool.network_magic != self.pool.network.magic() {
            return Err(OuroError::Validation(format!(
                "network_magic {} does not match {:?}",
                self.pool.network_magic, self.pool.network
            )));
        }
        if self.pool.ticker.len() < 3 || self.pool.ticker.len() > 5 {
            return Err(OuroError::Validation(
                "pool ticker must be 3-5 characters".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.pool.margin) {
            return Err(OuroError::Validation(
                "pool margin must be between 0 and 1".to_string(),
            ));
        }
        let mut ids = BTreeSet::new();
        let mut bp_count = 0;
        let mut relay_count = 0;
        for machine in &self.machines {
            if !ids.insert(machine.id.as_str()) {
                return Err(OuroError::Validation(format!(
                    "duplicate machine id {}",
                    machine.id
                )));
            }
            if machine.ssh.user != "ouro-exec" {
                return Err(OuroError::Validation(format!(
                    "machine {} must use ouro-exec ssh user",
                    machine.id
                )));
            }
            match machine.role {
                MachineRole::Bp => bp_count += 1,
                MachineRole::Relay => {
                    relay_count += 1;
                    if machine.public_endpoint.is_none() {
                        return Err(OuroError::Validation(format!(
                            "relay {} requires public_endpoint",
                            machine.id
                        )));
                    }
                }
            }
        }
        if bp_count != 1 {
            return Err(OuroError::Validation(
                "exactly one bp machine is required".to_string(),
            ));
        }
        if relay_count == 0 {
            return Err(OuroError::Validation(
                "at least one relay machine is required".to_string(),
            ));
        }
        if self.sync.mode == SyncMode::Mithril && self.sync.mithril.is_none() {
            return Err(OuroError::Validation(
                "mithril sync requires mithril config".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Network, PoolSpec};

    #[test]
    fn validates_network_magic() {
        let mut spec: PoolSpec = serde_json::from_str(include_str!(
            "../../../tests/fixtures/pool-spec/valid-minimal.json"
        ))
        .unwrap();
        assert!(spec.validate().is_ok());
        spec.pool.network = Network::Mainnet;
        assert!(spec.validate().is_err());
    }
}
