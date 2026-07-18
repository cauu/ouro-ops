//! Shared bootstrap-plan executor used by the current `ouro-ops onboard` flow.
//!
//! Dynamic content travels as file bytes over the bootstrap transport, never through shell
//! interpolation. This module deliberately contains no product-specific legacy install recipe;
//! `onboard.rs` owns the only supported plan.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::bootstrap::{BootstrapOutcome, BootstrapTarget, BootstrapTransport, HostKeyCheck};
use crate::Result;

/// One provisioning step. Dynamic content is carried by `PushContent`/`Push` (as bytes over
/// the SSH channel), never interpolated into `Run` command strings.
#[derive(Debug, Clone, Serialize)]
pub enum Step {
    /// An idempotent privileged command (no untrusted interpolation).
    Run { desc: String, cmd: String },
    /// Install a local file at a root-owned path.
    Push {
        desc: String,
        local: PathBuf,
        remote: String,
        mode: String,
    },
    /// Install generated, root-owned content at a path.
    PushContent {
        desc: String,
        content: String,
        remote: String,
        mode: String,
    },
}

impl Step {
    pub fn desc(&self) -> &str {
        match self {
            Step::Run { desc, .. } | Step::Push { desc, .. } | Step::PushContent { desc, .. } => {
                desc
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub desc: String,
    pub kind: String,
    pub remote: Option<String>,
    pub status: i32,
    pub changed: bool,
    /// True only for a dry-run entry: the step is part of the proposed plan but was not run.
    pub planned: bool,
    /// True only when the executor actually attempted the remote step.
    pub executed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallManifest {
    pub host: String,
    pub bootstrap_user: String,
    pub steps: Vec<StepResult>,
    pub ok: bool,
}

/// Run an ordered step list over the bootstrap transport, appending to `steps`/`ok`. Stops at
/// the first failing step (a half-provisioned host is reported, not silently continued).
fn run_steps(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
    plan: Vec<Step>,
    steps: &mut Vec<StepResult>,
    ok: &mut bool,
) -> Result<()> {
    if !*ok {
        return Ok(());
    }
    for step in plan {
        let (kind, remote, outcome) = run_step(transport, target, key_path, host_key, &step)?;
        let step_ok = outcome.status == 0;
        steps.push(outcome_result(
            step.desc(),
            kind,
            remote.as_deref(),
            &outcome,
            transport.dry_run,
        ));
        if !step_ok {
            *ok = false;
            break;
        }
    }
    Ok(())
}

fn manifest(target: &BootstrapTarget, steps: Vec<StepResult>, ok: bool) -> InstallManifest {
    InstallManifest {
        host: target.host.clone(),
        bootstrap_user: target.user.clone(),
        steps,
        ok,
    }
}

/// Stage the control public key and execute the current onboarding plan.
pub fn execute_plan(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
    control_pubkey: &str,
    authkey_stage: &str,
    plan: Vec<Step>,
) -> Result<(InstallManifest, Vec<StepResult>, bool)> {
    let mut steps = Vec::new();
    let mut ok = true;
    let stage = write_temp(control_pubkey.trim_end().as_bytes())?;
    let staged = transport.push(
        target,
        key_path,
        host_key,
        stage.path(),
        authkey_stage,
        "0644",
    )?;
    steps.push(outcome_result(
        "stage control key",
        "push",
        Some(authkey_stage),
        &staged,
        transport.dry_run,
    ));
    ok &= staged.status == 0;
    run_steps(
        transport, target, key_path, host_key, plan, &mut steps, &mut ok,
    )?;
    Ok((manifest(target, steps.clone(), ok), steps, ok))
}

fn run_step(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
    step: &Step,
) -> Result<(&'static str, Option<String>, BootstrapOutcome)> {
    match step {
        Step::Run { cmd, .. } => Ok(("run", None, transport.run(target, key_path, host_key, cmd)?)),
        Step::Push {
            local,
            remote,
            mode,
            ..
        } => Ok((
            "push",
            Some(remote.clone()),
            transport.push(target, key_path, host_key, local, remote, mode)?,
        )),
        Step::PushContent {
            content,
            remote,
            mode,
            ..
        } => {
            let tmp = write_temp(content.as_bytes())?;
            Ok((
                "push",
                Some(remote.clone()),
                transport.push(target, key_path, host_key, tmp.path(), remote, mode)?,
            ))
        }
    }
}

fn outcome_result(
    desc: &str,
    kind: &str,
    remote: Option<&str>,
    outcome: &BootstrapOutcome,
    dry_run: bool,
) -> StepResult {
    StepResult {
        desc: desc.to_string(),
        kind: kind.to_string(),
        remote: remote.map(str::to_string),
        status: outcome.status,
        changed: !dry_run && outcome.status == 0,
        planned: dry_run,
        executed: !dry_run,
    }
}

/// A minimal owned temp file (avoids a tempfile dependency). Removed on drop.
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_temp(bytes: &[u8]) -> Result<TempFile> {
    let path = std::env::temp_dir().join(format!("ouro-onboard-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(TempFile { path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_step_result_is_executed_not_planned() {
        let outcome = BootstrapOutcome {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
        };
        let result = outcome_result(
            "install binary",
            "push",
            Some("/usr/local/bin/ouro-ops"),
            &outcome,
            false,
        );
        assert!(result.changed);
        assert!(!result.planned);
        assert!(result.executed);
    }
}
