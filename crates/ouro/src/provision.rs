//! S0017 p1-2 — `ouro-ops init`: arm a bare machine into a constrained target.
//!
//! Builds the SAME security baseline the E2E bed bakes into `node/Dockerfile` — the two
//! principals (`ouro-exec`/`ouro-diag`), the root-owned `ouro-tool-run` wrapper + sudoers
//! confinement, pubkey-only sshd, the `ouro-ops` binary, and the control key — but against a
//! real bare host over the privileged bootstrap transport (`bootstrap.rs`), run once by
//! `ouro-ops init` as an existing sudo user.
//!
//! Injection posture: every dynamic value (the operator's public key, the bootstrap username)
//! travels as FILE CONTENT / stdin, never embedded in a remote shell command, so there is no
//! shell-injection surface from user input. The username is additionally validated by the CLI.
//! Each step is idempotent (guarded `useradd`, `install` overwrite), so re-running converges.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::bootstrap::{BootstrapOutcome, BootstrapTarget, BootstrapTransport, HostKeyCheck};
use crate::Result;

/// The fixed root-owned wrapper: sudoers lets `ouro-exec` run ONLY this, and it only ever
/// execs `ouro-ops tool run "$@"` — so a confined principal cannot invoke other subcommands.
pub const WRAPPER: &str = "#!/bin/sh\nexec /usr/local/bin/ouro-ops tool run \"$@\"\n";

/// sudoers confines `ouro-exec` to the wrapper (NOPASSWD, env reset, fixed secure_path).
pub const SUDOERS: &str = concat!(
    "Defaults:ouro-exec env_reset, secure_path=\"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\"\n",
    "ouro-exec ALL=(root) NOPASSWD: /usr/local/sbin/ouro-tool-run\n",
);

/// sshd drop-in: pubkey-only, no passwords, no root login, and only the two principals plus
/// the operator's bootstrap account may log in (keeping the bootstrap account avoids ever
/// locking the operator out and keeps `init` re-runnable; removing it is a deinit/hardening
/// choice, not init's).
pub fn sshd_conf(bootstrap_user: &str) -> String {
    format!(
        "PasswordAuthentication no\n\
         KbdInteractiveAuthentication no\n\
         PermitRootLogin no\n\
         PubkeyAuthentication yes\n\
         AllowUsers ouro-exec ouro-diag {bootstrap_user}\n"
    )
}

/// One provisioning step. Dynamic content is carried by `PushContent`/`Push` (as bytes over
/// the SSH channel), never interpolated into `Run` command strings.
#[derive(Debug, Clone, Serialize)]
pub enum Step {
    /// An idempotent privileged command (no untrusted interpolation).
    Run { desc: String, cmd: String },
    /// Install a local file (the `ouro-ops` binary) at a root-owned path.
    Push { desc: String, local: PathBuf, remote: String, mode: String },
    /// Install generated, root-owned content (config files) at a path.
    PushContent { desc: String, content: String, remote: String, mode: String },
}

impl Step {
    pub fn desc(&self) -> &str {
        match self {
            Step::Run { desc, .. } | Step::Push { desc, .. } | Step::PushContent { desc, .. } => desc,
        }
    }
}

/// The ordered install recipe. `bootstrap_user` is only used as sshd `AllowUsers` content;
/// `ouro_binary` is the local `ouro-ops` to push (default: the running executable).
pub fn init_plan(bootstrap_user: &str, ouro_binary: &Path) -> Vec<Step> {
    let run = |desc: &str, cmd: &str| Step::Run { desc: desc.into(), cmd: cmd.into() };
    let content = |desc: &str, content: String, remote: &str, mode: &str| Step::PushContent {
        desc: desc.into(),
        content,
        remote: remote.into(),
        mode: mode.into(),
    };
    vec![
        // Principals (idempotent: skip if the account already exists).
        run("create ouro-exec", "id -u ouro-exec >/dev/null 2>&1 || useradd -m -s /bin/bash ouro-exec"),
        run("create ouro-diag", "id -u ouro-diag >/dev/null 2>&1 || useradd -m -s /bin/bash ouro-diag"),
        run("create node", "id -u node >/dev/null 2>&1 || useradd -r -s /usr/sbin/nologin node"),
        // The single binary (same one that ran init).
        Step::Push {
            desc: "install ouro-ops binary".into(),
            local: ouro_binary.to_path_buf(),
            remote: "/usr/local/bin/ouro-ops".into(),
            mode: "0755".into(),
        },
        // Confinement: wrapper + sudoers (then validate the sudoers with visudo).
        content("install tool-run wrapper", WRAPPER.to_string(), "/usr/local/sbin/ouro-tool-run", "0755"),
        content("install sudoers confinement", SUDOERS.to_string(), "/etc/sudoers.d/ouro-exec", "0440"),
        run("validate sudoers", "visudo -cf /etc/sudoers.d/ouro-exec"),
        // Auth posture (pubkey-only, no root); keeps the bootstrap account so init is re-runnable.
        content("harden sshd (pubkey-only)", sshd_conf(bootstrap_user), "/etc/ssh/sshd_config.d/10-ouro.conf", "0644"),
        // Control key -> ouro-exec's authorized_keys. Staged root-owned, then install-chowned to
        // ouro-exec (the pubkey travels as file content, never a shell argument).
        run(
            "prepare ouro-exec .ssh",
            "install -d -m 0700 -o ouro-exec -g ouro-exec /home/ouro-exec/.ssh",
        ),
        run(
            "install control key",
            "install -m 0600 -o ouro-exec -g ouro-exec /tmp/ouro-init-authkey /home/ouro-exec/.ssh/authorized_keys \
             && rm -f /tmp/ouro-init-authkey",
        ),
        // Apply the sshd change only after `sshd -t` validates it (never break login on a typo).
        run(
            "reload sshd",
            "sshd -t && { systemctl reload ssh 2>/dev/null || service ssh reload 2>/dev/null || systemctl reload sshd 2>/dev/null || true; }",
        ),
    ]
}

/// The control public key is staged to this fixed temp path before `install control key` moves
/// it into place — inserted by the executor right before that step.
pub const AUTHKEY_STAGE: &str = "/tmp/ouro-init-authkey";

/// `ouro-ops deinit` recipe — the reverse of init, in a SAFE order: strip the confinement
/// mechanism (binary/wrapper/sudoers) first, then restore the default sshd posture, and remove
/// the access principals (ouro-exec/ouro-diag) LAST so a mid-run failure never orphans the box
/// without a way back in (the bootstrap sudo user, which init kept, is never touched). Only
/// unambiguously ouro-owned artifacts are removed. The shared `node` account MAY be a real
/// service account, so it is left unless `remove_node` is set. Every step is idempotent.
pub fn deinit_plan(remove_node: bool) -> Vec<Step> {
    let run = |desc: &str, cmd: &str| Step::Run { desc: desc.into(), cmd: cmd.into() };
    let mut steps = vec![
        run("remove ouro-ops binary", "rm -f /usr/local/bin/ouro-ops"),
        run("remove tool-run wrapper", "rm -f /usr/local/sbin/ouro-tool-run"),
        run("remove sudoers confinement", "rm -f /etc/sudoers.d/ouro-exec"),
        // Restore default sshd (only if the result still validates), then reload.
        run(
            "restore sshd (remove hardening)",
            "rm -f /etc/ssh/sshd_config.d/10-ouro.conf && sshd -t && \
             { systemctl reload ssh 2>/dev/null || service ssh reload 2>/dev/null || systemctl reload sshd 2>/dev/null || true; }",
        ),
        // Access principals removed LAST (userdel -r takes homes + authorized_keys with them).
        run("remove ouro-exec", "id -u ouro-exec >/dev/null 2>&1 && userdel -r ouro-exec 2>/dev/null || true"),
        run("remove ouro-diag", "id -u ouro-diag >/dev/null 2>&1 && userdel -r ouro-diag 2>/dev/null || true"),
    ];
    if remove_node {
        steps.push(run("remove node account", "id -u node >/dev/null 2>&1 && userdel -r node 2>/dev/null || true"));
    }
    steps
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub desc: String,
    pub kind: String,
    pub remote: Option<String>,
    pub status: i32,
    pub changed: bool,
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
        steps.push(outcome_result(step.desc(), kind, remote.as_deref(), &outcome));
        if !step_ok {
            *ok = false;
            break;
        }
    }
    Ok(())
}

fn manifest(target: &BootstrapTarget, steps: Vec<StepResult>, ok: bool) -> InstallManifest {
    InstallManifest { host: target.host.clone(), bootstrap_user: target.user.clone(), steps, ok }
}

/// `ouro-ops init`: stage the control pubkey, then run the install plan. Returns an auditable
/// install manifest (p1-4). Dry-run reports every step as a planned no-op.
pub fn execute(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
    control_pubkey: &str,
    ouro_binary: &Path,
) -> Result<InstallManifest> {
    let mut steps = Vec::new();
    let mut ok = true;

    // Stage the control pubkey (root-owned temp) before the plan installs it.
    let stage = write_temp(control_pubkey.trim_end().as_bytes())?;
    let staged = transport.push(target, key_path, host_key, stage.path(), AUTHKEY_STAGE, "0644")?;
    steps.push(outcome_result("stage control key", "push", Some(AUTHKEY_STAGE), &staged));
    ok &= staged.status == 0;

    run_steps(transport, target, key_path, host_key, init_plan(&target.user, ouro_binary), &mut steps, &mut ok)?;
    Ok(manifest(target, steps, ok))
}

/// `ouro-ops deinit`: run the removal plan (see `deinit_plan`). Returns a manifest of what was
/// removed. The caller is responsible for the running-node safety gate (see `node_is_running`).
pub fn execute_deinit(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
    remove_node: bool,
) -> Result<InstallManifest> {
    let mut steps = Vec::new();
    let mut ok = true;
    run_steps(transport, target, key_path, host_key, deinit_plan(remove_node), &mut steps, &mut ok)?;
    Ok(manifest(target, steps, ok))
}

/// Whether a cardano-node is running on the target (a privileged read used by `deinit` to
/// refuse by default rather than strand a running node). `None` on a transport/probe error
/// (fail closed — the caller should refuse). Dry-run reports not-running.
pub fn node_is_running(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
) -> Option<bool> {
    // `[c]ardano-node run` matches a real node but NOT this pgrep's own `sh -c` cmdline (which
    // contains the literal pattern) — avoids the classic pgrep-f self/parent match.
    let outcome = transport
        .run(target, key_path, host_key, "pgrep -f '[c]ardano-node run' >/dev/null 2>&1 && echo RUNNING || echo STOPPED")
        .ok()?;
    if outcome.status != 0 {
        return None;
    }
    Some(outcome.stdout.trim() == "RUNNING")
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
        Step::Push { local, remote, mode, .. } => Ok((
            "push",
            Some(remote.clone()),
            transport.push(target, key_path, host_key, local, remote, mode)?,
        )),
        Step::PushContent { content, remote, mode, .. } => {
            let tmp = write_temp(content.as_bytes())?;
            Ok((
                "push",
                Some(remote.clone()),
                transport.push(target, key_path, host_key, tmp.path(), remote, mode)?,
            ))
        }
    }
}

fn outcome_result(desc: &str, kind: &str, remote: Option<&str>, o: &BootstrapOutcome) -> StepResult {
    StepResult {
        desc: desc.to_string(),
        kind: kind.to_string(),
        remote: remote.map(str::to_string),
        status: o.status,
        // Without a target we cannot tell converged-vs-changed; report changed on success.
        changed: o.status == 0,
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
    // Uniqueness from a v4 uuid (Math.random-free); no timestamp needed.
    let path = std::env::temp_dir().join(format!("ouro-init-{}", uuid::Uuid::new_v4().simple()));
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
    use std::path::Path;

    #[test]
    fn plan_covers_the_baseline_in_a_safe_order() {
        let plan = init_plan("ubuntu", Path::new("/usr/local/bin/ouro-ops"));
        let descs: Vec<&str> = plan.iter().map(Step::desc).collect();
        // Principals before the binary; wrapper+sudoers before the validate; authorized_keys
        // staged then installed; sshd reloaded last (after validation).
        let pos = |d: &str| descs.iter().position(|x| *x == d).unwrap_or_else(|| panic!("missing step: {d}"));
        assert!(pos("create ouro-exec") < pos("install ouro-ops binary"));
        assert!(pos("install sudoers confinement") < pos("validate sudoers"));
        assert!(pos("prepare ouro-exec .ssh") < pos("install control key"));
        assert!(pos("reload sshd") == descs.len() - 1, "sshd reload must be last");
    }

    #[test]
    fn no_dynamic_value_is_interpolated_into_a_run_command() {
        // A hostile bootstrap username must NOT reach any Run command string (it only appears as
        // sshd config CONTENT). This is the anti-injection invariant for provisioning.
        let plan = init_plan("evil; rm -rf / #", Path::new("/x"));
        for step in &plan {
            if let Step::Run { cmd, .. } = step {
                assert!(!cmd.contains("evil"), "username leaked into a Run command: {cmd}");
            }
        }
        // It DOES appear in the sshd config content (as an AllowUsers entry).
        assert!(plan.iter().any(|s| matches!(s, Step::PushContent { content, .. } if content.contains("evil; rm -rf / #"))));
    }

    #[test]
    fn sshd_conf_is_pubkey_only_no_root_and_keeps_bootstrap_user() {
        let c = sshd_conf("ubuntu");
        assert!(c.contains("PasswordAuthentication no"));
        assert!(c.contains("PermitRootLogin no"));
        assert!(c.contains("AllowUsers ouro-exec ouro-diag ubuntu"));
    }

    #[test]
    fn wrapper_only_runs_tool_run() {
        assert!(WRAPPER.contains("ouro-ops tool run \"$@\""));
        assert!(!WRAPPER.contains("confirm"));
    }

    #[test]
    fn deinit_removes_access_principals_last_and_spares_node_by_default() {
        let plan = deinit_plan(false);
        let descs: Vec<&str> = plan.iter().map(Step::desc).collect();
        let pos = |d: &str| descs.iter().position(|x| *x == d).unwrap_or_else(|| panic!("missing: {d}"));
        // Confinement + binary stripped before the access principals are removed.
        assert!(pos("remove sudoers confinement") < pos("remove ouro-exec"));
        assert!(pos("restore sshd (remove hardening)") < pos("remove ouro-exec"));
        // Access principals are the LAST things removed (never orphan the box mid-run).
        assert!(pos("remove ouro-diag") == descs.len() - 1);
        // The shared `node` account is spared by default.
        assert!(!descs.iter().any(|d| d.contains("node account")));
        assert!(deinit_plan(true).iter().any(|s| s.desc().contains("node account")));
    }

    #[test]
    fn deinit_dry_run_reports_removal_manifest() {
        let transport = BootstrapTransport::new(true);
        let target = BootstrapTarget { host: "10.0.0.10".into(), port: 22, user: "ubuntu".into() };
        let m = execute_deinit(&transport, &target, Path::new("/k"), HostKeyCheck::AcceptNew, false).unwrap();
        assert!(m.ok);
        assert_eq!(m.steps.len(), deinit_plan(false).len());
        // dry-run node check reports not-running.
        assert_eq!(node_is_running(&transport, &target, Path::new("/k"), HostKeyCheck::AcceptNew), Some(false));
    }

    #[test]
    fn dry_run_execute_reports_every_step_without_touching_anything() {
        let transport = BootstrapTransport::new(true); // no-op
        let target = BootstrapTarget { host: "10.0.0.10".into(), port: 22, user: "ubuntu".into() };
        let manifest = execute(
            &transport,
            &target,
            Path::new("/k"),
            HostKeyCheck::AcceptNew,
            "ssh-ed25519 AAAAmockkey operator@control",
            Path::new("/usr/local/bin/ouro-ops"),
        )
        .unwrap();
        assert!(manifest.ok);
        // stage + the full plan.
        assert_eq!(manifest.steps.len(), 1 + init_plan("ubuntu", Path::new("/x")).len());
        assert!(manifest.steps.iter().all(|s| s.status == 0));
        assert_eq!(manifest.host, "10.0.0.10");
    }
}
