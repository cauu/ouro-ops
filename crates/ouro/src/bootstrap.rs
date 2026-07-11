//! S0017 p1-1 — target-mutating bootstrap transport for `ouro-ops init`.
//!
//! This is a DELIBERATE, isolated break from the per-operation dispatch in `ssh.rs`, which is
//! exec-only and asserts no file transfer (no scp): a running node is driven only through the
//! confined `ouro-exec` → `ouro-tool-run` wrapper. Provisioning a BARE machine, by contrast,
//! must push a binary and write root-owned files ONCE. That privileged, file-writing path lives
//! here — a separate module, used only by `ouro-ops init`, run as an existing sudo user (first
//! access = sudo-user + SSH key), never as `ouro-exec`.
//!
//! Files are pushed over the SSH channel itself (local bytes piped to a remote
//! `sudo sh -c 'cat > tmp && install … tmp dest'`), so no scp/sftp is required on the target and
//! the per-op no-scp invariant in `ssh.rs` is untouched.
//!
//! Security posture (P0-1 decision: convenience mode, honestly labeled): the bootstrap
//! credential is NOT mechanism-isolated from the agent — a poisoned prompt could, via the
//! agent, use it to provision a reachable host. This is documented, not defended; per the
//! user's decision it is delegated to upstream control-machine / agent-runtime security.
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::{OuroError, Result};

/// Single-quote a value for safe inclusion in a remote shell command (same discipline as
/// `ssh.rs`): ssh joins the remote argv into one string and runs it through the target shell,
/// so every dynamic field must be quoted or a metacharacter injects a command.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The first-access endpoint for `ouro-ops init`: an existing account that can `sudo` on the
/// still-bare target (e.g. the cloud image's `ubuntu`/`admin` user), plus the operator's key.
#[derive(Debug, Clone, Serialize)]
pub struct BootstrapTarget {
    pub host: String,
    pub port: u16,
    /// Existing sudo-capable user (NOT `ouro-exec`, which does not exist yet on a bare host).
    pub user: String,
}

/// Host-key policy for the first privileged connection. `accept-new` TOFU-trusts an unseen host
/// (today's default); p3-2/p3-3 replace this with a pinned per-machine known_hosts + `yes`.
#[derive(Debug, Clone, Copy)]
pub enum HostKeyCheck {
    AcceptNew,
    Yes,
}

impl HostKeyCheck {
    fn as_opt(self) -> &'static str {
        match self {
            HostKeyCheck::AcceptNew => "StrictHostKeyChecking=accept-new",
            HostKeyCheck::Yes => "StrictHostKeyChecking=yes",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BootstrapOutcome {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Validate a Unix file mode string (`0755` / `644`): 3–4 octal digits. Rejected values never
/// reach the remote command, so the mode can be inlined unquoted.
fn validate_mode(mode: &str) -> Result<()> {
    let ok = (3..=4).contains(&mode.len()) && mode.bytes().all(|b| (b'0'..=b'7').contains(&b));
    if ok {
        Ok(())
    } else {
        Err(OuroError::Validation(format!(
            "file mode must be 3-4 octal digits, got {mode:?}"
        )))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BootstrapTransport {
    pub dry_run: bool,
}

impl BootstrapTransport {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    /// Common ssh options + `user@host` prefix for a privileged bootstrap connection.
    fn ssh_prefix(target: &BootstrapTarget, key_path: &Path, host_key: HostKeyCheck) -> Vec<String> {
        vec![
            "-p".to_string(),
            target.port.to_string(),
            "-i".to_string(),
            key_path.display().to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            host_key.as_opt().to_string(),
            format!("{}@{}", target.user, target.host),
        ]
    }

    /// `ssh … <user>@<host> sudo -n sh -c '<cmd>'` — run a privileged provisioning command.
    /// `remote_cmd` is shell-quoted, so a crafted value cannot break out of the `sh -c` string.
    pub fn run_argv(
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        remote_cmd: &str,
    ) -> Vec<String> {
        let mut argv = Self::ssh_prefix(target, key_path, host_key);
        argv.extend([
            "sudo".to_string(),
            "-n".to_string(),
            "sh".to_string(),
            "-c".to_string(),
            shell_quote(remote_cmd),
        ]);
        argv
    }

    /// `ssh … sudo -n sh -c 'cat > $t && install -D -m <mode> -o root -g root $t <dest> && …'`
    /// — receive a file on stdin and install it atomically (temp → `install` → cleanup), so a
    /// partial transfer never leaves a half-written binary at `dest`. `dest` is shell-quoted;
    /// `mode` is validated to octal digits. Local file bytes are piped to ssh stdin by `push`.
    pub fn push_argv(
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        remote_path: &str,
        mode: &str,
    ) -> Result<Vec<String>> {
        validate_mode(mode)?;
        let remote = format!(
            "t=$(mktemp) && cat > \"$t\" && install -D -m {mode} -o root -g root \"$t\" {dest} && rm -f \"$t\"",
            mode = mode,
            dest = shell_quote(remote_path),
        );
        Ok(Self::run_argv(target, key_path, host_key, &remote))
    }

    /// Execute a privileged bootstrap command on the target. Dry-run returns a no-op success.
    pub fn run(
        &self,
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        remote_cmd: &str,
    ) -> Result<BootstrapOutcome> {
        if self.dry_run {
            return Ok(BootstrapOutcome { status: 0, stdout: String::new(), stderr: String::new() });
        }
        let argv = Self::run_argv(target, key_path, host_key, remote_cmd);
        let output = Command::new("ssh").args(&argv).output()?;
        Ok(BootstrapOutcome {
            status: output.status.code().unwrap_or(255),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Push a local file to `remote_path` with `mode`, piping the bytes over the SSH channel
    /// (no scp). Dry-run returns a no-op success.
    pub fn push(
        &self,
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        local_path: &Path,
        remote_path: &str,
        mode: &str,
    ) -> Result<BootstrapOutcome> {
        let argv = Self::push_argv(target, key_path, host_key, remote_path, mode)?;
        if self.dry_run {
            return Ok(BootstrapOutcome { status: 0, stdout: String::new(), stderr: String::new() });
        }
        let file = std::fs::File::open(local_path)?;
        let output = Command::new("ssh")
            .args(&argv)
            .stdin(Stdio::from(file))
            .output()?;
        Ok(BootstrapOutcome {
            status: output.status.code().unwrap_or(255),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn target() -> BootstrapTarget {
        BootstrapTarget { host: "10.0.0.10".to_string(), port: 22, user: "ubuntu".to_string() }
    }

    #[test]
    fn run_argv_uses_sudo_and_quotes_the_command() {
        let argv = BootstrapTransport::run_argv(
            &target(),
            Path::new("/home/op/.ouro/credentials/bootstrap"),
            HostKeyCheck::AcceptNew,
            "useradd -m ouro-exec",
        );
        let joined = argv.join(" ");
        // First access is the existing sudo user (not ouro-exec), via sudo -n sh -c '<quoted>'.
        assert!(joined.contains("ubuntu@10.0.0.10"));
        assert!(joined.contains("-i /home/op/.ouro/credentials/bootstrap"));
        assert!(joined.contains("BatchMode=yes"));
        assert!(joined.contains("sudo -n sh -c 'useradd -m ouro-exec'"));
    }

    #[test]
    fn run_argv_neutralizes_shell_metacharacters() {
        let argv = BootstrapTransport::run_argv(
            &target(),
            Path::new("/k"),
            HostKeyCheck::AcceptNew,
            "true; rm -rf / #",
        );
        let joined = argv.join(" ");
        // The metacharacters live inside the single-quoted sh -c argument → inert.
        assert!(joined.contains("sudo -n sh -c 'true; rm -rf / #'"));
        assert!(!joined.contains("-c true; rm -rf"));
    }

    #[test]
    fn push_argv_installs_atomically_and_quotes_dest() {
        let argv = BootstrapTransport::push_argv(
            &target(),
            Path::new("/k"),
            HostKeyCheck::AcceptNew,
            "/usr/local/bin/ouro-ops",
            "0755",
        )
        .unwrap();
        let joined = argv.join(" ");
        // Runs under `sudo -n sh -c`; temp → install (atomic, root-owned, parents created) →
        // cleanup. The literal (single-quote-free) parts survive the outer quoting verbatim.
        assert!(joined.contains("sudo -n sh -c"));
        assert!(joined.contains("t=$(mktemp)"));
        assert!(joined.contains("cat > \"$t\""));
        assert!(joined.contains("install -D -m 0755 -o root -g root \"$t\""));
        assert!(joined.contains("/usr/local/bin/ouro-ops"));
        assert!(joined.contains("rm -f \"$t\""));
        // Dest was quoted for the inner `sh -c`, then the whole remote quoted for the outer
        // ssh login shell => the inner single-quotes appear double-escaped (two shell layers).
        assert!(joined.contains(r#"'\''/usr/local/bin/ouro-ops'\''"#));
    }

    #[test]
    fn push_argv_rejects_bad_mode() {
        for bad in ["", "75", "0999", "rwx", "07555"] {
            assert!(
                BootstrapTransport::push_argv(&target(), Path::new("/k"), HostKeyCheck::AcceptNew, "/x", bad).is_err(),
                "mode {bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn push_argv_dest_injection_is_inert() {
        let argv = BootstrapTransport::push_argv(
            &target(),
            Path::new("/k"),
            HostKeyCheck::AcceptNew,
            "/etc/x'; rm -rf / #",
            "0644",
        )
        .unwrap();
        let joined = argv.join(" ");
        // The crafted dest never appears as an unquoted command right after the install target:
        // its quote is escaped through both shell layers, so `; rm -rf /` stays inert data.
        assert!(!joined.contains("\"$t\" /etc/x'; rm -rf"));
        assert!(!joined.contains("root \"$t\" /etc/x; rm"));
        // The install target is still the (quoted) crafted path, not a broken-out command.
        assert!(joined.contains("install -D -m 0644 -o root -g root \"$t\""));
    }

    #[test]
    fn dry_run_is_noop_success() {
        let out = BootstrapTransport::new(true)
            .run(&target(), Path::new("/k"), HostKeyCheck::AcceptNew, "id")
            .unwrap();
        assert_eq!(out.status, 0);
    }
}
