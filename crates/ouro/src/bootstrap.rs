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

/// A read-only probe of the target's platform facts, run BEFORE any write (S0017 p1-8). Selecting
/// the right (arch-matched) artifact and refusing an unsupported host is a correctness gate: init
/// pushes a binary, and a control machine of a different OS/arch would otherwise install one the
/// target cannot execute. Emits only a closed set of facts (no raw files).
pub const FACTS_PROBE: &str = "printf 'os=%s\\narch=%s\\n' \"$(uname -s)\" \"$(uname -m)\"; \
    . /etc/os-release 2>/dev/null; printf 'id=%s\\nid_like=%s\\n' \"${ID:-}\" \"${ID_LIKE:-}\"; \
    { [ -d /run/systemd/system ] && echo systemd=yes || echo systemd=no; }";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TargetFacts {
    pub os: String,
    pub arch: String,
    pub distro_family: String,
    pub has_systemd: bool,
}

impl TargetFacts {
    /// Parse the `key=value` lines emitted by `FACTS_PROBE`. `distro_family` normalizes the
    /// os-release `ID`/`ID_LIKE` to a supported family token, or "unknown".
    pub fn parse(probe_stdout: &str) -> Self {
        let mut os = String::new();
        let mut arch = String::new();
        let mut id = String::new();
        let mut id_like = String::new();
        let mut has_systemd = false;
        for line in probe_stdout.lines() {
            let (k, v) = match line.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            match k.trim() {
                "os" => os = v.trim().to_string(),
                "arch" => arch = v.trim().to_string(),
                "id" => id = v.trim().to_lowercase(),
                "id_like" => id_like = v.trim().to_lowercase(),
                "systemd" => has_systemd = v.trim() == "yes",
                _ => {}
            }
        }
        let hay = format!("{id} {id_like}");
        let family = ["debian", "ubuntu", "rhel", "fedora", "centos", "rocky", "almalinux"]
            .into_iter()
            .find(|fam| hay.split_whitespace().any(|t| t == *fam))
            .map(|fam| match fam {
                "ubuntu" => "debian",
                "centos" | "rocky" | "almalinux" | "fedora" => "rhel",
                other => other,
            })
            .unwrap_or("unknown")
            .to_string();
        TargetFacts { os, arch, distro_family: family, has_systemd }
    }

    /// Normalized arch token (`x86_64` / `aarch64`) or None if unsupported.
    pub fn norm_arch(&self) -> Option<&'static str> {
        match self.arch.as_str() {
            "x86_64" | "amd64" => Some("x86_64"),
            "aarch64" | "arm64" => Some("aarch64"),
            _ => None,
        }
    }

    /// Fail-closed support gate: bootstrap targets must be Linux, on a supported arch, of a
    /// supported distro family. Returns the reason it is unsupported, or Ok(()).
    pub fn require_supported(&self) -> Result<()> {
        if self.os != "Linux" {
            return Err(OuroError::Validation(format!(
                "unsupported target OS {:?}: ouro-ops init provisions Linux hosts only",
                self.os
            )));
        }
        if self.norm_arch().is_none() {
            return Err(OuroError::Validation(format!(
                "unsupported target arch {:?}: supported are x86_64 and aarch64",
                self.arch
            )));
        }
        if self.distro_family == "unknown" {
            return Err(OuroError::Validation(
                "unsupported target distro: supported families are debian/ubuntu and rhel/fedora"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

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
/// The CPU arch a binary targets, read from its file header — `x86_64` / `aarch64` for a Linux
/// ELF, or None if it is not a Linux ELF (e.g. a macOS Mach-O) or an unrecognized machine. Used to
/// refuse pushing a binary the target cannot execute (S0017 p1-8).
pub fn binary_arch(path: &Path) -> Option<&'static str> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 20 || &bytes[0..4] != b"\x7fELF" {
        return None; // not a Linux ELF (a macOS Mach-O control binary lands here → refused)
    }
    // e_machine is a little-endian u16 at offset 18.
    match u16::from_le_bytes([bytes[18], bytes[19]]) {
        0x3E => Some("x86_64"),
        0xB7 => Some("aarch64"),
        _ => None,
    }
}

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

/// A bare unix user/group token — no shell metacharacters can slip into the unquoted `install
/// -o <owner> -g <owner>` position.
fn validate_owner(owner: &str) -> Result<()> {
    let ok = !owner.is_empty()
        && owner.len() <= 32
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    let starts_ok = owner.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    if ok && starts_ok {
        Ok(())
    } else {
        Err(OuroError::Validation(format!(
            "owner must be a bare unix user token, got {owner:?}"
        )))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BootstrapTransport {
    pub dry_run: bool,
    known_hosts: Option<std::path::PathBuf>,
}

impl BootstrapTransport {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run, known_hosts: None }
    }

    /// Bind bootstrap SSH to Ouro's independently verified/pinned known_hosts file.
    pub fn with_known_hosts(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.known_hosts = Some(path.into());
        self
    }

    /// Common ssh options + `user@host` prefix for a privileged bootstrap connection.
    fn ssh_prefix(
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        known_hosts: Option<&Path>,
    ) -> Vec<String> {
        let mut argv = vec![
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
            host_key.as_opt().to_string(),
            "-o".to_string(),
            "GlobalKnownHostsFile=/dev/null".to_string(),
        ];
        if let Some(path) = known_hosts {
            argv.extend([
                "-o".to_string(),
                format!("UserKnownHostsFile={}", path.display()),
            ]);
        }
        argv.push(format!("{}@{}", target.user, target.host));
        argv
    }

    /// `ssh … <user>@<host> sudo -n sh -c '<cmd>'` — run a privileged provisioning command.
    /// `remote_cmd` is shell-quoted, so a crafted value cannot break out of the `sh -c` string.
    pub fn run_argv(
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        remote_cmd: &str,
    ) -> Vec<String> {
        Self::run_argv_with_known_hosts(target, key_path, host_key, remote_cmd, None)
    }

    fn run_argv_with_known_hosts(
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        remote_cmd: &str,
        known_hosts: Option<&Path>,
    ) -> Vec<String> {
        let mut argv = Self::ssh_prefix(target, key_path, host_key, known_hosts);
        argv.extend([
            "sudo".to_string(),
            "-n".to_string(),
            "sh".to_string(),
            "-c".to_string(),
            shell_quote(remote_cmd),
        ]);
        argv
    }

    fn configured_run_argv(
        &self,
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        remote_cmd: &str,
    ) -> Vec<String> {
        Self::run_argv_with_known_hosts(
            target,
            key_path,
            host_key,
            remote_cmd,
            self.known_hosts.as_deref(),
        )
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

    fn configured_push_argv(
        &self,
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
        Ok(self.configured_run_argv(target, key_path, host_key, &remote))
    }

    /// Install a PRIVATE KEY on the target over the same encrypted SSH channel, with restrictive
    /// perms: `0400` owned by the node runtime `owner` so only the cardano-node process can read
    /// it. This is the ONE private key the deploy flow moves cold→BP — vrf.skey (S0017 p4-9);
    /// cold.skey never moves (offline cold-sign) and KES/opcert are issued offline too. Same atomic
    /// temp→install→cleanup as `push_argv`. `owner` is validated to a bare user token.
    pub fn push_key_argv(
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
        remote_path: &str,
        owner: &str,
    ) -> Result<Vec<String>> {
        validate_owner(owner)?;
        let remote = format!(
            "t=$(mktemp) && cat > \"$t\" && install -D -m 0400 -o {owner} -g {owner} \"$t\" {dest} && rm -f \"$t\"",
            owner = owner,
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
        let argv = self.configured_run_argv(target, key_path, host_key, remote_cmd);
        let output = Command::new("ssh").args(&argv).output()?;
        Ok(BootstrapOutcome {
            status: output.status.code().unwrap_or(255),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Open a fresh unprivileged SSH connection and execute only `true`. Used after sshd reload to
    /// prove that each intended principal can authenticate in a new session.
    pub fn probe_login(
        &self,
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
    ) -> Result<BootstrapOutcome> {
        if self.dry_run {
            return Ok(BootstrapOutcome {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            });
        }
        let mut argv = Self::ssh_prefix(
            target,
            key_path,
            host_key,
            self.known_hosts.as_deref(),
        );
        argv.push("true".into());
        let output = Command::new("ssh").args(&argv).output()?;
        Ok(BootstrapOutcome {
            status: output.status.code().unwrap_or(255),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// p1-8 — run the read-only facts probe on the target and parse it. Dry-run returns None
    /// (nothing to probe). A probe that fails or yields no OS is surfaced as an error.
    pub fn detect_facts(
        &self,
        target: &BootstrapTarget,
        key_path: &Path,
        host_key: HostKeyCheck,
    ) -> Result<Option<TargetFacts>> {
        if self.dry_run {
            return Ok(None);
        }
        let out = self.run(target, key_path, host_key, FACTS_PROBE)?;
        if out.status != 0 {
            return Err(OuroError::Validation(format!(
                "could not probe target platform facts (ssh exit {}): {}",
                out.status,
                out.stderr.trim()
            )));
        }
        let facts = TargetFacts::parse(&out.stdout);
        if facts.os.is_empty() {
            return Err(OuroError::Validation(
                "target platform probe returned no OS — cannot verify support".to_string(),
            ));
        }
        Ok(Some(facts))
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
        let argv = self.configured_push_argv(target, key_path, host_key, remote_path, mode)?;
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
    fn configured_transport_uses_the_ouro_known_hosts_file() {
        let transport = BootstrapTransport::new(false).with_known_hosts("/control/ouro-known-hosts");
        let argv = transport.configured_run_argv(
            &target(),
            Path::new("/k"),
            HostKeyCheck::Yes,
            "true",
        );
        let joined = argv.join(" ");
        assert!(joined.contains("-F /dev/null"));
        assert!(joined.contains("IdentityFile=none"));
        assert!(joined.contains("IdentityAgent=none"));
        assert!(joined.contains("StrictHostKeyChecking=yes"));
        assert!(joined.contains("IdentitiesOnly=yes"));
        assert!(joined.contains("UserKnownHostsFile=/control/ouro-known-hosts"));
        assert!(joined.contains("GlobalKnownHostsFile=/dev/null"));
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
    fn target_facts_parse_and_support_gate() {
        // Ubuntu on aarch64 with systemd → supported, family normalized to debian.
        let f = TargetFacts::parse("os=Linux\narch=aarch64\nid=ubuntu\nid_like=debian\nsystemd=yes\n");
        assert_eq!(f.os, "Linux");
        assert_eq!(f.distro_family, "debian");
        assert_eq!(f.norm_arch(), Some("aarch64"));
        assert!(f.has_systemd);
        assert!(f.require_supported().is_ok());

        // Rocky (id_like rhel) on x86_64 → supported, family normalized to rhel.
        let f = TargetFacts::parse("os=Linux\narch=x86_64\nid=rocky\nid_like=\"rhel centos fedora\"\nsystemd=yes");
        assert_eq!(f.distro_family, "rhel");
        assert!(f.require_supported().is_ok());

        // macOS control machine mistaken for a target → refused (not Linux).
        let f = TargetFacts::parse("os=Darwin\narch=arm64\nid=\nid_like=\nsystemd=no");
        assert!(f.require_supported().is_err());

        // Linux on an unsupported arch → refused.
        let f = TargetFacts::parse("os=Linux\narch=riscv64\nid=debian\nid_like=\nsystemd=yes");
        assert!(f.norm_arch().is_none() && f.require_supported().is_err());

        // Linux, supported arch, but an unknown distro → refused (fail-closed).
        let f = TargetFacts::parse("os=Linux\narch=x86_64\nid=exoticos\nid_like=\nsystemd=yes");
        assert_eq!(f.distro_family, "unknown");
        assert!(f.require_supported().is_err());
    }

    #[test]
    fn binary_arch_reads_elf_machine_and_rejects_non_elf() {
        let dir = std::env::temp_dir().join(format!("ouro-elf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Minimal 20-byte ELF header with e_machine = 0x3E (x86_64) at offset 18.
        let mut elf = vec![0u8; 20];
        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[18] = 0x3E;
        elf[19] = 0x00;
        let p = dir.join("x86");
        std::fs::write(&p, &elf).unwrap();
        assert_eq!(binary_arch(&p), Some("x86_64"));
        // aarch64 machine = 0xB7.
        elf[18] = 0xB7;
        let p2 = dir.join("arm");
        std::fs::write(&p2, &elf).unwrap();
        assert_eq!(binary_arch(&p2), Some("aarch64"));
        // A non-ELF (e.g. Mach-O magic) → None (would be refused).
        let p3 = dir.join("macho");
        std::fs::write(&p3, [0xCF, 0xFA, 0xED, 0xFE, 0, 0, 0, 0]).unwrap();
        assert_eq!(binary_arch(&p3), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn push_key_argv_installs_0400_with_node_owner() {
        // p4-9: the one private key that moves cold→BP (vrf.skey) lands 0400 owned by the node
        // runtime user, atomically, over the same SSH channel.
        let argv = BootstrapTransport::push_key_argv(
            &target(), Path::new("/k"), HostKeyCheck::Yes, "/opt/cardano/keys/vrf.skey", "node",
        )
        .unwrap();
        let joined = argv.join(" ");
        assert!(joined.contains("install -D -m 0400 -o node -g node \"$t\""));
        assert!(joined.contains("t=$(mktemp)") && joined.contains("rm -f \"$t\""));
        assert!(joined.contains("/opt/cardano/keys/vrf.skey"));
    }

    #[test]
    fn push_key_argv_rejects_shell_metachar_owner() {
        for bad in ["", "no de", "node;rm", "-x", "$(id)", "a".repeat(33).as_str()] {
            assert!(
                BootstrapTransport::push_key_argv(&target(), Path::new("/k"), HostKeyCheck::Yes, "/x", bad).is_err(),
                "owner {bad:?} should be rejected"
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
