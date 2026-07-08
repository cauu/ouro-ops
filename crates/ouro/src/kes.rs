use chrono::Utc;
use serde::{Deserialize, Serialize};
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
pub struct KesGenerateReport {
    pub machine: String,
    pub vkey_path: PathBuf,
    pub vkey_hash: String,
    pub private_key_exported: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpCertMetadata {
    pub cert_type: String,
    pub machine: String,
    pub kes_vkey_hash: String,
    pub kes_period: u64,
    pub counter: u64,
    pub issuer_hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CounterState {
    pub machine: String,
    pub kes_vkey_hash: String,
    pub local_counter: u64,
    pub chain_counter: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KesPushReport {
    pub machine: String,
    pub cert_path: PathBuf,
    pub cert_hash: String,
    pub installed_payload: Vec<String>,
    pub audit_id: String,
}

pub fn generate_vkey(
    spec: &PoolSpec,
    machine_id: &str,
    out_dir: &Path,
) -> Result<KesGenerateReport> {
    let machine = spec
        .machines
        .iter()
        .find(|candidate| candidate.id == machine_id)
        .ok_or_else(|| OuroError::Validation(format!("unknown machine {machine_id}")))?;
    if machine.role != MachineRole::Bp {
        return Err(OuroError::Validation(
            "KES generation is only valid for bp".to_string(),
        ));
    }
    fs::create_dir_all(out_dir)?;
    let vkey_hash = stable_hash(&format!(
        "{}:{}:{}",
        spec.pool.network.as_str(),
        spec.node_version,
        machine_id
    ));
    let vkey_path = out_dir.join(format!("{machine_id}.kes.vkey.json"));
    let payload = serde_json::json!({
        "kind": "kes-vkey",
        "machine": machine_id,
        "network": spec.pool.network.as_str(),
        "vkey_hash": vkey_hash,
        "generated_at": Utc::now().to_rfc3339(),
        "private_key_exported": false
    });
    fs::write(&vkey_path, serde_json::to_string_pretty(&payload)?)?;
    Ok(KesGenerateReport {
        machine: machine_id.to_string(),
        vkey_path,
        vkey_hash,
        private_key_exported: false,
    })
}

pub fn read_counter_state(path: &Path) -> Result<CounterState> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn push_opcert(
    spec: &PoolSpec,
    machine_id: &str,
    cert_path: &Path,
    counter_path: &Path,
    confirm_token: Option<&str>,
    audit_store: &AuditStore,
) -> Result<KesPushReport> {
    validate_confirm_token(confirm_token, "kes-push", machine_id)?;
    let machine = spec
        .machines
        .iter()
        .find(|candidate| candidate.id == machine_id)
        .ok_or_else(|| OuroError::Validation(format!("unknown machine {machine_id}")))?;
    if machine.role != MachineRole::Bp {
        return Err(OuroError::Validation(
            "KES push is only valid for bp".to_string(),
        ));
    }
    let cert_text = fs::read_to_string(cert_path)?;
    let cert: OpCertMetadata = serde_json::from_str(&cert_text)?;
    let counter = read_counter_state(counter_path)?;
    validate_opcert(machine_id, &cert, &counter)?;
    let audit_id = audit_store.begin_invocation("kes/push", Some(machine_id))?;
    Ok(KesPushReport {
        machine: machine_id.to_string(),
        cert_path: cert_path.to_path_buf(),
        cert_hash: stable_hash(&cert_text),
        installed_payload: vec!["node.cert".to_string()],
        audit_id,
    })
}

fn validate_confirm_token(token: Option<&str>, action: &str, machine: &str) -> Result<()> {
    let expected = format!("confirm:{action}:{machine}");
    match token {
        Some(token) if token == expected => Ok(()),
        Some(_) => Err(OuroError::Validation(
            "confirmation token action or machine mismatch".to_string(),
        )),
        None => Err(OuroError::Validation(
            "dangerous KES push requires human-issued confirmation token".to_string(),
        )),
    }
}

fn validate_opcert(machine: &str, cert: &OpCertMetadata, counter: &CounterState) -> Result<()> {
    if cert.cert_type != "opcert" {
        return Err(OuroError::Validation(
            "KES push accepts opcert only".to_string(),
        ));
    }
    if cert.machine != machine || counter.machine != machine {
        return Err(OuroError::Validation(
            "cert/counter machine mismatch".to_string(),
        ));
    }
    if cert.kes_vkey_hash != counter.kes_vkey_hash {
        return Err(OuroError::Validation(
            "cert KES vkey hash mismatch".to_string(),
        ));
    }
    if cert.counter <= counter.local_counter || cert.counter <= counter.chain_counter {
        return Err(OuroError::Validation(
            "opcert counter must be greater than local and chain counters".to_string(),
        ));
    }
    Ok(())
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

    use super::{push_opcert, read_counter_state};

    #[test]
    fn rejects_kes_push_without_confirmation() {
        let spec = PoolSpec::from_file(Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        let audit = AuditStore::in_memory().unwrap();
        let result = push_opcert(
            &spec,
            "bp1",
            Path::new("tests/fixtures/kes/node-cert-valid.json"),
            Path::new("tests/fixtures/kes/counter-state.json"),
            None,
            &audit,
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_counter_replay() {
        let spec = PoolSpec::from_file(Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        let audit = AuditStore::in_memory().unwrap();
        let result = push_opcert(
            &spec,
            "bp1",
            Path::new("tests/fixtures/kes/node-cert-replay.json"),
            Path::new("tests/fixtures/kes/counter-state.json"),
            Some("confirm:kes-push:bp1"),
            &audit,
        );
        assert!(result.is_err());
    }

    #[test]
    fn reads_counter_state() {
        let state = read_counter_state(Path::new("tests/fixtures/kes/counter-state.json")).unwrap();
        assert_eq!(state.local_counter, 4);
        assert_eq!(state.chain_counter, 3);
    }
}
