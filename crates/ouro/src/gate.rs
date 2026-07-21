//! S0019 p1-4 (§2.4) — the central live re-attestation gate.
//!
//! Every managed op passes through here BEFORE any script/executor extraction. The gate:
//!   1. takes the exclusive per-node lock (so ouro writers serialize; cooperative out-of-band admin
//!      must honor the same lease, §2.9; non-cooperative root is out of scope, §2.12),
//!   2. resolves the live node by IMMUTABLE container id (never by name) and re-attests it against
//!      the attestation (identity + mutable state at the current generation),
//!   3. hands the caller a guard that MUST be used to re-check immediately before each irreversible
//!      commit (`recheck_before_commit`) — this closes the check→act TOCTOU window; a swap timed
//!      between the initial check and the commit is refused.
//!
//! The lock + `openat2`-beneath-no-symlink file access are OS mechanisms exercised on the target;
//! here we implement the ORDERING + the lock so the protocol is testable and the executor (p2-2)
//! composes it. `LiveProbe` is the closure the target-side probe supplies.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::attestation::{AdoptionAttestation, LiveObservation};
use crate::{OuroError, Result};

/// Exclusive per-node lock. Advisory (a lock file with the holder's audit id); the point is that
/// two ouro writers cannot both proceed. Released on drop.
pub struct NodeLock {
    _file: File,
}

#[cfg(unix)]
mod unix_lock {
    use std::os::raw::c_int;
    pub const LOCK_EX: c_int = 2;
    pub const LOCK_NB: c_int = 4;
    pub const LOCK_UN: c_int = 8;
    extern "C" {
        pub fn flock(fd: c_int, operation: c_int) -> c_int;
    }
}

impl NodeLock {
    /// Acquire the lock for `node_id` under `lock_root`. Fails if already held (single writer).
    pub fn acquire(lock_root: &Path, node_id: &str, audit_id: &str) -> Result<NodeLock> {
        std::fs::create_dir_all(lock_root)
            .map_err(|e| OuroError::Validation(format!("cannot create lock dir: {e}")))?;
        let path = lock_root.join(format!("{node_id}.lock"));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| OuroError::Validation(format!("cannot open node lock: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // Kernel flock is released when the process dies, so recovery cannot be blocked by a
            // stale create_new lock file after kill/OOM/reboot.
            let acquired = unsafe {
                unix_lock::flock(file.as_raw_fd(), unix_lock::LOCK_EX | unix_lock::LOCK_NB)
            } == 0;
            if !acquired {
                let mut held = String::new();
                let _ = File::open(&path).and_then(|mut holder| holder.read_to_string(&mut held));
                return Err(OuroError::Validation(format!(
                    "node {node_id} is locked by another operation (holder audit {}) — refused \
                     (single-writer, §2.4)",
                    held.trim()
                )));
            }
        }
        #[cfg(not(unix))]
        return Err(OuroError::Validation(
            "the S0019 node lock requires a Unix target".into(),
        ));
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(audit_id.as_bytes())?;
        file.sync_all()?;
        Ok(NodeLock { _file: file })
    }
}

impl Drop for NodeLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let _ = unsafe { unix_lock::flock(self._file.as_raw_fd(), unix_lock::LOCK_UN) };
        }
    }
}

/// A probe that returns the current LiveObservation of the node (target-side gather).
pub type LiveProbe<'a> = dyn Fn() -> Result<LiveObservation> + 'a;

/// A guard proving the node was attested under the held lock. The executor MUST call
/// `recheck_before_commit` immediately before each irreversible commit.
pub struct AttestedGuard<'a> {
    attestation: &'a AdoptionAttestation,
    probe: &'a LiveProbe<'a>,
    _lock: NodeLock,
}

impl<'a> AttestedGuard<'a> {
    /// §2.4 pre-commit re-check (CAS): re-probe the live node and refuse if it drifted since the
    /// gate opened. Call this immediately before every irreversible step.
    pub fn recheck_before_commit(&self) -> Result<()> {
        let live = (self.probe)()?;
        self.attestation.require_matches_live(&live)
    }

    /// Later steps may follow an intended content change; the held lock excludes other Ouro writers
    /// while this still catches an image/container/mount swap before the next executor step.
    pub fn recheck_identity_before_commit(&self) -> Result<()> {
        let live = (self.probe)()?;
        self.attestation.require_identity_matches(&live)
    }
}

/// The central gate. Takes the lock, re-attests once, and returns a guard for pre-commit re-checks.
/// `lock_root` is the target-local lock dir; `node_id` the immutable node id; `probe` gathers the
/// live observation (by immutable container id).
pub fn require_attested_node<'a>(
    attestation: &'a AdoptionAttestation,
    lock_root: &Path,
    node_id: &str,
    audit_id: &str,
    probe: &'a LiveProbe<'a>,
) -> Result<AttestedGuard<'a>> {
    let lock = NodeLock::acquire(lock_root, node_id, audit_id)?;
    let live = probe()?;
    attestation.require_matches_live(&live)?;
    Ok(AttestedGuard {
        attestation,
        probe,
        _lock: lock,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{ImmutableIdentity, ManagedState, Role, TypedMount};
    use std::cell::Cell;

    fn att() -> AdoptionAttestation {
        AdoptionAttestation {
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
                image_config_digest: "sha256:cfg".into(),
                platform: "linux/amd64".into(),
                container_creation_epoch: 1000,
                entrypoint: vec!["cardano-node".into()],
                args: vec!["run".into()],
                mounts: vec![TypedMount {
                    kind: "bind".into(),
                    source_id: "8:1:1".into(),
                    destination: "/data/db".into(),
                    read_only: false,
                    owner: "root".into(),
                    mode: "0755".into(),
                    no_symlink: true,
                }],
                network: "mainnet".into(),
                genesis_hash: "g".into(),
                public_credential_ids: vec![],
                approval_evidence_hash: "e".into(),
            },
            state: ManagedState {
                state_generation: 1,
                container_id: "cid".into(),
                topology_hash: "t0".into(),
                config_hash: "c0".into(),
                kes_opcert_id: "k".into(),
            },
        }
    }
    fn good_live() -> LiveObservation {
        LiveObservation {
            image_config_digest: "sha256:cfg".into(),
            container_id: "cid".into(),
            container_creation_epoch: 1000,
            entrypoint: vec!["cardano-node".into()],
            args: vec!["run".into()],
            mounts: att().immutable.mounts,
            topology_hash: "t0".into(),
            config_hash: "c0".into(),
            kes_opcert_id: "k".into(),
            has_forging_keys: false,
        }
    }

    #[test]
    fn lock_is_exclusive() {
        let dir = std::env::temp_dir().join(format!("ouro-gate-lock-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let a = NodeLock::acquire(&dir, "bp1", "audit-1").unwrap();
        assert!(
            NodeLock::acquire(&dir, "bp1", "audit-2").is_err(),
            "second holder refused"
        );
        drop(a);
        assert!(
            NodeLock::acquire(&dir, "bp1", "audit-3").is_ok(),
            "lock released on drop"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gate_refuses_drift_between_open_and_commit() {
        let dir = std::env::temp_dir().join(format!("ouro-gate-toctou-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let a = att();
        // Probe returns good state on the FIRST call (gate open) and a drifted state afterwards
        // (simulating an out-of-band swap timed between check and commit).
        let calls = Cell::new(0u32);
        let probe = move || -> Result<LiveObservation> {
            let n = calls.get();
            calls.set(n + 1);
            let mut l = good_live();
            if n >= 1 {
                l.container_id = "cid-swapped".into(); // recreated between open and commit
            }
            Ok(l)
        };
        let guard = require_attested_node(&a, &dir, "bp1", "audit-x", &probe).unwrap();
        // Pre-commit re-check must catch the swap that the initial gate accepted.
        assert!(
            guard.recheck_before_commit().is_err(),
            "TOCTOU swap refused at pre-commit"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
