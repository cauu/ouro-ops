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

/// sudoers confines `ouro-op` to the op wrapper (NOPASSWD, env reset, fixed secure_path).
pub const OP_SUDOERS: &str = concat!(
    "Defaults:ouro-op env_reset, secure_path=\"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\"\n",
    "ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-op-run\n",
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

/// The greenfield onboard plan (S0019). Idempotent; installs the S0019 confinement, binary, and
/// the /var/lib/ouro dir the adoption attestation lives in.
pub fn onboard_plan(bootstrap_user: &str, ouro_binary: &Path) -> Vec<Step> {
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
        // Root-owned state dir for the attestation (§2.3).
        run("prepare /var/lib/ouro", "install -d -m 0755 -o root -g root /var/lib/ouro"),
        // The two S0019 principals: ouro-op (write, via wrapper) and ouro-diag (read, unprivileged).
        principal("create ouro-op", "ouro-op", "useradd -m -s /bin/bash ouro-op"),
        principal("create ouro-diag", "ouro-diag", "useradd -m -s /bin/bash ouro-diag"),
        // The single binary (the same one that ran onboard, pushed to the target).
        Step::Push {
            desc: "install ouro-ops binary".into(),
            local: ouro_binary.to_path_buf(),
            remote: "/usr/local/bin/ouro-ops".into(),
            mode: "0755".into(),
        },
        // Confinement: the op wrapper + sudoers, validated by visudo.
        content("install op wrapper", OP_WRAPPER.to_string(), "/usr/local/sbin/ouro-op-run", "0755"),
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
    execute_plan(
        transport,
        target,
        key_path,
        host_key,
        control_pubkey,
        onboard_plan(&target.user, ouro_binary),
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
        let plan = onboard_plan("ubuntu", Path::new("/tmp/ouro-ops"));
        let d = descs(&plan);
        assert!(d.contains(&"create ouro-op") && d.contains(&"create ouro-diag"));
        assert!(d.contains(&"install op wrapper"));
        // The op wrapper only runs `ouro-ops op`, never tool run.
        assert!(OP_WRAPPER.contains("ouro-ops op \"$@\""));
        assert!(!OP_WRAPPER.contains("tool run"), "greenfield: no S0017 tool-run wrapper");
        // sudoers confines ouro-op to the op wrapper only.
        assert!(OP_SUDOERS.contains("ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-op-run"));
        assert!(!OP_SUDOERS.contains("ouro-tool-run"));
    }

    #[test]
    fn ordering_binary_and_wrapper_before_keys() {
        let plan = onboard_plan("ubuntu", Path::new("/tmp/ouro-ops"));
        let d = descs(&plan);
        let pos = |x: &str| d.iter().position(|s| *s == x).unwrap();
        assert!(pos("install ouro-ops binary") < pos("install op wrapper"));
        assert!(pos("install op wrapper") < pos("install control key (ouro-op)"));
        assert!(pos("prepare /var/lib/ouro") == 0, "attestation dir first");
        assert!(pos("reload sshd") == d.len() - 1, "reload sshd last");
    }

    #[test]
    fn sshd_allows_only_s0019_principals_and_bootstrap() {
        let c = sshd_conf("ubuntu");
        assert!(c.contains("PermitRootLogin no") && c.contains("PasswordAuthentication no"));
        assert!(c.contains("AllowUsers ouro-op ouro-diag ubuntu"));
        assert!(!c.contains("ouro-exec"), "greenfield principals only");
    }
}
