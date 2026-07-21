use serde::Serialize;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::domain::SshTarget;
use crate::{OuroError, Result};

const DIAG_STREAM_CAP: usize = 256 * 1024;

enum StdinSource {
    Bytes(Vec<u8>),
    BytesThenFile(Vec<u8>, std::fs::File),
}

fn bounded_command(
    program: &str,
    args: &[String],
    stdin_source: Option<StdinSource>,
    deadline: std::time::Duration,
    stream_cap: usize,
    context: &'static str,
) -> Result<SshOutcome> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if stdin_source.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdin_rx = if let Some(source) = stdin_source {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| OuroError::Validation("bounded subprocess has no stdin pipe".into()))?;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = match source {
                StdinSource::Bytes(bytes) => stdin.write_all(&bytes),
                StdinSource::BytesThenFile(bytes, mut file) => stdin
                    .write_all(&bytes)
                    .and_then(|_| std::io::copy(&mut file, &mut stdin).map(|_| ())),
            }
            .and_then(|_| stdin.flush());
            drop(stdin);
            let _ = sender.send(result);
        });
        Some(receiver)
    } else {
        None
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| OuroError::Validation("bounded subprocess has no stdout pipe".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| OuroError::Validation("bounded subprocess has no stderr pipe".into()))?;
    let spawn_drain = |mut pipe: Box<dyn Read + Send>| {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe
                .by_ref()
                .take((stream_cap + 1) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = sender.send(result);
        });
        receiver
    };
    let stdout_rx = spawn_drain(Box::new(stdout));
    let stderr_rx = spawn_drain(Box::new(stderr));
    let started = std::time::Instant::now();
    let mut stdout = None;
    let mut stderr = None;
    let status = loop {
        if stdout.is_none() {
            if let Ok(result) = stdout_rx.try_recv() {
                let bytes = result?;
                if bytes.len() > stream_cap {
                    child.kill().ok();
                    child.wait().ok();
                    return Err(OuroError::Validation(format!(
                        "{context} stdout exceeded the bounded transport cap"
                    )));
                }
                stdout = Some(bytes);
            }
        }
        if stderr.is_none() {
            if let Ok(result) = stderr_rx.try_recv() {
                let bytes = result?;
                if bytes.len() > stream_cap {
                    child.kill().ok();
                    child.wait().ok();
                    return Err(OuroError::Validation(format!(
                        "{context} stderr exceeded the bounded transport cap"
                    )));
                }
                stderr = Some(bytes);
            }
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= deadline {
            child.kill().ok();
            child.wait().ok();
            return Err(OuroError::Validation(format!(
                "{context} exceeded its local deadline"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let receive = |current: Option<Vec<u8>>,
                   receiver: std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>|
     -> Result<Vec<u8>> {
        let bytes = match current {
            Some(bytes) => bytes,
            None => receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .map_err(|_| {
                    OuroError::Validation(format!("{context} output drain did not terminate"))
                })??,
        };
        if bytes.len() > stream_cap {
            return Err(OuroError::Validation(format!(
                "{context} output exceeded the bounded transport cap"
            )));
        }
        Ok(bytes)
    };
    let stdout = receive(stdout, stdout_rx)?;
    let stderr = receive(stderr, stderr_rx)?;
    if let Some(receiver) = stdin_rx {
        receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| {
                OuroError::Validation(format!("{context} stdin writer did not terminate"))
            })??;
    }
    Ok(SshOutcome {
        status: status.code().unwrap_or(255),
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    })
}

/// Execute one already-sealed SSH argv with concurrent bounded drains and a local deadline. Remote
/// stdout/stderr are untrusted DATA; every S0019 control dispatch uses this instead of `output()`
/// so a target cannot exhaust control memory or hang forever.
pub fn bounded_ssh(
    args: &[String],
    deadline: std::time::Duration,
    stream_cap: usize,
    context: &'static str,
) -> Result<SshOutcome> {
    bounded_command("ssh", args, None, deadline, stream_cap, context)
}

/// Execute one host-key-pinned SSH command while streaming an opaque, control-selected payload to
/// its stdin. The writer runs concurrently with the bounded stdout/stderr drains so a multi-megabyte
/// static runner cannot deadlock on pipe capacity. EOF is delivered before the remote command
/// continues from receipt to digest verification and execution.
pub fn bounded_ssh_with_input(
    args: &[String],
    input: &[u8],
    deadline: std::time::Duration,
    stream_cap: usize,
    context: &'static str,
) -> Result<SshOutcome> {
    bounded_command(
        "ssh",
        args,
        Some(StdinSource::Bytes(input.to_vec())),
        deadline,
        stream_cap,
        context,
    )
}

/// Stream a control-selected runner followed immediately by one already-opened, bounded public
/// artifact. The target transport knows both exact lengths and splits the binary stream inside its
/// private run directory; no shared inbox or target path is part of the public command.
pub fn bounded_ssh_with_payload(
    args: &[String],
    runner: &[u8],
    payload: std::fs::File,
    deadline: std::time::Duration,
    stream_cap: usize,
    context: &'static str,
) -> Result<SshOutcome> {
    bounded_command(
        "ssh",
        args,
        Some(StdinSource::BytesThenFile(runner.to_vec(), payload)),
        deadline,
        stream_cap,
        context,
    )
}

/// Single-quote a value for safe inclusion in a remote shell command. `ssh` joins the
/// remote argv into one string and runs it through the target's shell, so every dynamic
/// field MUST be quoted or a metacharacter (`;`, `|`, `$(...)`, backticks) injects a
/// command that runs as the SSH user on the target.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_join_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Serialize)]
pub struct SshRunner {
    pub dry_run: bool,
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

    /// S0020: free-form diagnostics argv — SSH as the operator account already declared in the
    /// pool spec. There is no Ouro-added sudo or resident diagnostic principal. This is a
    /// diagnostic-intent interface for the honest-agent threat model, not an OS-enforced read-only
    /// boundary. The agent-authored command is
    /// argv is reconstructed with every post-`--` argument separately quoted, then carried as ONE
    /// argument to `sh -c` and bounded by a remote timeout.
    pub fn diag_exec_argv(
        target: &SshTarget,
        key_path: &Path,
        known_hosts: &Path,
        command: &[String],
        timeout_s: u32,
    ) -> Vec<String> {
        vec![
            "-F".to_string(),
            "/dev/null".to_string(),
            "-p".to_string(),
            target.port.to_string(),
            "-o".to_string(),
            "IdentityFile=none".to_string(),
            "-o".to_string(),
            "IdentityAgent=none".to_string(),
            "-i".to_string(),
            key_path.display().to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "IdentitiesOnly=yes".to_string(),
            "-o".to_string(),
            format!("UserKnownHostsFile={}", known_hosts.display()),
            "-o".to_string(),
            "GlobalKnownHostsFile=/dev/null".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=yes".to_string(),
            format!("{}@{}", target.user, target.host),
            format!(
                "timeout {}s sh -c {}",
                timeout_s,
                shell_quote(&shell_join_argv(command))
            ),
        ]
    }

    /// Execute a bounded free-form diagnostic through the existing operator SSH account.
    pub fn diag_exec(
        &self,
        target: &SshTarget,
        key_path: &Path,
        known_hosts: &Path,
        command: &[String],
        timeout_s: u32,
    ) -> Result<SshOutcome> {
        if self.dry_run {
            return Ok(SshOutcome {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            });
        }
        let args = Self::diag_exec_argv(target, key_path, known_hosts, command, timeout_s);
        bounded_command(
            "ssh",
            &args,
            None,
            std::time::Duration::from_secs(u64::from(timeout_s) + 15),
            DIAG_STREAM_CAP,
            "diagnostic transport",
        )
    }
}

#[cfg(test)]
mod bounded_tests {
    use super::*;

    #[test]
    fn bounded_input_stream_reaches_child_and_closes() {
        let payload = vec![b'x'; 512 * 1024];
        let outcome = bounded_command(
            "sh",
            &["-c".into(), "wc -c".into()],
            Some(StdinSource::Bytes(payload.clone())),
            std::time::Duration::from_secs(5),
            1024,
            "stdin fixture",
        )
        .unwrap();
        assert_eq!(outcome.status, 0);
        assert_eq!(outcome.stdout.trim(), payload.len().to_string());
        assert!(outcome.stderr.is_empty());
    }

    #[test]
    fn bounded_runner_then_file_stream_is_exact() {
        let path = std::env::temp_dir().join(format!(
            "ouro-payload-stream-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&path, b"PUBLIC-PAYLOAD").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let outcome = bounded_command(
            "sh",
            &["-c".into(), "sha256sum | awk '{print $1}'".into()],
            Some(StdinSource::BytesThenFile(b"RUNNER".to_vec(), file)),
            std::time::Duration::from_secs(5),
            1024,
            "runner payload fixture",
        )
        .unwrap();
        let expected = crate::assets::sha256_hex(b"RUNNERPUBLIC-PAYLOAD");
        assert_eq!(outcome.stdout.trim(), expected);
        std::fs::remove_file(path).unwrap();
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
    fn diag_exec_argv_uses_existing_operator_account_and_is_pinned_and_bounded() {
        // S0020 adds no privilege escalation to the supplied diagnostic command. Host-key pinning,
        // exact argv grouping and output/deadline bounds remain mechanical guarantees.
        let args = SshRunner::diag_exec_argv(
            &target(),
            Path::new("/home/op/.ouro/credentials/relay1"),
            Path::new("/home/op/.ouro/known_hosts"),
            &[
                "df".into(),
                "-h;".into(),
                "ss".into(),
                "-tn".into(),
                "state".into(),
                "established".into(),
            ],
            30,
        );
        let joined = args.join(" ");
        assert!(
            !joined.contains("sudo"),
            "ouro must not add escalation to diagnostics"
        );
        assert!(
            !joined.contains("ouro-tool-run"),
            "diag is not the write wrapper"
        );
        assert!(
            joined.contains("ouro-exec@relay1.example.com"),
            "uses the spec account"
        );
        assert!(joined.contains("StrictHostKeyChecking=yes"));
        assert!(joined.contains("UserKnownHostsFile=/home/op/.ouro/known_hosts"));
        assert!(joined.contains("GlobalKnownHostsFile=/dev/null"));
        assert!(joined.contains("-F /dev/null"));
        assert!(joined.contains("IdentityFile=none"));
        assert!(joined.contains("IdentityAgent=none"));
        assert!(joined.contains("IdentitiesOnly=yes"));
        // Bounded + quoted: the agent-authored command rides as ONE sh -c argument.
        assert!(joined.contains("timeout 30s sh -c"));

        // A crafted command cannot break out of the quoting.
        let args = SshRunner::diag_exec_argv(
            &target(),
            Path::new("/k"),
            Path::new("/kh"),
            &["x'; rm -rf / #".into()],
            30,
        );
        let last = args.last().unwrap();
        assert!(last.contains("x"), "diagnostic payload carried: {last}");

        // Preserve the exact argv grouping supplied after CLI `--`; joining with spaces would make
        // `zero` become sh's $0 and lose the expected $1="value" binding.
        let grouped = SshRunner::diag_exec_argv(
            &target(),
            Path::new("/k"),
            Path::new("/kh"),
            &[
                "sh".into(),
                "-c".into(),
                "printf 'EXPECTED:%s\\n' \"$1\"".into(),
                "zero".into(),
                "value".into(),
            ],
            30,
        );
        let remote = grouped
            .last()
            .unwrap()
            .strip_prefix("timeout 30s ")
            .unwrap();
        let local = std::process::Command::new("sh")
            .arg("-c")
            .arg(remote)
            .output()
            .unwrap();
        assert!(
            local.status.success(),
            "grouped diagnostic command should execute: argv={:?} stderr={}",
            grouped.last().unwrap(),
            String::from_utf8_lossy(&local.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&local.stdout), "EXPECTED:value\n");
    }
}
