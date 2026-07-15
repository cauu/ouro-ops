//! S0019 p5-1 — SSH dispatch for the greenfield commands, so `adopt`/`op` actually REACH the
//! target instead of running control-local.
//!
//! Two channels:
//! - `op` dispatch runs as the confined `ouro-op` principal (the write principal `ouro-ops onboard`
//!   installs) through a fixed root-owned wrapper that only ever runs `ouro-ops op "$@"` (a sudoers
//!   allowlist keyed to `ouro-op`) — so a confined principal cannot
//!   invoke other subcommands. Host key is PINNED (StrictHostKeyChecking=yes + ouro known_hosts),
//!   so a swapped target key is refused. Parity (§2.8) rides as `--expect-embedded` (the retained
//!   flag name now carries the complete security identity digest, not only Skills).
//! - `adopt` dispatch runs as the operator's bootstrap account (adoption is an onboarding-class,
//!   privileged action, like S0017 init) and runs `ouro-ops adopt --local "$@"` on the target.
//!
//! The remote command is `--local`: the target runs the pipeline itself (reads the target-side
//! attestation, probes locally, executes locally) rather than re-dispatching. Every dynamic field
//! is shell-quoted so a hostile value can never break out of the remote command.

use std::path::Path;

/// The fixed confined wrapper the `ouro-op` sudoers entry allows for the S0019 op channel. This is
/// exactly what `onboard.rs` installs (`OP_WRAPPER`/`OP_SUDOERS`), so the dispatch principal, the
/// sudoers grant, and the sshd `AllowUsers` all agree on `ouro-op`.
pub const OP_WRAPPER: &str = "/usr/local/sbin/ouro-op-run";

/// The confined write principal `ouro-ops onboard` creates (must match `onboard.rs`).
pub const OP_PRINCIPAL: &str = "ouro-op";

fn shell_quote(v: &str) -> String {
    // Single-quote and escape embedded single quotes: 'a'\''b'. ssh reassembles argv into a remote
    // shell command, so every dynamic field must be quoted.
    format!("'{}'", v.replace('\'', "'\\''"))
}

fn base_ssh(port: u16, key: &Path, known_hosts: &Path, user: &str, host: &str) -> Vec<String> {
    vec![
        "-p".into(),
        port.to_string(),
        "-i".into(),
        key.display().to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", known_hosts.display()),
        "-o".into(),
        "StrictHostKeyChecking=yes".into(), // pinned host key; no accept-new TOFU
        format!("{user}@{host}"),
    ]
}

/// SSH argv to run an `ouro-ops op` command on the target as the confined `ouro-op` principal,
/// through the fixed wrapper. `remote_args` are the op arguments (e.g. `["run","--op",...,"--local"]`).
pub fn op_dispatch_argv(
    host: &str,
    port: u16,
    key: &Path,
    known_hosts: &Path,
    remote_args: &[String],
    expected_security_digest: &str,
) -> Vec<String> {
    let mut argv = base_ssh(port, key, known_hosts, OP_PRINCIPAL, host);
    argv.push("sudo".into());
    argv.push("-n".into());
    argv.push(OP_WRAPPER.into()); // only ever runs `ouro-ops op "$@"`
    for a in remote_args {
        argv.push(shell_quote(a));
    }
    // §2.8 parity: the target compares the complete security identity before executing.
    argv.push("--expect-embedded".into());
    argv.push(shell_quote(expected_security_digest));
    argv
}

/// SSH argv to run `ouro-ops adopt --local` on the target as the operator's bootstrap account
/// (privileged onboarding-class action).
pub fn adopt_dispatch_argv(
    host: &str,
    port: u16,
    bootstrap_user: &str,
    key: &Path,
    known_hosts: &Path,
    remote_args: &[String],
) -> Vec<String> {
    let mut argv = base_ssh(port, key, known_hosts, bootstrap_user, host);
    argv.push("sudo".into());
    argv.push("-n".into());
    argv.push("ouro-ops".into());
    argv.push("adopt".into());
    argv.push("--local".into());
    for a in remote_args {
        argv.push(shell_quote(a));
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn op_dispatch_is_confined_and_pinned() {
        let argv = op_dispatch_argv(
            "10.0.0.1",
            22,
            Path::new("/creds/relay1"),
            Path::new("/home/op/.ouro/known_hosts"),
            &["run".into(), "--op".into(), "runtime/restart".into(), "--local".into()],
            "sha256:abc",
        );
        let j = argv.join(" ");
        assert!(j.contains("ouro-op@10.0.0.1"), "confined principal");
        assert!(!j.contains("ouro-exec@"), "must be the onboard-installed ouro-op, not ouro-exec");
        assert!(j.contains("StrictHostKeyChecking=yes"), "host key pinned");
        assert!(j.contains("UserKnownHostsFile=/home/op/.ouro/known_hosts"));
        assert!(j.contains("sudo -n /usr/local/sbin/ouro-op-run"), "fixed wrapper");
        assert!(j.contains("'runtime/restart'"), "op quoted");
        assert!(j.contains("--expect-embedded 'sha256:abc'"), "parity carried");
        assert!(!j.contains("accept-new"));
    }

    #[test]
    fn hostile_arg_stays_quoted() {
        let argv = op_dispatch_argv(
            "h", 22, Path::new("/k"), Path::new("/kh"),
            &["run".into(), "--param".into(), "machine=x'; rm -rf / #".into()],
            "d",
        );
        let j = argv.join(" ");
        // The metachars are inside a single-quoted span; the remote shell sees them as data.
        assert!(j.contains("'machine=x'\\''; rm -rf / #'"), "hostile arg quoted: {j}");
    }

    #[test]
    fn adopt_dispatch_uses_bootstrap_account() {
        let argv = adopt_dispatch_argv(
            "10.0.0.2", 22, "ubuntu", Path::new("/creds/bp1"),
            Path::new("/kh"), &["--node".into(), "bp1".into(), "--role".into(), "bp".into()],
        );
        let j = argv.join(" ");
        assert!(j.contains("ubuntu@10.0.0.2"), "bootstrap account");
        assert!(j.contains("ouro-ops adopt --local"), "adopt --local on target");
        assert!(j.contains("'bp1'"));
    }
}
