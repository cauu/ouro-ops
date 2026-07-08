use serde::{Deserialize, Serialize};
use serde_json::json;
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

    pub fn as_str(self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Preprod => "preprod",
            Network::Preview => "preview",
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
    pub fn from_file(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let spec: PoolSpec = match path.extension().and_then(|ext| ext.to_str()) {
            Some("yaml" | "yml") => serde_yaml::from_str(&text).map_err(|err| {
                OuroError::Validation(format!("pool spec yaml parse failed: {err}"))
            })?,
            _ => serde_json::from_str(&text)?,
        };
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
        self.pool.genesis_hashes.validate()?;
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

    pub fn resolved_non_secret_plan(&self) -> serde_json::Value {
        let bp = self
            .machines
            .iter()
            .find(|machine| machine.role == MachineRole::Bp)
            .map(|machine| machine.id.as_str())
            .unwrap_or_default();
        let relays = self
            .machines
            .iter()
            .filter(|machine| machine.role == MachineRole::Relay)
            .map(|machine| {
                json!({
                    "id": machine.id,
                    "public_endpoint": machine.public_endpoint
                })
            })
            .collect::<Vec<_>>();
        json!({
            "spec_version": self.spec_version,
            "pool": {
                "ticker": self.pool.ticker,
                "network": self.pool.network.as_str(),
                "network_magic": self.pool.network_magic,
                "genesis_hashes": self.pool.genesis_hashes,
                "metadata_url": self.pool.metadata_url,
                "pledge_lovelace": self.pool.pledge_lovelace,
                "margin": self.pool.margin,
                "cost_lovelace": self.pool.cost_lovelace
            },
            "topology_mode": self.topology_mode,
            "node_version": self.node_version,
            "machines": {
                "bp": bp,
                "relays": relays,
                "count": self.machines.len()
            },
            "sync": {
                "mode": self.sync.mode,
                "mithril_enabled": self.sync.mode == SyncMode::Mithril
            },
            "secrets": {
                "policy": "redacted",
                "credential_refs_present": self.credential_ref_count()
            }
        })
    }

    fn credential_ref_count(&self) -> usize {
        let ssh_refs = self.machines.len();
        let mithril_refs = usize::from(self.sync.mithril.is_some());
        ssh_refs + mithril_refs
    }
}

impl GenesisHashes {
    fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("byron", self.byron.as_ref()),
            ("shelley", Some(&self.shelley)),
            ("alonzo", self.alonzo.as_ref()),
            ("conway", self.conway.as_ref()),
        ] {
            if let Some(value) = value {
                if value.len() < 16
                    || value.len() > 128
                    || !value.chars().all(|ch| ch.is_ascii_hexdigit())
                {
                    return Err(OuroError::Validation(format!(
                        "genesis hash {name} must be 16-128 hex characters"
                    )));
                }
            }
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

    #[test]
    fn resolved_plan_redacts_credential_refs() {
        let spec =
            PoolSpec::from_file(std::path::Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        let plan = spec.resolved_non_secret_plan();
        let text = serde_json::to_string(&plan).unwrap();
        assert!(text.contains("\"policy\":\"redacted\""));
        assert!(!text.contains("creds://"));
    }
}
