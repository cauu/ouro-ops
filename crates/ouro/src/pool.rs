use chrono::Utc;
use serde::Serialize;
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use crate::{audit::AuditStore, domain::PoolSpec, Result};

#[derive(Debug, Clone, Serialize)]
pub struct RegisterTxReport {
    pub tx_cbor_path: PathBuf,
    pub manifest_path: PathBuf,
    pub tx_hash: String,
    pub signed: bool,
    pub witnesses: Vec<String>,
    pub audit_id: String,
}

pub fn build_register_tx(
    spec: &PoolSpec,
    out_dir: &Path,
    audit_store: &AuditStore,
) -> Result<RegisterTxReport> {
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
    Ok(RegisterTxReport {
        tx_cbor_path,
        manifest_path,
        tx_hash,
        signed: false,
        witnesses: Vec::new(),
        audit_id,
    })
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
