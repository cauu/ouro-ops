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
    ssh::SshRunner,
    status::StatusSnapshot,
    OuroError, Result,
};

pub fn run(args: Vec<String>) -> Result<()> {
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    match command {
        "help" | "--help" | "-h" => print_help(),
        "--version" | "version" => {
            output::print_json(&ToolOutput::ok("ouro.version", false).with_data(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "binary": "ouro",
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
        "confirm" => run_confirm(&args[2..])?,
        "config" => run_config(&args[2..])?,
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

/// `ouro self-update --check [--against <signed-metadata.json>]` (S0016 p2-3).
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
    consume_confirmation(&paths, token, "rollback", machine)?;
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

/// `ouro skill show <name>` / `ouro skill list` (S0016 p2-7).
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

/// `ouro manifest show | verify --against <file>` (S0016 p2-6).
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
    match args.first().map(String::as_str) {
        Some("create") => {
            let action = flag_value(args, "--action")?;
            let machine = flag_value(args, "--machine")?;
            let ttl = confirm::parse_ttl(flag_value(args, "--ttl").unwrap_or("60s"))?;
            let paths = ConfigPaths::discover();
            let token = ConfirmationStore::create(&paths.confirmations, action, machine, ttl)?;
            output::print_json(
                &ToolOutput::ok("ouro.confirm.create", true).with_data(json!({
                    "token": token.token,
                    "action": token.action,
                    "machine": token.machine,
                    "expires_at": token.expires_at,
                    "single_use": true
                })),
            )?;
            Ok(())
        }
        // `ouro confirm preview --tool <t> --dispatch <m> --spec <f>` (S0016 p4-2).
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
fn resolve_skill_script(tool_name: &str) -> Result<(PathBuf, Option<PathBuf>)> {
    validate_tool_name(tool_name)?;
    let (skill, script) = tool_name.split_once('/').expect("validated tool name");
    let disk = skills_root().join(format!("{skill}/scripts/{script}.sh"));
    if disk.is_file() {
        return Ok((disk, None));
    }
    // Installed-binary path: extract the embedded skill's shell assets to a fresh temp base.
    if crate::skills::script(skill, script).is_none() {
        return Err(OuroError::Validation(format!(
            "no such tool script: {}/scripts/{}.sh (neither on disk nor embedded)",
            skill, script
        )));
    }
    let base = std::env::temp_dir().join(format!("ouro-run-{}", uuid::Uuid::new_v4()));
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
/// and credential from the spec, then run `sudo ouro tool run <tool> --machine <m>` on
/// the target over SSH. Audit + token are minted/verified ON THE TARGET (§2.1 D2), so
/// control mints nothing and passes no `--audit-id`; it relays the target's JSON + exit
/// code. `--remote-spec` overrides the target-side spec path (default: same as --spec).
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
    let outcome = SshRunner::new(false).execute(
        &machine.ssh,
        &key_path,
        &tool_name,
        machine_id,
        remote_spec,
    )?;
    std::io::stdout().write_all(outcome.stdout.as_bytes())?;
    std::io::stdout().flush()?;
    if !outcome.stderr.is_empty() {
        std::io::stderr().write_all(outcome.stderr.as_bytes())?;
    }
    std::process::exit(outcome.status);
}

/// `ouro tool run` — the sole audited write entrypoint. It creates (or reuses via
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
    let (script, temp_base) = resolve_skill_script(&tool_name)?;

    let paths = ConfigPaths::discover();
    // Version gate BEFORE any execution (p3-2/p3-3): required = max(prompt, embedded,
    // monotonic anti-rollback, security floor). Fails closed if the binary is below it.
    let gate = match crate::version::gate(&paths.home, min_ouro) {
        Ok(g) => g,
        Err(e) => {
            if let Some(base) = &temp_base {
                let _ = std::fs::remove_dir_all(base);
            }
            return Err(e);
        }
    };
    let store = AuditStore::open(&paths.audit_db)?;
    // Reuse a caller-supplied audit id (e.g. an orchestrator's) when it refers to a
    // real invocation; otherwise begin a fresh one.
    let audit_id = match provided_audit {
        Some(id) if store.invocation_has_start(id)? => id.to_string(),
        _ => store.begin_invocation(&tool_name, machine)?,
    };
    let secret = confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let token = confirm::invocation_token(&secret, &audit_id);
    let self_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "ouro".to_string());

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
    // Self-clean the per-run extraction (p2-1): operations leave no accumulated scripts.
    // Done before the exit()s below (which skip destructors). Disk/dev runs have no temp.
    if let Some(base) = &temp_base {
        let _ = std::fs::remove_dir_all(base);
    }
    // Forward the child's stdout (the single-line JSON contract) to our caller.
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stdout().flush()?;

    match output.status.code() {
        Some(code) => {
            // p3-4: record the ACTUAL executing ouro version (and a monotonic-floor reset,
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

/// Verify that an L2 script is running inside a genuine `ouro tool run` context: the
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
            "invalid or forged invocation token; run via ouro tool run".to_string(),
        ))
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
            consume_confirmation(&paths, token, "kes-push", machine)?;
            let store = AuditStore::open(&paths.audit_db)?;
            let report = kes::push_opcert(&spec, machine, &cert, &counter, &store)?;
            output::print_json(&ToolOutput::ok("ouro.kes.push", true).with_data(json!(report)))?;
            Ok(())
        }
        _ => Err(OuroError::InvalidArgs(
            "expected kes generate|counter status|push".to_string(),
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
    println!("ouro: deterministic Cardano stake pool operations CLI");
    println!("commands: version, paths, contract, spec validate --spec <path>, config render/apply, audit init, legacy inspect --db <path>");
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
/// minted out-of-band by `ouro confirm create` and consumed from the store. There is
/// deliberately no static/guessable fallback — a literal an agent can construct from
/// public spec fields must never satisfy the gate (§2.2#3).
fn consume_confirmation(
    paths: &ConfigPaths,
    token: Option<&str>,
    action: &str,
    machine: &str,
) -> Result<()> {
    match token {
        Some(token) if token.starts_with("tok_") => {
            ConfirmationStore::consume(&paths.confirmations, token, action, machine)
        }
        Some(_) => Err(OuroError::Validation(format!(
            "invalid confirmation token; issue one out-of-band with `ouro confirm create --action {action} --machine {machine}`"
        ))),
        None => Err(OuroError::Validation(format!(
            "dangerous {action} requires a human-issued confirmation token (ouro confirm create)"
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
            resolve_skill_script("deploy/status").expect("resolve embedded deploy/status");
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
    fn unknown_tool_rejected_even_when_absent_on_disk() {
        std::env::set_var("OURO_SKILLS_DIR", "/nonexistent-ouro-skills-xyz");
        let err = resolve_skill_script("deploy/does-not-exist");
        std::env::remove_var("OURO_SKILLS_DIR");
        assert!(err.is_err(), "absent-everywhere tool must error, not fabricate");
    }
}
