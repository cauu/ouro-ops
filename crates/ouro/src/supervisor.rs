//! S0019 p1-2 (§2.2) — the pinned v1 supervisor/host contract.
//!
//! Convergence works only if the SUPERVISION shape is finite, not just the in-container paths. The
//! v1 contract is exactly ONE shape; every other daemon/orchestration/mount/network/host-cardinality
//! is refused at adoption (not adapted). This is the decision layer: the target reports observed
//! facts (a closed projection gathered by the adopt-time probe), and `require_conformant` refuses
//! anything outside the contract. A second supported shape would be a SEPARATE versioned contract
//! with its own executor + fixtures — never a generic `runtime` field.

use serde::{Deserialize, Serialize};

use crate::{OuroError, Result};

/// The v1 supervisor contract (pinned constants).
pub mod v1 {
    pub const RUNTIME: &str = "docker";
    pub const ROOTFUL: bool = true;
    pub const NODES_PER_HOST: u32 = 1;
    /// Bind mounts are required (named-volume-only is refused): a bind source has a stable
    /// device+inode we can re-verify (§2.4), a bare named volume does not give us that handle.
    pub const REQUIRE_BIND_MOUNTS: bool = true;
    pub const DAEMON_SOCKET: &str = "/var/run/docker.sock";
    /// Fixed restart policy the node unit must use.
    pub const RESTART_POLICY: &str = "unless-stopped";
}

/// Closed projection of what the adopt-time probe observed on the host (no raw daemon JSON).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ComposeObservation {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub config_files: Vec<String>,
    #[serde(default)]
    pub config_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SupervisorObservation {
    /// `docker` | `podman` | `containerd` | ...
    pub runtime: String,
    /// True only for a rootful daemon reachable at the standard socket.
    pub rootful: bool,
    /// Whether a rootless `/run/user/<uid>` runtime owns the container.
    pub rootless: bool,
    /// How many cardano-node containers this host runs (must be exactly 1).
    pub node_container_count: u32,
    /// True if the node's data/keys/socket arrive via bind mounts (not named-volume-only).
    pub uses_bind_mounts: bool,
    pub daemon_socket: String,
    pub restart_policy: String,
    /// Upgrade routing observation: `run`, `compose`, or `unsupported`.
    pub orchestration: String,
    #[serde(default)]
    pub orchestration_reason: Option<String>,
    #[serde(default)]
    pub compose: Option<ComposeObservation>,
}

impl SupervisorObservation {
    pub fn upgrade_routing(&self) -> (String, Option<String>, Option<&ComposeObservation>) {
        let unsupported = |reason: String| ("unsupported".into(), Some(reason), None);
        if self.runtime != "docker" {
            return unsupported(format!("unsupported_runtime:{}", self.runtime));
        }
        if !self.rootful || self.rootless {
            return unsupported("unsupported_runtime_mode:rootless".into());
        }
        if self.daemon_socket != "/var/run/docker.sock" {
            return unsupported(format!("unsupported_daemon_socket:{}", self.daemon_socket));
        }
        (
            self.orchestration.clone(),
            self.orchestration_reason.clone(),
            self.compose.as_ref(),
        )
    }

    /// Refuse everything outside the v1 contract with a specific reason. This is the ONLY place a
    /// node's supervision shape is judged; adoption calls it before writing the attestation.
    pub fn require_conformant(&self) -> Result<()> {
        let refuse = |why: &str| {
            Err(OuroError::Validation(format!(
                "node supervision does not conform to the S0019 v1 contract (§2.2): {why}; \
                 unsupported — not adapted"
            )))
        };
        if self.rootless {
            return refuse("rootless runtime (only rootful docker is supported in v1)");
        }
        if self.runtime != v1::RUNTIME {
            return refuse(&format!(
                "runtime={} (only {} in v1)",
                self.runtime,
                v1::RUNTIME
            ));
        }
        if !self.rootful {
            return refuse("non-rootful daemon");
        }
        if self.orchestration != "run" {
            return refuse(&format!(
                "orchestration={} (only direct `run` in v1; compose/swarm/k8s are separate contracts)",
                self.orchestration
            ));
        }
        if self.node_container_count != v1::NODES_PER_HOST {
            return refuse(&format!(
                "{} node containers on this host (v1 requires exactly {})",
                self.node_container_count,
                v1::NODES_PER_HOST
            ));
        }
        if v1::REQUIRE_BIND_MOUNTS && !self.uses_bind_mounts {
            return refuse(
                "node data/keys are not bind-mounted (named-volume-only is refused in v1)",
            );
        }
        if self.daemon_socket != v1::DAEMON_SOCKET {
            return refuse(&format!(
                "daemon socket {} (v1 requires {})",
                self.daemon_socket,
                v1::DAEMON_SOCKET
            ));
        }
        if self.restart_policy != v1::RESTART_POLICY {
            return refuse(&format!(
                "restart policy {} (v1 requires {})",
                self.restart_policy,
                v1::RESTART_POLICY
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conforming() -> SupervisorObservation {
        SupervisorObservation {
            runtime: "docker".into(),
            rootful: true,
            rootless: false,
            node_container_count: 1,
            uses_bind_mounts: true,
            daemon_socket: "/var/run/docker.sock".into(),
            restart_policy: "unless-stopped".into(),
            orchestration: "run".into(),
            orchestration_reason: None,
            compose: None,
        }
    }

    #[test]
    fn v1_conforming_shape_accepted() {
        assert!(conforming().require_conformant().is_ok());
    }

    #[test]
    fn every_non_v1_shape_refused() {
        type Case = (&'static str, fn(&mut SupervisorObservation));
        let cases: &[Case] = &[
            ("rootless", |o| o.rootless = true),
            ("podman", |o| o.runtime = "podman".into()),
            ("non-rootful", |o| o.rootful = false),
            ("compose", |o| o.orchestration = "compose".into()),
            ("multi-node", |o| o.node_container_count = 2),
            ("named-volume-only", |o| o.uses_bind_mounts = false),
            ("nonstandard-socket", |o| {
                o.daemon_socket = "/run/user/1000/docker.sock".into()
            }),
            ("wrong-restart", |o| o.restart_policy = "no".into()),
        ];
        for (name, mutate) in cases {
            let mut o = conforming();
            mutate(&mut o);
            assert!(
                o.require_conformant().is_err(),
                "shape {name} must be refused"
            );
        }
    }

    #[test]
    fn nonstandard_runtime_routes_to_unsupported() {
        let mut observation = conforming();
        observation.runtime = "podman".into();
        assert_eq!(
            observation.upgrade_routing(),
            (
                "unsupported".into(),
                Some("unsupported_runtime:podman".into()),
                None
            )
        );
    }
}
