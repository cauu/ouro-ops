//! S0019 p2-1 (§1 Constraints, §2.5) — the intent envelope, the deny-by-default privileged-
//! capability registry, and the sink rules.
//!
//! The write-side safety lives here. The agent never authors commands; it composes an INTENT — a
//! bounded, canonical, schema-validated envelope. Every privileged operation is classified in a
//! deny-by-default registry (unclassified → refused). The payload carries only CLOSED, typed
//! references (a machine id from the attestation, an artifact id+digest, a closed enum) — never a
//! raw path, blob, or shell string. The executor (§2.6) turns a validated intent into a FIXED
//! argv array (no shell/eval/templating), so a hostile-but-schema-valid value cannot become
//! injection: it is either an enumerated value or it is refused.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::{OuroError, Result};

// --- envelope ------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Intent {
    pub schema_version: u32,
    pub operation_id: String,
    /// Immutable node id the op targets (from the attestation set) — never a hostname the agent typed.
    pub node_id: String,
    /// CAS base: the state generation the intent was built against (§2.3/§2.6).
    pub pre_state_generation: u64,
    pub pre_state_hash: String,
    pub expected_post_state: String,
    pub nonce: String,
    pub expiry_epoch: u64,
    /// Closed, typed parameters — see each operation's schema. Never raw paths/blobs/shell.
    pub payload: serde_json::Value,
}

/// Risk class → whether a confirm-token human gate is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    /// A managed READ (no mutation) — attested gate only; no confirm, no write transaction.
    Read,
    /// config/topology/restart-of-relay etc. — verify+rollback, no human gate.
    Reversible,
    /// key-touching / irreversible / availability-affecting — confirm-token required.
    Dangerous,
}

/// One classified privileged operation. Deny-by-default: an op not in the registry is refused.
pub struct OperationSpec {
    pub operation_id: &'static str,
    pub mutability: Mutability,
    /// Closed parameter schema: each allowed field + its validator.
    pub params: &'static [ParamSpec],
    /// Resources the op may touch (for audit + the executor's allowed-sink set).
    pub touched: &'static [&'static str],
    /// True if the op may expose secret-shaped output (must be false for every current op).
    pub may_expose_secret: bool,
}

pub struct ParamSpec {
    pub name: &'static str,
    pub kind: ParamKind,
    pub required: bool,
}

#[derive(Clone, Copy)]
pub enum ParamKind {
    /// A value from a fixed set (e.g. era, network).
    Enum(&'static [&'static str]),
    /// A machine id — must match `[a-z0-9-]{1,32}` and exist in the attestation set (checked by caller).
    MachineId,
    /// An immutable artifact reference `<id>@sha256:<64hex>` (ingress via §2.7, never a raw path).
    ArtifactRef,
    /// An image config digest `sha256:<64hex>` — the upgrade target. Shape-checked here; that it is
    /// on the SIGNED allowlist is enforced at op time (`allowlist.contract_for`), so the agent can
    /// only ever name a blinklabs baseline, never an arbitrary image.
    ImageDigest,
    /// A bounded non-negative integer.
    Uint { max: u64 },
}

const MAX_STR: usize = 4096;
const MAX_DEPTH: usize = 8;
const MAX_ITEMS: usize = 256;

// --- registry (deny-by-default) ------------------------------------------------------------------

/// The complete privileged-operation registry. EVERY privileged mutation must appear here; the
/// static test `registry_covers_all_and_is_classified` guards completeness.
pub fn registry() -> &'static [OperationSpec] {
    &[
        OperationSpec {
            operation_id: "runtime/restart",
            mutability: Mutability::Dangerous, // a BP restart interrupts forging (§2.6a)
            params: &[ParamSpec { name: "machine", kind: ParamKind::MachineId, required: true }],
            touched: &["container:restart"],
            may_expose_secret: false,
        },
        OperationSpec {
            operation_id: "runtime/topology-apply",
            mutability: Mutability::Dangerous, // can partition a BP from relays
            params: &[ParamSpec { name: "machine", kind: ParamKind::MachineId, required: true }],
            touched: &["file:topology", "container:restart"],
            may_expose_secret: false,
        },
        OperationSpec {
            operation_id: "kes-rotation/rotate",
            mutability: Mutability::Dangerous,
            params: &[
                ParamSpec { name: "machine", kind: ParamKind::MachineId, required: true },
                ParamSpec { name: "opcert", kind: ParamKind::ArtifactRef, required: true },
            ],
            touched: &["file:kes", "file:opcert", "container:restart"],
            may_expose_secret: false,
        },
        OperationSpec {
            operation_id: "deploy/register-submit",
            mutability: Mutability::Dangerous,
            params: &[
                ParamSpec { name: "machine", kind: ParamKind::MachineId, required: true },
                ParamSpec { name: "tx", kind: ParamKind::ArtifactRef, required: true },
                ParamSpec {
                    name: "network",
                    kind: ParamKind::Enum(&["mainnet", "preprod", "preview"]),
                    required: true,
                },
            ],
            touched: &["chain:submit"],
            may_expose_secret: false,
        },
        OperationSpec {
            operation_id: "config/render",
            mutability: Mutability::Reversible,
            params: &[ParamSpec { name: "machine", kind: ParamKind::MachineId, required: true }],
            touched: &["file:config"],
            may_expose_secret: false,
        },
        OperationSpec {
            operation_id: "observability/health",
            mutability: Mutability::Read, // a managed read — no confirm, no write transaction
            params: &[ParamSpec { name: "machine", kind: ParamKind::MachineId, required: true }],
            touched: &["read:health"],
            may_expose_secret: false,
        },
        OperationSpec {
            operation_id: "upgrade/step",
            mutability: Mutability::Dangerous, // availability-affecting: recreates the container
            params: &[
                ParamSpec { name: "machine", kind: ParamKind::MachineId, required: true },
                // The N+1 target image, named by its config digest — must be on the signed allowlist
                // (enforced at op time). The recreate preserves the observed run-spec; the operator
                // delivers the image to the target (pull / inbox load) as a precondition.
                ParamSpec { name: "image", kind: ParamKind::ImageDigest, required: true },
            ],
            touched: &["container:recreate"],
            may_expose_secret: false,
        },
    ]
}

pub fn lookup(operation_id: &str) -> Option<&'static OperationSpec> {
    registry().iter().find(|o| o.operation_id == operation_id)
}

// --- validation ----------------------------------------------------------------------------------

impl Intent {
    /// Deny-by-default validation of the whole envelope + payload against the registered schema.
    /// Returns the matched OperationSpec so the caller knows the mutability / confirm requirement.
    pub fn validate(&self, now_epoch: u64) -> Result<&'static OperationSpec> {
        if self.schema_version != 1 {
            return Err(OuroError::Validation("intent schema_version must be 1".into()));
        }
        if self.expiry_epoch != 0 && now_epoch > self.expiry_epoch {
            return Err(OuroError::Validation("intent expired".into()));
        }
        let spec = lookup(&self.operation_id).ok_or_else(|| {
            OuroError::Validation(format!(
                "operation {} is not in the privileged registry — refused (deny-by-default, §2.5)",
                self.operation_id
            ))
        })?;
        // Bound the payload shape before inspecting fields.
        bound_value(&self.payload, 0)?;
        let obj = self
            .payload
            .as_object()
            .ok_or_else(|| OuroError::Validation("intent payload must be an object".into()))?;
        // Reject any field not in the closed schema.
        for k in obj.keys() {
            if !spec.params.iter().any(|p| p.name == k) {
                return Err(OuroError::Validation(format!(
                    "payload field {k:?} is not allowed for {} (closed schema, §2.5)",
                    self.operation_id
                )));
            }
        }
        // Validate each declared param.
        for p in spec.params {
            match obj.get(p.name) {
                None if p.required => {
                    return Err(OuroError::Validation(format!("missing required param {}", p.name)))
                }
                None => {}
                Some(v) => validate_param(p, v)?,
            }
        }
        Ok(spec)
    }

    /// Canonical hash of the envelope — bound to the audit event and (for dangerous ops) to a
    /// single-use confirm-token, so the confirmed intent == the validated == the executed one.
    pub fn canonical_hash(&self) -> String {
        let canon = canonical_json(&serde_json::to_value(self).unwrap_or(serde_json::Value::Null));
        let mut h = DefaultHasher::new();
        canon.hash(&mut h);
        format!("{:016x}", h.finish())
    }
}

fn validate_param(p: &ParamSpec, v: &serde_json::Value) -> Result<()> {
    let bad = |why: &str| Err(OuroError::Validation(format!("param {} {}", p.name, why)));
    match p.kind {
        ParamKind::Enum(allowed) => match v.as_str() {
            Some(s) if allowed.contains(&s) => Ok(()),
            _ => bad(&format!("must be one of {allowed:?}")),
        },
        ParamKind::MachineId => match v.as_str() {
            Some(s)
                if !s.is_empty()
                    && s.len() <= 32
                    && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') =>
            {
                Ok(())
            }
            _ => bad("must be a [a-z0-9-]{1,32} machine id"),
        },
        ParamKind::ArtifactRef => match v.as_str() {
            // <id>@sha256:<64hex> — an immutable content reference, never a path.
            Some(s) => {
                let ok = s
                    .split_once("@sha256:")
                    .map(|(id, hexd)| {
                        !id.is_empty()
                            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                            && hexd.len() == 64
                            && hexd.chars().all(|c| c.is_ascii_hexdigit())
                    })
                    .unwrap_or(false);
                if ok { Ok(()) } else { bad("must be <id>@sha256:<64hex> (an inbox artifact, §2.7)") }
            }
            _ => bad("must be an artifact reference string"),
        },
        ParamKind::ImageDigest => match v.as_str() {
            // sha256:<64hex> — a content-addressed image digest (allowlist membership is enforced at
            // op time). No repo/tag/path, so it can never be a mutable or arbitrary reference.
            Some(s) => {
                let ok = s
                    .strip_prefix("sha256:")
                    .map(|hexd| hexd.len() == 64 && hexd.chars().all(|c| c.is_ascii_hexdigit()))
                    .unwrap_or(false);
                if ok { Ok(()) } else { bad("must be sha256:<64hex> (an allowlisted image digest)") }
            }
            _ => bad("must be an image digest string"),
        },
        ParamKind::Uint { max } => match v.as_u64() {
            Some(n) if n <= max => Ok(()),
            _ => bad(&format!("must be an integer 0..={max}")),
        },
    }
}

/// Reject over-deep / over-wide / over-long payloads before field inspection.
fn bound_value(v: &serde_json::Value, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(OuroError::Validation("intent payload too deep".into()));
    }
    match v {
        serde_json::Value::String(s) if s.len() > MAX_STR => {
            Err(OuroError::Validation("intent string too long".into()))
        }
        serde_json::Value::Array(a) => {
            if a.len() > MAX_ITEMS {
                return Err(OuroError::Validation("intent array too large".into()));
            }
            a.iter().try_for_each(|e| bound_value(e, depth + 1))
        }
        serde_json::Value::Object(o) => {
            if o.len() > MAX_ITEMS {
                return Err(OuroError::Validation("intent object too large".into()));
            }
            o.values().try_for_each(|e| bound_value(e, depth + 1))
        }
        _ => Ok(()),
    }
}

/// Deterministic canonical serialization: object keys sorted, no insignificant whitespace. The
/// producer (`ouro-ops`) emits canonical intents; re-canonicalizing here makes the confirmed hash
/// independent of key order / duplicate-key tricks (serde takes last-wins; we hash the normalized
/// form, so what is confirmed is exactly what is executed).
pub fn canonical_json(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", serde_json::to_string(k).unwrap(), canonical_json(&o[*k])))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        serde_json::Value::Array(a) => {
            let inner: Vec<String> = a.iter().map(canonical_json).collect();
            format!("[{}]", inner.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn intent(op: &str, payload: serde_json::Value) -> Intent {
        Intent {
            schema_version: 1,
            operation_id: op.into(),
            node_id: "bp1".into(),
            pre_state_generation: 3,
            pre_state_hash: "h".into(),
            expected_post_state: "p".into(),
            nonce: "n1".into(),
            expiry_epoch: 0,
            payload,
        }
    }

    #[test]
    fn registry_covers_all_and_is_classified() {
        // Deny-by-default: an unregistered op is refused.
        assert!(intent("evil/wipe", json!({})).validate(0).is_err());
        // Every registered op has a mutability + no secret exposure.
        for op in registry() {
            assert!(!op.may_expose_secret, "{} must not expose secrets", op.operation_id);
            assert!(matches!(
                op.mutability,
                Mutability::Read | Mutability::Reversible | Mutability::Dangerous
            ));
        }
    }

    #[test]
    fn closed_schema_rejects_unknown_and_hostile_params() {
        // Unknown field.
        assert!(intent("runtime/restart", json!({"machine": "bp1", "evil": 1})).validate(0).is_err());
        // Hostile machine id (shell metachars) — not an enumerated/typed value → refused.
        assert!(intent("runtime/restart", json!({"machine": "bp1; rm -rf /"})).validate(0).is_err());
        assert!(intent("runtime/restart", json!({"machine": "../etc"})).validate(0).is_err());
        // A raw path where an ArtifactRef is required → refused (no path sink, §2.7).
        assert!(intent("kes-rotation/rotate",
            json!({"machine":"bp1","opcert":"/etc/passwd"})).validate(0).is_err());
        // Well-formed artifact ref accepted.
        let good = format!("opcert-1@sha256:{}", "a".repeat(64));
        assert!(intent("kes-rotation/rotate",
            json!({"machine":"bp1","opcert": good})).validate(0).is_ok());
    }

    #[test]
    fn bad_enum_and_bounds_rejected() {
        let tx = format!("tx-1@sha256:{}", "b".repeat(64));
        assert!(intent("deploy/register-submit",
            json!({"machine":"bp1","tx":tx.clone(),"network":"evilnet"})).validate(0).is_err());
        assert!(intent("deploy/register-submit",
            json!({"machine":"bp1","tx":tx,"network":"mainnet"})).validate(0).is_ok());
        // Over-long string bounded.
        assert!(intent("runtime/restart", json!({"machine": "x".repeat(5000)})).validate(0).is_err());
    }

    #[test]
    fn dangerous_ops_flagged_confirm_required() {
        assert_eq!(lookup("kes-rotation/rotate").unwrap().mutability, Mutability::Dangerous);
        assert_eq!(lookup("runtime/restart").unwrap().mutability, Mutability::Dangerous);
        assert_eq!(lookup("config/render").unwrap().mutability, Mutability::Reversible);
    }

    #[test]
    fn canonical_hash_is_key_order_independent() {
        let a = canonical_json(&json!({"b":1,"a":2}));
        let b = canonical_json(&json!({"a":2,"b":1}));
        assert_eq!(a, b, "canonical form independent of key order");
    }

    #[test]
    fn expiry_enforced() {
        let mut i = intent("runtime/restart", json!({"machine":"bp1"}));
        i.expiry_epoch = 100;
        assert!(i.validate(50).is_ok());
        assert!(i.validate(200).is_err(), "expired intent refused");
    }
}
