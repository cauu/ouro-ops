use std::process::Command;

use tauri::State;

use crate::db::{
    audit_log_insert, machine_delete_cascade, machine_get, machine_insert,
    machine_list as repo_machine_list, pool_get_single, DbState, MachineRow,
};
use crate::error::AppError;
use crate::keychain::{
    prompt_add_key, ssh_agent_list_keys as keychain_ssh_agent_list_keys, verify_ssh_agent_key,
    SshKeyInfo,
};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MachineAddPayload {
    pub name: String,
    pub ip: String,
    pub port: i64,
    pub ssh_user: String,
    pub role: String,
    pub network: String,
    pub ssh_key_fingerprint: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MachineFilter {
    pub role: Option<String>,
    pub network: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct Machine {
    pub id: i64,
    pub pool_id: i64,
    pub name: String,
    pub ip: String,
    pub port: i64,
    pub ssh_user: String,
    pub role: String,
    pub network: String,
    pub ssh_key_fingerprint: Option<String>,
    pub os_version: Option<String>,
    pub cardano_version: Option<String>,
    pub image_registry: String,
    pub image_digest: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, serde::Serialize)]
pub struct PreflightReport {
    pub ssh_ok: bool,
    pub os_version: String,
    pub disk_available_gb: i64,
    pub memory_total_gb: i64,
    pub disk_iops: i64,
    pub warnings: Vec<String>,
}

fn validate_role(role: &str) -> Result<(), AppError> {
    if matches!(role, "relay" | "bp" | "archive") {
        Ok(())
    } else {
        Err(AppError::Internal(format!("invalid role: {role}")))
    }
}

fn classify_ssh_error(target: &str, stderr: &str) -> AppError {
    if stderr.contains("Permission denied") || stderr.contains("Authentication failed") {
        return AppError::SshAuthFailed(target.to_string());
    }
    if stderr.contains("Connection timed out")
        || stderr.contains("Operation timed out")
        || stderr.contains("No route to host")
        || stderr.contains("Connection refused")
        || stderr.contains("Could not resolve hostname")
    {
        return AppError::SshTimeout(target.to_string());
    }
    AppError::Internal(format!("ssh command failed: {stderr}"))
}

fn run_ssh_command(
    ssh_user: &str,
    ip: &str,
    port: i64,
    remote_cmd: &str,
) -> Result<String, AppError> {
    let target = format!("{ssh_user}@{ip}");
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=8",
            "-p",
        ])
        .arg(port.to_string())
        .arg(target.as_str())
        .arg(remote_cmd)
        .output()?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(classify_ssh_error(
        format!("{ssh_user}@{ip}:{port}").as_str(),
        stderr.as_str(),
    ))
}

fn parse_i64(value: &str) -> i64 {
    value.trim().parse::<i64>().unwrap_or_default()
}

fn to_machine(row: MachineRow) -> Machine {
    Machine {
        id: row.id,
        pool_id: row.pool_id,
        name: row.name,
        ip: row.ip,
        port: row.ssh_port,
        ssh_user: row.ssh_user,
        role: row.role,
        network: row.network,
        ssh_key_fingerprint: row.ssh_key_fingerprint,
        os_version: row.os_version,
        cardano_version: row.cardano_version,
        image_registry: row.image_registry,
        image_digest: row.image_digest,
        sort_order: row.sort_order,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

type VerifyKeyFn = dyn Fn(&str) -> Result<bool, AppError>;
type SshExecFn = dyn Fn(&str, &str, i64, &str) -> Result<String, AppError>;

fn machine_add_with_deps(
    conn: &rusqlite::Connection,
    payload: MachineAddPayload,
    verify_key: &VerifyKeyFn,
    ssh_exec: &SshExecFn,
) -> Result<Machine, AppError> {
    validate_role(payload.role.as_str())?;
    if payload.port <= 0 || payload.port > 65535 {
        return Err(AppError::Internal("invalid ssh port".into()));
    }

    if !verify_key(payload.ssh_key_fingerprint.as_str())? {
        return Err(AppError::SshKeyNotFound(payload.ssh_key_fingerprint));
    }

    let pool =
        pool_get_single(conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?;
    if pool.network != payload.network {
        return Err(AppError::Internal(
            "machine network must match pool network".into(),
        ));
    }

    ssh_exec(
        payload.ssh_user.as_str(),
        payload.ip.as_str(),
        payload.port,
        "echo ouro-ops-ssh-ok >/dev/null",
    )?;

    let new_id = machine_insert(
        conn,
        pool.id,
        payload.name.as_str(),
        payload.ip.as_str(),
        payload.port,
        payload.ssh_user.as_str(),
        payload.role.as_str(),
        Some(payload.ssh_key_fingerprint.as_str()),
    )?;
    let inserted = machine_get(conn, new_id)?
        .ok_or_else(|| AppError::Internal("insert machine failed".into()))?;

    audit_log_insert(
        conn,
        "machine_add",
        &serde_json::json!({
            "machine_id": inserted.id,
            "name": inserted.name,
            "ip": inserted.ip,
            "role": inserted.role
        }),
    )?;

    Ok(to_machine(inserted))
}

fn machine_remove_with_conn(conn: &rusqlite::Connection, machine_id: i64) -> Result<(), AppError> {
    let existing = machine_get(conn, machine_id)?
        .ok_or_else(|| AppError::Internal(format!("machine not found: {machine_id}")))?;
    machine_delete_cascade(conn, machine_id)?;
    audit_log_insert(
        conn,
        "machine_remove",
        &serde_json::json!({
            "machine_id": machine_id,
            "name": existing.name,
            "ip": existing.ip
        }),
    )?;
    Ok(())
}

fn machine_list_with_conn(
    conn: &rusqlite::Connection,
    filter: Option<MachineFilter>,
) -> Result<Vec<Machine>, AppError> {
    let role = filter.as_ref().and_then(|f| f.role.as_deref());
    let network = filter.as_ref().and_then(|f| f.network.as_deref());
    let rows = repo_machine_list(conn, role, network)?;
    Ok(rows.into_iter().map(to_machine).collect())
}

fn machine_preflight_with_ssh(
    machine: &MachineRow,
    ssh_exec: &SshExecFn,
) -> Result<PreflightReport, AppError> {
    ssh_exec(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "echo preflight-ok >/dev/null",
    )?;
    let os_version = ssh_exec(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "[ -f /etc/os-release ] && . /etc/os-release && echo \"${PRETTY_NAME:-$NAME}\" || uname -srm",
    )?;
    let disk_available_gb = parse_i64(&ssh_exec(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "df -BG / | awk 'NR==2 {gsub(/G/, \"\", $4); print $4}'",
    )?);
    let memory_total_gb = parse_i64(&ssh_exec(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "awk '/MemTotal/ {printf \"%d\", $2/1024/1024}' /proc/meminfo",
    )?);
    let disk_iops = parse_i64(&ssh_exec(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "if command -v iostat >/dev/null 2>&1; then iostat -d 1 2 | awk 'END {print int($3+$4)}'; else echo 0; fi",
    )?);

    let mut warnings = Vec::new();
    if disk_available_gb < 300 {
        warnings.push(format!("磁盘空间不足 300GB（当前 {disk_available_gb}GB）"));
    }
    if memory_total_gb < 16 {
        warnings.push(format!("内存低于建议值 16GB（当前 {memory_total_gb}GB）"));
    }
    let os_lower = os_version.to_lowercase();
    if !(os_lower.contains("ubuntu") || os_lower.contains("debian")) {
        warnings.push(format!("OS 可能不受支持: {os_version}"));
    }

    Ok(PreflightReport {
        ssh_ok: true,
        os_version,
        disk_available_gb,
        memory_total_gb,
        disk_iops,
        warnings,
    })
}

#[tauri::command]
pub async fn machine_add(
    payload: MachineAddPayload,
    db: State<'_, DbState>,
) -> Result<Machine, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    machine_add_with_deps(&conn, payload, &verify_ssh_agent_key, &run_ssh_command)
}

#[tauri::command]
pub async fn machine_remove(machine_id: i64, db: State<'_, DbState>) -> Result<(), AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    machine_remove_with_conn(&conn, machine_id)
}

#[tauri::command]
pub async fn machine_list(
    filter: Option<MachineFilter>,
    db: State<'_, DbState>,
) -> Result<Vec<Machine>, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    machine_list_with_conn(&conn, filter)
}

#[tauri::command]
pub async fn ssh_agent_list_keys() -> Result<Vec<SshKeyInfo>, AppError> {
    keychain_ssh_agent_list_keys()
}

#[tauri::command]
pub async fn ssh_agent_add_key(key_path: String) -> Result<Vec<SshKeyInfo>, AppError> {
    prompt_add_key(key_path.as_str())?;
    keychain_ssh_agent_list_keys()
}

#[tauri::command]
pub async fn machine_preflight(
    machine_id: i64,
    db: State<'_, DbState>,
) -> Result<PreflightReport, AppError> {
    let machine = {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        machine_get(&conn, machine_id)?
            .ok_or_else(|| AppError::Internal(format!("machine not found: {machine_id}")))?
    };

    machine_preflight_with_ssh(&machine, &run_ssh_command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{pool_insert, run_migrations};
    use rusqlite::Connection;

    fn new_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");
        conn
    }

    fn create_single_pool(conn: &Connection, network: &str) -> i64 {
        pool_insert(conn, "OURO", network, Some(0.02), Some(340000000)).expect("insert pool")
    }

    #[test]
    fn parse_number_default_zero() {
        assert_eq!(parse_i64("12"), 12);
        assert_eq!(parse_i64("xx"), 0);
    }

    #[test]
    fn validate_role_values() {
        assert!(validate_role("relay").is_ok());
        assert!(validate_role("bp").is_ok());
        assert!(validate_role("archive").is_ok());
        assert!(validate_role("other").is_err());
    }

    #[test]
    fn tc_mch_001_add_success_and_audit() {
        let conn = new_db();
        let _ = create_single_pool(&conn, "preprod");
        let machine = machine_add_with_deps(
            &conn,
            MachineAddPayload {
                name: "relay-1".into(),
                ip: "203.0.113.10".into(),
                port: 22,
                ssh_user: "root".into(),
                role: "relay".into(),
                network: "preprod".into(),
                ssh_key_fingerprint: "SHA256:ok".into(),
            },
            &|_| Ok(true),
            &|_, _, _, _| Ok("ok".into()),
        )
        .expect("machine add");
        assert_eq!(machine.name, "relay-1");
        let count_machine: i64 = conn
            .query_row("SELECT COUNT(*) FROM machine", [], |r| r.get(0))
            .expect("count machine");
        let count_audit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'machine_add'",
                [],
                |r| r.get(0),
            )
            .expect("count audit");
        assert_eq!(count_machine, 1);
        assert_eq!(count_audit, 1);
    }

    #[test]
    fn tc_mch_002_add_fails_without_pool() {
        let conn = new_db();
        let err = machine_add_with_deps(
            &conn,
            MachineAddPayload {
                name: "relay-1".into(),
                ip: "203.0.113.10".into(),
                port: 22,
                ssh_user: "root".into(),
                role: "relay".into(),
                network: "preprod".into(),
                ssh_key_fingerprint: "SHA256:ok".into(),
            },
            &|_| Ok(true),
            &|_, _, _, _| Ok("ok".into()),
        )
        .expect_err("should fail without pool");
        assert!(format!("{err}").contains("pool not initialized"));
    }

    #[test]
    fn tc_mch_003_duplicate_ip_rejected() {
        let conn = new_db();
        let _ = create_single_pool(&conn, "preprod");
        let payload = MachineAddPayload {
            name: "relay-1".into(),
            ip: "203.0.113.10".into(),
            port: 22,
            ssh_user: "root".into(),
            role: "relay".into(),
            network: "preprod".into(),
            ssh_key_fingerprint: "SHA256:ok".into(),
        };
        let _ = machine_add_with_deps(&conn, payload.clone(), &|_| Ok(true), &|_, _, _, _| {
            Ok("ok".into())
        })
        .expect("first add");
        let err =
            machine_add_with_deps(&conn, payload, &|_| Ok(true), &|_, _, _, _| Ok("ok".into()))
                .expect_err("duplicate should fail");
        assert!(matches!(err, AppError::Database(_)));
    }

    #[test]
    fn tc_mch_004_key_not_found() {
        let conn = new_db();
        let _ = create_single_pool(&conn, "preprod");
        let err = machine_add_with_deps(
            &conn,
            MachineAddPayload {
                name: "relay-1".into(),
                ip: "203.0.113.10".into(),
                port: 22,
                ssh_user: "root".into(),
                role: "relay".into(),
                network: "preprod".into(),
                ssh_key_fingerprint: "SHA256:missing".into(),
            },
            &|_| Ok(false),
            &|_, _, _, _| Ok("ok".into()),
        )
        .expect_err("missing key should fail");
        assert!(matches!(err, AppError::SshKeyNotFound(_)));
    }

    #[test]
    fn tc_mch_005_remove_success() {
        let conn = new_db();
        let _ = create_single_pool(&conn, "preprod");
        let machine = machine_add_with_deps(
            &conn,
            MachineAddPayload {
                name: "relay-1".into(),
                ip: "203.0.113.10".into(),
                port: 22,
                ssh_user: "root".into(),
                role: "relay".into(),
                network: "preprod".into(),
                ssh_key_fingerprint: "SHA256:ok".into(),
            },
            &|_| Ok(true),
            &|_, _, _, _| Ok("ok".into()),
        )
        .expect("machine add");
        let task_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO task (id, task_type, status) VALUES (?1, 'deploy', 'running')",
            rusqlite::params![task_id],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status) VALUES (?1, ?2, 'running')",
            rusqlite::params![task_id, machine.id],
        )
        .expect("insert task_machine");
        conn.execute(
            "INSERT INTO kes_state (machine_id) VALUES (?1)",
            rusqlite::params![machine.id],
        )
        .expect("insert kes");
        conn.execute(
            "INSERT INTO machine_health (machine_id) VALUES (?1)",
            rusqlite::params![machine.id],
        )
        .expect("insert health");

        machine_remove_with_conn(&conn, machine.id).expect("remove machine");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM machine", [], |r| r.get(0))
            .expect("count machine");
        let count_tm: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_machine", [], |r| r.get(0))
            .expect("count task_machine");
        assert_eq!(count, 0);
        assert_eq!(count_tm, 0);
    }

    #[test]
    fn tc_mch_006_list_all() {
        let conn = new_db();
        let _ = create_single_pool(&conn, "preprod");
        let _ = machine_add_with_deps(
            &conn,
            MachineAddPayload {
                name: "relay-1".into(),
                ip: "10.0.0.1".into(),
                port: 22,
                ssh_user: "root".into(),
                role: "relay".into(),
                network: "preprod".into(),
                ssh_key_fingerprint: "SHA256:ok-1".into(),
            },
            &|_| Ok(true),
            &|_, _, _, _| Ok("ok".into()),
        )
        .expect("add relay");
        let _ = machine_add_with_deps(
            &conn,
            MachineAddPayload {
                name: "bp-1".into(),
                ip: "10.0.0.2".into(),
                port: 22,
                ssh_user: "root".into(),
                role: "bp".into(),
                network: "preprod".into(),
                ssh_key_fingerprint: "SHA256:ok-2".into(),
            },
            &|_| Ok(true),
            &|_, _, _, _| Ok("ok".into()),
        )
        .expect("add bp");
        let all = machine_list_with_conn(&conn, None).expect("list all");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn tc_mch_007_list_by_filters() {
        let conn = new_db();
        let pool_preprod = pool_insert(&conn, "OURO", "preprod", Some(0.02), Some(340000000))
            .expect("insert preprod pool");
        let pool_mainnet = pool_insert(&conn, "OUR2", "mainnet", Some(0.02), Some(340000000))
            .expect("insert mainnet pool");
        let _ = machine_insert(
            &conn,
            pool_preprod,
            "relay-1",
            "10.0.0.1",
            22,
            "root",
            "relay",
            Some("SHA256:r1"),
        )
        .expect("insert relay");
        let _ = machine_insert(
            &conn,
            pool_mainnet,
            "bp-1",
            "10.0.0.2",
            22,
            "root",
            "bp",
            Some("SHA256:b1"),
        )
        .expect("insert bp");
        let only_relay = machine_list_with_conn(
            &conn,
            Some(MachineFilter {
                role: Some("relay".into()),
                network: None,
            }),
        )
        .expect("filter by role");
        assert_eq!(only_relay.len(), 1);
        let only_mainnet = machine_list_with_conn(
            &conn,
            Some(MachineFilter {
                role: None,
                network: Some("mainnet".into()),
            }),
        )
        .expect("filter by network");
        assert_eq!(only_mainnet.len(), 1);
        assert_eq!(only_mainnet[0].network, "mainnet");
    }

    #[test]
    fn tc_mch_008_preflight_success() {
        let conn = new_db();
        let pool_id = create_single_pool(&conn, "preprod");
        let machine_id = machine_insert(
            &conn,
            pool_id,
            "relay-1",
            "10.0.0.1",
            22,
            "root",
            "relay",
            Some("SHA256:r1"),
        )
        .expect("insert machine");
        let machine = machine_get(&conn, machine_id)
            .expect("get machine")
            .expect("machine exists");
        let report = machine_preflight_with_ssh(&machine, &|_, _, _, cmd| {
            if cmd.contains("os-release") {
                return Ok("Ubuntu 24.04".into());
            }
            if cmd.contains("df -BG") {
                return Ok("280".into());
            }
            if cmd.contains("/proc/meminfo") {
                return Ok("32".into());
            }
            if cmd.contains("iostat") {
                return Ok("1200".into());
            }
            Ok("ok".into())
        })
        .expect("preflight");
        assert!(report.ssh_ok);
        assert_eq!(report.disk_available_gb, 280);
        assert_eq!(report.memory_total_gb, 32);
        assert_eq!(report.disk_iops, 1200);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn tc_mch_009_preflight_ssh_unreachable() {
        let conn = new_db();
        let pool_id = create_single_pool(&conn, "preprod");
        let machine_id = machine_insert(
            &conn,
            pool_id,
            "relay-1",
            "10.0.0.1",
            22,
            "root",
            "relay",
            Some("SHA256:r1"),
        )
        .expect("insert machine");
        let machine = machine_get(&conn, machine_id)
            .expect("get machine")
            .expect("machine exists");
        let err = machine_preflight_with_ssh(&machine, &|_, _, _, _| {
            Err(AppError::SshTimeout("root@10.0.0.1:22".into()))
        })
        .expect_err("preflight should fail");
        assert!(matches!(err, AppError::SshTimeout(_)));
    }

    #[test]
    fn tc_sec_001_no_private_key_exposure() {
        let sample = SshKeyInfo {
            bits: Some(3072),
            fingerprint: "SHA256:abc".into(),
            comment: "user@host".into(),
            key_type: "RSA".into(),
        };
        let json = serde_json::to_string(&sample).expect("serialize key info");
        assert!(json.contains("fingerprint"));
        assert!(!json.to_lowercase().contains("private"));
        assert!(!json.to_lowercase().contains("skey"));
    }
}
