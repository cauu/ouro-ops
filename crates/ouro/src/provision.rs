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

/// p1-9 — root-owned install ledger on the target: records which principals init CREATED vs
/// ADOPTED so deinit reverses only what init created (never deletes a pre-existing account).
pub const LEDGER: &str = "/var/lib/ouro/install-ledger";

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
    // p1-9 install ledger — a root-owned, versioned record of what init CREATED vs ADOPTED
    // (found pre-existing), so deinit can precisely reverse only what init created and never delete
    // a principal that already belonged to the box. Files are adopt-free by design (init only
    // writes new ouro-specific drop-ins — see p1-3), so the ledger tracks the principals, which are
    // the only items that can legitimately pre-exist (e.g. a real `node` service account).
    // Each principal records `created` (init made it) or `adopted` (already present) with the id.
    // Idempotent-safe: a principal's created/adopted verdict is recorded ONCE (on the first init
    // that sees it) and never downgraded on a re-init — otherwise re-running init on an already-
    // provisioned box would see ouro's own accounts as "adopted" and deinit would spare them.
    let principal = |desc: &str, user: &str, useradd: &str| {
        run(
            desc,
            &format!(
                "if grep -q '^principal:{user}:' {LEDGER} 2>/dev/null; then \
                   id -u {user} >/dev/null 2>&1 || {useradd}; \
                 elif id -u {user} >/dev/null 2>&1; then printf 'principal:{user}:adopted\\n' >> {LEDGER}; \
                 else {useradd} && printf 'principal:{user}:created\\n' >> {LEDGER}; fi"
            ),
        )
    };
    vec![
        // Initialize the ledger (root-owned, 0600) ONLY if absent — never truncate an existing one,
        // so idempotent re-inits preserve the original created/adopted verdicts.
        run(
            "init install ledger",
            &format!(
                "install -d -m 0700 -o root -g root /var/lib/ouro && \
                 {{ [ -f {LEDGER} ] || {{ printf 'ledger_version:1\\n' > {LEDGER} && chown root:root {LEDGER} && chmod 0600 {LEDGER}; }}; }}"
            ),
        ),
        // Principals (idempotent: skip if the account already exists) + record created/adopted.
        principal("create ouro-exec", "ouro-exec", "useradd -m -s /bin/bash ouro-exec"),
        principal("create ouro-diag", "ouro-diag", "useradd -m -s /bin/bash ouro-diag"),
        principal("create node", "node", "useradd -r -s /usr/sbin/nologin node"),
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
            "install -m 0600 -o ouro-exec -g ouro-exec /tmp/ouro-init-authkey /home/ouro-exec/.ssh/authorized_keys",
        ),
        // p5-18: the SAME control key also authorizes the read-only diagnostics principal.
        // ouro-diag has NO sudoers entry at all — free-form diagnosis (`ouro-ops diag exec`)
        // is confined by the Unix permission model (cannot write node content, cannot read
        // 0700 secret dirs), not by a command list.
        run(
            "prepare ouro-diag .ssh",
            "install -d -m 0700 -o ouro-diag -g ouro-diag /home/ouro-diag/.ssh",
        ),
        run(
            "install diag key",
            "install -m 0600 -o ouro-diag -g ouro-diag /tmp/ouro-init-authkey /home/ouro-diag/.ssh/authorized_keys \
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
    ];
    // Access principals removed LAST (userdel -r takes homes + authorized_keys with them) — but
    // ONLY the ones the ledger marks `created` (p1-9). An ADOPTED principal (one init found already
    // present) is never deleted; if the ledger is absent (legacy install), preserve = fail-safe.
    let remove_principal = |user: &str| {
        run(
            &format!("remove {user}"),
            &format!(
                "if grep -qx 'principal:{user}:created' {LEDGER} 2>/dev/null; then \
                   id -u {user} >/dev/null 2>&1 && userdel -r {user} 2>/dev/null || true; \
                 else printf '{user} adopted or no ledger; preserved\\n' >&2; fi"
            ),
        )
    };
    steps.push(remove_principal("ouro-exec"));
    steps.push(remove_principal("ouro-diag"));
    if remove_node {
        steps.push(remove_principal("node"));
    }
    // Remove the ledger itself last (only an ouro artifact).
    steps.push(run("remove install ledger", &format!("rm -f {LEDGER}")));
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
    execute_plan(
        transport,
        target,
        key_path,
        host_key,
        control_pubkey,
        init_plan(&target.user, ouro_binary),
    )
    .map(|(m, ..)| m)
}

/// Generic install-plan executor (S0019 p6-1): stage the control pubkey, run any plan, return the
/// manifest. Both `init` (S0017) and the greenfield `onboard` (S0019) build a plan and run it here.
pub fn execute_plan(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
    control_pubkey: &str,
    plan: Vec<Step>,
) -> Result<(InstallManifest, Vec<StepResult>, bool)> {
    let mut steps = Vec::new();
    let mut ok = true;
    let stage = write_temp(control_pubkey.trim_end().as_bytes())?;
    let staged = transport.push(target, key_path, host_key, stage.path(), AUTHKEY_STAGE, "0644")?;
    steps.push(outcome_result("stage control key", "push", Some(AUTHKEY_STAGE), &staged));
    ok &= staged.status == 0;
    run_steps(transport, target, key_path, host_key, plan, &mut steps, &mut ok)?;
    Ok((manifest(target, steps.clone(), ok), steps, ok))
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
        // p5-18: the diag principal is authorized with the SAME control key, right after
        // ouro-exec — and it must NEVER appear in sudoers (its fence is Unix permissions).
        assert!(pos("install control key") < pos("prepare ouro-diag .ssh"));
        assert!(pos("prepare ouro-diag .ssh") < pos("install diag key"));
        assert!(!SUDOERS.contains("ouro-diag"), "ouro-diag must have no sudoers entry");
        assert!(pos("reload sshd") == descs.len() - 1, "sshd reload must be last");
    }

    #[test]
    fn base_install_is_minimal_only_constraint_required_items() {
        // p1-3 base minimization: the base provision installs ONLY what the confinement/dispatch
        // constraints require — no OS packages, no node runtime, and only the allowlisted paths +
        // the three access principals. Deploy (not init) brings the cardano-node runtime.
        let plan = init_plan("ubuntu", Path::new("/usr/local/bin/ouro-ops"));

        // (1) no package manager / network fetch of extra software in any Run command.
        let forbidden = [
            "apt-get install", "apt install", "dnf install", "yum install", "apk add",
            "pip install", "npm install", "curl ", "wget ", "cardano-node",
        ];
        for step in &plan {
            if let Step::Run { cmd, .. } = step {
                for bad in forbidden {
                    assert!(!cmd.contains(bad), "base install pulls extra software: {bad:?} in {cmd:?}");
                }
            }
        }

        // (2) every file the base writes is in the minimal allowlisted set.
        let allowed_paths = [
            "/usr/local/bin/ouro-ops",
            "/usr/local/sbin/ouro-tool-run",
            "/etc/sudoers.d/ouro-exec",
            "/etc/ssh/sshd_config.d/10-ouro.conf",
        ];
        for step in &plan {
            let remote = match step {
                Step::Push { remote, .. } | Step::PushContent { remote, .. } => Some(remote.as_str()),
                Step::Run { .. } => None,
            };
            if let Some(r) = remote {
                assert!(allowed_paths.contains(&r), "base writes an unexpected path: {r}");
            }
        }

        // (3) exactly the three required principals are created — nothing else.
        let useradds: Vec<&str> = plan
            .iter()
            .filter_map(|s| match s { Step::Run { cmd, .. } if cmd.contains("useradd") => Some(cmd.as_str()), _ => None })
            .collect();
        assert_eq!(useradds.len(), 3, "expected exactly 3 principals, got {}", useradds.len());
        for u in ["ouro-exec", "ouro-diag", "node"] {
            assert!(useradds.iter().any(|c| c.contains(u)), "missing required principal {u}");
        }
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
    fn init_records_created_vs_adopted_principals_in_ledger() {
        // p1-9: each principal step records created (init made it) OR adopted (already present)
        // into the root-owned ledger, so deinit can precisely reverse only what init created.
        let plan = init_plan("ubuntu", Path::new("/usr/local/bin/ouro-ops"));
        let descs: Vec<&str> = plan.iter().map(Step::desc).collect();
        // The ledger is initialized (root-owned, 0600) before any principal is recorded.
        let pos = |d: &str| descs.iter().position(|x| *x == d).unwrap_or_else(|| panic!("missing: {d}"));
        assert!(pos("init install ledger") < pos("create ouro-exec"));
        for step in &plan {
            if let Step::Run { desc, cmd } = step {
                if desc.starts_with("create ") {
                    // both branches recorded: adopted if already present, created after useradd.
                    assert!(cmd.contains(":adopted") && cmd.contains(":created"),
                            "principal step must record created/adopted: {cmd}");
                    assert!(cmd.contains(LEDGER), "principal step must write the ledger: {cmd}");
                }
                if desc == "init install ledger" {
                    assert!(cmd.contains("-o root -g root") && cmd.contains("0600"),
                            "ledger must be root-owned 0600: {cmd}");
                }
            }
        }
    }

    #[test]
    fn deinit_removes_access_principals_last_and_spares_node_by_default() {
        let plan = deinit_plan(false);
        let descs: Vec<&str> = plan.iter().map(Step::desc).collect();
        let pos = |d: &str| descs.iter().position(|x| *x == d).unwrap_or_else(|| panic!("missing: {d}"));
        // Confinement + binary stripped before the access principals are removed.
        assert!(pos("remove sudoers confinement") < pos("remove ouro-exec"));
        assert!(pos("restore sshd (remove hardening)") < pos("remove ouro-exec"));
        // Access principals are removed after the mechanism (never orphan the box mid-run); the
        // ledger itself is the very last artifact removed (p1-9).
        assert!(pos("remove ouro-exec") < pos("remove ouro-diag"));
        assert!(pos("remove ouro-diag") < pos("remove install ledger"));
        assert!(pos("remove install ledger") == descs.len() - 1);
        // The shared `node` account is spared by default; --remove-node adds it.
        assert!(!descs.iter().any(|d| *d == "remove node"));
        assert!(deinit_plan(true).iter().any(|s| s.desc() == "remove node"));
        // p1-9: principal removal is ledger-gated (only `created` principals are deleted).
        for step in deinit_plan(true) {
            if let Step::Run { desc, cmd } = step {
                if matches!(desc.as_str(), "remove ouro-exec" | "remove ouro-diag" | "remove node") {
                    assert!(cmd.contains(":created") && cmd.contains("userdel"),
                            "principal removal must be gated on the created-ledger: {cmd}");
                }
            }
        }
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
