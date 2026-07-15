use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use crate::{
    audit::AuditStore,
    bootstrap::{BootstrapTarget, BootstrapTransport, HostKeyCheck},
    config::ConfigPaths,
    confirm::{self, ConfirmationStore},
    domain::PoolSpec,
    kes, migration,
    output::{self, ToolOutput},
    pool, provision, render,
    secrets::CredentialRef,
    ssh::SshRunner,
    status::StatusSnapshot,
    OuroError, Result,
};

pub fn run(args: Vec<String>) -> Result<()> {
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    // `<command> --help` / `-h` prints that command's usage instead of running it (agent
    // discoverability). The bare `help`/`--help`/`-h` command falls through to the full list.
    if !matches!(command, "help" | "--help" | "-h")
        && args.iter().skip(2).any(|a| a == "--help" || a == "-h")
    {
        if let Some(usage) = command_usage(command) {
            println!("{usage}");
            return Ok(());
        }
    }
    match command {
        "help" | "--help" | "-h" => print_help(),
        "--version" | "version" => {
            output::print_json(&ToolOutput::ok("ouro.version", false).with_data(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "binary": "ouro-ops",
                "runtime": "standalone-rust"
            })))?;
        }
        "paths" => {
            output::print_json(
                &ToolOutput::ok("ouro.paths", false).with_data(json!(ConfigPaths::discover())),
            )?;
        }
        "contract" => {
            output::print_json(
                &ToolOutput::ok("ouro.contract", false).with_data(output::contract_summary()),
            )?;
        }
        "audit" => run_audit(&args[2..])?,
        "init" => run_init(&args[2..])?,
        "deinit" => run_deinit(&args[2..])?,
        "confirm" => run_confirm(&args[2..])?,
        "config" => run_config(&args[2..])?,
        "creds" => run_creds(&args[2..])?,
        "deploy" => run_deploy(&args[2..])?,
        "diag" => run_diag(&args[2..])?,
        "onboard" => run_onboard(&args[2..])?,
        "adopt" => crate::s0019_cli::run_adopt(&args[2..])?,
        "op" => crate::s0019_cli::run_op(&args[2..])?,
        "inbox" => crate::s0019_cli::run_inbox(&args[2..])?,
        "fleet" => crate::s0019_cli::run_fleet(&args[2..])?,
        "kes" => run_kes(&args[2..])?,
        "legacy" => run_legacy(&args[2..])?,
        "manifest" => run_manifest(&args[2..])?,
        "pool" => run_pool(&args[2..])?,
        "rollback" => run_rollback(&args[2..])?,
        "self-update" => run_self_update(&args[2..])?,
        "skill" => run_skill(&args[2..])?,
        "spec" => run_spec(&args[2..])?,
        "status" => run_status(&args[2..])?,
        "tool" => run_tool(&args[2..])?,
        other => return Err(OuroError::InvalidArgs(format!("unknown command {other}"))),
    }
    Ok(())
}

/// Named-only credential namespace management. There is deliberately no list operation: the
/// operator chooses one existing private-key path, and this command can check/preview/register
/// only the exact name and path supplied in the invocation. Registration creates a symlink and
/// never opens or copies the key contents.
fn run_creds(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("check") => {
            validate_creds_args(args, false)?;
            let name = flag_value(args, "--name")?;
            let paths = ConfigPaths::discover();
            let status = crate::secrets::credential_status(&paths.credentials_dir, name)?;
            output::print_json(
                &ToolOutput::ok("ouro.creds.check", false).with_data(json!(status)),
            )?;
            Ok(())
        }
        Some("register") => {
            validate_creds_args(args, true)?;
            let name = flag_value(args, "--name")?;
            let source = PathBuf::from(flag_value(args, "--path")?);
            let dry_run = args.iter().any(|arg| arg == "--dry-run");
            let paths = ConfigPaths::discover();
            let result = crate::secrets::register_existing_credential(
                &paths.credentials_dir,
                name,
                &source,
                dry_run,
            )?;
            output::print_json(
                &ToolOutput::ok("ouro.creds.register", result.changed).with_data(json!(result)),
            )?;
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected creds check --name <name> | register --name <name> --path <absolute-operator-path> [--dry-run]; listing and replacement are intentionally unsupported"
                .into(),
        )),
    }
}

fn validate_creds_args(args: &[String], register: bool) -> Result<()> {
    let mut saw_name = false;
    let mut saw_path = false;
    let mut saw_dry_run = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--name" if !saw_name => {
                if args
                    .get(index + 1)
                    .map(|value| value.starts_with("--"))
                    .unwrap_or(true)
                {
                    return Err(OuroError::InvalidArgs(
                        "missing value for credential --name; use `ouro-ops creds --help`".into(),
                    ));
                }
                saw_name = true;
                index += 2;
            }
            "--path" if register && !saw_path => {
                if args
                    .get(index + 1)
                    .map(|value| value.starts_with("--"))
                    .unwrap_or(true)
                {
                    return Err(OuroError::InvalidArgs(
                        "missing value for credential --path; use `ouro-ops creds --help`".into(),
                    ));
                }
                saw_path = true;
                index += 2;
            }
            "--dry-run" if register && !saw_dry_run => {
                saw_dry_run = true;
                index += 1;
            }
            _ => {
                return Err(OuroError::InvalidArgs(
                    "unexpected or duplicate credential argument; use `ouro-ops creds --help`"
                        .into(),
                ))
            }
        }
    }
    if !saw_name || (register && !saw_path) {
        return Err(OuroError::InvalidArgs(
            "missing required credential argument; use `ouro-ops creds --help`".into(),
        ));
    }
    Ok(())
}

/// `ouro-ops self-update --check [--against <signed-metadata.json>]` (S0016 p2-3).
///
/// Reports the running version and the built-in required floor. With `--against`, compares
/// to a (signed at release) metadata file and reports whether an update is warranted, WITHOUT
/// downgrading below the current version (monotonic). The actual network fetch + signature
/// verification + in-place swap are RELEASE INFRASTRUCTURE (signing key, transparency log,
/// stable channel) documented in `packaging/RELEASE.md` — deliberately not runnable in-repo,
/// so this never pretends to have applied an unverified update.
fn run_self_update(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("--check") {
        return Err(OuroError::InvalidArgs(
            "self-update supports --check [--against <file>]; network apply is release infra (packaging/RELEASE.md)".to_string(),
        ));
    }
    let current = crate::version::fmt(crate::version::current());
    let embedded_floor = crate::skills::required_ouro();
    let mut update_available = false;
    let mut latest = current.clone();
    if let Some(path) = optional_flag_value(args, "--against") {
        let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|e| OuroError::Validation(format!("metadata {path} not JSON: {e}")))?;
        if let Some(v) = meta.get("latest_version").and_then(|v| v.as_str()) {
            latest = v.to_string();
            if let (Some(c), Some(l)) =
                (crate::skills::parse_floor(&current), crate::skills::parse_floor(v))
            {
                update_available = l > c; // never downgrade: only flag when strictly newer
            }
        }
    }
    output::print_json(&ToolOutput::ok("ouro.self-update.check", false).with_data(json!({
        "current": current,
        "embedded_required_floor": embedded_floor,
        "latest_seen": latest,
        "update_available": update_available,
        "apply": "release infra: verify signature + transparency log, then swap (packaging/RELEASE.md)",
    })))?;
    Ok(())
}

fn run_rollback(args: &[String]) -> Result<()> {
    let machine = flag_value(args, "--machine")?;
    let backup_id = flag_value(args, "--to")?;
    let token = optional_flag_value(args, "--confirm-token");
    let paths = ConfigPaths::discover();
    consume_confirmation(&paths, token, "rollback", machine, None)?;
    let store = AuditStore::open(&paths.audit_db)?;
    let audit_id = store.begin_invocation("rollback", Some(machine))?;
    store.finish_invocation(&audit_id, "rollback")?;
    output::print_json(&ToolOutput::ok("ouro.rollback", true).with_data(json!({
        "audit_id": audit_id,
        "machine": machine,
        "to": backup_id,
        "confirmation": "accepted",
        "execution": "planned-forward-change"
    })))?;
    Ok(())
}

/// `ouro-ops init` (S0017 p1-2) — arm a bare machine into a constrained target over the
/// privileged bootstrap transport. First access is an existing sudo user + SSH key; init
/// installs the two principals, the tool-run wrapper + sudoers confinement, pubkey-only sshd,
/// the `ouro-ops` binary (the one running init, by default), and the control public key. Prints
/// an auditable install manifest. Idempotent (re-run converges).
///
/// P0-1 DECISION (convenience mode): the bootstrap credential is NOT mechanism-isolated from
/// the agent — a poisoned prompt could, via the agent, invoke this against a reachable host.
/// This is documented, not defended; per the operator's decision it relies on upstream
/// control-machine / agent-runtime security. The manifest carries this note.
/// p2-4 — record + verify the DECLARED runtime for the target machine at init time. Loading the
/// spec already validated the declaration's consistency (systemd needs unit, docker needs
/// container|image); this captures it in the init output so the intended supervision mode is
/// recorded for the later detect↔declared cross-check (p2-5). An absent declaration is honestly
/// recorded as "undeclared" — the tool DETECTS the mode at operation time (v1 stays optional; see
/// the required-v2 decision in the spec §7). Detection always governs; this never substitutes.
fn init_runtime_record(spec: &PoolSpec, machine_id: &str) -> Result<serde_json::Value> {
    let machine = spec
        .machines
        .iter()
        .find(|m| m.id == machine_id)
        .ok_or_else(|| OuroError::Validation(format!("unknown machine {machine_id} in spec")))?;
    Ok(match &machine.runtime {
        Some(rt) => json!({ "machine": machine_id, "declared": true, "runtime": rt }),
        None => json!({
            "machine": machine_id,
            "declared": false,
            "note": "runtime undeclared (v1 optional); mode is DETECTED at operation time, detection governs",
        }),
    })
}

/// S0019 p6-1 — `ouro-ops onboard`: the greenfield host-onboard (host-onboarded state). Installs
/// the S0019 confined principals + op wrapper + binary, pins the host key. No S0017 compat.
fn run_onboard(args: &[String]) -> Result<()> {
    let host = flag_value(args, "--host")?.to_string();
    let port: u16 = optional_flag_value(args, "--port")
        .unwrap_or("22")
        .parse()
        .map_err(|_| OuroError::InvalidArgs("--port must be a number".to_string()))?;
    let user = flag_value(args, "--bootstrap-user")?.to_string();
    if user.is_empty()
        || user.len() > 32
        || !user.bytes().next().map(|byte| byte.is_ascii_lowercase() || byte == b'_').unwrap_or(false)
        || !user.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
    {
        return Err(OuroError::Validation(format!(
            "--bootstrap-user must be a valid unix username [a-z_][a-z0-9_-]*: {user}"
        )));
    }
    let key_ref = CredentialRef::parse(flag_value(args, "--bootstrap-key")?)?;
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let host_key = match optional_flag_value(args, "--host-key") {
        Some("yes") => crate::bootstrap::HostKeyCheck::Yes,
        _ => crate::bootstrap::HostKeyCheck::AcceptNew,
    };
    let paths = ConfigPaths::discover();
    let key_path = key_ref.resolve(&paths.credentials_dir)?;
    if !dry_run && !key_path.is_file() {
        return Err(OuroError::Validation(format!(
            "bootstrap credential key not found: {}",
            key_path.display()
        )));
    }
    let control_pubkey = match optional_flag_value(args, "--control-pubkey") {
        Some(p) => std::fs::read_to_string(p)
            .map_err(|e| OuroError::Validation(format!("cannot read --control-pubkey {p}: {e}")))?,
        None => {
            return Err(OuroError::Validation(
                "missing --control-pubkey: pass the operator's control public key (derive it with \
                 `ssh-keygen -y -f <the operator's key>`). See `ouro-ops skill show adopt`."
                    .to_string(),
            ))
        }
    };
    validate_control_pubkey(&control_pubkey)?;
    let ouro_binary = match optional_flag_value(args, "--ouro-binary") {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe()
            .map_err(|e| OuroError::Validation(format!("cannot resolve own binary path: {e}")))?,
    };
    let expected_host_key = optional_flag_value(args, "--expected-host-key");

    let target = crate::bootstrap::BootstrapTarget { host: host.clone(), port, user };
    let transport = crate::bootstrap::BootstrapTransport::new(dry_run);

    let mut pinned_fp = None;
    if let Some(fp) = expected_host_key.filter(|_| !dry_run) {
        pinned_fp = Some(pin_host_key(&host, port, &paths.known_hosts, Some(fp))?);
    }

    let target_facts = transport.detect_facts(&target, &key_path, host_key)?;
    if let Some(facts) = &target_facts {
        facts.require_supported()?;
        match crate::bootstrap::binary_arch(&ouro_binary) {
            Some(binary_arch) if Some(binary_arch) == facts.norm_arch() => {}
            Some(binary_arch) => {
                return Err(OuroError::Validation(format!(
                    "--ouro-binary is {binary_arch} but the target is {}; supply a matching Linux binary",
                    facts.norm_arch().unwrap_or(&facts.arch)
                )))
            }
            None => {
                return Err(OuroError::Validation(format!(
                    "--ouro-binary {} is not a Linux ELF for {}",
                    ouro_binary.display(),
                    facts.norm_arch().unwrap_or(&facts.arch)
                )))
            }
        }
    }

    let manifest = crate::onboard::execute_onboard(
        &transport,
        &target,
        &key_path,
        host_key,
        &control_pubkey,
        &ouro_binary,
    )?;

    if pinned_fp.is_none() && manifest.ok && !dry_run {
        pinned_fp = Some(pin_host_key(&host, port, &paths.known_hosts, None)?);
    }

    let (state, planned_state, host_key_status) =
        onboard_output_semantics(dry_run, manifest.ok, pinned_fp.is_some());
    output::print_json(&ToolOutput::ok("ouro.onboard", manifest.ok && !dry_run).with_data(json!({
        "manifest": manifest,
        "dry_run": dry_run,
        "pinned_host_key": pinned_fp,
        "host_key_status": host_key_status,
        "expected_host_key_supplied": expected_host_key.is_some(),
        "state": state,
        "planned_state": planned_state,
        "ssh_access_policy": crate::onboard::ssh_access_policy(&target.user),
        "security_note": "bootstrap credential is NOT mechanism-isolated from the agent \
            (convenience mode, P0-1, carried from S0017). Closing this is a separate hardening spec.",
    })))?;
    if !manifest.ok {
        return Err(OuroError::Validation("onboard did not complete cleanly".to_string()));
    }
    Ok(())
}

fn onboard_output_semantics(
    dry_run: bool,
    manifest_ok: bool,
    has_pinned_host_key: bool,
) -> (&'static str, Option<&'static str>, &'static str) {
    if dry_run {
        return ("preview", Some("host-onboarded"), "not_checked_in_dry_run");
    }
    let state = if manifest_ok {
        "host-onboarded"
    } else {
        "onboard-failed"
    };
    let host_key_status = if has_pinned_host_key { "pinned" } else { "not_pinned" };
    (state, None, host_key_status)
}

fn validate_control_pubkey(value: &str) -> Result<()> {
    if value.contains('\r') || value.lines().count() != 1 {
        return Err(OuroError::Validation(
            "--control-pubkey must contain exactly one OpenSSH public key".into(),
        ));
    }
    let mut fields = value.split_whitespace();
    let algorithm = fields.next().unwrap_or("");
    let encoded = fields.next().unwrap_or("");
    let allowed = matches!(
        algorithm,
        "ssh-ed25519"
            | "sk-ssh-ed25519@openssh.com"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ssh-rsa"
    );
    if !allowed
        || encoded.len() < 16
        || !encoded.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/' || byte == b'='
        })
    {
        return Err(OuroError::Validation(
            "--control-pubkey is not a supported OpenSSH public key".into(),
        ));
    }
    Ok(())
}

fn run_init(args: &[String]) -> Result<()> {
    let host = flag_value(args, "--host")?.to_string();
    let port: u16 = optional_flag_value(args, "--port")
        .unwrap_or("22")
        .parse()
        .map_err(|_| OuroError::InvalidArgs("--port must be a number".to_string()))?;
    let user = flag_value(args, "--bootstrap-user")?.to_string();
    // A real Unix username — it becomes sshd `AllowUsers` content; reject anything odd.
    if user.is_empty()
        || user.len() > 32
        || !user.bytes().next().map(|b| b.is_ascii_lowercase() || b == b'_').unwrap_or(false)
        || !user.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err(OuroError::Validation(format!(
            "--bootstrap-user must be a valid unix username [a-z_][a-z0-9_-]*: {user}"
        )));
    }
    let key_ref = CredentialRef::parse(flag_value(args, "--bootstrap-key")?)?;
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let host_key = match optional_flag_value(args, "--host-key") {
        Some("yes") => HostKeyCheck::Yes,
        _ => HostKeyCheck::AcceptNew,
    };

    let paths = ConfigPaths::discover();
    let key_path = key_ref.resolve(&paths.credentials_dir)?;
    if !dry_run && !key_path.is_file() {
        return Err(OuroError::Validation(format!(
            "bootstrap credential key not found: {} (the private key for {})",
            key_path.display(),
            key_ref.as_str()
        )));
    }
    // The control key is the operator's — the agent supplies its PUBLIC half (to authorize for
    // `ouro-exec`); the tool NEVER generates a key. Reuse the operator's existing SSH key: derive
    // the public half of the bootstrap key with `ssh-keygen -y -f <key>`, or pass a chosen key via
    // `--control-pubkey`. If neither is available the agent must ask the operator (see skill onboard).
    let control_pubkey = match optional_flag_value(args, "--control-pubkey") {
        Some(p) => std::fs::read_to_string(p)
            .map_err(|e| OuroError::Validation(format!("cannot read --control-pubkey {p}: {e}")))?,
        None => {
            return Err(OuroError::Validation(
                "missing --control-pubkey: pass the operator's control public key to authorize for \
                 ouro-exec (derive it with `ssh-keygen -y -f <the operator's key>`, or use their \
                 existing *.pub). See `ouro-ops skill show onboard`."
                    .to_string(),
            ));
        }
    };
    let ouro_binary = match optional_flag_value(args, "--ouro-binary") {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe()
            .map_err(|e| OuroError::Validation(format!("cannot resolve own binary path: {e}")))?,
    };

    let expected_host_key = optional_flag_value(args, "--expected-host-key");

    // p2-4: optionally record + verify the declared runtime for this target machine.
    let declared_runtime = match (optional_flag_value(args, "--spec"), optional_flag_value(args, "--machine")) {
        (Some(spec_path), Some(machine_id)) => {
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?; // load validates consistency
            Some(init_runtime_record(&spec, machine_id)?)
        }
        _ => None,
    };

    let target = BootstrapTarget { host: host.clone(), port, user };
    let transport = BootstrapTransport::new(dry_run);

    // p3-3: when an out-of-band fingerprint is supplied, authenticate the host BEFORE writing
    // anything — pin+verify first, so a first-hop MITM (whose key won't match) is refused before
    // any privileged provisioning step runs. Without `--expected-host-key` the pin is TOFU and
    // happens AFTER a successful provision (nothing to authenticate against yet).
    let mut pinned_fp = None;
    if expected_host_key.is_some() && !dry_run {
        pinned_fp = Some(pin_host_key(&host, port, &paths.known_hosts, expected_host_key)?);
    }

    // p1-8: probe the target platform BEFORE any write and fail closed on an unsupported host or a
    // binary the target cannot execute (e.g. a macOS control machine pushing a Mach-O to Linux).
    let target_facts = transport.detect_facts(&target, &key_path, host_key)?;
    if let Some(facts) = &target_facts {
        facts.require_supported()?;
        match crate::bootstrap::binary_arch(&ouro_binary) {
            Some(bin_arch) if Some(bin_arch) == facts.norm_arch() => {}
            Some(bin_arch) => {
                return Err(OuroError::Validation(format!(
                    "--ouro-binary is {bin_arch} but the target is {}; supply a matching Linux binary",
                    facts.norm_arch().unwrap_or(&facts.arch)
                )));
            }
            None => {
                return Err(OuroError::Validation(format!(
                    "--ouro-binary {} is not a Linux ELF for {} (a macOS/other-arch binary cannot run on the target)",
                    ouro_binary.display(),
                    facts.norm_arch().unwrap_or(&facts.arch)
                )));
            }
        }
    }

    let manifest = provision::execute(
        &transport,
        &target,
        &key_path,
        host_key,
        &control_pubkey,
        &ouro_binary,
    )?;

    // TOFU pin (only when no expected fingerprint was pre-verified above): after a successful
    // provision, capture and pin the target host key so later dispatch enforces it.
    if pinned_fp.is_none() && manifest.ok && !dry_run {
        pinned_fp = Some(pin_host_key(&host, port, &paths.known_hosts, None)?);
    }

    output::print_json(&ToolOutput::ok("ouro.init", manifest.ok && !dry_run).with_data(json!({
        "manifest": manifest,
        "dry_run": dry_run,
        "pinned_host_key": pinned_fp,
        "declared_runtime": declared_runtime,
        "target_facts": target_facts,
        "security_note": "bootstrap credential is NOT mechanism-isolated from the agent \
            (convenience mode, P0-1); a poisoned prompt could invoke init via the agent. \
            Relies on upstream control-machine / agent-runtime security.",
    })))?;
    if !manifest.ok {
        return Err(OuroError::Validation(
            "init did not complete: a provisioning step failed (see manifest)".to_string(),
        ));
    }
    Ok(())
}

/// S0017 p3-3 — pin the target's SSH host key so later dispatch enforces it. Captures the key
/// with `ssh-keyscan`, optionally verifies it against an out-of-band fingerprint (`sha256:…` or
/// `SHA256:…`) the operator obtained through a trusted channel (this is what defends the FIRST
/// connection against MITM; without it the pin is trust-on-first-use — good against LATER key
/// swaps), then writes it (idempotently) into the ouro-managed known_hosts. Returns the pinned
/// key's fingerprint.
fn fingerprint_of(entry: &str) -> Option<String> {
    // Fingerprint a single known_hosts entry so each key's fingerprint is unambiguously its own
    // (fixes the "match one, pin all" flaw — pairing by position across ssh-keygen output is
    // fragile). Returns the SHA256 token, e.g. "SHA256:abc…".
    let mut kg = Command::new("ssh-keygen")
        .args(["-l", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    kg.stdin.take()?.write_all(format!("{entry}\n").as_bytes()).ok()?;
    let out = kg.wait_with_output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .find(|t| t.starts_with("SHA256:"))
        .map(str::to_string)
}

fn pin_host_key(host: &str, port: u16, known_hosts: &std::path::Path, expected: Option<&str>) -> Result<String> {
    let scan = Command::new("ssh-keyscan")
        .args(["-T", "5", "-p", &port.to_string(), host])
        .output()
        .map_err(|e| OuroError::Validation(format!("ssh-keyscan failed to run: {e}")))?;
    let scanned = String::from_utf8_lossy(&scan.stdout);
    let entries: Vec<&str> = scanned
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    if entries.is_empty() {
        return Err(OuroError::Validation(format!(
            "ssh-keyscan captured no host key for {host}:{port} (is sshd reachable?)"
        )));
    }
    let norm = |s: &str| s.trim().trim_start_matches("SHA256:").trim_start_matches("sha256:").to_string();

    // Fingerprint each entry INDEPENDENTLY; when an expected fingerprint is given, KEEP ONLY the
    // entries whose own fingerprint matches it. A MITM presenting the genuine key alongside an
    // extra attacker key therefore cannot get the attacker key pinned.
    let mut kept: Vec<(&str, String)> = Vec::new();
    let mut seen_fps: Vec<String> = Vec::new();
    for e in &entries {
        let Some(fp) = fingerprint_of(e) else { continue };
        seen_fps.push(fp.clone());
        match expected {
            Some(exp) if norm(&fp) == norm(exp) => kept.push((*e, fp)),
            Some(_) => {}       // non-matching key type — do NOT pin it
            None => kept.push((*e, fp)), // no expected fingerprint => TOFU: pin what was offered
        }
    }
    if let Some(exp) = expected {
        if kept.is_empty() {
            return Err(OuroError::Validation(format!(
                "host key fingerprint mismatch for {host}:{port} — expected {exp}, got {}. \
                 REFUSING to pin (possible MITM).",
                seen_fps.join(", ")
            )));
        }
    }
    if kept.is_empty() {
        return Err(OuroError::Validation(format!(
            "could not fingerprint any host key for {host}:{port}"
        )));
    }
    let primary = kept[0].1.clone();

    // Idempotent write: drop any prior entry for this host, then append only the kept key(s).
    if let Some(parent) = known_hosts.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !known_hosts.exists() {
        std::fs::write(known_hosts, "")?;
    }
    let remove_target = if port == 22 { host.to_string() } else { format!("[{host}]:{port}") };
    let _ = Command::new("ssh-keygen")
        .args(["-R", &remove_target, "-f", &known_hosts.display().to_string()])
        .output();
    let mut existing = std::fs::read_to_string(known_hosts).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    for (e, _) in &kept {
        existing.push_str(e);
        existing.push('\n');
    }
    std::fs::write(known_hosts, existing)?;
    Ok(primary)
}

/// `ouro-ops deinit` (S0017 p1-5) — the reverse of init: remove the ouro base and restore the
/// target, over the same privileged bootstrap transport (as the bootstrap sudo user init kept).
/// SAFE by default: refuses while a cardano-node is running (removing the base would orphan it)
/// unless `--force`; access principals are removed last so a mid-run failure never locks the
/// box. Only unambiguously ouro-owned artifacts are removed; the shared `node` account is kept
/// unless `--remove-node`. Idempotent. Prints a removal manifest.
fn run_deinit(args: &[String]) -> Result<()> {
    let host = flag_value(args, "--host")?.to_string();
    let port: u16 = optional_flag_value(args, "--port")
        .unwrap_or("22")
        .parse()
        .map_err(|_| OuroError::InvalidArgs("--port must be a number".to_string()))?;
    let user = flag_value(args, "--bootstrap-user")?.to_string();
    let key_ref = CredentialRef::parse(flag_value(args, "--bootstrap-key")?)?;
    let force = args.iter().any(|a| a == "--force");
    let remove_node = args.iter().any(|a| a == "--remove-node");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let host_key = match optional_flag_value(args, "--host-key") {
        Some("yes") => HostKeyCheck::Yes,
        _ => HostKeyCheck::AcceptNew,
    };

    let paths = ConfigPaths::discover();
    let key_path = key_ref.resolve(&paths.credentials_dir)?;
    if !dry_run && !key_path.is_file() {
        return Err(OuroError::Validation(format!(
            "bootstrap credential key not found: {}",
            key_path.display()
        )));
    }

    let target = BootstrapTarget { host, port, user };
    let transport = BootstrapTransport::new(dry_run);

    // Running-node safety gate: refuse by default rather than orphan a forging node. Fail closed
    // if the state cannot be determined (unless the operator forces it).
    if !force && !dry_run {
        match provision::node_is_running(&transport, &target, &key_path, host_key) {
            Some(true) => {
                return Err(OuroError::Validation(
                    "a cardano-node is running on the target; deinit refused (removing the base \
                     would orphan it). Stop the node first, or pass --force to proceed anyway."
                        .to_string(),
                ))
            }
            None => {
                return Err(OuroError::Validation(
                    "could not determine whether a node is running on the target; deinit refused \
                     (fail-closed). Pass --force to override."
                        .to_string(),
                ))
            }
            Some(false) => {}
        }
    }

    let manifest = provision::execute_deinit(&transport, &target, &key_path, host_key, remove_node)?;
    output::print_json(&ToolOutput::ok("ouro.deinit", manifest.ok && !dry_run).with_data(json!({
        "manifest": manifest,
        "dry_run": dry_run,
        "removed_node_account": remove_node,
    })))?;
    if !manifest.ok {
        return Err(OuroError::Validation(
            "deinit did not complete: a removal step failed (see manifest)".to_string(),
        ));
    }
    Ok(())
}

/// `ouro-ops skill show <name>` / `ouro-ops skill list` (S0016 p2-7).
///
/// `show` prints the skill's decision tree (its embedded, compiled-in `SKILL.md`) as raw
/// markdown for the agent to consume. This is the AUTHORITATIVE decision source: it comes
/// from the verified binary, never from the (untrusted) pasted prompt (R2 N3). A spoofed
/// onboarding site cannot alter what the agent follows here.
fn run_skill(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("list") => {
            output::print_json(&ToolOutput::ok("ouro.skill.list", false).with_data(json!({
                "skills": crate::skills::skill_names(),
                "embedded_digest": crate::skills::embedded_digest(),
            })))?;
            Ok(())
        }
        Some("show") => {
            let name = args.get(1).map(String::as_str).ok_or_else(|| {
                OuroError::InvalidArgs("expected skill show <name>".to_string())
            })?;
            // A skill name is a single [a-z0-9-] segment (same discipline as tool names).
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return Err(OuroError::InvalidArgs(format!(
                    "skill name must be a single [a-z0-9-] segment: {name}"
                )));
            }
            match crate::skills::skill_doc(name) {
                Some(doc) => {
                    std::io::stdout().write_all(doc.as_bytes())?;
                    std::io::stdout().flush()?;
                    Ok(())
                }
                None => Err(OuroError::Validation(format!(
                    "unknown skill {name}; available: {}",
                    crate::skills::skill_names().join(", ")
                ))),
            }
        }
        _ => Err(OuroError::InvalidArgs(
            "expected skill show <name> | skill list".to_string(),
        )),
    }
}

/// `ouro-ops manifest show | verify --against <file>` (S0016 p2-6).
///
/// `show` prints the bare bundle manifest derived from the binary's embedded assets (commit
/// it as `packaging/bundle-manifest.json`). `verify` proves the running binary's embedded
/// decision/mechanism/schema content matches a signed/committed manifest — the drift &
/// tamper gate (TC-4/TC-13). A per-class mismatch names exactly which layer drifted.
fn run_manifest(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("show") => {
            println!(
                "{}",
                serde_json::to_string_pretty(&crate::skills::bundle_manifest())
                    .expect("manifest serializes")
            );
            Ok(())
        }
        Some("verify") => {
            let path = flag_value(args, "--against")?;
            let expected: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path)?).map_err(|e| {
                    OuroError::Validation(format!("manifest {path} is not valid JSON: {e}"))
                })?;
            let actual = crate::skills::bundle_manifest();
            let mut drift = Vec::new();
            for key in ["decision_hash", "skills_hash", "schema_hash", "embedded_digest"] {
                if expected.get(key) != actual.get(key) {
                    drift.push(key);
                }
            }
            if drift.is_empty() {
                output::print_json(&ToolOutput::ok("ouro.manifest.verify", false).with_data(
                    json!({ "verified": true, "embedded_digest": actual.get("embedded_digest") }),
                ))?;
                Ok(())
            } else {
                Err(OuroError::Validation(format!(
                    "bundle manifest drift in: {} — embedded assets differ from the signed manifest",
                    drift.join(", ")
                )))
            }
        }
        _ => Err(OuroError::InvalidArgs(
            "expected manifest show | manifest verify --against <file>".to_string(),
        )),
    }
}

fn run_confirm(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) == Some("adopt")
        && args.get(1).map(String::as_str) == Some("create")
    {
        return crate::s0019_cli::run_adopt_confirm_create(&args[2..]);
    }
    // S0019 p4-1: `confirm create --op <id> --node <id> --intent-hash <h>` mints a token bound to
    // the exact canonical intent (§2.5). Routed here so the greenfield `op` path reuses `confirm`.
    if args.first().map(String::as_str) == Some("create")
        && optional_flag_value(args, "--op").is_some()
    {
        return crate::s0019_cli::run_confirm_create(&args[1..]);
    }
    match args.first().map(String::as_str) {
        Some("create") => {
            let action = flag_value(args, "--action")?;
            let machine = flag_value(args, "--machine")?;
            let ttl = confirm::parse_ttl(flag_value(args, "--ttl").unwrap_or("60s"))?;
            // p2-5b: bind the token to the target fingerprint the human reviewed (the
            // `data.evidence_hash` from `detect/runtime`), so it only fires against that
            // exact, unchanged target.
            let evidence = optional_flag_value(args, "--runtime-evidence");
            let paths = ConfigPaths::discover();
            let token =
                ConfirmationStore::create(&paths.confirmations, action, machine, ttl, evidence)?;
            output::print_json(
                &ToolOutput::ok("ouro.confirm.create", true).with_data(json!({
                    "token": token.token,
                    "action": token.action,
                    "machine": token.machine,
                    "expires_at": token.expires_at,
                    "runtime_evidence": token.evidence,
                    "single_use": true
                })),
            )?;
            Ok(())
        }
        // `ouro-ops confirm preview --tool <t> --dispatch <m> --spec <f>` (S0016 p4-2).
        // GROUND-TRUTH confirmation: prints the EXACT command that would run on the target
        // (the resolved ssh + sudo wrapper argv), WITHOUT executing. A human/agent approves
        // the real action — not the prompt's narrative of it.
        Some("preview") => {
            let tool_name = flag_value(args, "--tool")?;
            validate_tool_name(tool_name)?;
            let machine_id = flag_value(args, "--dispatch")?;
            let spec_path = flag_value(args, "--spec")?;
            let remote_spec = optional_flag_value(args, "--remote-spec").unwrap_or(spec_path);
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
            let machine = spec
                .machines
                .iter()
                .find(|m| m.id == machine_id)
                .ok_or_else(|| OuroError::Validation(format!("unknown machine {machine_id}")))?;
            let paths = ConfigPaths::discover();
            let key_path = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
            let argv = SshRunner::tool_run_argv(
                &machine.ssh,
                &key_path,
                &paths.known_hosts,
                tool_name,
                machine_id,
                remote_spec,
            );
            let command = std::iter::once("ssh".to_string())
                .chain(argv)
                .collect::<Vec<_>>()
                .join(" ");
            output::print_json(&ToolOutput::ok("ouro.confirm.preview", false).with_data(json!({
                "tool": tool_name,
                "machine": machine_id,
                "ssh_command": command,
                "note": "ground-truth: this exact command runs on the target if you approve",
            })))?;
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected confirm create --action <a> --machine <m> | confirm preview --tool <t> --dispatch <m> --spec <f>".to_string(),
        )),
    }
}

fn run_tool(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        // `--dispatch <m>` on control = Model B remote dispatch: SSH to m and run the
        // tool there. Absent it, execute the L2 script locally (S0014 semantics).
        Some("run") if optional_flag_value(args, "--dispatch").is_some() => run_tool_dispatch(args),
        Some("run") => run_tool_exec(args),
        Some("verify-context") => run_tool_verify_context(args),
        _ => Err(OuroError::InvalidArgs(
            "expected tool run <skill/script> --spec <path> | tool verify-context".to_string(),
        )),
    }
}

/// Root dir holding `ouro-skills/`. Tries `$OURO_SKILLS_DIR`, then a repo-relative
/// `ouro-skills`, then the installed `/opt/ouro/ouro-skills` (the bed target layout).
fn skills_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("OURO_SKILLS_DIR") {
        return PathBuf::from(dir);
    }
    for candidate in ["ouro-skills", "/opt/ouro/ouro-skills"] {
        if std::path::Path::new(candidate).is_dir() {
            return PathBuf::from(candidate);
        }
    }
    PathBuf::from("ouro-skills")
}

/// Validate a `<skill>/<script>` tool name: exactly two segments, each `[a-z0-9-]`, no
/// traversal. Enforced BEFORE remote dispatch (control) AND before local execution
/// (target), so a crafted name can never reach a remote shell or the filesystem.
fn validate_tool_name(tool_name: &str) -> Result<()> {
    let mut parts = tool_name.split('/');
    let skill = parts.next().unwrap_or_default();
    let script = parts.next().unwrap_or_default();
    if skill.is_empty()
        || script.is_empty()
        || parts.next().is_some()
        || tool_name.contains("..")
        || !skill.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        || !script.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(OuroError::InvalidArgs(format!(
            "tool name must be <skill>/<script> of [a-z0-9-]: got {tool_name}"
        )));
    }
    Ok(())
}

/// Resolve `<skill>/<script>` to an allowlisted L2 script path, rejecting traversal.
///
/// Resolution order (S0016 p2-1):
///   1. On-disk `skills_root()` — used in dev / the E2E bed / tests, or via `OURO_SKILLS_DIR`.
///   2. The compile-time-embedded copy — the installed single binary carries no on-disk
///      skills, so the script is materialized into a per-run `0700` temp dir under
///      `ouro-skills/…` (so the scripts' `$ROOT/ouro-skills/lib/...` sourcing still resolves),
///      run from there, and removed afterwards. Nothing is fetched from disk or network.
///
/// Returns the script path plus an optional temp base dir the caller MUST remove after use.
fn resolve_skill_script(tool_name: &str, label: &str) -> Result<(PathBuf, Option<PathBuf>)> {
    validate_tool_name(tool_name)?;
    let (skill, script) = tool_name.split_once('/').expect("validated tool name");
    let disk = skills_root().join(format!("{skill}/scripts/{script}.sh"));
    if disk.is_file() {
        return Ok((disk, None));
    }
    // Installed-binary path: extract the embedded skill's shell assets to a fresh temp base.
    if crate::skills::script(skill, script).is_none() {
        // p5-16: name WHICH binary is missing the tool — when this fires on a dispatched
        // TARGET, the operator sees it via the control machine and easily misreads it as a
        // local install problem (real acceptance case: a stale target binary predating a
        // newly added tool).
        return Err(OuroError::Validation(format!(
            "no such tool script: {skill}/scripts/{script}.sh (neither on disk nor embedded \
             in THIS ouro-ops v{}). If this error came from a dispatched run, the TARGET's \
             installed ouro-ops predates the tool — re-run `ouro-ops init` with a newer \
             --ouro-binary to update it.",
            env!("CARGO_PKG_VERSION")
        )));
    }
    // p1-6: per-invocation, audit-id-namespaced scratch (traceable + GC-visible via the
    // `ouro-run-` prefix). Terminal-only deletion is the caller's responsibility.
    let base = crate::state::run_dir(
        &crate::state::run_root(),
        label,
        std::process::id(),
        &uuid::Uuid::new_v4().to_string(),
    );
    std::fs::create_dir_all(&base)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700));
    }
    crate::skills::extract_shell_assets(skill, &base.join("ouro-skills"))?;
    let path = base
        .join("ouro-skills")
        .join(skill)
        .join("scripts")
        .join(format!("{script}.sh"));
    Ok((path, Some(base)))
}

/// Model B remote dispatch (control side): resolve the target machine's SSH endpoint
/// and credential from the spec, then run `sudo ouro-ops tool run <tool> --machine <m>` on
/// the target over SSH. Audit + token are minted/verified ON THE TARGET (§2.1 D2), so
/// control mints nothing and passes no `--audit-id`; it relays the target's JSON + exit
/// code. `--remote-spec` overrides the target-side spec path (default: same as --spec).
/// Run the read-only `detect/runtime` probe on a target over SSH and extract its
/// `data.evidence_hash` — the live target fingerprint the confirm gate binds against.
/// Fails closed (a non-zero probe or unparseable output refuses the destructive action).
fn dispatch_runtime_evidence(
    target: &crate::domain::SshTarget,
    key_path: &std::path::Path,
    known_hosts: &std::path::Path,
    machine_id: &str,
    remote_spec: &str,
) -> Result<String> {
    let outcome =
        SshRunner::new(false).execute(target, key_path, known_hosts, "detect/runtime", machine_id, remote_spec)?;
    if outcome.status != 0 {
        return Err(OuroError::Validation(format!(
            "could not detect {machine_id} runtime before a destructive action (exit {}); refusing",
            outcome.status
        )));
    }
    let value: serde_json::Value = serde_json::from_str(outcome.stdout.trim()).map_err(|e| {
        OuroError::Validation(format!("detect/runtime output not JSON for {machine_id}: {e}"))
    })?;
    value
        .get("data")
        .and_then(|d| d.get("evidence_hash"))
        .and_then(|h| h.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            OuroError::Validation(format!("detect/runtime gave no evidence_hash for {machine_id}"))
        })
}

fn run_tool_dispatch(args: &[String]) -> Result<()> {
    let tool_name = args
        .get(1)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| OuroError::InvalidArgs("missing tool name".to_string()))?
        .clone();
    // Validate on control BEFORE building the remote command (defense in depth on top of
    // ssh.rs shell-quoting): a crafted tool name is rejected here, not on the target.
    validate_tool_name(&tool_name)?;
    let machine_id = flag_value(args, "--dispatch")?;
    let spec_path = flag_value(args, "--spec")?;
    // remote_spec defaults to the same path on the target (provisioning pushes the spec
    // to the same absolute path); override with --remote-spec if they differ.
    let remote_spec = optional_flag_value(args, "--remote-spec").unwrap_or(spec_path);
    let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
    let machine = spec
        .machines
        .iter()
        .find(|candidate| candidate.id == machine_id)
        .ok_or_else(|| OuroError::Validation(format!("unknown machine {machine_id}")))?;
    let paths = ConfigPaths::discover();
    let key_path = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    if !key_path.is_file() {
        return Err(OuroError::Validation(format!(
            "credential key not found for {machine_id}: {} (run provisioning)",
            key_path.display()
        )));
    }
    // p2-5b enforcement: a directly-dispatched destructive lifecycle op requires a human
    // confirmation token BOUND to the target's LIVE fingerprint. Control re-detects the
    // target here (read-only) and consumes the token against that live evidence, so a token
    // approved for a different — or since-changed — target cannot drive the action (review P1
    // + TOCTOU). Orchestrated upgrade (upgrade/rollout -> upgrade-one) is a separate rollout-
    // level confirm and is intentionally NOT gated here.
    const CONFIRM_BOUND_TOOLS: &[&str] = &[
        "runtime/restart",
        "runtime/topology-apply",
        "kes-rotation/rotate",
        // p4-6 offline install: promotes the staged KES key + cold-signed opcert and restarts —
        // as target-disruptive as rotate, so it takes the same evidence-bound confirm gate.
        "kes-rotation/push-offline",
        // p4-2 registration submit: broadcasts an on-chain pool-registration tx (irreversible) —
        // confirm-bound so a human approves the specific target before it is submitted.
        "deploy/register-submit",
    ];
    if CONFIRM_BOUND_TOOLS.contains(&tool_name.as_str()) {
        let token = optional_flag_value(args, "--confirm-token").ok_or_else(|| {
            OuroError::Validation(format!(
                "{tool_name} on {machine_id} requires a target-bound confirmation token — the \
                 token represents the OPERATOR'S approval: run `ouro-ops tool run detect/runtime \
                 --dispatch {machine_id} --spec <spec>`, present data.evidence_hash and the \
                 pending action to the operator and get their explicit go-ahead, then `ouro-ops \
                 confirm create --action {tool_name} --machine {machine_id} --runtime-evidence \
                 <hash>` and pass --confirm-token"
            ))
        })?;
        let live_fp =
            dispatch_runtime_evidence(&machine.ssh, &key_path, &paths.known_hosts, machine_id, remote_spec)?;
        consume_confirmation(&paths, Some(token), &tool_name, machine_id, Some(&live_fp))?;
    }
    let outcome = SshRunner::new(false).execute(
        &machine.ssh,
        &key_path,
        &paths.known_hosts,
        &tool_name,
        machine_id,
        remote_spec,
    )?;
    output::forward_tool_stdout(outcome.stdout.as_bytes())?;
    std::io::stdout().flush()?;
    if !outcome.stderr.is_empty() {
        std::io::stderr().write_all(outcome.stderr.as_bytes())?;
    }
    std::process::exit(outcome.status);
}

/// S0017 p5-18 — `ouro-ops diag ...`: free-form UNPRIVILEGED diagnostics channel.
fn run_diag(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("exec") => run_diag_exec(&args[1..]),
        _ => Err(OuroError::InvalidArgs(
            "expected diag exec --dispatch <machine> --spec <spec> [--timeout <s>] -- <command>"
                .to_string(),
        )),
    }
}

/// `diag exec` — run ONE agent-authored command on the target as the unprivileged
/// `ouro-diag` principal. The fence is the Unix permission model, not a command list:
/// ouro-diag has no sudoers entry, cannot write node content, and cannot read 0700 secret
/// dirs. Every invocation is audited on the control side; output is truncated to a bounded
/// size so a noisy command cannot flood the agent context. The command's own exit code is
/// DATA in the payload — a failing probe is still a delivered diagnosis (this tool only
/// errors on transport failure).
fn run_diag_exec(args: &[String]) -> Result<()> {
    let machine_id = flag_value(args, "--dispatch")?;
    let spec_path = flag_value(args, "--spec")?;
    let timeout_s: u32 = optional_flag_value(args, "--timeout")
        .unwrap_or("30")
        .parse()
        .map_err(|_| OuroError::InvalidArgs("--timeout must be seconds (max 300)".to_string()))?;
    let timeout_s = timeout_s.clamp(1, 300);
    let sep = args
        .iter()
        .position(|a| a == "--")
        .ok_or_else(|| OuroError::InvalidArgs(
            "missing `--` separator before the diagnostic command".to_string(),
        ))?;
    let command = args[sep + 1..].join(" ");
    if command.trim().is_empty() {
        return Err(OuroError::InvalidArgs("empty diagnostic command".to_string()));
    }

    let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
    spec.validate()?;
    let machine = spec
        .machines
        .iter()
        .find(|candidate| candidate.id == machine_id)
        .ok_or_else(|| OuroError::Validation(format!("unknown machine {machine_id}")))?;
    let paths = ConfigPaths::discover();
    let key_path = machine.ssh.key_ref.resolve(&paths.credentials_dir)?;
    if !key_path.is_file() {
        return Err(OuroError::Validation(format!(
            "credential key not found for {machine_id}: {} (run onboarding)",
            key_path.display()
        )));
    }

    let store = AuditStore::open(&paths.audit_db)?;
    let audit_id = store.begin_invocation("diag/exec", Some(machine_id))?;
    let outcome = SshRunner::new(false).diag_exec(
        &machine.ssh,
        &key_path,
        &paths.known_hosts,
        &command,
        timeout_s,
    )?;
    store.finish_invocation(&audit_id, "diag/exec")?;

    // ssh exit 255 = transport-level failure (unreachable, key not authorized for ouro-diag,
    // host-key mismatch) — that IS this tool's failure, with the onboarding recovery path.
    if outcome.status == 255 {
        return Err(OuroError::Validation(format!(
            "diag transport to {machine_id} failed as ouro-diag: {} — is the target onboarded \
             with a binary that authorizes ouro-diag (re-run `ouro-ops init` if it predates \
             the diag channel)?",
            outcome.stderr.lines().last().unwrap_or("(no stderr)")
        )));
    }

    const CAP: usize = 16 * 1024;
    let cap = |s: &str| -> (String, bool) {
        if s.len() > CAP {
            let mut end = CAP;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            (s[..end].to_string(), true)
        } else {
            (s.to_string(), false)
        }
    };
    let (stdout, stdout_truncated) = cap(&outcome.stdout);
    let (stderr, stderr_truncated) = cap(&outcome.stderr);
    output::print_json(&ToolOutput::ok("ouro.diag.exec", false).with_data(json!({
        "machine": machine_id,
        "principal": "ouro-diag",
        "command": command,
        "timeout_s": timeout_s,
        "exit_code": outcome.status,
        "timed_out": outcome.status == 124,
        "stdout": stdout,
        "stdout_truncated": stdout_truncated,
        "stderr": stderr,
        "stderr_truncated": stderr_truncated,
        "audit_id": audit_id,
    })))?;
    Ok(())
}

/// `ouro-ops tool run` — the sole audited write entrypoint. It creates (or reuses via
/// `--audit-id`) an audit invocation, signs an invocation token bound to that id,
/// executes the resolved L2 script with a controlled environment, captures its
/// output, and records a finish/crash terminal audit event before propagating the
/// child's exit code. Scripts verify the signed token (via `tool verify-context`),
/// so a fabricated `OURO_AUDIT_ID` env var alone can no longer satisfy the gate.
fn run_tool_exec(args: &[String]) -> Result<()> {
    let tool_name = args
        .get(1)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| OuroError::InvalidArgs("missing tool name".to_string()))?
        .clone();
    let spec_path = optional_flag_value(args, "--spec");
    let machine = optional_flag_value(args, "--machine");
    let provided_audit = optional_flag_value(args, "--audit-id");
    // The (untrusted) min_ouro_version a pasted prompt may carry — it can only RAISE the
    // requirement, never lower it (S0016 p3-2 / R2 P0-2).
    let min_ouro = optional_flag_value(args, "--min-ouro");
    // S0017 p5-17: skill-pack parity — a dispatching control machine sends its embedded
    // digest; if THIS binary embeds a different pack, executing would silently run outdated
    // tool logic, so fail closed BEFORE any gate/audit/extraction with the recovery path.
    if let Some(expected) = optional_flag_value(args, "--expect-embedded") {
        let actual = crate::skills::embedded_digest();
        if expected != actual {
            return Err(OuroError::Validation(format!(
                "skill-pack digest mismatch: the control machine expects {expected} but THIS \
                 ouro-ops v{} embeds {actual} — the target's installed binary is out of date; \
                 re-run `ouro-ops init` with a newer --ouro-binary to update it",
                env!("CARGO_PKG_VERSION")
            )));
        }
    }

    let paths = ConfigPaths::discover();
    // Version gate BEFORE any execution (p3-2/p3-3): required = max(prompt, embedded,
    // monotonic anti-rollback, security floor). Fails closed if the binary is below it. Runs
    // before extraction, so a gate failure leaves no scratch to clean up.
    let gate = crate::version::gate(&paths.home, min_ouro)?;
    let store = AuditStore::open(&paths.audit_db)?;
    // Reuse a caller-supplied audit id (e.g. an orchestrator's) when it refers to a
    // real invocation; otherwise begin a fresh one.
    let audit_id = match provided_audit {
        Some(id) if store.invocation_has_start(id)? => id.to_string(),
        _ => store.begin_invocation(&tool_name, machine)?,
    };
    // p1-6 crash-tolerant GC: reclaim scratch left by runs that died before their terminal
    // cleanup. Surface (never swallow) any dir it could not remove — an unreclaimable scratch
    // area is an operational fact the operator must see.
    for (path, err) in crate::state::gc_stale_runs(
        &crate::state::run_root(),
        std::time::Duration::from_secs(3600),
        std::time::SystemTime::now(),
    ) {
        eprintln!("ouro: could not GC stale run state {}: {err}", path.display());
    }
    // Extract the embedded skill into a per-invocation, audit-id-namespaced scratch dir (p1-6).
    let (script, temp_base) = resolve_skill_script(&tool_name, &audit_id)?;
    let secret = confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let token = confirm::invocation_token(&secret, &audit_id);
    let self_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "ouro-ops".to_string());

    // env_clear + allowlist: the child script receives ONLY a controlled environment.
    // This prevents an agent from injecting policy or test hooks (e.g. a quorum knob or
    // a failure-injection flag) through the caller's environment — policy comes from the
    // spec, not env (§2.2#4 / p5-2). Only inert runtime *inputs* are passed through.
    const ENV_ALLOWLIST: &[&str] = &[
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "TMPDIR",
        "OURO_STATE_DIR",
        "OURO_STATUS_SNAPSHOT",
        "OURO_MITHRIL_DIGEST",
        "OURO_MITHRIL_CERT_CHAIN",
        "OURO_LEGACY_MANIFEST",
        "OURO_CARDANO_ROOT",
        // p5-18 troubleshooting/logs inputs: a log-source override (test seam pointing the
        // classifier at a fixture file) and a line bound — inert read inputs, not policy.
        "OURO_LOGS_SOURCE",
        "OURO_LOG_LINES",
    ];
    let mut cmd = Command::new("bash");
    cmd.env_clear();
    for key in ENV_ALLOWLIST {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.arg(&script)
        .env("OURO_HOME", &paths.home)
        .env("OURO_AUDIT_ID", &audit_id)
        .env("OURO_TOOL_NAME", &tool_name)
        .env("OURO_INVOCATION_TOKEN", &token)
        .env("OURO_BIN", &self_bin);
    if let Some(spec_path) = spec_path {
        cmd.env("OURO_SPEC", spec_path);
    }
    if let Some(machine) = machine {
        cmd.env("OURO_MACHINE", machine);
    }
    let output = cmd.output()?;
    // Self-clean the per-run extraction (p2-1): operations leave no accumulated scripts. TERMINAL
    // deletion — only now, after the child has exited (never mid-run). Done before the exit()s
    // below (which skip destructors). Cleanup errors are surfaced, not swallowed (p1-6).
    if let Some(base) = &temp_base {
        if let Err(e) = std::fs::remove_dir_all(base) {
            eprintln!("ouro: could not remove run scratch {}: {e}", base.display());
        }
    }
    // Forward the child's stdout: single-line JSON when captured, human-readable on a TTY.
    crate::output::forward_tool_stdout(&output.stdout)?;
    std::io::stdout().flush()?;

    match output.status.code() {
        Some(code) => {
            // p3-4: record the ACTUAL executing ouro-ops version (and a monotonic-floor reset,
            // if the anti-rollback state had to be re-established) for reproducibility.
            let detail = format!(
                "exit_{code} ouro={}{}",
                crate::version::fmt(crate::version::current()),
                if gate.rollback_reset { " rollback_reset=1" } else { "" },
            );
            store.record_terminal(&audit_id, &tool_name, machine, "finish", Some(code as i64), &detail)?;
            std::process::exit(code);
        }
        None => {
            // Terminated by a signal — a crash, not a structured exit.
            store.record_crash(&audit_id, &tool_name, "child terminated by signal")?;
            std::process::exit(40);
        }
    }
}

/// Verify that an L2 script is running inside a genuine `ouro-ops tool run` context: the
/// invocation id must exist as a `start` audit event AND the supplied token must match
/// the id signed with the local secret. Exits nonzero (→ script refuses to write) on
/// any mismatch. Called by `ouro_require_audit_context` in `ouro-lib.sh`.
fn run_tool_verify_context(args: &[String]) -> Result<()> {
    let audit_id = flag_value(args, "--audit-id")?;
    let token = flag_value(args, "--token")?;
    let paths = ConfigPaths::discover();
    let secret = confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let store = AuditStore::open(&paths.audit_db)?;
    if confirm::verify_invocation_token(&secret, audit_id, token)
        && store.invocation_has_start(audit_id)?
    {
        output::print_json(&ToolOutput::ok("ouro.tool.verify-context", false).with_data(json!({
            "audit_id": audit_id,
            "verified": true
        })))?;
        Ok(())
    } else {
        Err(OuroError::Validation(
            "invalid or forged invocation token; run via ouro-ops tool run".to_string(),
        ))
    }
}

fn run_deploy(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        // S0017 p4-2: emit a self-contained cold-signing script for an unsigned registration/deploy
        // transaction. It embeds ONLY the PUBLIC tx body and witnesses it with the cold key(s) on
        // the air-gapped machine (`cardano-cli <era> transaction witness`, read in place). Only the
        // public witnesses come back for online assemble + submit. --cold-key is repeatable.
        Some("cold-sign-script") => {
            let tx_body_path = flag_value(args, "--tx-body")?;
            let era = optional_flag_value(args, "--era").unwrap_or("conway");
            let cardano_cli = optional_flag_value(args, "--cardano-cli").unwrap_or("cardano-cli");
            let roles: Vec<String> = args
                .iter()
                .zip(args.iter().skip(1))
                .filter(|(flag, _)| flag.as_str() == "--cold-key")
                .map(|(_, role)| role.clone())
                .collect();
            if roles.is_empty() {
                return Err(OuroError::InvalidArgs(
                    "expected at least one --cold-key <role> (e.g. --cold-key cold --cold-key stake)"
                        .to_string(),
                ));
            }
            // Optional network flag some cardano-cli versions require on `transaction witness`.
            let network = if args.iter().any(|a| a == "--mainnet") {
                "--mainnet".to_string()
            } else if let Some(magic) = optional_flag_value(args, "--testnet-magic") {
                format!("--testnet-magic {magic}")
            } else {
                String::new()
            };
            let tx_body = std::fs::read_to_string(tx_body_path).map_err(|e| {
                OuroError::Validation(format!("cannot read --tx-body {tx_body_path}: {e}"))
            })?;
            let generated_at = chrono::Utc::now().to_rfc3339();
            let script = crate::cold_sign::tx_cold_sign_script(
                &tx_body,
                &roles,
                era,
                &network,
                cardano_cli,
                &generated_at,
            )?;
            std::io::stdout().write_all(script.as_bytes())?;
            std::io::stdout().flush()?;
            // p4-8 trusted delivery: digest to STDERR for out-of-band verification (see kes above).
            eprintln!("sha256={}", crate::skills::sha256_hex(script.as_bytes()));
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected deploy cold-sign-script --tx-body <path> --cold-key <role> [--cold-key <role>...] [--era conway]"
                .to_string(),
        )),
    }
}

fn run_pool(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("register-tx") => {
            let spec_path = flag_value(args, "--spec")?;
            let out_dir = flag_value(args, "--out").unwrap_or(".ouro/staging/register-tx");
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
            let paths = ConfigPaths::discover();
            let store = AuditStore::open(&paths.audit_db)?;
            let report = pool::build_register_tx(&spec, &PathBuf::from(out_dir), &store)?;
            output::print_json(
                &ToolOutput::ok("ouro.pool.register-tx", true).with_data(json!(report)),
            )?;
            Ok(())
        }
        Some("overview") => {
            let spec_path = flag_value(args, "--spec")?;
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
            let snapshot = optional_flag_value(args, "--snapshot").map(PathBuf::from);
            let overview = pool::overview(&spec, snapshot.as_deref())?;
            output::print_json(
                &ToolOutput::ok("ouro.pool.overview", false).with_data(json!(overview)),
            )?;
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected pool register-tx --spec <path> | pool overview --spec <path>".to_string(),
        )),
    }
}

fn run_kes(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("generate") => {
            let spec_path = flag_value(args, "--spec")?;
            let machine = flag_value(args, "--machine")?;
            let out_dir = flag_value(args, "--out").unwrap_or(".ouro/staging/kes");
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
            let report = kes::generate_vkey(&spec, machine, &PathBuf::from(out_dir))?;
            output::print_json(
                &ToolOutput::ok("ouro.kes.generate", true).with_data(json!(report)),
            )?;
            Ok(())
        }
        Some("counter") if args.get(1).map(String::as_str) == Some("status") => {
            let state = kes::read_counter_state(&PathBuf::from(flag_value(args, "--state")?))?;
            output::print_json(
                &ToolOutput::ok("ouro.kes.counter.status", false).with_data(json!(state)),
            )?;
            Ok(())
        }
        Some("push") => {
            let spec_path = flag_value(args, "--spec")?;
            let machine = flag_value(args, "--machine")?;
            let cert = PathBuf::from(flag_value(args, "--cert")?);
            let counter = PathBuf::from(flag_value(args, "--counter-state")?);
            let token = optional_flag_value(args, "--confirm-token");
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
            let paths = ConfigPaths::discover();
            // Single confirmation gate: consume the out-of-band token here, then run
            // the push. push_opcert no longer re-checks a (fabricated) token.
            consume_confirmation(&paths, token, "kes-push", machine, None)?;
            let store = AuditStore::open(&paths.audit_db)?;
            let report = kes::push_opcert(&spec, machine, &cert, &counter, &store)?;
            output::print_json(&ToolOutput::ok("ouro.kes.push", true).with_data(json!(report)))?;
            Ok(())
        }
        // S0017 p4-1: emit a self-contained KES cold-signing script to stdout. It embeds ONLY the
        // public KES vkey + period; the operator runs it on the air-gapped machine to issue the
        // opcert (cold.skey read in place, never moved). --kes-vkey = the PUBLIC vkey file.
        Some("cold-sign-script") => {
            let vkey_path = flag_value(args, "--kes-vkey")?;
            let kes_period: u64 = flag_value(args, "--kes-period")?
                .parse()
                .map_err(|_| OuroError::InvalidArgs("--kes-period must be a non-negative integer".to_string()))?;
            let cardano_cli = optional_flag_value(args, "--cardano-cli").unwrap_or("cardano-cli");
            let vkey = std::fs::read_to_string(vkey_path).map_err(|e| {
                OuroError::Validation(format!("cannot read --kes-vkey {vkey_path}: {e}"))
            })?;
            let generated_at = chrono::Utc::now().to_rfc3339();
            let script =
                crate::cold_sign::kes_cold_sign_script(&vkey, kes_period, cardano_cli, &generated_at)?;
            std::io::stdout().write_all(script.as_bytes())?;
            std::io::stdout().flush()?;
            // p4-8 trusted delivery: print the script digest to STDERR (out of the stdout script
            // stream) so the operator can verify the file on the cold machine matches this exactly.
            eprintln!("sha256={}", crate::skills::sha256_hex(script.as_bytes()));
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected kes generate|counter status|push|cold-sign-script".to_string(),
        )),
    }
}

fn run_status(args: &[String]) -> Result<()> {
    let snapshot_path = flag_value(args, "--snapshot")?;
    let snapshot = StatusSnapshot::from_file(&PathBuf::from(snapshot_path))?;
    let diff = if args.iter().any(|arg| arg == "--diff-spec") {
        let spec_path = flag_value(args, "--spec")?;
        let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
        Some(snapshot.diff_spec(&spec))
    } else {
        None
    };
    output::print_json(&ToolOutput::ok("ouro.status", false).with_data(json!({
        "machines": snapshot.machines,
        "diff_spec": diff
    })))?;
    Ok(())
}

fn print_help() {
    println!("ouro-ops: deterministic Cardano stake pool operations CLI");
    println!("  Agent contract: read the procedure for any operation with `ouro-ops skill show <skill>`;");
    println!("  run `<command> --help` for a command's usage.\n");
    println!("Onboarding (once per target):");
    println!("  creds     check/register one operator-named existing SSH key (no list, no copy)");
    println!("  onboard   prepare a host for S0019 adopt/op dispatch (ouro-op + ouro-diag)");
    println!("  adopt     approve one conforming running node as managed (non-disruptive)");
    println!("  init/deinit  legacy S0017 setup/removal; not an inverse for S0019 onboard");
    println!("Operate (via the agent):");
    println!("  skill     show|list — the authoritative decision trees + red lines");
    println!("  op        run --op <operation> --node <id> — S0019 managed read/write intent path");
    println!("  inbox     stage a typed, content-addressed public artifact");
    println!("  fleet     permit create — authorize one disruptive fleet step");
    println!("  diag      exec --dispatch <machine> --spec <pool-spec> -- <cmd> — unprivileged diagnosis");
    println!("  confirm   create — mint an exact intent-bound one-time approval");
    println!("  tool      run <skill>/<script> — legacy S0017 dispatch path");
    println!("  kes       cold-sign-script | counter status | generate | push");
    println!("  deploy    cold-sign-script — offline tx witnessing");
    println!("  pool      overview | register-tx");
    println!("  rollback  roll back a prior change");
    println!("  self-update  --check");
    println!("Read-only / meta:");
    println!("  status    node status from a snapshot | spec validate | detect (via tool run detect/*)");
    println!("  version | paths | contract | manifest show|verify | audit init|log");
    println!("\nOutput is single-line JSON when captured (agents/pipes/dispatch); human-readable on a TTY (force JSON: --json).");
}

/// One-line usage for `<command> --help`. Covers the agent-facing surface; None → fall through.
fn command_usage(command: &str) -> Option<&'static str> {
    Some(match command {
        "onboard" => "ouro-ops onboard --host <target> [--port 22] --bootstrap-user <account> \
                      --bootstrap-key creds://<name> --control-pubkey <operator-pub> \
                      --ouro-binary <target-arch ouro-ops> [--expected-host-key <sha256>] [--dry-run]\n  \
                      Installs the S0019 ouro-op/ouro-diag confinement; then adopt the node.",
        "creds" => "ouro-ops creds check --name <name> | ouro-ops creds register --name <name> \
                    --path <absolute-operator-named-private-key> [--dry-run]\n  \
                    Checks/registers exactly one name as a symlink; never lists, reads, copies, \
                    replaces, or chooses private keys.",
        "init" => "ouro-ops init --host <target> [--port 22] --bootstrap-user <account> \
                   --bootstrap-key creds://<name> --control-pubkey <operator-pub> \
                   --ouro-binary <target-arch ouro-ops> --spec <pool-spec> --machine <id> [--expected-host-key <sha256>]\n  \
                   Onboards a target using the operator's OWN SSH access (never generates a key). \
                   See `ouro-ops skill show onboard`.",
        "deinit" => "ouro-ops deinit --host <target> [--port 22] --bootstrap-user <account> \
                     --bootstrap-key creds://<name> [--force] [--remove-node]\n  Reverses onboarding (refuses while a node runs).",
        "tool" => "ouro-ops tool run <skill>/<script> [--dispatch <machine>] --spec <pool-spec> \
                   [--machine <id>] [--confirm-token <tok>]\n  The sole audited write path. Read the steps from `ouro-ops skill show <skill>`.",
        "confirm" => "ouro-ops confirm create --op <id> --node <id> --intent-hash <hash> | \
                      confirm adopt create --node <id> --candidate-hash <hash> --host-key <sha256>\n  \
                      Mints a one-time S0019 approval bound to the exact intent or adoption candidate.",
        "kes" => "ouro-ops kes cold-sign-script --kes-vkey <pub> --kes-period <n> | counter status --state <json> \
                  | generate | push\n  Rotations run via `tool run kes-rotation/*` — see `ouro-ops skill show kes-rotation`.",
        "deploy" => "ouro-ops deploy cold-sign-script --tx-body <path> --cold-key <role> [--cold-key <role>...] \
                     [--era conway] [--testnet-magic <n>|--mainnet]",
        "diag" => "ouro-ops diag exec --dispatch <machine> --spec <pool-spec> [--timeout <s>] -- <command>\n  \
                   Free-form UNPRIVILEGED diagnosis as ouro-diag (no sudo; cannot write node \
                   content or read secret dirs, but can use its own resources/egress). Audited; output bounded. \
                   See `ouro-ops skill show troubleshooting`.",
        "pool" => "ouro-ops pool overview --spec <pool-spec> [--snapshot <json>] | register-tx --spec <pool-spec>",
        "skill" => "ouro-ops skill list | show <skill>   (skills: deploy, detect, kes-rotation, observability, runtime, troubleshooting, upgrade, onboard)",
        "spec" => "ouro-ops spec validate --spec <pool-spec>",
        "status" => "ouro-ops status --snapshot <json> [--diff-spec --spec <pool-spec>]",
        "manifest" => "ouro-ops manifest show | verify",
        "config" => "ouro-ops config render --spec <pool-spec> --machine <id> [--out <dir>] | apply ...",
        "audit" => "ouro-ops audit init | log",
        "rollback" => "ouro-ops rollback --spec <pool-spec> ...",
        "self-update" => "ouro-ops self-update --check [--against <signed-metadata>]",
        _ => return None,
    })
}

fn run_config(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("render") => {
            let spec_path = flag_value(args, "--spec")?;
            let machine = flag_value(args, "--machine")?;
            let output_root = flag_value(args, "--out").unwrap_or(".ouro/staging/render");
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
            let rendered = render::render_machine(&spec, machine, &PathBuf::from(output_root))?;
            output::print_json(
                &ToolOutput::ok("ouro.config.render", true).with_data(json!({
                    "machine": rendered.machine,
                    "role": rendered.role,
                    "output_dir": rendered.output_dir,
                    "files": rendered.files,
                    "topology_mode": spec.topology_mode
                })),
            )?;
            Ok(())
        }
        Some("apply") => {
            let spec_path = flag_value(args, "--spec")?;
            let machine_id = flag_value(args, "--machine")?;
            let rendered_dir = flag_value(args, "--rendered-dir")?;
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
            let machine = spec
                .machines
                .iter()
                .find(|candidate| candidate.id == machine_id)
                .ok_or_else(|| OuroError::Validation(format!("unknown machine {machine_id}")))?;
            let paths = ConfigPaths::discover();
            let store = AuditStore::open(&paths.audit_db)?;
            let audit_id = store.begin_invocation("config/apply", Some(machine_id))?;
            store.finish_invocation(&audit_id, "config/apply")?;
            let prepared = SshRunner::new(true).prepare_tool_run(
                &machine.ssh,
                "config/apply",
                spec_path,
                &audit_id,
            );
            output::print_json(&ToolOutput::ok("ouro.config.apply", true).with_data(json!({
                "audit_id": audit_id,
                "machine": machine_id,
                "rendered_dir": rendered_dir,
                "transport": "ouro-tool-run",
                "prepared_command": prepared,
                "dry_run": true
            })))?;
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected config render/apply".to_string(),
        )),
    }
}

fn run_spec(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("validate") => {
            let spec_path = flag_value(args, "--spec")?;
            let spec = PoolSpec::from_file(&PathBuf::from(spec_path))?;
            output::print_json(
                &ToolOutput::ok("ouro.spec.validate", false).with_data(json!({
                    "valid": true,
                    "resolved_plan": spec.resolved_non_secret_plan()
                })),
            )?;
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected spec validate --spec <path>".to_string(),
        )),
    }
}

fn run_audit(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("init") => {
            let paths = ConfigPaths::discover();
            let store = AuditStore::open(&paths.audit_db)?;
            let invocation = store.begin_invocation("audit/init", None)?;
            output::print_json(&ToolOutput::ok("ouro.audit.init", true).with_data(json!({
                "audit_id": invocation,
                "audit_db": paths.audit_db
            })))?;
            Ok(())
        }
        Some("log") => {
            let limit = optional_flag_value(args, "--limit")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(20);
            let paths = ConfigPaths::discover();
            let store = AuditStore::open(&paths.audit_db)?;
            let events = store.list(limit)?;
            output::print_json(
                &ToolOutput::ok("ouro.audit.log", false).with_data(json!({ "events": events })),
            )?;
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs("expected audit init".to_string())),
    }
}

fn run_legacy(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("inspect") => {
            let db = flag_value(args, "--db")?;
            let report = migration::inspect_legacy_db(&PathBuf::from(db))?;
            output::print_json(
                &ToolOutput::ok("ouro.legacy.inspect", false).with_data(json!(report)),
            )?;
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected legacy inspect --db <path>".to_string(),
        )),
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Result<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| OuroError::InvalidArgs(format!("missing {flag}")))
}

fn optional_flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

/// The ONLY confirmation path for dangerous commands: a single-use `tok_` token
/// minted out-of-band by `ouro-ops confirm create` and consumed from the store. There is
/// deliberately no static/guessable fallback — a literal an agent can construct from
/// public spec fields must never satisfy the gate (§2.2#3).
fn consume_confirmation(
    paths: &ConfigPaths,
    token: Option<&str>,
    action: &str,
    machine: &str,
    evidence: Option<&str>,
) -> Result<()> {
    match token {
        Some(token) if token.starts_with("tok_") => {
            ConfirmationStore::consume(&paths.confirmations, token, action, machine, evidence)
        }
        Some(_) => Err(OuroError::Validation(format!(
            "invalid confirmation token; issue one out-of-band with `ouro-ops confirm create --action {action} --machine {machine}`"
        ))),
        None => Err(OuroError::Validation(format!(
            "dangerous {action} requires a human-issued confirmation token (ouro-ops confirm create)"
        ))),
    }
}

#[cfg(test)]
mod tests_embedded_resolution {
    use super::*;

    // S0016 p2-1: when no on-disk skill exists (installed single binary), the tool script is
    // materialized from the compiled-in copy into a per-run temp base, with the ouro-skills/
    // layout preserved so the script's `$ROOT/ouro-skills/lib/...` sourcing still resolves.
    #[test]
    fn embedded_fallback_materializes_runnable_layout() {
        // Force a disk miss (nonexistent OURO_SKILLS_DIR) so resolution takes the embedded path.
        std::env::set_var("OURO_SKILLS_DIR", "/nonexistent-ouro-skills-xyz");
        let (script, temp_base) =
            resolve_skill_script("deploy/status", "test-audit").expect("resolve embedded deploy/status");
        std::env::remove_var("OURO_SKILLS_DIR");

        let base = temp_base.expect("embedded path returns a temp base to clean");
        assert!(script.is_file(), "extracted script exists: {}", script.display());
        // The shared lib must sit where the script expects it (ROOT/ouro-skills/lib).
        assert!(base.join("ouro-skills/lib/ouro-lib.sh").is_file());
        // Extracted bytes must equal the embedded source (no drift on materialization).
        let on_disk = std::fs::read(&script).unwrap();
        let embedded = crate::skills::script("deploy", "status").unwrap();
        assert_eq!(on_disk, embedded);

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn init_records_declared_and_undeclared_runtime() {
        // p2-4: init records the declared runtime (or honestly notes it undeclared).
        let mut spec =
            PoolSpec::from_file(std::path::Path::new("examples/pool-spec.minimal.yaml")).unwrap();
        // v1 default: bp1 has no runtime block → undeclared, detection governs.
        let rec = init_runtime_record(&spec, "bp1").unwrap();
        assert_eq!(rec["declared"], json!(false));
        assert!(rec["note"].as_str().unwrap().contains("DETECTED"));
        // an unknown machine id is rejected.
        assert!(init_runtime_record(&spec, "does-not-exist").is_err());
        // when declared, the runtime is recorded verbatim (consistency already checked at load).
        spec.machines[0].runtime = Some(crate::domain::RuntimeDecl {
            mode: crate::domain::RuntimeMode::Systemd,
            unit: Some("cardano-node.service".to_string()),
            container: None,
            image: None,
        });
        let rec2 = init_runtime_record(&spec, "bp1").unwrap();
        assert_eq!(rec2["declared"], json!(true));
        assert_eq!(rec2["runtime"]["mode"], json!("systemd"));
        assert_eq!(rec2["runtime"]["unit"], json!("cardano-node.service"));
    }

    #[test]
    fn unknown_tool_rejected_even_when_absent_on_disk() {
        std::env::set_var("OURO_SKILLS_DIR", "/nonexistent-ouro-skills-xyz");
        let err = resolve_skill_script("deploy/does-not-exist", "test-audit");
        std::env::remove_var("OURO_SKILLS_DIR");
        assert!(err.is_err(), "absent-everywhere tool must error, not fabricate");
    }

    #[test]
    fn onboard_preview_never_claims_attained_state_or_host_key_check() {
        assert_eq!(
            onboard_output_semantics(true, true, false),
            (
                "preview",
                Some("host-onboarded"),
                "not_checked_in_dry_run"
            )
        );
        assert_eq!(
            onboard_output_semantics(false, true, true),
            ("host-onboarded", None, "pinned")
        );
        assert_eq!(
            onboard_output_semantics(false, false, false),
            ("onboard-failed", None, "not_pinned")
        );
    }
}
