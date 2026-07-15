use chrono::Utc;
use ed25519_dalek::{Signature, VerifyingKey};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOperationalCertificate {
    pub hot_kes_verification_key: Vec<u8>,
    pub cold_verification_key: [u8; 32],
    pub counter: u64,
    pub kes_period: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextEnvelope {
    #[serde(rename = "type")]
    envelope_type: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "cborHex")]
    cbor_hex: String,
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OuroError::Validation("text-envelope cborHex is malformed".into()));
    }
    (0..value.len()).step_by(2).map(|index| {
        u8::from_str_radix(&value[index..index + 2], 16)
            .map_err(|_| OuroError::Validation("text-envelope cborHex is malformed".into()))
    }).collect()
}

fn cbor_u64(value: &serde_cbor::Value, field: &str) -> Result<u64> {
    match value {
        serde_cbor::Value::Integer(integer) => {
            u64::try_from(*integer).map_err(|_| {
                OuroError::Validation(format!("operational certificate {field} is out of range"))
            })
        }
        _ => Err(OuroError::Validation(format!(
            "operational certificate {field} is not an integer"
        ))),
    }
}

/// Parse the canonical Cardano API text envelope. `OperationalCertificate` serializes as the CBOR
/// tuple `(OCert, cold_vkey)` and `OCert` as `[hot_kes_vkey, counter, kes_period, signature]`.
pub fn parse_operational_certificate(bytes: &[u8]) -> Result<ParsedOperationalCertificate> {
    let envelope: TextEnvelope = serde_json::from_slice(bytes).map_err(|error| {
        OuroError::Validation(format!("operational certificate envelope is malformed: {error}"))
    })?;
    let _ = envelope.description;
    if envelope.envelope_type != "NodeOperationalCertificate" {
        return Err(OuroError::Validation(
            "artifact is not a NodeOperationalCertificate".into(),
        ));
    }
    let value: serde_cbor::Value = serde_cbor::from_slice(&decode_hex(&envelope.cbor_hex)?)
        .map_err(|error| OuroError::Validation(format!("operational certificate CBOR is malformed: {error}")))?;
    let outer = match value {
        serde_cbor::Value::Array(values) if values.len() == 2 => values,
        _ => return Err(OuroError::Validation("operational certificate CBOR has the wrong outer shape".into())),
    };
    let cert = match &outer[0] {
        serde_cbor::Value::Array(values) if values.len() == 4 => values,
        _ => return Err(OuroError::Validation("operational certificate CBOR has the wrong OCert shape".into())),
    };
    let hot = match &cert[0] {
        serde_cbor::Value::Bytes(bytes) if !bytes.is_empty() => bytes.clone(),
        _ => return Err(OuroError::Validation("operational certificate has no hot KES verification key".into())),
    };
    let cold: [u8; 32] = match &outer[1] {
        serde_cbor::Value::Bytes(bytes) => bytes.as_slice().try_into().map_err(|_| {
            OuroError::Validation(
                "operational certificate cold verification key must be 32 bytes".into(),
            )
        })?,
        _ => {
            return Err(OuroError::Validation(
                "operational certificate has no cold verification key".into(),
            ))
        }
    };
    let signature = match &cert[3] {
        serde_cbor::Value::Bytes(bytes) => Signature::from_slice(bytes).map_err(|_| {
            OuroError::Validation("operational certificate signature must be 64 bytes".into())
        })?,
        _ => {
            return Err(OuroError::Validation(
                "operational certificate has no cold-key signature".into(),
            ))
        }
    };
    let counter = cbor_u64(&cert[1], "counter")?;
    let kes_period = cbor_u64(&cert[2], "KES period")?;

    // Cardano's OCertSignable is deliberately not the CBOR tuple: the protocol signs the raw KES
    // verification-key bytes followed by the counter and start period as big-endian Word64s.
    // Verify here because `cardano-cli query kes-period-info` checks the live counter/window but
    // does not authenticate this signature; a bad certificate can otherwise survive until a
    // block-production slot.
    let mut signable = Vec::with_capacity(hot.len() + 16);
    signable.extend_from_slice(&hot);
    signable.extend_from_slice(&counter.to_be_bytes());
    signable.extend_from_slice(&kes_period.to_be_bytes());
    VerifyingKey::from_bytes(&cold)
        .and_then(|key| key.verify_strict(&signable, &signature))
        .map_err(|_| {
            OuroError::Validation(
                "operational certificate cold-key signature is invalid".into(),
            )
        })?;
    Ok(ParsedOperationalCertificate {
        hot_kes_verification_key: hot,
        cold_verification_key: cold,
        counter,
        kes_period,
    })
}

pub fn parse_kes_verification_key(bytes: &[u8]) -> Result<Vec<u8>> {
    let envelope: TextEnvelope = serde_json::from_slice(bytes).map_err(|error| {
        OuroError::Validation(format!("KES verification-key envelope is malformed: {error}"))
    })?;
    let _ = envelope.description;
    if !envelope.envelope_type.to_ascii_lowercase().contains("kesverificationkey") {
        return Err(OuroError::Validation("public key is not a KES verification key".into()));
    }
    let value: serde_cbor::Value = serde_cbor::from_slice(&decode_hex(&envelope.cbor_hex)?)
        .map_err(|error| OuroError::Validation(format!("KES verification-key CBOR is malformed: {error}")))?;
    match value {
        serde_cbor::Value::Bytes(bytes) if !bytes.is_empty() => Ok(bytes),
        _ => Err(OuroError::Validation("KES verification-key CBOR has the wrong shape".into())),
    }
}

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
    // p5-12: node_version is optional — it only seasons this local metadata hash, so an
    // omitted value is a stable sentinel, not an error.
    let vkey_hash = stable_hash(&format!(
        "{}:{}:{}",
        spec.pool.network.as_str(),
        spec.node_version.as_deref().unwrap_or("unspecified"),
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

/// Install an opcert (KES rotation). The out-of-band confirmation gate is enforced
/// by the caller (`cli::consume_confirmation`); this function assumes confirmation
/// has already been consumed and focuses on cert/counter validation + audit.
pub fn push_opcert(
    spec: &PoolSpec,
    machine_id: &str,
    cert_path: &Path,
    counter_path: &Path,
    audit_store: &AuditStore,
) -> Result<KesPushReport> {
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
    audit_store.finish_invocation(&audit_id, "kes/push")?;
    Ok(KesPushReport {
        machine: machine_id.to_string(),
        cert_path: cert_path.to_path_buf(),
        cert_hash: stable_hash(&cert_text),
        installed_payload: vec!["node.cert".to_string()],
        audit_id,
    })
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

    use ed25519_dalek::{Signer, SigningKey};
    use serde_cbor::Value;

    use crate::{audit::AuditStore, domain::PoolSpec};

    use super::{parse_operational_certificate, push_opcert, read_counter_state};

    fn operational_certificate(corrupt_signature: bool) -> Vec<u8> {
        let signing = SigningKey::from_bytes(&[7_u8; 32]);
        let hot = vec![9_u8; 32];
        let counter = 4_u64;
        let period = 123_u64;
        let mut signable = hot.clone();
        signable.extend_from_slice(&counter.to_be_bytes());
        signable.extend_from_slice(&period.to_be_bytes());
        let mut signature = signing.sign(&signable).to_bytes();
        if corrupt_signature {
            signature[0] ^= 1;
        }
        let value = Value::Array(vec![
            Value::Array(vec![
                Value::Bytes(hot),
                Value::Integer(counter.into()),
                Value::Integer(period.into()),
                Value::Bytes(signature.to_vec()),
            ]),
            Value::Bytes(signing.verifying_key().to_bytes().to_vec()),
        ]);
        let cbor = serde_cbor::to_vec(&value).unwrap();
        let cbor_hex = cbor.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        serde_json::to_vec(&serde_json::json!({
            "type": "NodeOperationalCertificate",
            "description": "synthetic protocol-valid opcert",
            "cborHex": cbor_hex,
        }))
        .unwrap()
    }

    #[test]
    fn verifies_operational_certificate_cold_key_signature() {
        let parsed = parse_operational_certificate(&operational_certificate(false)).unwrap();
        assert_eq!(parsed.counter, 4);
        assert_eq!(parsed.kes_period, 123);
    }

    #[test]
    fn rejects_operational_certificate_with_corrupt_signature() {
        let error = parse_operational_certificate(&operational_certificate(true)).unwrap_err();
        assert!(error.to_string().contains("signature is invalid"));
    }

    #[test]
    fn accepts_valid_opcert_and_audits_finish() {
        let spec = PoolSpec::from_file(Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        let audit = AuditStore::in_memory().unwrap();
        let report = push_opcert(
            &spec,
            "bp1",
            Path::new("tests/fixtures/kes/node-cert-valid.json"),
            Path::new("tests/fixtures/kes/counter-state.json"),
            &audit,
        )
        .unwrap();
        assert_eq!(report.installed_payload, vec!["node.cert".to_string()]);
        assert!(audit.invocation_has_start(&report.audit_id).unwrap());
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
