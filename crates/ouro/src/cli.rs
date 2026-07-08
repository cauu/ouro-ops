use serde_json::json;
use std::path::PathBuf;

use crate::{
    audit::AuditStore,
    config::ConfigPaths,
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
        "config" => run_config(&args[2..])?,
        "kes" => run_kes(&args[2..])?,
        "legacy" => run_legacy(&args[2..])?,
        "pool" => run_pool(&args[2..])?,
        "rollback" => run_rollback(&args[2..])?,
        "spec" => run_spec(&args[2..])?,
        "status" => run_status(&args[2..])?,
        other => return Err(OuroError::InvalidArgs(format!("unknown command {other}"))),
    }
    Ok(())
}

fn run_rollback(args: &[String]) -> Result<()> {
    let machine = flag_value(args, "--machine")?;
    let backup_id = flag_value(args, "--to")?;
    let token = optional_flag_value(args, "--confirm-token");
    validate_confirmation(token, "rollback", machine)?;
    let paths = ConfigPaths::discover();
    let store = AuditStore::open(&paths.audit_db)?;
    let audit_id = store.begin_invocation("rollback", Some(machine))?;
    output::print_json(&ToolOutput::ok("ouro.rollback", true).with_data(json!({
        "audit_id": audit_id,
        "machine": machine,
        "to": backup_id,
        "confirmation": "accepted",
        "execution": "planned-forward-change"
    })))?;
    Ok(())
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
        _ => Err(OuroError::InvalidArgs(
            "expected pool register-tx --spec <path>".to_string(),
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
            let store = AuditStore::open(&paths.audit_db)?;
            let report = kes::push_opcert(&spec, machine, &cert, &counter, token, &store)?;
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

fn validate_confirmation(token: Option<&str>, action: &str, machine: &str) -> Result<()> {
    let expected = format!("confirm:{action}:{machine}");
    match token {
        Some(token) if token == expected => Ok(()),
        Some(_) => Err(OuroError::Validation(
            "confirmation token action or machine mismatch".to_string(),
        )),
        None => Err(OuroError::Validation(format!(
            "dangerous {action} requires human-issued confirmation token"
        ))),
    }
}
