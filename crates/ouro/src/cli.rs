use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use crate::{
    audit::AuditStore,
    config::ConfigPaths,
    confirm::{self, ConfirmationStore},
    domain::PoolSpec,
    kes, migration,
    output::{self, ToolOutput},
    pool, render,
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
                "runtime": "standalone-rust",
                "security_identity": crate::parity::SecurityIdentity::local().wire_digest(),
            })))?;
        }
        "paths" => {
            output::print_json(
                &ToolOutput::ok("ouro.paths", false).with_data(json!(ConfigPaths::discover())),
            )?;
        }
        "contract" => run_contract(&args[2..])?,
        "audit" => run_audit(&args[2..])?,
        "confirm" => run_confirm(&args[2..])?,
        "config" => run_config(&args[2..])?,
        "creds" => run_creds(&args[2..])?,
        "deploy" => run_deploy(&args[2..])?,
        "diag" => run_diag(&args[2..])?,
        "onboard" => run_onboard(&args[2..])?,
        "adopt" => crate::s0019_cli::run_adopt(&args[2..])?,
        "op" => crate::s0019_cli::run_op(&args[2..])?,
        // Closed target-side surface used only by the S0020 ephemeral runner transport.
        "target" => crate::s0019_cli::run_target(&args[2..])?,
        "inbox" => crate::s0019_cli::run_inbox(&args[2..])?,
        "fleet" => crate::s0019_cli::run_fleet(&args[2..])?,
        "kes" => run_kes(&args[2..])?,
        "legacy" => run_legacy(&args[2..])?,
        "pool" => run_pool(&args[2..])?,
        "release" => run_release(&args[2..])?,
        "rollback" => run_rollback(&args[2..])?,
        "self-update" => run_self_update(&args[2..])?,
        "spec" => run_spec(&args[2..])?,
        "status" => run_status(&args[2..])?,
        other => return Err(OuroError::InvalidArgs(format!("unknown command {other}"))),
    }
    Ok(())
}

/// Pure external-Skill compatibility check. Argument parsing and comparison happen before any
/// ConfigPaths discovery or stateful subsystem is reached.
fn run_contract(args: &[String]) -> Result<()> {
    if args.is_empty() {
        output::print_json(
            &ToolOutput::ok("ouro.contract", false).with_data(output::contract_summary()),
        )?;
        return Ok(());
    }
    if args.first().map(String::as_str) != Some("check") {
        return contract_refusal(
            "invalid_contract_command",
            "expected `contract check --requires-ouro >=MAJOR.MINOR.PATCH --requires-contract N`",
        );
    }

    let mut requires_ouro = None;
    let mut requires_contract = None;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--requires-ouro" => &mut requires_ouro,
            "--requires-contract" => &mut requires_contract,
            other => {
                return contract_refusal(
                    "invalid_contract_arguments",
                    &format!("unknown contract check argument {other}"),
                )
            }
        };
        if slot.is_some() {
            return contract_refusal(
                "invalid_contract_arguments",
                &format!("duplicate contract check argument {flag}"),
            );
        }
        let Some(value) = args.get(index + 1) else {
            return contract_refusal(
                "invalid_contract_arguments",
                &format!("missing value for {flag}"),
            );
        };
        if value.starts_with("--") {
            return contract_refusal(
                "invalid_contract_arguments",
                &format!("missing value for {flag}"),
            );
        }
        *slot = Some(value.as_str());
        index += 2;
    }

    let Some(requires_ouro) = requires_ouro else {
        return contract_refusal("invalid_contract_arguments", "missing --requires-ouro");
    };
    let Some(requires_contract) = requires_contract else {
        return contract_refusal("invalid_contract_arguments", "missing --requires-contract");
    };
    match crate::contract::check(requires_ouro, requires_contract) {
        Ok(compatibility) => {
            output::print_json(
                &ToolOutput::ok("ouro.contract.check", false)
                    .with_data(serde_json::to_value(compatibility)?),
            )?;
            Ok(())
        }
        Err(refusal) => contract_refusal(refusal.code, &refusal.detail),
    }
}

fn contract_refusal(code: &str, detail: &str) -> Result<()> {
    let mut record = ToolOutput::failure("ouro.contract.check", code, detail);
    if let Some(error) = record.error.as_mut() {
        error.hint = "install the ouro-ops release matching the Skill requirements, then restart the workflow"
            .to_string();
    }
    output::print_json(&record)?;
    Err(OuroError::Reported(10))
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
/// Reports the running version. With `--against`, compares
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
    let mut update_available = false;
    let mut latest = current.clone();
    if let Some(path) = optional_flag_value(args, "--against") {
        let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|e| OuroError::Validation(format!("metadata {path} not JSON: {e}")))?;
        if let Some(v) = meta.get("latest_version").and_then(|v| v.as_str()) {
            latest = v.to_string();
            if let (Some(c), Some(l)) = (crate::version::parse(&current), crate::version::parse(v))
            {
                update_available = l > c; // never downgrade: only flag when strictly newer
            }
        }
    }
    output::print_json(&ToolOutput::ok("ouro.self-update.check", false).with_data(json!({
        "current": current,
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

/// S0019 p6-1 — `ouro-ops onboard`: the greenfield host-onboard (host-onboarded state). Installs
/// the S0019 confined principals + op wrapper + binary, pins the host key. No S0017 compat.
fn run_onboard(args: &[String]) -> Result<()> {
    validate_onboard_args(args)?;
    let host = flag_value(args, "--host")?.to_string();
    let port: u16 = optional_flag_value(args, "--port")
        .unwrap_or("22")
        .parse()
        .map_err(|_| OuroError::InvalidArgs("--port must be a number".to_string()))?;
    let user = flag_value(args, "--bootstrap-user")?.to_string();
    crate::onboard::validate_bootstrap_user(&user)?;
    let key_ref = CredentialRef::parse(flag_value(args, "--bootstrap-key")?)?;
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let mut host_key = match optional_flag_value(args, "--host-key") {
        Some("yes") => crate::bootstrap::HostKeyCheck::Yes,
        Some("accept-new") | None => crate::bootstrap::HostKeyCheck::AcceptNew,
        Some(value) => {
            return Err(OuroError::InvalidArgs(format!(
                "--host-key must be yes or accept-new, got {value:?}"
            )))
        }
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
                 `ssh-keygen -y -f <the operator's key>`). See the operator's onboarding procedure."
                    .to_string(),
            ))
        }
    };
    validate_control_pubkey(&control_pubkey)?;
    if !dry_run {
        validate_control_key_matches(&key_path, &control_pubkey)?;
    }
    let ouro_binary = match optional_flag_value(args, "--ouro-binary") {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe()
            .map_err(|e| OuroError::Validation(format!("cannot resolve own binary path: {e}")))?,
    };
    let expected_host_key = optional_flag_value(args, "--expected-host-key");
    if expected_host_key.is_some_and(|fingerprint| !is_ssh_sha256_fingerprint(fingerprint)) {
        return Err(OuroError::InvalidArgs(
            "--expected-host-key must be an OpenSSH SHA256:<base64> fingerprint".into(),
        ));
    }

    let target = crate::bootstrap::BootstrapTarget {
        host: host.clone(),
        port,
        user,
    };
    let mut transport = crate::bootstrap::BootstrapTransport::new(dry_run);

    let mut pinned_fp = None;
    if let Some(fp) = expected_host_key.filter(|_| !dry_run) {
        pinned_fp = Some(pin_host_key(&host, port, &paths.known_hosts, Some(fp))?);
        host_key = crate::bootstrap::HostKeyCheck::Yes;
        transport = transport.with_known_hosts(paths.known_hosts.clone());
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
    let manifest_ok = manifest.ok;
    let changed = manifest.steps.iter().any(|step| step.changed);
    let convergence = if dry_run {
        "preview"
    } else if !manifest_ok && changed {
        "failed_after_change"
    } else if !manifest_ok {
        "verification_failed"
    } else if changed {
        "applied"
    } else {
        "already_converged"
    };
    let data = json!({
        "manifest": manifest,
        "dry_run": dry_run,
        "convergence": convergence,
        "pinned_host_key": pinned_fp,
        "host_key_status": host_key_status,
        "expected_host_key_supplied": expected_host_key.is_some(),
        "state": state,
        "planned_state": planned_state,
        "ssh_access_policy": crate::onboard::ssh_access_policy(&target.user),
        "effective_ssh_policy_verified": manifest_ok && !dry_run,
        "security_note": "bootstrap credential is NOT mechanism-isolated from the agent \
            (convenience mode, P0-1, carried from S0017). Closing this is a separate hardening spec.",
    });
    if !manifest_ok {
        let mut failure = ToolOutput::failure(
            "ouro.onboard",
            "verification_failed",
            "onboard did not complete cleanly; the SSH rollback guard remains armed when needed",
        )
        .with_data(data);
        failure.changed = changed;
        output::print_json(&failure)?;
        return Err(OuroError::Reported(10));
    }
    output::print_json(&ToolOutput::ok("ouro.onboard", changed).with_data(data))?;
    Ok(())
}

/// Onboarding can rewrite SSH access, sudoers, users and the installed binary. Keep its CLI
/// grammar closed and require an explicit mode so a misspelled preview flag can never become an
/// apply. Parsing happens before credential resolution, host-key pinning or transport creation.
fn validate_onboard_args(args: &[String]) -> Result<()> {
    let value_flags = [
        "--host",
        "--port",
        "--bootstrap-user",
        "--bootstrap-key",
        "--control-pubkey",
        "--ouro-binary",
        "--expected-host-key",
        "--host-key",
    ];
    let bool_flags = ["--dry-run", "--apply"];
    let mut seen = std::collections::BTreeSet::new();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        if bool_flags.contains(&name) {
            if !seen.insert(name) {
                return Err(OuroError::InvalidArgs(format!("duplicate flag {name}")));
            }
            index += 1;
            continue;
        }
        if !value_flags.contains(&name) {
            return Err(OuroError::InvalidArgs(format!(
                "unexpected onboard argument {name:?}"
            )));
        }
        if !seen.insert(name) {
            return Err(OuroError::InvalidArgs(format!("duplicate flag {name}")));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| OuroError::InvalidArgs(format!("missing value for {name}")))?;
        if value.starts_with("--") {
            return Err(OuroError::InvalidArgs(format!("missing value for {name}")));
        }
        index += 2;
    }
    let dry_run = seen.contains("--dry-run");
    let apply = seen.contains("--apply");
    if dry_run == apply {
        return Err(OuroError::InvalidArgs(
            "onboard requires exactly one of --dry-run or --apply".into(),
        ));
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
    let host_key_status = if has_pinned_host_key {
        "pinned"
    } else {
        "not_pinned"
    };
    (state, None, host_key_status)
}

fn is_ssh_sha256_fingerprint(value: &str) -> bool {
    value.strip_prefix("SHA256:").is_some_and(|encoded| {
        encoded.len() == 43
            && encoded
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
    })
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

fn public_key_identity(value: &str) -> Option<String> {
    let mut fields = value.split_whitespace();
    Some(format!("{} {}", fields.next()?, fields.next()?))
}

/// Prove locally, before host-key pinning or any remote write, that the selected bootstrap private
/// key is exactly the private half of the public key being installed for the S0019 principals.
fn validate_control_key_matches(private_key: &std::path::Path, control_pubkey: &str) -> Result<()> {
    let derived = Command::new("ssh-keygen")
        .args(["-y", "-f"])
        .arg(private_key)
        .output()
        .map_err(|error| {
            OuroError::Validation(format!(
                "cannot derive the selected bootstrap credential's public key: {error}"
            ))
        })?;
    if !derived.status.success() {
        return Err(OuroError::Validation(
            "cannot derive the selected bootstrap credential's public key non-interactively".into(),
        ));
    }
    let derived = String::from_utf8_lossy(&derived.stdout);
    if public_key_identity(&derived) != public_key_identity(control_pubkey) {
        return Err(OuroError::Validation(
            "--control-pubkey does not match the selected --bootstrap-key credential".into(),
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
pub(crate) fn fingerprint_of(entry: &str) -> Option<String> {
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
    kg.stdin
        .take()?
        .write_all(format!("{entry}\n").as_bytes())
        .ok()?;
    let out = kg.wait_with_output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .find(|t| t.starts_with("SHA256:"))
        .map(str::to_string)
}

fn pin_host_key(
    host: &str,
    port: u16,
    known_hosts: &std::path::Path,
    expected: Option<&str>,
) -> Result<String> {
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
    let norm = |s: &str| {
        s.trim()
            .trim_start_matches("SHA256:")
            .trim_start_matches("sha256:")
            .to_string()
    };

    // Fingerprint each entry INDEPENDENTLY; when an expected fingerprint is given, KEEP ONLY the
    // entries whose own fingerprint matches it. A MITM presenting the genuine key alongside an
    // extra attacker key therefore cannot get the attacker key pinned.
    let mut kept: Vec<(&str, String)> = Vec::new();
    let mut seen_fps: Vec<String> = Vec::new();
    for e in &entries {
        let Some(fp) = fingerprint_of(e) else {
            continue;
        };
        seen_fps.push(fp.clone());
        match expected {
            Some(exp) if norm(&fp) == norm(exp) => kept.push((*e, fp)),
            Some(_) => {}                // non-matching key type — do NOT pin it
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
    let primary = kept
        .iter()
        .find_map(|(entry, fingerprint)| {
            (entry.split_whitespace().nth(1) == Some("ssh-ed25519")).then(|| fingerprint.clone())
        })
        .ok_or_else(|| {
            OuroError::Validation(format!(
                "{host}:{port} did not present the Ed25519 host key required by S0019 adoption"
            ))
        })?;

    // Idempotent write: drop any prior entry for this host, then append only the kept key(s).
    if let Some(parent) = known_hosts.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !known_hosts.exists() {
        std::fs::write(known_hosts, "")?;
    }
    let remove_target = if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    };
    let _ = Command::new("ssh-keygen")
        .args([
            "-R",
            &remove_target,
            "-f",
            &known_hosts.display().to_string(),
        ])
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
        _ => Err(OuroError::InvalidArgs(
            "expected confirm create --op <id> --node <id> --intent-hash <hash>".to_string(),
        )),
    }
}

/// S0020 — `ouro-ops diag ...`: bounded free-form diagnostics through existing operator SSH.
fn run_diag(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("exec") => run_diag_exec(&args[1..]),
        _ => Err(OuroError::InvalidArgs(
            "expected diag exec --dispatch <machine> --spec <spec> [--timeout <s>] -- <command>"
                .to_string(),
        )),
    }
}

/// `diag exec` — run ONE agent-authored command using the target's operator account from the pool
/// spec. This deliberately matches S0020's honest-agent threat model: Ouro does not claim an
/// OS-enforced read-only boundary or provision a resident diagnostic principal. Every invocation
/// is audited on control; transport is host-key pinned and output/deadline bounded. The command's
/// own exit code is DATA in the payload — a failing probe is still a delivered diagnosis (this
/// tool only errors on transport failure).
fn parse_diag_exec_args(args: &[String]) -> Result<(&str, &str, u32, &[String])> {
    let sep = args.iter().position(|arg| arg == "--").ok_or_else(|| {
        OuroError::InvalidArgs("missing `--` separator before the diagnostic command".to_string())
    })?;
    let mut dispatch = None;
    let mut spec = None;
    let mut timeout = None;
    let mut index = 0;
    while index < sep {
        let name = args[index].as_str();
        if !matches!(name, "--dispatch" | "--spec" | "--timeout") {
            return Err(OuroError::InvalidArgs(format!(
                "unexpected diagnostic control argument {name:?}"
            )));
        }
        let value = args
            .get(index + 1)
            .filter(|_| index + 1 < sep)
            .ok_or_else(|| {
                OuroError::InvalidArgs(format!("missing value for diagnostic argument {name}"))
            })?;
        if value.starts_with("--") {
            return Err(OuroError::InvalidArgs(format!(
                "missing value for diagnostic argument {name}"
            )));
        }
        let slot = match name {
            "--dispatch" => &mut dispatch,
            "--spec" => &mut spec,
            "--timeout" => &mut timeout,
            _ => unreachable!(),
        };
        if slot.replace(value.as_str()).is_some() {
            return Err(OuroError::InvalidArgs(format!(
                "duplicate diagnostic argument {name}"
            )));
        }
        index += 2;
    }
    let machine = dispatch.ok_or_else(|| OuroError::InvalidArgs("missing --dispatch".into()))?;
    let spec = spec.ok_or_else(|| OuroError::InvalidArgs("missing --spec".into()))?;
    let timeout_s = timeout
        .unwrap_or("30")
        .parse::<u32>()
        .map_err(|_| OuroError::InvalidArgs("--timeout must be seconds (max 300)".to_string()))?;
    if !(1..=300).contains(&timeout_s) {
        return Err(OuroError::InvalidArgs(
            "--timeout must be between 1 and 300 seconds".to_string(),
        ));
    }
    let command = &args[sep + 1..];
    if command.first().is_none_or(|program| program.is_empty()) {
        return Err(OuroError::InvalidArgs(
            "empty diagnostic command".to_string(),
        ));
    }
    Ok((machine, spec, timeout_s, command))
}

fn run_diag_exec(args: &[String]) -> Result<()> {
    let (machine_id, spec_path, timeout_s, command) = parse_diag_exec_args(args)?;

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
            "credential key not found for {machine_id}: {} (register the operator-named creds:// reference on this control machine)",
            key_path.display()
        )));
    }

    let store = AuditStore::open(&paths.audit_db)?;
    let audit_id = store.begin_invocation("diag/exec", Some(machine_id))?;
    let outcome = match SshRunner::new(false).diag_exec(
        &machine.ssh,
        &key_path,
        &paths.known_hosts,
        command,
        timeout_s,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            store.record_terminal(
                &audit_id,
                "diag/exec",
                Some(machine_id),
                "crash",
                Some(20),
                "diagnostic transport failed before a remote result",
            )?;
            return Err(error);
        }
    };

    // ssh exit 255 = transport-level failure (unreachable, key not authorized, host-key mismatch).
    if outcome.status == 255 {
        store.record_terminal(
            &audit_id,
            "diag/exec",
            Some(machine_id),
            "crash",
            Some(20),
            "diagnostic SSH transport returned exit 255",
        )?;
        return Err(OuroError::Validation(format!(
            "diag transport to {machine_id} failed as {}: {} — verify the existing SSH account, \
             named credential and pinned host key",
            machine.ssh.user,
            outcome.stderr.lines().last().unwrap_or("(no stderr)")
        )));
    }
    store.record_terminal(
        &audit_id,
        "diag/exec",
        Some(machine_id),
        "finish",
        Some(0),
        &format!(
            "diagnostic result delivered with remote exit {}",
            outcome.status
        ),
    )?;

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
        "principal": machine.ssh.user,
        "assurance": "operator_ssh_diagnostic",
        "read_only_enforced": false,
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
            eprintln!("sha256={}", crate::assets::sha256_hex(script.as_bytes()));
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected deploy cold-sign-script --tx-body <path> --cold-key <role> [--cold-key <role>...] [--era conway]"
                .to_string(),
        )),
    }
}

fn run_release(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("select") {
        return Err(OuroError::InvalidArgs(
            "expected release select --platform <linux/amd64|linux/arm64> [--from sha256:<digest>]"
                .into(),
        ));
    }
    let platform = flag_value(args, "--platform")?;
    if !matches!(platform, "linux/amd64" | "linux/arm64") {
        return Err(OuroError::Validation(
            "--platform must be linux/amd64 or linux/arm64".into(),
        ));
    }
    let catalog = crate::convention::fetch_release_catalog()?;
    let digest = catalog.policy.signed_digest()?;
    let from = optional_flag_value(args, "--from");
    let (selection, image, transition) = if let Some(current) = from {
        let (image, transition) = catalog.policy.recommended_upgrade_for(current, platform)?;
        ("upgrade_recommended", image, transition)
    } else {
        (
            "deploy_recommended",
            catalog.policy.recommended_for(platform)?,
            None,
        )
    };
    output::print_json(
        &ToolOutput::ok("ouro.release.select", false).with_data(json!({
            "selection": selection,
            "platform": platform,
            "source": catalog.source,
            "policy_version": catalog.policy.allowlist_version,
            "policy_digest": digest,
            "repository": catalog.policy.repository,
            "image": image,
            "transition": transition,
            "cache_written": false,
        })),
    )
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
        Some("airgap-bundle") => {
            let vkey = PathBuf::from(flag_value(args, "--kes-vkey")?);
            let kes_period: u64 = flag_value(args, "--kes-period")?.parse().map_err(|_| {
                OuroError::InvalidArgs("--kes-period must be a non-negative integer".to_string())
            })?;
            let cardano_cli_version = flag_value(args, "--cardano-cli-version")?;
            let platform = flag_value(args, "--platform")?;
            let out = match (
                optional_flag_value(args, "--spec"),
                optional_flag_value(args, "--node"),
                optional_flag_value(args, "--out"),
            ) {
                (Some(spec), Some(node), None) => {
                    crate::kes_bundle::pending_dir(&PathBuf::from(spec), node)?
                }
                (None, None, Some(out)) => PathBuf::from(out),
                _ => {
                    return Err(OuroError::InvalidArgs(
                        "use --spec <pool-spec> --node <bp>; legacy --out cannot be combined with them"
                            .into(),
                    ));
                }
            };
            let report = crate::kes_bundle::create_airgap_bundle(
                &vkey,
                kes_period,
                cardano_cli_version,
                platform,
                &out,
            )?;
            output::print_json(
                &ToolOutput::ok("ouro.kes.airgap-bundle", report.changed)
                    .with_data(json!(report)),
            )?;
            Ok(())
        }
        Some("airgap-cleanup") => {
            let spec = PathBuf::from(flag_value(args, "--spec")?);
            let node = flag_value(args, "--node")?;
            let expected = flag_value(args, "--expected-vkey-sha256")?;
            let changed = crate::kes_bundle::cleanup_pending_bundle(&spec, node, expected)?;
            let pending = crate::kes_bundle::pending_dir(&spec, node)?;
            output::print_json(
                &ToolOutput::ok("ouro.kes.airgap-cleanup", changed).with_data(json!({
                    "node": node,
                    "pending_dir": pending,
                    "expected_vkey_sha256": expected,
                    "absent": true
                })),
            )?;
            Ok(())
        }
        // S0017 p4-1: emit a self-contained KES cold-signing script to stdout. It embeds ONLY the
        // public KES vkey + period; the operator runs it on the air-gapped machine to issue the
        // opcert (cold.skey read in place, never moved). --kes-vkey = the PUBLIC vkey file.
        Some("cold-sign-script") => {
            let vkey_path = flag_value(args, "--kes-vkey")?;
            let kes_period: u64 = flag_value(args, "--kes-period")?.parse().map_err(|_| {
                OuroError::InvalidArgs("--kes-period must be a non-negative integer".to_string())
            })?;
            let cardano_cli = optional_flag_value(args, "--cardano-cli").unwrap_or("cardano-cli");
            let vkey = std::fs::read_to_string(vkey_path).map_err(|e| {
                OuroError::Validation(format!("cannot read --kes-vkey {vkey_path}: {e}"))
            })?;
            let generated_at = chrono::Utc::now().to_rfc3339();
            let script = crate::cold_sign::kes_cold_sign_script(
                &vkey,
                kes_period,
                cardano_cli,
                &generated_at,
            )?;
            std::io::stdout().write_all(script.as_bytes())?;
            std::io::stdout().flush()?;
            // p4-8 trusted delivery: print the script digest to STDERR (out of the stdout script
            // stream) so the operator can verify the file on the cold machine matches this exactly.
            eprintln!("sha256={}", crate::assets::sha256_hex(script.as_bytes()));
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected kes airgap-bundle|airgap-cleanup|cold-sign-script|counter status|generate|push"
                .to_string(),
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
    println!("  Agent contract: use the complete canonical Skill supplied by the operator's website prompt;");
    println!("  run `<command> --help` for a command's usage.\n");
    println!("Control setup:");
    println!("  creds     check/register one operator-named existing SSH key (no list, no copy)");
    println!(
        "  Ordinary targets use each pool-spec machine's existing SSH account; no target Ouro install/adoption."
    );
    println!("Operate (via the agent, S0020):");
    println!("  op        run --op <operation> --node <id> — live stateless read/plan/apply path");
    println!("  inbox     preview a typed public artifact locally (legacy stage also available)");
    println!("  fleet     permit create — authorize one disruptive fleet step");
    println!("  diag      exec --dispatch <machine> --spec <pool-spec> -- <cmd> — bounded operator-SSH diagnosis");
    println!("  confirm   create — mint an exact intent-bound one-time approval");
    println!(
        "  kes       airgap-bundle | airgap-cleanup | cold-sign-script | counter status | generate | push"
    );
    println!("  deploy    cold-sign-script — offline tx witnessing");
    println!("  release   select — current signed deploy recommendation or next Upgrade hop");
    println!("  pool      overview | register-tx");
    println!("  rollback  roll back a prior change");
    println!("  self-update  --check");
    println!("Legacy migration/recovery only:");
    println!("  onboard/adopt  S0019 resident-target model (not an ordinary-flow prerequisite)");
    println!("Read-only / meta:");
    println!("  status    node status from a snapshot | spec validate | detection occurs in live reads/plans");
    println!("  version | paths | contract | audit init|log");
    println!("\nOutput is single-line JSON when captured (agents/pipes/dispatch); human-readable on a TTY (force JSON: --json).");
}

/// One-line usage for `<command> --help`. Covers the agent-facing surface; None → fall through.
fn command_usage(command: &str) -> Option<&'static str> {
    Some(match command {
        "contract" => "ouro-ops contract check --requires-ouro '>=MAJOR.MINOR.PATCH' \
                       --requires-contract <integer>\n  \
                       Pure external-Skill compatibility preflight; performs no filesystem, \
                       credential, audit, confirmation, candidate, network, or SSH access.",
        "onboard" => "ouro-ops onboard --host <target> [--port 22] --bootstrap-user <account> \
                      --bootstrap-key creds://<name> --control-pubkey <operator-pub> \
                      --ouro-binary <target-arch ouro-ops> [--expected-host-key <SHA256:base64>] \
                      (--dry-run | --apply)\n  \
                      LEGACY S0019 migration/recovery only; ordinary S0020 operations do not use it.",
        "creds" => "ouro-ops creds check --name <name> | ouro-ops creds register --name <name> \
                    --path <absolute-operator-named-private-key> [--dry-run]\n  \
                    Checks/registers exactly one name as a symlink; never lists, reads, copies, \
                    replaces, or chooses private keys.",
        "adopt" => "ouro-ops adopt --dispatch <host> --bootstrap-user <account> \
                    --ssh-key creds://<name> --spec <pool-spec> --node <id> \
                    --role <bp|relay> --preview\n  \
                    LEGACY S0019 migration/recovery only. After exact operator approval, add \
                    --approve-token <token> without --preview.",
        "op" => "ouro-ops op run --op <operation> --spec <pool-spec> --dispatch <host> \
                 [--ssh-key creds://<name>] --node <id> --param machine=<id> [--param k=v] \
                 [--candidate-hash <approved-hash> --artifact-file <path>] \
                 [--fleet-permit <json>] [--confirm-token <token>] \
                 [--plan|--artifact-preflight|--transport-plan]\n  \
                 --plan returns a final target-validated intent and rejects permit/confirm \
                 capabilities. KES --artifact-preflight sends the exact public opcert for deep \
                 target validation without a capability or executor. Apply repeats the same \
                 command without an inspection flag, adds the approved candidate hash and token, \
                 and automatically transports the ephemeral runner. \
                 Dangerous disruptive ops mint a short-lived 180-second fleet permit last.",
        "inbox" => "ouro-ops inbox preview --type <opcert|tx> --file <path>\n  \
                    Hashes and validates the operator-named public file without copying it. Pass \
                    data.artifact_ref to --plan; apply sends the same file once with --artifact-file.",
        "fleet" => "ouro-ops fleet spec identity --spec <pool-spec> | \
                    fleet permit create --spec <pool-spec> --node <id> --op <id> \
                    --intent-hash <final-plan-hash> --holder <id> \
                    [--target-image sha256:<digest>] [--artifact-file <public-node.cert>]\n  \
                    Identity exposes stable pool id + exact spec revision. Permit derives signed \
                    target/network/host/quorum/BP-last facts and upgrade.min_online_relays directly \
                    from the spec, then expires after 180 seconds. KES activation additionally \
                    binds healthy-relay protocol evidence for the reviewed public certificate.",
        "confirm" => "ouro-ops confirm create --op <id> --node <id> --intent-hash <hash> | \
                      confirm adopt create --node <id> --candidate-hash <hash> --host-key <sha256>\n  \
                      Mints a one-time approval bound to the exact S0020 candidate/intent; the adopt \
                      form is legacy migration only.",
        "kes" => "ouro-ops kes airgap-bundle --kes-vkey <pub> --kes-period <n> \
                  --cardano-cli-version <x.y.z.w> \
                  --platform <mac-apple-silicon|mac-intel|linux-intel-amd|linux-arm> \
                  --spec <pool-spec> --node <bp>\n  \
                  Downloads and verifies the matching official Intersect cardano-cli release, then \
                  creates or validates the deterministic pending public bundle. Cleanup after typed \
                  discard/success: ouro-ops kes airgap-cleanup --spec <pool-spec> --node <bp> \
                  --expected-vkey-sha256 <sha256>. Rotation runs via \
                  `op run kes-rotation/stage-key`, this handoff, then \
                  `op run kes-rotation/install-opcert`; see the Skill. Legacy script-only output: \
                  ouro-ops kes cold-sign-script --kes-vkey <pub> --kes-period <n>.",
        "deploy" => "ouro-ops deploy cold-sign-script --tx-body <path> --cold-key <role> [--cold-key <role>...] \
                     [--era conway] [--testnet-magic <n>|--mainnet]",
        "release" => "ouro-ops release select --platform <linux/amd64|linux/arm64> \
                       [--from sha256:<current-image-config-digest>]\n  \
                       Fetches and verifies the current signed release catalog without caching. \
                       Without --from returns the deploy recommendation; with --from returns the \
                       unique next signed Upgrade hop.",
        "diag" => "ouro-ops diag exec --dispatch <machine> --spec <pool-spec> [--timeout <s>] -- <command>\n  \
                   Free-form diagnosis through the spec's existing operator SSH account. Ouro \
                   adds no sudo but does not enforce read-only access. Host-key pinned, audited, \
                   deadline/output bounded. See the canonical Troubleshooting Skill supplied by the operator.",
        "pool" => "ouro-ops pool overview --spec <pool-spec> [--snapshot <json>] | register-tx --spec <pool-spec>",
        "spec" => "ouro-ops spec validate --spec <pool-spec>",
        "status" => "ouro-ops status --snapshot <json> [--diff-spec --spec <pool-spec>]",
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
        _ => Err(OuroError::InvalidArgs(
            "expected config render --spec <pool-spec> --machine <id> --out <dir>".to_string(),
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
mod tests {
    use super::*;

    #[test]
    fn onboard_preview_never_claims_attained_state_or_host_key_check() {
        assert_eq!(
            onboard_output_semantics(true, true, false),
            ("preview", Some("host-onboarded"), "not_checked_in_dry_run")
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

    #[test]
    fn public_key_identity_ignores_comments_but_not_key_material() {
        let derived = "ssh-ed25519 AAAA0123456789abcdef\n";
        let same = "ssh-ed25519 AAAA0123456789abcdef operator@control";
        let other = "ssh-ed25519 BBBB0123456789abcdef operator@control";
        assert_eq!(public_key_identity(derived), public_key_identity(same));
        assert_ne!(public_key_identity(derived), public_key_identity(other));
    }

    #[test]
    fn selected_private_key_must_match_the_installed_control_key() {
        let dir = std::env::temp_dir().join(format!(
            "ouro-onboard-key-match-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let key_a = dir.join("a");
        let key_b = dir.join("b");
        for key in [&key_a, &key_b] {
            assert!(Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(key)
                .status()
                .unwrap()
                .success());
        }
        let pub_a = std::fs::read_to_string(key_a.with_extension("pub")).unwrap();
        let pub_b = std::fs::read_to_string(key_b.with_extension("pub")).unwrap();
        assert!(validate_control_key_matches(&key_a, &pub_a).is_ok());
        assert!(validate_control_key_matches(&key_a, &pub_b).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
