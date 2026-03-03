use std::process::Command;

use tauri::State;

use crate::db::{
    audit_log_insert, machine_delete_cascade, machine_get, machine_insert,
    machine_list as repo_machine_list, pool_get_single, DbState, MachineRow,
};
use crate::error::AppError;
use crate::keychain::{
    ssh_agent_list_keys as keychain_ssh_agent_list_keys, verify_ssh_agent_key, SshKeyInfo,
};

#[derive(Debug, serde::Deserialize)]
pub struct MachineAddPayload {
    pub name: String,
    pub ip: String,
    pub port: i64,
    pub ssh_user: String,
    pub role: String,
    pub network: String,
    pub ssh_key_fingerprint: String,
}

#[derive(Debug, serde::Deserialize)]
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

#[tauri::command]
pub async fn machine_add(
    payload: MachineAddPayload,
    db: State<'_, DbState>,
) -> Result<Machine, AppError> {
    validate_role(payload.role.as_str())?;
    if payload.port <= 0 || payload.port > 65535 {
        return Err(AppError::Internal("invalid ssh port".into()));
    }

    if !verify_ssh_agent_key(payload.ssh_key_fingerprint.as_str())? {
        return Err(AppError::SshKeyNotFound(payload.ssh_key_fingerprint));
    }

    let pool = {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        pool_get_single(&conn)?.ok_or_else(|| AppError::Internal("pool not initialized".into()))?
    };
    if pool.network != payload.network {
        return Err(AppError::Internal(
            "machine network must match pool network".into(),
        ));
    }

    run_ssh_command(
        payload.ssh_user.as_str(),
        payload.ip.as_str(),
        payload.port,
        "echo ouro-ops-ssh-ok >/dev/null",
    )?;

    let inserted = {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        let new_id = machine_insert(
            &conn,
            pool.id,
            payload.name.as_str(),
            payload.ip.as_str(),
            payload.port,
            payload.ssh_user.as_str(),
            payload.role.as_str(),
            Some(payload.ssh_key_fingerprint.as_str()),
        )?;
        machine_get(&conn, new_id)?
            .ok_or_else(|| AppError::Internal("insert machine failed".into()))?
    };

    {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        audit_log_insert(
            &conn,
            "machine_add",
            &serde_json::json!({
                "machine_id": inserted.id,
                "name": inserted.name,
                "ip": inserted.ip,
                "role": inserted.role
            }),
        )?;
    }

    Ok(to_machine(inserted))
}

#[tauri::command]
pub async fn machine_remove(machine_id: i64, db: State<'_, DbState>) -> Result<(), AppError> {
    let deleted_machine = {
        let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
        let existing = machine_get(&conn, machine_id)?
            .ok_or_else(|| AppError::Internal(format!("machine not found: {machine_id}")))?;
        machine_delete_cascade(&conn, machine_id)?;
        audit_log_insert(
            &conn,
            "machine_remove",
            &serde_json::json!({
                "machine_id": machine_id,
                "name": existing.name,
                "ip": existing.ip
            }),
        )?;
        existing
    };
    let _ = deleted_machine;
    Ok(())
}

#[tauri::command]
pub async fn machine_list(
    filter: Option<MachineFilter>,
    db: State<'_, DbState>,
) -> Result<Vec<Machine>, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    let role = filter.as_ref().and_then(|f| f.role.as_deref());
    let network = filter.as_ref().and_then(|f| f.network.as_deref());
    let rows = repo_machine_list(&conn, role, network)?;
    Ok(rows.into_iter().map(to_machine).collect())
}

#[tauri::command]
pub async fn ssh_agent_list_keys() -> Result<Vec<SshKeyInfo>, AppError> {
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

    run_ssh_command(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "echo preflight-ok >/dev/null",
    )?;
    let os_version = run_ssh_command(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "[ -f /etc/os-release ] && . /etc/os-release && echo \"${PRETTY_NAME:-$NAME}\" || uname -srm",
    )?;
    let disk_available_gb = parse_i64(&run_ssh_command(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "df -BG / | awk 'NR==2 {gsub(/G/, \"\", $4); print $4}'",
    )?);
    let memory_total_gb = parse_i64(&run_ssh_command(
        machine.ssh_user.as_str(),
        machine.ip.as_str(),
        machine.ssh_port,
        "awk '/MemTotal/ {printf \"%d\", $2/1024/1024}' /proc/meminfo",
    )?);
    let disk_iops = parse_i64(&run_ssh_command(
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
