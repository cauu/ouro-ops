//! S0019 p2-2 (§2.6) — the sealed executor's crash-durable write transaction.
//!
//! A managed write is a state machine whose every transition is fsync'd to a target-local journal
//! BEFORE its side effect is observable, so a crash at any point is recoverable. A recovery pass
//! runs at the start of every `tool run` and reconciles an interrupted transaction before any new
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
pub struct JournalRecord {
    pub audit_id: String,
    pub operation_id: String,
    pub node_id: String,
    pub state: TxState,
}

/// Durable, fsync'd journal (one per node). `record` persists a transition before its side effect.
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn at(dir: &Path, node_id: &str) -> Journal {
        Journal { path: dir.join(format!("{node_id}.txn.json")) }
    }

    pub fn record(&self, rec: &JournalRecord) -> Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        // Write + fsync the file and its parent dir so the transition survives power loss.
        let tmp = self.path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)
                .map_err(|e| OuroError::Validation(format!("journal write: {e}")))?;
            f.write_all(serde_json::to_string(rec).unwrap().as_bytes()).ok();
            f.sync_all().ok();
        }
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| OuroError::Validation(format!("journal commit: {e}")))?;
        if let Some(p) = self.path.parent() {
            if let Ok(d) = std::fs::File::open(p) {
                d.sync_all().ok();
            }
        }
        Ok(())
    }

    pub fn read(&self) -> Option<JournalRecord> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
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

/// Durable write-seal: once set, all writes refuse until operator recovery clears it.
pub struct WriteSeal {
    path: PathBuf,
}

impl WriteSeal {
    pub fn at(dir: &Path, node_id: &str) -> WriteSeal {
        WriteSeal { path: dir.join(format!("{node_id}.seal")) }
    }
    pub fn is_sealed(&self) -> bool {
        self.path.exists()
    }
    pub fn set(&self, reason: &str) -> Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        std::fs::write(&self.path, reason)
            .map_err(|e| OuroError::Validation(format!("cannot set write-seal: {e}")))
    }
    /// Operator-only recovery clears the seal.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
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
    JournalRecord { state, ..base.clone() }
}

/// Run a write transaction to a terminal state. Records every transition before its side effect.
/// On a verify failure it rolls back; a failed rollback seals writes. Returns the terminal state.
pub fn run(
    journal: &Journal,
    seal: &WriteSeal,
    base: &JournalRecord,
    ops: &TxOps<'_>,
) -> Result<TxState> {
    seal.require_clear()?;
    journal.record(&rec(base, TxState::Prepared))?;

    journal.record(&rec(base, TxState::Committing))?;
    match (ops.commit)() {
        Ok(()) => journal.record(&rec(base, TxState::Committed))?,
        Err(_) => return finish_rollback(journal, seal, base, ops),
    }

    journal.record(&rec(base, TxState::Verifying))?;
    match (ops.verify)() {
        Ok(()) => {
            journal.record(&rec(base, TxState::Verified))?;
            journal.clear(); // terminal success
            Ok(TxState::Verified)
        }
        Err(_) => finish_rollback(journal, seal, base, ops),
    }
}

fn finish_rollback(
    journal: &Journal,
    seal: &WriteSeal,
    base: &JournalRecord,
    ops: &TxOps<'_>,
) -> Result<TxState> {
    journal.record(&rec(base, TxState::RollingBack))?;
    match (ops.rollback)() {
        Ok(()) => {
            journal.record(&rec(base, TxState::RolledBack))?;
            journal.clear();
            Ok(TxState::RolledBack)
        }
        Err(e) => {
            journal.record(&rec(base, TxState::Sealed))?;
            seal.set(&format!("rollback failed for {}: {e}", base.operation_id))?;
            Err(OuroError::Validation(format!(
                "rollback failed → writes sealed (exit 40, operator recovery): {e}"
            )))
        }
    }
}

/// Recovery pass — run at the START of every `tool run` before any new write. Reconciles an
/// interrupted transaction from the journal: re-drive verify (and rollback on failure), re-run an
/// idempotent rollback, or leave a seal in place. Returns the terminal state (or None if clean).
pub fn recover(journal: &Journal, seal: &WriteSeal, ops: &TxOps<'_>) -> Result<Option<TxState>> {
    let Some(r) = journal.read() else {
        return Ok(None); // clean — no interrupted transaction
    };
    match r.state {
        TxState::Verified | TxState::RolledBack => {
            journal.clear();
            Ok(Some(r.state))
        }
        TxState::Sealed => Ok(Some(TxState::Sealed)), // seal stands until operator clears
        // Crashed after committing (or mid-commit): re-verify; if bad, roll back. commit/verify
        // are idempotent so re-driving is safe.
        TxState::Committing | TxState::Committed | TxState::Verifying | TxState::Prepared => {
            match (ops.verify)() {
                Ok(()) => {
                    journal.record(&rec(&r, TxState::Verified))?;
                    journal.clear();
                    Ok(Some(TxState::Verified))
                }
                Err(_) => finish_rollback(journal, seal, &r, ops).map(Some),
            }
        }
        // Crashed mid-rollback: re-run rollback (idempotent).
        TxState::RollingBack => finish_rollback(journal, seal, &r, ops).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn dirs(name: &str) -> (PathBuf, JournalRecord) {
        let d = std::env::temp_dir().join(format!("ouro-txn-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        (d, JournalRecord {
            audit_id: "a1".into(),
            operation_id: "runtime/restart".into(),
            node_id: "bp1".into(),
            state: TxState::Prepared,
        })
    }

    #[test]
    fn happy_path_reaches_verified_and_clears() {
        let (d, base) = dirs("happy");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        let ops = TxOps { commit: &|| Ok(()), verify: &|| Ok(()), rollback: &|| Ok(()) };
        assert_eq!(run(&j, &s, &base, &ops).unwrap(), TxState::Verified);
        assert!(j.read().is_none(), "journal cleared on success");
        assert!(!s.is_sealed());
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
            rollback: &|| { rolled.set(true); Ok(()) },
        };
        assert_eq!(run(&j, &s, &base, &ops).unwrap(), TxState::RolledBack);
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
        s.clear();
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
        let ops = TxOps { commit: &|| Ok(()), verify: &|| Ok(()), rollback: &|| Ok(()) };
        assert_eq!(recover(&j, &s, &ops).unwrap(), Some(TxState::Verified));
        assert!(j.read().is_none());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn recovery_rolls_back_a_bad_interrupted_commit() {
        let (d, base) = dirs("recrollback");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        j.record(&rec(&base, TxState::Committing)).unwrap(); // crashed mid-commit
        let rolled = Cell::new(false);
        let ops = TxOps {
            commit: &|| Ok(()),
            verify: &|| Err(OuroError::Validation("unhealthy after crash".into())),
            rollback: &|| { rolled.set(true); Ok(()) },
        };
        assert_eq!(recover(&j, &s, &ops).unwrap(), Some(TxState::RolledBack));
        assert!(rolled.get());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn recovery_on_clean_journal_is_noop() {
        let (d, _base) = dirs("clean");
        let (j, s) = (Journal::at(&d, "bp1"), WriteSeal::at(&d, "bp1"));
        let ops = TxOps { commit: &|| Ok(()), verify: &|| Ok(()), rollback: &|| Ok(()) };
        assert_eq!(recover(&j, &s, &ops).unwrap(), None);
        std::fs::remove_dir_all(&d).ok();
    }
}
