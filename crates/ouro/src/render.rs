use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    domain::{Machine, MachineRole, Network, PoolSpec, TopologyMode},
    OuroError, Result,
};

#[derive(Debug, Clone, Serialize)]
pub struct RenderedConfig {
    pub machine: String,
    pub role: MachineRole,
    pub output_dir: PathBuf,
    pub files: Vec<PathBuf>,
    pub config: Value,
    pub topology: Value,
}

pub fn render_machine(
    spec: &PoolSpec,
    machine_id: &str,
    output_root: &Path,
) -> Result<RenderedConfig> {
    let machine = spec
        .machines
        .iter()
        .find(|candidate| candidate.id == machine_id)
        .ok_or_else(|| OuroError::Validation(format!("unknown machine {machine_id}")))?;
    let output_dir = output_root.join(machine_id);
    fs::create_dir_all(&output_dir)?;

    let config = render_node_config(spec, machine);
    let topology = render_topology(spec, machine)?;
    let config_path = output_dir.join("config.json");
    let topology_path = output_dir.join("topology.json");
    fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    fs::write(&topology_path, serde_json::to_string_pretty(&topology)?)?;

    Ok(RenderedConfig {
        machine: machine.id.clone(),
        role: machine.role,
        output_dir,
        files: vec![config_path, topology_path],
        config,
        topology,
    })
}

fn render_node_config(spec: &PoolSpec, machine: &Machine) -> Value {
    let requires_network_magic = match spec.pool.network {
        Network::Mainnet => "RequiresNoMagic",
        Network::Preprod | Network::Preview => "RequiresMagic",
    };
    json!({
        "ApplicationName": "cardano-sl",
        "ApplicationVersion": 1,
        "NodeVersion": spec.node_version,
        "OuroMachineId": machine.id,
        "OuroRole": machine.role,
        "NetworkName": spec.pool.network.as_str(),
        "RequiresNetworkMagic": requires_network_magic,
        "TestNetMagic": if spec.pool.network == Network::Mainnet {
            Value::Null
        } else {
            json!(spec.pool.network_magic)
        },
        "GenesisHashes": spec.pool.genesis_hashes,
        "TraceBlockFetchClient": true,
        "TraceChainDb": true,
        "TraceMempool": true,
        "hasEKG": 12788,
        "hasPrometheus": ["127.0.0.1", 12798],
        "PeerSharing": spec.topology_mode == TopologyMode::P2p
    })
}

fn render_topology(spec: &PoolSpec, machine: &Machine) -> Result<Value> {
    match spec.topology_mode {
        TopologyMode::P2p => render_p2p_topology(spec, machine),
        TopologyMode::Legacy => render_legacy_topology(spec, machine),
    }
}

fn render_p2p_topology(spec: &PoolSpec, machine: &Machine) -> Result<Value> {
    let relay_access_points = relay_access_points(spec)?;
    let local_roots = match machine.role {
        MachineRole::Bp => relay_access_points.clone(),
        MachineRole::Relay => relay_access_points
            .into_iter()
            .filter(|peer| {
                peer.get("address").and_then(Value::as_str)
                    != machine.public_endpoint.as_ref().map(|ep| ep.host.as_str())
            })
            .collect(),
    };
    let public_roots = if machine.role == MachineRole::Relay {
        bootstrap_peers(spec.pool.network)
    } else {
        Vec::new()
    };
    Ok(json!({
        "bootstrapPeers": public_roots,
        "localRoots": [{
            "accessPoints": local_roots,
            "advertise": false,
            "trustable": true,
            "valency": 1
        }],
        "publicRoots": [{
            "accessPoints": public_roots,
            "advertise": true
        }],
        "useLedgerAfterSlot": 0,
        "PeerSharing": machine.role == MachineRole::Relay
    }))
}

fn render_legacy_topology(spec: &PoolSpec, machine: &Machine) -> Result<Value> {
    let producers = match machine.role {
        MachineRole::Bp => relay_access_points(spec)?,
        MachineRole::Relay => bootstrap_peers(spec.pool.network),
    };
    Ok(json!({
        "Producers": producers
    }))
}

fn relay_access_points(spec: &PoolSpec) -> Result<Vec<Value>> {
    spec.machines
        .iter()
        .filter(|machine| machine.role == MachineRole::Relay)
        .map(|relay| {
            let endpoint = relay.public_endpoint.as_ref().ok_or_else(|| {
                OuroError::Validation(format!("relay {} requires public_endpoint", relay.id))
            })?;
            Ok(json!({
                "address": endpoint.host,
                "port": endpoint.port
            }))
        })
        .collect()
}

fn bootstrap_peers(network: Network) -> Vec<Value> {
    match network {
        Network::Mainnet => vec![
            json!({"address": "backbone.mainnet.emurgornd.com", "port": 3001}),
            json!({"address": "backbone.mainnet.cardanofoundation.org", "port": 3001}),
        ],
        Network::Preprod => {
            vec![json!({"address": "preprod-node.world.dev.cardano.org", "port": 30000})]
        }
        Network::Preview => {
            vec![json!({"address": "preview-node.world.dev.cardano.org", "port": 30002})]
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::domain::PoolSpec;

    use super::{render_machine, TopologyMode};

    #[test]
    fn renders_p2p_topology_with_local_roots() {
        let spec = PoolSpec::from_file(Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        assert_eq!(spec.topology_mode, TopologyMode::P2p);
        let temp = std::env::temp_dir().join(format!("ouro-render-{}", std::process::id()));
        let rendered = render_machine(&spec, "bp1", &temp).unwrap();
        let topology = rendered.topology.to_string();
        assert!(topology.contains("localRoots"));
        assert!(topology.contains("relay1.example.com"));
        assert!(rendered.files.iter().all(|path| path.exists()));
    }
}
