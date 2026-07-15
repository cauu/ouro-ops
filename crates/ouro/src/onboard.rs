//! S0019 p6-1 — greenfield host onboarding (`ouro-ops onboard`). Brings a target to the
//! `host-onboarded` state so the S0019 dispatch chain can reach it: installs the two confined
//! principals, the FIXED op wrapper (sudoers: only `ouro-ops op "$@"`), the ouro-ops binary, and a
//! hardened sshd; pins the host key. Operator-initiated via the bootstrap credential.
//!
//! This is a FRESH plan — it does NOT install or depend on the S0017 tool-run wrapper. The write
//! principal is `ouro-op` (confined to `op`); the read principal is `ouro-diag` (unprivileged).

use std::path::Path;

use crate::bootstrap::{BootstrapTarget, BootstrapTransport, HostKeyCheck};
use crate::provision::{execute_plan, InstallManifest, Step};
use crate::Result;

/// The fixed root-owned wrapper the `ouro-op` sudoers entry allows: it ONLY ever runs
/// `ouro-ops op "$@"`, so the confined principal cannot invoke adopt/onboard/other subcommands.
pub const OP_WRAPPER: &str = "#!/bin/sh\nexec /usr/local/bin/ouro-ops op \"$@\"\n";

/// Separate fixed ingress wrapper. It accepts exactly one closed artifact kind and streams stdin
/// into the target-local inbox; it cannot invoke any other ouro command.
pub const INBOX_WRAPPER: &str = "#!/bin/sh\n\
[ \"$#\" -eq 1 ] || exit 64\n\
case \"$1\" in opcert|tx|image) ;; *) exit 64 ;; esac\n\
exec /usr/local/bin/ouro-ops inbox stage --local --type \"$1\" --stdin\n";

/// sudoers confines `ouro-op` to the op wrapper (NOPASSWD, env reset, fixed secure_path).
pub const OP_SUDOERS: &str = concat!(
    "Defaults:ouro-op env_reset, secure_path=\"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\"\n",
    "ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-op-run\n",
    "ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-inbox-stage\n",
);

/// sshd drop-in: pubkey-only, no root, only the two S0019 principals + the bootstrap account.
pub fn sshd_conf(bootstrap_user: &str) -> String {
    format!(
        "PasswordAuthentication no\n\
         KbdInteractiveAuthentication no\n\
         PermitRootLogin no\n\
         PubkeyAuthentication yes\n\
         AllowUsers ouro-op ouro-diag {bootstrap_user}\n"
    )
}

const AUTHKEY_STAGE: &str = "/tmp/ouro-onboard-authkey";

/// Fixed target path for the SHARED confirm secret (p6-3): a control-minted confirm-token is bound
/// with this HMAC key, and the target verifies with the SAME key, so the operator's approval on the
/// control machine is honored by the target. Provisioned here (root-owned, 0400 for ouro-op).
pub const CONFIRM_SECRET_PATH: &str = "/var/lib/ouro/confirm.secret";

/// The greenfield onboard plan (S0019). Idempotent; installs the S0019 confinement, binary, the
/// /var/lib/ouro dir the adoption attestation lives in, and the shared confirm secret.
pub fn onboard_plan(bootstrap_user: &str, ouro_binary: &Path, confirm_secret: &str) -> Vec<Step> {
    let run = |desc: &str, cmd: &str| Step::Run { desc: desc.into(), cmd: cmd.into() };
    let content = |desc: &str, content: String, remote: &str, mode: &str| Step::PushContent {
        desc: desc.into(),
        content,
        remote: remote.into(),
        mode: mode.into(),
    };
    // Create a confined principal only if absent (idempotent).
    let principal = |desc: &str, user: &str, cmd: &str| {
        run(desc, &format!("id -u {user} >/dev/null 2>&1 || {cmd}"))
    };
    vec![
        run("create attestation reader group", "getent group ouro-attest >/dev/null 2>&1 || groupadd --system ouro-attest"),
        // The two S0019 principals: ouro-op (write, via wrapper) and ouro-diag (read, unprivileged).
        principal("create ouro-op", "ouro-op", "useradd -m -s /bin/bash ouro-op"),
        principal("create ouro-diag", "ouro-diag", "useradd -m -s /bin/bash ouro-diag"),
        run("grant attestation read group", "usermod -a -G ouro-attest ouro-op && usermod -a -G ouro-attest ouro-diag"),
        // Root-owned state; only the dedicated read group can traverse/read attestations.
        run("prepare /var/lib/ouro", "install -d -m 0750 -o root -g ouro-attest /var/lib/ouro && install -d -m 0700 -o root -g root /var/lib/ouro/inbox"),
        // The single binary (the same one that ran onboard, pushed to the target).
        Step::Push {
            desc: "install ouro-ops binary".into(),
            local: ouro_binary.to_path_buf(),
            remote: "/usr/local/bin/ouro-ops".into(),
            mode: "0755".into(),
        },
        // Shared confirm secret (p6-3): the target verifies a control-minted confirm-token with the
        // SAME HMAC key, so the operator's approval on control is honored here.
        content("install shared confirm secret", confirm_secret.to_string(), CONFIRM_SECRET_PATH, "0400"),
        run("own confirm secret", &format!("chown root:root {CONFIRM_SECRET_PATH}")),
        // Confinement: the op wrapper + sudoers, validated by visudo.
        content("install op wrapper", OP_WRAPPER.to_string(), "/usr/local/sbin/ouro-op-run", "0755"),
        content("install inbox wrapper", INBOX_WRAPPER.to_string(), "/usr/local/sbin/ouro-inbox-stage", "0755"),
        content("install sudoers confinement", OP_SUDOERS.to_string(), "/etc/sudoers.d/ouro-op", "0440"),
        run("validate sudoers", "visudo -cf /etc/sudoers.d/ouro-op"),
        // Auth posture: pubkey-only, no root, only the S0019 principals + bootstrap.
        content("harden sshd (pubkey-only)", sshd_conf(bootstrap_user), "/etc/ssh/sshd_config.d/20-ouro-s0019.conf", "0644"),
        // Control key -> BOTH principals' authorized_keys (staged root-owned, then install-chowned).
        run("prepare ouro-op .ssh", "install -d -m 0700 -o ouro-op -g ouro-op /home/ouro-op/.ssh"),
        run(
            "install control key (ouro-op)",
            &format!("install -m 0600 -o ouro-op -g ouro-op {AUTHKEY_STAGE} /home/ouro-op/.ssh/authorized_keys"),
        ),
        run("prepare ouro-diag .ssh", "install -d -m 0700 -o ouro-diag -g ouro-diag /home/ouro-diag/.ssh"),
        run(
            "install control key (ouro-diag)",
            &format!(
                "install -m 0600 -o ouro-diag -g ouro-diag {AUTHKEY_STAGE} /home/ouro-diag/.ssh/authorized_keys \
                 && rm -f {AUTHKEY_STAGE}"
            ),
        ),
        run(
            "reload sshd",
            "sshd -t && { systemctl reload ssh 2>/dev/null || service ssh reload 2>/dev/null || systemctl reload sshd 2>/dev/null || true; }",
        ),
    ]
}

/// Run the greenfield onboard plan on a target. Returns the auditable install manifest.
pub fn execute_onboard(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
    control_pubkey: &str,
    ouro_binary: &Path,
) -> Result<InstallManifest> {
    // The shared confirm secret provisioned to the target = the control's own confirm/tool-run
    // secret, so a control-minted confirm-token verifies target-side (p6-3).
    let paths = crate::config::ConfigPaths::discover();
    let confirm_secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
    execute_plan(
        transport,
        target,
        key_path,
        host_key,
        control_pubkey,
        AUTHKEY_STAGE,
        onboard_plan(&target.user, ouro_binary, &confirm_secret),
    )
    .map(|(m, ..)| m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn descs(plan: &[Step]) -> Vec<&str> {
        plan.iter().map(|s| s.desc()).collect()
    }

    #[test]
    fn plan_installs_s0019_confinement_not_s0017() {
        let plan = onboard_plan("ubuntu", Path::new("/tmp/ouro-ops"), "secret");
        let d = descs(&plan);
        assert!(d.contains(&"create ouro-op") && d.contains(&"create ouro-diag"));
        assert!(d.contains(&"install op wrapper"));
        assert!(d.contains(&"install inbox wrapper"));
        // The op wrapper only runs `ouro-ops op`, never tool run.
        assert!(OP_WRAPPER.contains("ouro-ops op \"$@\""));
        assert!(!OP_WRAPPER.contains("tool run"), "greenfield: no S0017 tool-run wrapper");
        // sudoers confines ouro-op to the op wrapper only.
        assert!(OP_SUDOERS.contains("ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-op-run"));
        assert!(OP_SUDOERS.contains("/usr/local/sbin/ouro-inbox-stage"));
        assert!(!OP_SUDOERS.contains("ouro-tool-run"));
    }

    #[test]
    fn ordering_binary_and_wrapper_before_keys() {
        let plan = onboard_plan("ubuntu", Path::new("/tmp/ouro-ops"), "secret");
        let d = descs(&plan);
        let pos = |x: &str| d.iter().position(|s| *s == x).unwrap();
        assert!(pos("install ouro-ops binary") < pos("install op wrapper"));
        assert!(pos("install op wrapper") < pos("install control key (ouro-op)"));
        assert!(pos("create attestation reader group") == 0, "attestation group first");
        assert!(pos("reload sshd") == d.len() - 1, "reload sshd last");
    }

    #[test]
    fn sshd_allows_only_s0019_principals_and_bootstrap() {
        let c = sshd_conf("ubuntu");
        assert!(c.contains("PermitRootLogin no") && c.contains("PasswordAuthentication no"));
        assert!(c.contains("AllowUsers ouro-op ouro-diag ubuntu"));
        assert!(!c.contains("ouro-exec"), "greenfield principals only");
    }

    #[test]
    fn executor_stages_key_at_the_path_the_onboard_plan_consumes() {
        let transport = BootstrapTransport::new(true);
        let target = BootstrapTarget {
            host: "10.0.0.10".into(),
            port: 22,
            user: "ubuntu".into(),
        };
        let (manifest, _, _) = crate::provision::execute_plan(
            &transport,
            &target,
            Path::new("/k"),
            HostKeyCheck::AcceptNew,
            "ssh-ed25519 AAAA0123456789abcdef operator@control",
            AUTHKEY_STAGE,
            onboard_plan("ubuntu", Path::new("/tmp/ouro-ops"), "secret"),
        )
        .unwrap();
        assert_eq!(manifest.steps[0].remote.as_deref(), Some(AUTHKEY_STAGE));
        assert!(onboard_plan("ubuntu", Path::new("/tmp/ouro-ops"), "secret")
            .iter()
            .filter_map(|step| match step { Step::Run { cmd, .. } => Some(cmd), _ => None })
            .any(|command| command.contains(AUTHKEY_STAGE)));
    }
}
