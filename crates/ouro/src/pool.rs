use chrono::Utc;
use serde::Serialize;
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use crate::{
    audit::AuditStore,
    domain::{MachineRole, PoolSpec},
    OuroError, Result,
};

#[derive(Debug, Clone, Serialize)]
pub struct RegisterTxReport {
    pub tx_cbor_path: PathBuf,
    pub manifest_path: PathBuf,
    pub tx_hash: String,
    pub signed: bool,
    pub witnesses: Vec<String>,
    pub audit_id: String,
}

/// Offline DRAFT manifest of a pool registration — a local, node-less preview of the declared
/// pool parameters (ticker/pledge/margin/cost). It does NOT build a submittable transaction.
///
/// The REAL, submittable registration is the staged cold-sign flow (S0017 p4-2), which needs a
/// live node + chain snapshot and therefore runs as dispatched L2 scripts:
/// Build the unsigned pool-registration transaction payload. S0027 removes the former Deploy
/// command/operation wrappers; this pure builder remains available only through the Pool surface.
pub fn build_register_tx(
    spec: &PoolSpec,
    out_dir: &Path,
    audit_store: &AuditStore,
) -> Result<RegisterTxReport> {
    // p5-12: registration is the consumer of the optional ticker/metadata/economics — fail
    // closed here (json! would otherwise serialize absent fields as null into the manifest).
    spec.registration_fields()?;
    fs::create_dir_all(out_dir)?;
    let audit_id = audit_store.begin_invocation("pool/register-tx", None)?;
    let tx_hash = stable_hash(&serde_json::to_string(&spec.resolved_non_secret_plan())?);
    let tx_cbor_path = out_dir.join("register-tx.unsigned.cbor");
    let manifest_path = out_dir.join("register-tx.manifest.json");
    let cbor_hex = format!("84a40081825820{tx_hash}01828200581c{tx_hash}a0f5f6");
    fs::write(&tx_cbor_path, cbor_hex)?;
    let manifest = serde_json::json!({
        "kind": "pool-register-tx-draft",
        "created_at": Utc::now().to_rfc3339(),
        "network": spec.pool.network.as_str(),
        "ticker": spec.pool.ticker,
        "metadata_url": spec.pool.metadata_url,
        "pledge_lovelace": spec.pool.pledge_lovelace,
        "margin": spec.pool.margin,
        "cost_lovelace": spec.pool.cost_lovelace,
        "tx_cbor_path": tx_cbor_path,
        "tx_hash": tx_hash,
        "signed": false,
        "witnesses": []
    });
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    audit_store.finish_invocation(&audit_id, "pool/register-tx")?;
    Ok(RegisterTxReport {
        tx_cbor_path,
        manifest_path,
        tx_hash,
        signed: false,
        witnesses: Vec::new(),
        audit_id,
    })
}

/// Read-only pool overview — the structured-JSON replacement for the retired
/// Delegators/staking UI (§2.4/§2.7). Pool parameters and relay endpoints come from
/// the spec; point-in-time staking facts (active stake, delegators, saturation) are
/// merged from an optional snapshot JSON (`--snapshot`, e.g. a Koios query result),
/// so the command renders fully offline and network params stay consistent with the
/// spec's declared network.
pub fn overview(spec: &PoolSpec, snapshot_path: Option<&Path>) -> Result<serde_json::Value> {
    let relays: Vec<_> = spec
        .machines
        .iter()
        .filter(|machine| machine.role == MachineRole::Relay)
        .map(|relay| {
            serde_json::json!({
                "id": relay.id,
                "public_endpoint": relay.public_endpoint,
            })
        })
        .collect();

    let staking = match snapshot_path {
        Some(path) => {
            let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(path)?)?;
            let snapshot_network = value.get("network").and_then(serde_json::Value::as_str);
            if let Some(network) = snapshot_network {
                if network != spec.pool.network.as_str() {
                    return Err(OuroError::Validation(format!(
                        "staking snapshot network {network} does not match spec network {}",
                        spec.pool.network.as_str()
                    )));
                }
            }
            value
        }
        None => {
            serde_json::json!({ "source": "spec-only", "note": "no live staking snapshot provided" })
        }
    };

    Ok(serde_json::json!({
        "pool": {
            "ticker": spec.pool.ticker,
            "network": spec.pool.network.as_str(),
            "network_magic": spec.pool.network_magic,
            "pledge_lovelace": spec.pool.pledge_lovelace,
            "margin": spec.pool.margin,
            "cost_lovelace": spec.pool.cost_lovelace,
            "metadata_url": spec.pool.metadata_url,
        },
        "relays": relays,
        "staking": staking,
    }))
}

fn stable_hash(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{audit::AuditStore, domain::PoolSpec};

    use super::build_register_tx;

    #[test]
    fn overview_is_readonly_and_network_consistent() {
        let spec = PoolSpec::from_file(Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        let overview = super::overview(&spec, None).unwrap();
        assert_eq!(
            overview["pool"]["ticker"],
            serde_json::json!(spec.pool.ticker)
        );
        assert_eq!(
            overview["pool"]["network"],
            serde_json::json!(spec.pool.network.as_str())
        );
        assert!(!overview["relays"].as_array().unwrap().is_empty());
        let text = serde_json::to_string(&overview).unwrap();
        assert!(!text.contains("creds://"));
    }

    #[test]
    fn register_tx_manifest_is_unsigned() {
        let spec = PoolSpec::from_file(Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        let audit = AuditStore::in_memory().unwrap();
        let out = std::env::temp_dir().join(format!("ouro-register-{}", std::process::id()));
        let report = build_register_tx(&spec, &out, &audit).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(report.manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["signed"], false);
        assert_eq!(manifest["witnesses"].as_array().unwrap().len(), 0);
    }
}
