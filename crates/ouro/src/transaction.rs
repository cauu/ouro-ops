//! S0019 p2-2 (§2.6) — the sealed executor's crash-durable write transaction.
//!
//! A managed write is a state machine whose every transition is fsync'd to a target-local journal
//! BEFORE its side effect is observable, so a crash at any point is recoverable. A recovery pass
//! runs at the start of every typed write and reconciles an interrupted transaction before any new
//! write. Each phase is idempotent / CAS-guarded so recovery can re-drive it. A failed or
//! unverifiable rollback writes a durable WRITE-SEAL; further writes refuse until an explicit
//! operator recovery clears it.
//!
//! The actual mutation/verify/rollback are FIXED operations supplied by the executor (built from a
//! validated intent → fixed argv, §2.5); this module owns the durable ordering, recovery, and seal.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{OuroError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TxState {
    Prepared,
    Committing,
    Committed,
    Verifying,
    Verified,
    RollingBack,
    RolledBack,
    Sealed,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DurableTransaction {
    /// The exact validated intent whose executor plans are persisted below.
    pub intent: crate::intent::Intent,
    /// Pre-state used to verify or restore an interrupted operation after process death.
    pub pre_attestation: crate::attestation::AdoptionAttestation,
    /// Fixed, already-resolved target-side argv sequence. Kept for audit/reconciliation; recovery
    /// verifies before deciding whether a partial commit must roll back.
    pub commit_plan: Vec<Vec<String>>,
    /// `None` means the operation is irreversible/ambiguous and recovery must seal for operator
    /// reconciliation instead of pretending that a restart can undo it.
    pub rollback_plan: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JournalRecord {
    pub audit_id: String,
    pub operation_id: String,
    pub node_id: String,
    pub state: TxState,
    #[serde(default)]
    pub durable: Option<DurableTransaction>,
}

/// Durable, fsync'd journal (one per node). `record` persists a transition before its side effect.
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn at(dir: &Path, node_id: &str) -> Journal {
        Journal {
            path: dir.join(format!("{node_id}.txn.json")),
        }
    }

    pub fn record(&self, rec: &JournalRecord) -> Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)
                .map_err(|e| OuroError::Validation(format!("journal mkdir: {e}")))?;
        }
        // Write + fsync the file and its parent dir so the transition survives power loss.
        let tmp = self.path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.create(true).truncate(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut f = options
                .open(&tmp)
                .map_err(|e| OuroError::Validation(format!("journal write: {e}")))?;
            let bytes = serde_json::to_vec(rec)
                .map_err(|e| OuroError::Validation(format!("journal serialize: {e}")))?;
            f.write_all(&bytes)
                .map_err(|e| OuroError::Validation(format!("journal write: {e}")))?;
            f.sync_all()
                .map_err(|e| OuroError::Validation(format!("journal fsync: {e}")))?;
        }
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| OuroError::Validation(format!("journal commit: {e}")))?;
        if let Some(p) = self.path.parent() {
            std::fs::File::open(p)
                .and_then(|dir| dir.sync_all())
                .map_err(|e| OuroError::Validation(format!("journal directory fsync: {e}")))?;
        }
        Ok(())
    }

    pub fn read(&self) -> Result<Option<JournalRecord>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| OuroError::Validation(format!("malformed transaction journal: {e}")))
    }

    pub fn clear(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        if let Some(parent) = self.path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

/// Fixed operations supplied by the executor for a specific validated intent. Each returns Ok on
/// success. `commit` performs the mutation; `verify` runs readiness proxies (§2.6a); `rollback`
/// restores the pre-state from the fsync'd artifact. All MUST be idempotent (recovery re-drives).
pub struct TxOps<'a> {
    pub commit: &'a dyn Fn() -> Result<()>,
    pub verify: &'a dyn Fn() -> Result<()>,
    pub rollback: &'a dyn Fn() -> Result<()>,
}

/// Operation-aware recovery supplied by the S0019 target pipeline. Unlike the former no-op
/// callbacks, both functions receive the exact durable journal record being reconciled.
pub struct RecoveryOps<'a> {
    pub verify: &'a dyn Fn(&JournalRecord) -> Result<()>,
    pub rollback: &'a dyn Fn(&JournalRecord) -> Result<()>,
}

/// Durable write-seal: once set, all writes refuse until operator recovery clears it.
pub struct WriteSeal {
    path: PathBuf,
}

impl WriteSeal {
    pub fn at(dir: &Path, node_id: &str) -> WriteSeal {
        WriteSeal {
            path: dir.join(format!("{node_id}.seal")),
        }
    }
    pub fn is_sealed(&self) -> bool {
        self.path.exists()
    }
    pub fn set(&self, reason: &str) -> Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = self.path.with_extension("seal.tmp");
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.create(true).truncate(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp)?;
            file.write_all(reason.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        if let Some(parent) = self.path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
    /// Operator-only recovery clears the seal.
    pub fn clear(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        if let Some(parent) = self.path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
    pub fn require_clear(&self) -> Result<()> {
        if self.is_sealed() {
            let why = std::fs::read_to_string(&self.path).unwrap_or_default();
            return Err(OuroError::Validation(format!(
                "writes are sealed ({}) — operator recovery required before further writes (§2.6)",
                why.trim()
            )));
        }
        Ok(())
    }
}

fn rec(base: &JournalRecord, state: TxState) -> JournalRecord {
    JournalRecord {
        state,
        ..base.clone()
    }
}

/// Run a write transaction to a terminal state. Records every transition before its side effect.
/// On a verify failure it rolls back; a failed rollback seals writes. Returns the terminal state.
pub fn run(
    journal: &Journal,
    seal: &WriteSeal,
    base: &JournalRecord,
    ops: &TxOps<'_>,
) -> Result<TxState> {
    run_observed(journal, seal, base, ops, &|_| Ok(()))
}

/// Run a write transaction while emitting each durable phase through `observe`. The journal is
/// always persisted first, so an audit failure cannot make an unjournaled side effect observable.
pub fn run_observed(
    journal: &Journal,
    seal: &WriteSeal,
    base: &JournalRecord,
    ops: &TxOps<'_>,
    observe: &dyn Fn(TxState) -> Result<()>,
) -> Result<TxState> {
    seal.require_clear()?;
    if base.durable.is_none() {
        return Err(OuroError::Validation(
            "refusing write without durable transaction context (§2.6)".into(),
        ));
    }
    journal.record(&rec(base, TxState::Prepared))?;
    observe(TxState::Prepared)?;

    journal.record(&rec(base, TxState::Committing))?;
    observe(TxState::Committing)?;
    match (ops.commit)() {
        Ok(()) => {
            journal.record(&rec(base, TxState::Committed))?;
            observe(TxState::Committed)?;
        }
        Err(error) => {
            rollback_to_terminal(journal, seal, base, ops, observe)?;
            return Err(OuroError::Validation(format!(
                "commit failed for {} and was rolled back: {error}",
                base.operation_id
            )));
        }
    }

    journal.record(&rec(base, TxState::Verifying))?;
    observe(TxState::Verifying)?;
    match (ops.verify)() {
        Ok(()) => {
            journal.record(&rec(base, TxState::Verified))?;
            observe(TxState::Verified)?;
            journal.clear()?; // terminal success
            Ok(TxState::Verified)
        }
        Err(error) => {
            rollback_to_terminal(journal, seal, base, ops, observe)?;
            Err(OuroError::Validation(format!(
                "verification failed for {} and was rolled back: {error}",
                base.operation_id
            )))
        }
    }
}

fn rollback_to_terminal(
    journal: &Journal,
    seal: &WriteSeal,
    base: &JournalRecord,
    ops: &TxOps<'_>,
    observe: &dyn Fn(TxState) -> Result<()>,
) -> Result<()> {
    journal.record(&rec(base, TxState::RollingBack))?;
    observe(TxState::RollingBack)?;
    match (ops.rollback)() {
        Ok(()) => {
            journal.record(&rec(base, TxState::RolledBack))?;
            observe(TxState::RolledBack)?;
            journal.clear()?;
            Ok(())
        }
        Err(e) => {
            journal.record(&rec(base, TxState::Sealed))?;
            seal.set(&format!("rollback failed for {}: {e}", base.operation_id))?;
            observe(TxState::Sealed)?;
            Err(OuroError::Validation(format!(
                "rollback failed → writes sealed (exit 40, operator recovery): {e}"
            )))
        }
    }
}

/// Recovery pass — run at the START of every typed write before any new write. Reconciles an
/// interrupted transaction from the journal: re-drive verify (and rollback on failure), re-run an
/// idempotent rollback, or leave a seal in place. Returns the terminal state (or None if clean).
pub fn recover(
    journal: &Journal,
    seal: &WriteSeal,
    ops: &RecoveryOps<'_>,
) -> Result<Option<TxState>> {
    let Some(r) = journal.read()? else {
        return Ok(None); // clean — no interrupted transaction
    };
    match r.state {
        TxState::Verified | TxState::RolledBack => {
            journal.clear()?;
            Ok(Some(r.state))
        }
        TxState::Sealed => Ok(Some(TxState::Sealed)), // seal stands until operator clears
        // Prepared is persisted before Committing, so no executor step was allowed to start.
        TxState::Prepared => {
            journal.record(&rec(&r, TxState::RolledBack))?;
            journal.clear()?;
            Ok(Some(TxState::RolledBack))
        }
        // Once Committing was durable, a side effect may be partial. Refuse any legacy/incomplete
        // record that lacks the exact intent/pre-state/plans needed to reconcile it.
        TxState::Committing => {
            if r.durable.is_none() {
                journal.record(&rec(&r, TxState::Sealed))?;
                seal.set("interrupted transaction lacks durable recovery context")?;
                return Err(OuroError::Validation(
                    "interrupted transaction lacks durable recovery context — writes sealed".into(),
                ));
            }
            // At Committing an arbitrary prefix of the executor sequence may have run. Treating a
            // superficially healthy node as success is unsafe (e.g. artifact copied but restart or
            // tx submit not reached), so always restore/seal rather than verify-to-success.
            rollback_recovery(journal, seal, &r, ops)?;
            Ok(Some(TxState::RolledBack))
        }
        TxState::Committed | TxState::Verifying => {
            if r.durable.is_none() {
                journal.record(&rec(&r, TxState::Sealed))?;
                seal.set("interrupted transaction lacks durable recovery context")?;
                return Err(OuroError::Validation(
                    "interrupted transaction lacks durable recovery context — writes sealed".into(),
                ));
            }
            match (ops.verify)(&r) {
                Ok(()) => {
                    journal.record(&rec(&r, TxState::Verified))?;
                    journal.clear()?;
                    Ok(Some(TxState::Verified))
                }
                Err(_) => {
                    rollback_recovery(journal, seal, &r, ops)?;
                    Ok(Some(TxState::RolledBack))
                }
            }
        }
        // Crashed mid-rollback: re-run rollback (idempotent).
        TxState::RollingBack => {
            if r.durable.is_none() {
                journal.record(&rec(&r, TxState::Sealed))?;
                seal.set("interrupted rollback lacks durable recovery context")?;
                return Err(OuroError::Validation(
                    "interrupted rollback lacks durable recovery context — writes sealed".into(),
                ));
            }
            rollback_recovery(journal, seal, &r, ops)?;
            Ok(Some(TxState::RolledBack))
        }
    }
}

fn rollback_recovery(
    journal: &Journal,
    seal: &WriteSeal,
    record: &JournalRecord,
    ops: &RecoveryOps<'_>,
) -> Result<()> {
    journal.record(&rec(record, TxState::RollingBack))?;
    match (ops.rollback)(record) {
        Ok(()) => {
            journal.record(&rec(record, TxState::RolledBack))?;
            journal.clear()?;
            Ok(())
        }
        Err(error) => {
            journal.record(&rec(record, TxState::Sealed))?;
            seal.set(&format!(
                "recovery rollback failed for {}: {error}",
                record.operation_id
            ))?;
            Err(OuroError::Validation(format!(
                "recovery rollback failed → writes sealed: {error}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{AdoptionAttestation, ImmutableIdentity, ManagedState, Role};
    use crate::intent::Intent;
    use serde_json::json;
    use std::cell::Cell;

    fn durable() -> DurableTransaction {
        DurableTransaction {
            intent: Intent {
                schema_version: 1,
                operation_id: "runtime/restart".into(),
                node_id: "bp1".into(),
                pre_state_generation: 1,
                pre_state_hash: "h".into(),
                expected_post_state: "".into(),
                nonce: "n".into(),
                expiry_epoch: 0,
                payload: json!({"machine":"bp1"}),
            },
            pre_attestation: AdoptionAttestation {
                immutable: ImmutableIdentity {
                    role: Role::Bp,
                    contract_id: "c".into(),
                    convention_version: 1,
                    allowlist_version: 1,
                    allowlist_digest: "sha256:a".into(),
                    host_key_sha256: "hk".into(),
                    machine_id: "bp1".into(),
                    oci_index_digest: "i".into(),
                    platform_manifest_digest: "p".into(),
                    image_config_digest: "cfg".into(),
                    platform: "linux/amd64".into(),
                    container_creation_epoch: 1,
                    entrypoint: vec![],
                    args: vec![],
                    mounts: vec![],
                    network: "mainnet".into(),
                    genesis_hash: "g".into(),
                    public_credential_ids: vec![],
                    approval_evidence_hash: "e".into(),
                },
                state: ManagedState {
                    state_generation: 1,
                    container_id: "cid".into(),
                    topology_hash: "t".into(),
                    config_hash: "c".into(),
                    kes_opcert_id: "k".into(),
                },
            },
            commit_plan: vec![vec!["docker".into(), "restart".into(), "cid".into()]],
            rollback_plan: Some(vec![vec!["docker".into(), "restart".into(), "cid".into()]]),
        }
    }

    fn dirs(name: &str) -> (PathBuf, JournalRecord) {
        let d = std::env::temp_dir().join(format!("ouro-txn-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        (
            d,
            JournalRecord {
                audit_id: "a1".into(),
                operation_id: "runtime/restart".into(),
                node_id: "bp1".into(),
                state: TxState::Prepared,
                durable: Some(durable()),
            },
        )
    }

    #[test]
    fn happy_path_reaches_verified_and_clears() {
        let (d, base) = dirs("happy");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        let ops = TxOps {
            commit: &|| Ok(()),
            verify: &|| Ok(()),
            rollback: &|| Ok(()),
        };
        assert_eq!(run(&j, &s, &base, &ops).unwrap(), TxState::Verified);
        assert!(j.read().unwrap().is_none(), "journal cleared on success");
        assert!(!s.is_sealed());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn observer_receives_every_durable_happy_path_phase() {
        use std::cell::RefCell;

        let (d, base) = dirs("observed");
        let (journal, seal) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        let ops = TxOps {
            commit: &|| Ok(()),
            verify: &|| Ok(()),
            rollback: &|| Ok(()),
        };
        let phases = RefCell::new(Vec::new());
        run_observed(&journal, &seal, &base, &ops, &|state| {
            phases.borrow_mut().push(state);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            phases.into_inner(),
            vec![
                TxState::Prepared,
                TxState::Committing,
                TxState::Committed,
                TxState::Verifying,
                TxState::Verified,
            ]
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn verify_failure_rolls_back() {
        let (d, base) = dirs("verifyfail");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        let rolled = Cell::new(false);
        let ops = TxOps {
            commit: &|| Ok(()),
            verify: &|| Err(OuroError::Validation("not healthy".into())),
            rollback: &|| {
                rolled.set(true);
                Ok(())
            },
        };
        let error = run(&j, &s, &base, &ops).unwrap_err().to_string();
        assert!(
            error.contains("rolled back"),
            "requested operation still reports failure"
        );
        assert!(rolled.get(), "rollback ran on verify failure");
        assert!(!s.is_sealed());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn failed_rollback_seals_writes() {
        let (d, base) = dirs("seal");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        let ops = TxOps {
            commit: &|| Ok(()),
            verify: &|| Err(OuroError::Validation("bad".into())),
            rollback: &|| Err(OuroError::Validation("rollback broke".into())),
        };
        assert!(run(&j, &s, &base, &ops).is_err());
        assert!(s.is_sealed(), "writes sealed after failed rollback");
        // A subsequent write refuses until the operator clears the seal.
        assert!(s.require_clear().is_err());
        s.clear().unwrap();
        assert!(s.require_clear().is_ok());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn recovery_redrives_an_interrupted_commit() {
        let (d, base) = dirs("reccommit");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        // Simulate a crash: journal left at Committed, process died before verify.
        j.record(&rec(&base, TxState::Committed)).unwrap();
        // Recovery re-verifies; node is healthy → reaches Verified, clears.
        let ops = RecoveryOps {
            verify: &|_| Ok(()),
            rollback: &|_| Ok(()),
        };
        assert_eq!(recover(&j, &s, &ops).unwrap(), Some(TxState::Verified));
        assert!(j.read().unwrap().is_none());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn recovery_rolls_back_a_bad_interrupted_commit() {
        let (d, base) = dirs("recrollback");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        j.record(&rec(&base, TxState::Committing)).unwrap(); // crashed mid-commit
        let rolled = Cell::new(false);
        let ops = RecoveryOps {
            verify: &|_| panic!("Committing must never be promoted by verify"),
            rollback: &|_| {
                rolled.set(true);
                Ok(())
            },
        };
        assert_eq!(recover(&j, &s, &ops).unwrap(), Some(TxState::RolledBack));
        assert!(rolled.get());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn recovery_on_clean_journal_is_noop() {
        let (d, _base) = dirs("clean");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        let ops = RecoveryOps {
            verify: &|_| Ok(()),
            rollback: &|_| Ok(()),
        };
        assert_eq!(recover(&j, &s, &ops).unwrap(), None);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn legacy_committed_journal_seals_instead_of_false_success() {
        let (d, mut base) = dirs("legacy");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        base.state = TxState::Committed;
        base.durable = None;
        j.record(&base).unwrap();
        let ops = RecoveryOps {
            verify: &|_| Ok(()),
            rollback: &|_| Ok(()),
        };
        assert!(recover(&j, &s, &ops).is_err());
        assert!(s.is_sealed(), "uncertain legacy write must seal");
        assert_eq!(j.read().unwrap().unwrap().state, TxState::Sealed);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn prepared_crash_clears_without_claiming_commit() {
        let (d, base) = dirs("prepared");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        j.record(&base).unwrap();
        let ops = RecoveryOps {
            verify: &|_| panic!("Prepared must not verify"),
            rollback: &|_| panic!("Prepared has no side effect to roll back"),
        };
        assert_eq!(recover(&j, &s, &ops).unwrap(), Some(TxState::RolledBack));
        assert!(j.read().unwrap().is_none());
        std::fs::remove_dir_all(&d).ok();
    }
}
