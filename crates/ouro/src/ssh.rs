use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::domain::SshTarget;
use crate::Result;

/// Single-quote a value for safe inclusion in a remote shell command. `ssh` joins the
/// remote argv into one string and runs it through the target's shell, so every dynamic
/// field MUST be quoted or a metacharacter (`;`, `|`, `$(...)`, backticks) injects a
/// command that runs as the SSH user on the target.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, Clone, Serialize)]
pub struct SshRunner {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreparedCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SshOutcome {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl SshRunner {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    /// Redacted command shape for audit/dry-run plans (credential path masked).
    pub fn prepare_tool_run(
        &self,
        target: &SshTarget,
        tool: &str,
        spec_path: &str,
        invocation_id: &str,
    ) -> PreparedCommand {
        PreparedCommand {
            program: "ssh".to_string(),
            args: vec![
                "-p".to_string(),
                target.port.to_string(),
                "-i".to_string(),
                "<credential-ref>".to_string(),
                format!("{}@{}", target.user, target.host),
                "sudo".to_string(),
                "-n".to_string(),
                "ouro".to_string(),
                "tool".to_string(),
                "run".to_string(),
                tool.to_string(),
                "--spec".to_string(),
                spec_path.to_string(),
                "--audit-id".to_string(),
                invocation_id.to_string(),
            ],
        }
    }

    /// Real `ssh` argv for Model B remote dispatch: run `sudo -n ouro tool run <tool>`
    /// on the target (no `--machine`, so the target executes L2 locally). The audit_id
    /// and invocation token are minted+verified on the TARGET (§2.1 D2), so nothing
    /// secret is passed on the argv; `key_path` is a local private key file (resolved
    /// from a `creds://` ref), never inlined key material.
    pub fn tool_run_argv(
        target: &SshTarget,
        key_path: &Path,
        tool: &str,
        machine: &str,
        remote_spec: &str,
    ) -> Vec<String> {
        vec![
            "-p".to_string(),
            target.port.to_string(),
            "-i".to_string(),
            key_path.display().to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            // accept-new: known hosts are pre-populated by provision.sh (ssh-keyscan), so
            // this verifies them; it only TOFU-trusts a host never seen before. Production
            // should use `yes` with a managed known_hosts (bed-convenience default).
            "StrictHostKeyChecking=accept-new".to_string(),
            format!("{}@{}", target.user, target.host),
            "sudo".to_string(),
            "-n".to_string(),
            // Fixed root-owned wrapper (sudoers allowlist, D3): it only runs
            // `ouro tool run "$@"`, so ouro-exec cannot invoke other ouro subcommands.
            "/usr/local/sbin/ouro-tool-run".to_string(),
            // Every dynamic field is shell-quoted: ssh reassembles these into a remote
            // shell command, so an unquoted `<tool>`/`<remote_spec>` would allow injection.
            shell_quote(tool),
            // Target-side LOCAL execution: `--machine` (not `--dispatch`) so the target
            // sets OURO_MACHINE and runs the L2 script itself instead of re-dispatching.
            "--machine".to_string(),
            shell_quote(machine),
            "--spec".to_string(),
            shell_quote(remote_spec),
        ]
    }

    /// Execute the remote `ouro tool run` over SSH and capture its output + exit code.
    /// In dry-run mode returns a no-op success (used where a live target is absent).
    pub fn execute(
        &self,
        target: &SshTarget,
        key_path: &Path,
        tool: &str,
        machine: &str,
        remote_spec: &str,
    ) -> Result<SshOutcome> {
        if self.dry_run {
            return Ok(SshOutcome {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            });
        }
        let args = Self::tool_run_argv(target, key_path, tool, machine, remote_spec);
        let output = Command::new("ssh").args(&args).output()?;
        Ok(SshOutcome {
            status: output.status.code().unwrap_or(255),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{domain::SshTarget, secrets::CredentialRef};

    use super::SshRunner;

    fn target() -> SshTarget {
        SshTarget {
            host: "relay1.example.com".to_string(),
            port: 22,
            user: "ouro-exec".to_string(),
            key_ref: CredentialRef::parse("creds://relay1").unwrap(),
        }
    }

    #[test]
    fn only_prepares_allowlisted_tool_run_shape() {
        let cmd =
            SshRunner::new(true).prepare_tool_run(&target(), "deploy/preflight", "pool-spec.json", "audit-1");
        let joined = cmd.args.join(" ");
        assert!(joined.contains("sudo -n ouro tool run deploy/preflight"));
        assert!(joined.contains("<credential-ref>"));
        assert!(!joined.contains("creds://"));
        assert!(!joined.contains(" docker rm "));
        assert!(!joined.contains(" scp "));
    }

    #[test]
    fn tool_run_argv_uses_fixed_cli_path_and_no_secret_inline() {
        let args = SshRunner::tool_run_argv(
            &target(),
            Path::new("/home/op/.ouro/credentials/relay1"),
            "deploy/provision",
            "relay1",
            "/opt/ouro/pool-spec.yaml",
        );
        let joined = args.join(" ");
        // Fixed root-owned wrapper path (matches the sudoers allowlist, D3); the tool is
        // shell-quoted.
        assert!(joined.contains("sudo -n /usr/local/sbin/ouro-tool-run 'deploy/provision'"));
        assert!(joined.contains("ouro-exec@relay1.example.com"));
        assert!(joined.contains("BatchMode=yes"));
        // Target-side call uses --machine (local exec, no re-dispatch); no secret inlined.
        assert!(joined.contains("--machine 'relay1'"));
        assert!(!joined.contains("--dispatch"));
        assert!(!joined.contains("creds://"));
        assert!(joined.contains("-i /home/op/.ouro/credentials/relay1"));
    }

    #[test]
    fn tool_run_argv_neutralizes_shell_metacharacters() {
        // A crafted tool / remote_spec must NOT break out of the quoted remote command.
        let args = SshRunner::tool_run_argv(
            &target(),
            Path::new("/k"),
            "deploy/preflight; touch /tmp/pwned #",
            "bp1",
            "/spec; rm -rf /",
        );
        let joined = args.join(" ");
        // The metacharacters are inside single quotes → inert; ouro then rejects the name.
        assert!(joined.contains("'deploy/preflight; touch /tmp/pwned #'"));
        assert!(joined.contains("'/spec; rm -rf /'"));
        // No unquoted `;` that the remote shell could act on.
        assert!(!joined.contains("run deploy/preflight; touch"));
    }

    #[test]
    fn dry_run_execute_is_noop_success() {
        let outcome = SshRunner::new(true)
            .execute(&target(), Path::new("/tmp/key"), "deploy/preflight", "bp1", "/opt/ouro/spec.yaml")
            .unwrap();
        assert_eq!(outcome.status, 0);
    }
}
