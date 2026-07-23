//! S0027 Fleet Deploy contracts shared by Inspect, Apply and Check.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::convention::{Allowlist, DeployBootstrapContract, DeployNetworkContract};
use crate::domain::{MachineRole, Network};
use crate::readiness::NodeLifecycle;
use crate::{OuroError, Result};

pub const TOPOLOGY_DESTINATION: &str = "/ouro/topology.json";
pub const DATA_DESTINATION: &str = "/data/db";
pub const IPC_DESTINATION: &str = "/ipc";
pub const KEYS_DESTINATION: &str = "/opt/cardano/config/keys";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeployMountPolicy {
    pub destination: &'static str,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignedDeploySelection {
    pub repository: String,
    pub platform: String,
    pub release: String,
    pub oci_index_digest: String,
    pub platform_manifest_digest: String,
    pub image_config_digest: String,
    pub contract_id: String,
    pub bootstrap: DeployBootstrapContract,
    pub network: DeployNetworkContract,
}

pub fn select_signed_deploy(
    policy: &Allowlist,
    platform: &str,
    network: Network,
    expected_genesis_hash: &str,
) -> Result<SignedDeploySelection> {
    let (layout, image, bootstrap) = policy.recommended_deploy_for(platform)?;
    let network_name = network.as_str();
    let network_contract = bootstrap.networks.get(network_name).ok_or_else(|| {
        OuroError::Validation(format!(
            "signed Fleet Deploy contract has no {network_name} bootstrap facts"
        ))
    })?;
    if network_contract.genesis_hash != expected_genesis_hash {
        return Err(OuroError::Validation(format!(
            "pool spec genesis {} does not match signed {network_name} image genesis {}",
            expected_genesis_hash, network_contract.genesis_hash
        )));
    }
    Ok(SignedDeploySelection {
        repository: policy.repository.clone(),
        platform: platform.to_string(),
        release: image.release.clone(),
        oci_index_digest: image.oci_index_digest.clone(),
        platform_manifest_digest: image.platform_manifest_digest.clone(),
        image_config_digest: image.image_config_digest.clone(),
        contract_id: layout.contract_id.clone(),
        bootstrap: bootstrap.clone(),
        network: network_contract.clone(),
    })
}

pub fn desired_environment(network: Network) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("CARDANO_BLOCK_PRODUCER", "false".into()),
        ("CARDANO_DATABASE_PATH", DATA_DESTINATION.into()),
        ("CARDANO_NETWORK", network.as_str().into()),
        ("CARDANO_SOCKET_PATH", "/ipc/node.socket".into()),
        ("CARDANO_TOPOLOGY", TOPOLOGY_DESTINATION.into()),
        ("RESTORE_SNAPSHOT", "true".into()),
    ])
}

pub fn lifecycle_for(role: MachineRole) -> NodeLifecycle {
    match role {
        MachineRole::Bp => NodeLifecycle::Bootstrap,
        MachineRole::Relay => NodeLifecycle::Operational,
    }
}

pub fn selective_mount_policy(role: MachineRole) -> Vec<DeployMountPolicy> {
    let mut mounts = vec![
        DeployMountPolicy {
            destination: DATA_DESTINATION,
            read_only: false,
        },
        DeployMountPolicy {
            destination: IPC_DESTINATION,
            read_only: false,
        },
        DeployMountPolicy {
            destination: TOPOLOGY_DESTINATION,
            read_only: true,
        },
    ];
    if role == MachineRole::Bp {
        mounts.push(DeployMountPolicy {
            destination: KEYS_DESTINATION,
            read_only: false,
        });
    }
    mounts.sort_by_key(|mount| mount.destination);
    mounts
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/releases.json"));

    #[test]
    fn signed_deploy_selection_binds_exact_image_and_network_facts() {
        let policy = Allowlist::release_document(RELEASES).unwrap();
        let selected = select_signed_deploy(
            &policy,
            "linux/amd64",
            Network::Mainnet,
            "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81",
        )
        .unwrap();
        assert_eq!(selected.release, "11.0.1-1");
        assert_eq!(
            selected.image_config_digest,
            "sha256:0bb21e45159327c4e6109704df256c3c297c725a4b2cdf6d0e1899e3a9df468f"
        );
        assert_eq!(selected.network.config_sha256.len(), 64);
        assert_eq!(
            selected.bootstrap.database_marker,
            "/data/db/protocolMagicId"
        );
        assert_eq!(selected.bootstrap.metrics.host_ip, "127.0.0.1");
        assert!(
            select_signed_deploy(&policy, "linux/amd64", Network::Mainnet, &"0".repeat(64),)
                .is_err()
        );
    }

    #[test]
    fn fresh_shape_keeps_image_config_visible_and_bp_keys_writable() {
        let environment = desired_environment(Network::Preview);
        assert_eq!(environment.len(), 6);
        assert_eq!(environment["CARDANO_BLOCK_PRODUCER"], "false");
        assert_eq!(environment["RESTORE_SNAPSHOT"], "true");

        let relay = selective_mount_policy(MachineRole::Relay);
        let bp = selective_mount_policy(MachineRole::Bp);
        for mounts in [&relay, &bp] {
            assert!(mounts
                .iter()
                .all(|mount| mount.destination != "/opt/cardano/config"));
        }
        assert!(!relay
            .iter()
            .any(|mount| mount.destination == KEYS_DESTINATION));
        assert!(bp
            .iter()
            .any(|mount| { mount.destination == KEYS_DESTINATION && !mount.read_only }));
        assert_eq!(lifecycle_for(MachineRole::Bp), NodeLifecycle::Bootstrap);
        assert_eq!(
            lifecycle_for(MachineRole::Relay),
            NodeLifecycle::Operational
        );
    }
}
