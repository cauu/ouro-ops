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
pub const INBOX_WRAPPER: &str = "/usr/local/sbin/ouro-inbox-stage";

/// The confined write principal `ouro-ops onboard` creates (must match `onboard.rs`).
pub const OP_PRINCIPAL: &str = "ouro-op";

fn shell_quote(v: &str) -> String {
    // Single-quote and escape embedded single quotes: 'a'\''b'. ssh reassembles argv into a remote
    // shell command, so every dynamic field must be quoted.
    format!("'{}'", v.replace('\'', "'\\''"))
}

fn base_ssh(port: u16, key: &Path, known_hosts: &Path, user: &str, host: &str) -> Vec<String> {
    vec![
        "-F".into(),
        "/dev/null".into(),
        "-p".into(),
        port.to_string(),
        "-o".into(),
        "IdentityFile=none".into(),
        "-o".into(),
        "IdentityAgent=none".into(),
        "-i".into(),
        key.display().to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "IdentitiesOnly=yes".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", known_hosts.display()),
        "-o".into(),
        "GlobalKnownHostsFile=/dev/null".into(),
        "-o".into(),
        "StrictHostKeyChecking=yes".into(), // pinned host key; no accept-new TOFU
        format!("{user}@{host}"),
    ]
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Build the only SSH transport used by S0020 operations. The target receives runner bytes on
/// stdin, stores them beneath a run-unique 0700 directory, checks the exact control-known SHA-256,
/// executes the closed target argv with a clean root environment, and removes the directory on
/// every shell exit. Neither the agent nor target selects a binary/path/digest, and no shared PATH
/// entry or target Ouro installation is involved.
pub fn ephemeral_runner_dispatch_argv(
    host: &str,
    port: u16,
    user: &str,
    key: &Path,
    known_hosts: &Path,
    runner_sha256: &str,
    target_args: &[String],
) -> crate::Result<Vec<String>> {
    crate::onboard::validate_bootstrap_user(user)?;
    if !valid_sha256(runner_sha256) {
        return Err(crate::OuroError::Validation(
            "ephemeral runner SHA-256 must be 64 lowercase hex characters".into(),
        ));
    }
    if target_args.is_empty() {
        return Err(crate::OuroError::InvalidArgs(
            "ephemeral runner requires one closed target command".into(),
        ));
    }
    let target = target_args
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let expected = shell_quote(runner_sha256);
    let remote = format!(
        "set -eu\n\
         umask 077\n\
         run_dir=$(mktemp -d /tmp/ouro-run.XXXXXXXXXX)\n\
         cleanup() {{ rm -rf -- \"$run_dir\"; }}\n\
         trap cleanup EXIT\n\
         trap 'exit 143' HUP INT TERM\n\
         runner=\"$run_dir/ouro-runner\"\n\
         dd of=\"$runner\" bs=65536 status=none\n\
         actual=$(sha256sum \"$runner\" | awk '{{print $1}}')\n\
         if test \"$actual\" != {expected}; then\n\
           echo 'ephemeral runner digest mismatch' >&2\n\
           exit 74\n\
         fi\n\
         chmod 0500 \"$runner\"\n\
         sudo -n env -i HOME=/root PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
           \"$runner\" {target}"
    );
    let mut argv = base_ssh(port, key, known_hosts, user, host);
    argv.push(remote);
    Ok(argv)
}

/// Artifact-bearing variant of the ephemeral transport. Stdin is exactly `runner || payload`; one
/// fixed Python splitter reads both declared lengths in the same process (so buffered reads cannot
/// lose boundary bytes), rejects truncation/trailing bytes, and writes both beneath the private
/// run directory. The payload path is provided only through the clean internal environment.
pub struct EphemeralPayloadInput<'a> {
    pub runner_sha256: &'a str,
    pub runner_size: usize,
    pub payload_sha256: &'a str,
    pub payload_size: u64,
}

pub fn ephemeral_runner_payload_dispatch_argv(
    host: &str,
    port: u16,
    user: &str,
    key: &Path,
    known_hosts: &Path,
    input: EphemeralPayloadInput<'_>,
    target_args: &[String],
) -> crate::Result<Vec<String>> {
    crate::onboard::validate_bootstrap_user(user)?;
    if !valid_sha256(input.runner_sha256) || !valid_sha256(input.payload_sha256) {
        return Err(crate::OuroError::Validation(
            "ephemeral runner and payload SHA-256 values must be lowercase hex".into(),
        ));
    }
    if input.runner_size == 0 || input.payload_size == 0 || target_args.is_empty() {
        return Err(crate::OuroError::InvalidArgs(
            "ephemeral payload dispatch requires non-empty runner, payload and target command"
                .into(),
        ));
    }
    let target = target_args
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let runner_expected = shell_quote(input.runner_sha256);
    let payload_expected = shell_quote(input.payload_sha256);
    let runner_size = input.runner_size;
    let payload_size = input.payload_size;
    let splitter = shell_quote(concat!(
        "import sys\n",
        "src=sys.stdin.buffer\n",
        "def take(path,n):\n",
        " out=open(path,'wb'); left=n\n",
        " while left:\n",
        "  chunk=src.read(min(left,1048576))\n",
        "  if not chunk: raise SystemExit('truncated ephemeral input')\n",
        "  out.write(chunk); left-=len(chunk)\n",
        " out.flush(); out.close()\n",
        "take(sys.argv[1],int(sys.argv[2])); take(sys.argv[3],int(sys.argv[4]))\n",
        "if src.read(1): raise SystemExit('trailing ephemeral input')\n",
    ));
    let remote = format!(
        "set -eu\n\
         umask 077\n\
         run_dir=$(mktemp -d /tmp/ouro-run.XXXXXXXXXX)\n\
         cleanup() {{ rm -rf -- \"$run_dir\"; }}\n\
         trap cleanup EXIT\n\
         trap 'exit 143' HUP INT TERM\n\
         runner=\"$run_dir/ouro-runner\"\n\
         payload=\"$run_dir/public-payload\"\n\
         python3 -c {splitter} \"$runner\" '{runner_size}' \"$payload\" '{payload_size}'\n\
         runner_actual=$(sha256sum \"$runner\" | awk '{{print $1}}')\n\
         payload_actual=$(sha256sum \"$payload\" | awk '{{print $1}}')\n\
         test \"$runner_actual\" = {runner_expected} || {{ echo 'ephemeral runner digest mismatch' >&2; exit 74; }}\n\
         test \"$payload_actual\" = {payload_expected} || {{ echo 'ephemeral payload digest mismatch' >&2; exit 74; }}\n\
         chmod 0500 \"$runner\"\n\
         chmod 0400 \"$payload\"\n\
         sudo -n env -i HOME=/root PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
           OURO_EPHEMERAL_PAYLOAD=\"$payload\" \"$runner\" {target}"
    );
    let mut argv = base_ssh(port, key, known_hosts, user, host);
    argv.push(remote);
    Ok(argv)
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

/// SSH argv for bounded stdin artifact ingress. The only dynamic selector is the closed artifact
/// kind; bytes travel on stdin and are validated/finalized by the root-owned target wrapper.
pub fn inbox_dispatch_argv(
    host: &str,
    port: u16,
    key: &Path,
    known_hosts: &Path,
    artifact_type: &str,
    expected_ref: &str,
) -> Vec<String> {
    let mut argv = base_ssh(port, key, known_hosts, OP_PRINCIPAL, host);
    argv.push("sudo".into());
    argv.push("-n".into());
    argv.push(INBOX_WRAPPER.into());
    argv.push(shell_quote(artifact_type));
    argv.push(shell_quote(expected_ref));
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
    expected_security_digest: &str,
) -> crate::Result<Vec<String>> {
    crate::onboard::validate_bootstrap_user(bootstrap_user)?;
    let mut argv = base_ssh(port, key, known_hosts, bootstrap_user, host);
    argv.push("sudo".into());
    argv.push("-n".into());
    argv.push("env".into());
    argv.push("-i".into());
    argv.push("HOME=/root".into());
    argv.push("OURO_HOME=/var/lib/ouro".into());
    argv.push("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into());
    argv.push("/usr/local/bin/ouro-ops".into());
    argv.push("adopt".into());
    argv.push("--local".into());
    for a in remote_args {
        argv.push(shell_quote(a));
    }
    // The real privileged adoption command is independently parity-bound; a separate identity
    // preflight is useful evidence but cannot substitute for this commit-time argument.
    argv.push("--expect-embedded".into());
    argv.push(shell_quote(expected_security_digest));
    Ok(argv)
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
            &[
                "run".into(),
                "--op".into(),
                "runtime/restart".into(),
                "--local".into(),
            ],
            "sha256:abc",
        );
        let j = argv.join(" ");
        assert!(j.contains("ouro-op@10.0.0.1"), "confined principal");
        assert!(
            !j.contains("ouro-exec@"),
            "must be the onboard-installed ouro-op, not ouro-exec"
        );
        assert!(j.contains("StrictHostKeyChecking=yes"), "host key pinned");
        assert!(j.contains("UserKnownHostsFile=/home/op/.ouro/known_hosts"));
        assert!(j.contains("GlobalKnownHostsFile=/dev/null"));
        assert!(j.contains("-F /dev/null"));
        assert!(j.contains("IdentityFile=none") && j.contains("IdentityAgent=none"));
        assert!(j.contains("IdentitiesOnly=yes"));
        assert!(
            j.contains("sudo -n /usr/local/sbin/ouro-op-run"),
            "fixed wrapper"
        );
        assert!(j.contains("'runtime/restart'"), "op quoted");
        assert!(
            j.contains("--expect-embedded 'sha256:abc'"),
            "parity carried"
        );
        assert_eq!(
            j.matches("--expect-embedded").count(),
            1,
            "parity flag exactly once"
        );
        assert!(!j.contains("accept-new"));
    }

    #[test]
    fn ephemeral_runner_is_private_digest_bound_and_self_cleaning() {
        let argv = ephemeral_runner_dispatch_argv(
            "10.0.0.9",
            22,
            "cardano",
            Path::new("/creds/bp1"),
            Path::new("/kh"),
            &"a".repeat(64),
            &[
                "target".into(),
                "observe".into(),
                "--node".into(),
                "bp1".into(),
            ],
        )
        .unwrap();
        let joined = argv.join(" ");
        assert!(joined.contains("cardano@10.0.0.9"));
        assert!(joined.contains("StrictHostKeyChecking=yes"));
        assert!(joined.contains("mktemp -d /tmp/ouro-run.XXXXXXXXXX"));
        assert!(joined.contains("umask 077"));
        assert!(joined.contains("sha256sum \"$runner\""));
        assert!(joined.contains("chmod 0500 \"$runner\""));
        assert!(joined.contains("trap cleanup EXIT"));
        assert!(joined.contains("rm -rf -- \"$run_dir\""));
        assert!(joined.contains("sudo -n env -i HOME=/root"));
        assert!(joined.contains("\"$runner\" 'target' 'observe' '--node' 'bp1'"));
        assert!(!joined.contains("/usr/local/bin/ouro-ops"));
        assert!(!joined.contains(OP_WRAPPER));
    }

    #[test]
    fn ephemeral_runner_rejects_digest_user_and_empty_command() {
        assert!(ephemeral_runner_dispatch_argv(
            "h",
            22,
            "cardano",
            Path::new("/k"),
            Path::new("/kh"),
            "ABC",
            &["x".into()]
        )
        .is_err());
        assert!(ephemeral_runner_dispatch_argv(
            "h",
            22,
            "-oProxyCommand=id",
            Path::new("/k"),
            Path::new("/kh"),
            &"a".repeat(64),
            &["x".into()]
        )
        .is_err());
        assert!(ephemeral_runner_dispatch_argv(
            "h",
            22,
            "cardano",
            Path::new("/k"),
            Path::new("/kh"),
            &"a".repeat(64),
            &[]
        )
        .is_err());
    }

    #[test]
    fn ephemeral_runner_quotes_every_target_argument() {
        let argv = ephemeral_runner_dispatch_argv(
            "h",
            22,
            "cardano",
            Path::new("/k"),
            Path::new("/kh"),
            &"a".repeat(64),
            &["target".into(), "x'; touch /tmp/pwned #".into()],
        )
        .unwrap();
        let remote = argv.last().unwrap();
        assert!(remote.contains("'x'\\''; touch /tmp/pwned #'"));
    }

    #[test]
    fn ephemeral_payload_is_split_verified_private_and_cleaned() {
        let argv = ephemeral_runner_payload_dispatch_argv(
            "10.0.0.9",
            22,
            "cardano",
            Path::new("/creds/bp1"),
            Path::new("/kh"),
            EphemeralPayloadInput {
                runner_sha256: &"a".repeat(64),
                runner_size: 1234,
                payload_sha256: &"b".repeat(64),
                payload_size: 5678,
            },
            &[
                "target".into(),
                "apply".into(),
                "--approved-candidate".into(),
                "c".repeat(64),
            ],
        )
        .unwrap();
        let joined = argv.join(" ");
        assert!(joined.contains("python3 -c"));
        assert!(joined.contains("truncated ephemeral input"));
        assert!(joined.contains("trailing ephemeral input"));
        assert!(joined.contains("'1234'"));
        assert!(joined.contains("'5678'"));
        assert!(joined.contains("sha256sum \"$runner\""));
        assert!(joined.contains("sha256sum \"$payload\""));
        assert!(joined.contains("chmod 0500 \"$runner\""));
        assert!(joined.contains("chmod 0400 \"$payload\""));
        assert!(joined.contains("OURO_EPHEMERAL_PAYLOAD=\"$payload\""));
        assert!(joined.contains("trap cleanup EXIT"));
        assert!(joined.contains("rm -rf -- \"$run_dir\""));
        assert!(!joined.contains("/usr/local/bin/ouro-ops"));
        assert!(!joined.contains(OP_WRAPPER));
    }

    #[test]
    fn ephemeral_payload_rejects_empty_or_malformed_input() {
        let target = ["target".into(), "apply".into()];
        let build = |runner_digest: &str, runner_size, payload_digest: &str, payload_size| {
            ephemeral_runner_payload_dispatch_argv(
                "h",
                22,
                "cardano",
                Path::new("/k"),
                Path::new("/kh"),
                EphemeralPayloadInput {
                    runner_sha256: runner_digest,
                    runner_size,
                    payload_sha256: payload_digest,
                    payload_size,
                },
                &target,
            )
        };
        assert!(build("bad", 1, &"b".repeat(64), 1).is_err());
        assert!(build(&"a".repeat(64), 1, "BAD", 1).is_err());
        assert!(build(&"a".repeat(64), 0, &"b".repeat(64), 1).is_err());
        assert!(build(&"a".repeat(64), 1, &"b".repeat(64), 0).is_err());
    }

    #[test]
    fn hostile_arg_stays_quoted() {
        let argv = op_dispatch_argv(
            "h",
            22,
            Path::new("/k"),
            Path::new("/kh"),
            &[
                "run".into(),
                "--param".into(),
                "machine=x'; rm -rf / #".into(),
            ],
            "d",
        );
        let j = argv.join(" ");
        // The metachars are inside a single-quoted span; the remote shell sees them as data.
        assert!(
            j.contains("'machine=x'\\''; rm -rf / #'"),
            "hostile arg quoted: {j}"
        );
    }

    #[test]
    fn adopt_dispatch_uses_bootstrap_account() {
        let argv = adopt_dispatch_argv(
            "10.0.0.2",
            22,
            "ubuntu",
            Path::new("/creds/bp1"),
            Path::new("/kh"),
            &["--node".into(), "bp1".into(), "--role".into(), "bp".into()],
            "sha256:abc",
        )
        .unwrap();
        let j = argv.join(" ");
        assert!(j.contains("ubuntu@10.0.0.2"), "bootstrap account");
        assert!(j.contains("sudo -n env -i HOME=/root OURO_HOME=/var/lib/ouro"));
        assert!(
            j.contains("/usr/local/bin/ouro-ops adopt --local"),
            "adopt --local on target"
        );
        assert!(j.contains("'bp1'"));
        assert!(
            j.contains("--expect-embedded 'sha256:abc'"),
            "parity carried"
        );
        assert_eq!(
            j.matches("--expect-embedded").count(),
            1,
            "parity flag exactly once"
        );
    }

    #[test]
    fn adopt_dispatch_rejects_ssh_option_shaped_bootstrap_user() {
        assert!(adopt_dispatch_argv(
            "10.0.0.2",
            22,
            "-oProxyCommand=touch /tmp/pwned",
            Path::new("/creds/bp1"),
            Path::new("/kh"),
            &[],
            "sha256:abc",
        )
        .is_err());
    }

    #[test]
    fn inbox_dispatch_is_pinned_and_fixed() {
        let argv = inbox_dispatch_argv(
            "10.0.0.3",
            22,
            Path::new("/creds/ouro-op"),
            Path::new("/kh"),
            "opcert",
            &format!("opcert-deadbeef@sha256:{}", "a".repeat(64)),
        );
        let joined = argv.join(" ");
        assert!(joined.contains("ouro-op@10.0.0.3"));
        assert!(joined.contains("StrictHostKeyChecking=yes"));
        assert!(joined.contains(
            "sudo -n /usr/local/sbin/ouro-inbox-stage 'opcert' 'opcert-deadbeef@sha256:"
        ));
    }
}
