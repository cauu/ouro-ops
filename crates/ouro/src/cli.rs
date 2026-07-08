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
        "pool" => run_pool(&args[2..])?,
        "rollback" => run_rollback(&args[2..])?,
        "spec" => run_spec(&args[2..])?,
        "status" => run_status(&args[2..])?,
        "tool" => run_tool(&args[2..])?,
        other => return Err(OuroError::InvalidArgs(format!("unknown command {other}"))),
    }
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
        _ => Err(OuroError::InvalidArgs(
            "expected confirm create --action <a> --machine <m>".to_string(),
        )),
    }
}

fn run_tool(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("run") => run_tool_exec(args),
        Some("verify-context") => run_tool_verify_context(args),
        _ => Err(OuroError::InvalidArgs(
            "expected tool run <skill/script> --spec <path> | tool verify-context".to_string(),
        )),
    }
}

/// Resolve `<skill>/<script>` to an allowlisted L2 script path, rejecting traversal.
fn resolve_skill_script(tool_name: &str) -> Result<PathBuf> {
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
            "tool name must be <skill>/<script>: got {tool_name}"
        )));
    }
    let path = PathBuf::from(format!("ouro-skills/{skill}/scripts/{script}.sh"));
    if !path.is_file() {
        return Err(OuroError::Validation(format!(
            "no such tool script: {}",
            path.display()
        )));
    }
    Ok(path)
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
    let script = resolve_skill_script(&tool_name)?;

    let paths = ConfigPaths::discover();
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
    // Forward the child's stdout (the single-line JSON contract) to our caller.
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stdout().flush()?;

    match output.status.code() {
        Some(code) => {
            store.record_terminal(
                &audit_id,
                &tool_name,
                machine,
                "finish",
                &format!("exit_{code}"),
            )?;
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
