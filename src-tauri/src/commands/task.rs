use rusqlite::{params, Connection};
use tauri::State;

use crate::db::DbState;
use crate::error::AppError;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecentTaskSummary {
    pub task_id: String,
    pub task_type: String,
    pub status: String,
    pub phase: Option<String>,
    pub error_msg: Option<String>,
    pub machine_count: i64,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct TaskLogQueryPayload {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub keyword: Option<String>,
    pub task_type: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskLogPage {
    pub items: Vec<RecentTaskSummary>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub total_pages: i64,
}

fn extract_phase(payload: Option<&str>) -> Option<String> {
    payload
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|value| {
            value
                .get("phase")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
}

fn recent_task_list_with_conn(
    conn: &Connection,
    limit: i64,
) -> Result<Vec<RecentTaskSummary>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT
            t.id,
            t.task_type,
            t.status,
            t.payload,
            t.error_msg,
            COUNT(tm.machine_id) AS machine_count,
            t.created_at,
            t.started_at,
            t.finished_at
         FROM task t
         LEFT JOIN task_machine tm ON tm.task_id = t.id
         GROUP BY t.id
         ORDER BY COALESCE(t.started_at, t.created_at) DESC, t.created_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| {
        let payload: Option<String> = row.get(3)?;
        let phase = extract_phase(payload.as_deref());
        Ok(RecentTaskSummary {
            task_id: row.get(0)?,
            task_type: row.get(1)?,
            status: row.get(2)?,
            phase,
            error_msg: row.get(4)?,
            machine_count: row.get(5)?,
            created_at: row.get(6)?,
            started_at: row.get(7)?,
            finished_at: row.get(8)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

fn task_log_page_with_conn(
    conn: &Connection,
    query: &TaskLogQueryPayload,
) -> Result<TaskLogPage, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;
    let keyword = query
        .keyword
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| format!("%{value}%"));
    let task_type = query
        .task_type
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let status = query
        .status
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let total: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM task t
         WHERE (?1 IS NULL OR t.task_type = ?1)
           AND (?2 IS NULL OR t.status = ?2)
           AND (
             ?3 IS NULL
             OR t.id LIKE ?3
             OR t.task_type LIKE ?3
             OR t.status LIKE ?3
             OR IFNULL(t.payload, '') LIKE ?3
             OR IFNULL(t.error_msg, '') LIKE ?3
           )",
        params![task_type, status, keyword],
        |row| row.get(0),
    )?;

    let mut stmt = conn.prepare(
        "SELECT
            t.id,
            t.task_type,
            t.status,
            t.payload,
            t.error_msg,
            COUNT(tm.machine_id) AS machine_count,
            t.created_at,
            t.started_at,
            t.finished_at
         FROM task t
         LEFT JOIN task_machine tm ON tm.task_id = t.id
         WHERE (?1 IS NULL OR t.task_type = ?1)
           AND (?2 IS NULL OR t.status = ?2)
           AND (
             ?3 IS NULL
             OR t.id LIKE ?3
             OR t.task_type LIKE ?3
             OR t.status LIKE ?3
             OR IFNULL(t.payload, '') LIKE ?3
             OR IFNULL(t.error_msg, '') LIKE ?3
           )
         GROUP BY t.id
         ORDER BY COALESCE(t.started_at, t.created_at) DESC, t.created_at DESC
         LIMIT ?4 OFFSET ?5",
    )?;

    let rows = stmt.query_map(
        params![task_type, status, keyword, page_size, offset],
        |row| {
            let payload: Option<String> = row.get(3)?;
            let phase = extract_phase(payload.as_deref());
            Ok(RecentTaskSummary {
                task_id: row.get(0)?,
                task_type: row.get(1)?,
                status: row.get(2)?,
                phase,
                error_msg: row.get(4)?,
                machine_count: row.get(5)?,
                created_at: row.get(6)?,
                started_at: row.get(7)?,
                finished_at: row.get(8)?,
            })
        },
    )?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }

    let total_pages = ((total + page_size - 1) / page_size).max(1);
    Ok(TaskLogPage {
        items,
        page,
        page_size,
        total,
        total_pages,
    })
}

#[tauri::command]
pub async fn task_recent_list(
    limit: Option<i64>,
    db: State<'_, DbState>,
) -> Result<Vec<RecentTaskSummary>, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    recent_task_list_with_conn(&conn, limit.unwrap_or(8).clamp(1, 20))
}

#[tauri::command]
pub async fn task_log_query(
    query: Option<TaskLogQueryPayload>,
    db: State<'_, DbState>,
) -> Result<TaskLogPage, AppError> {
    let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
    task_log_page_with_conn(&conn, &query.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::{recent_task_list_with_conn, task_log_page_with_conn, TaskLogQueryPayload};
    use crate::db::open_and_migrate;

    #[test]
    fn tc_task_001_recent_task_list_sorts_latest_first() {
        let db_path =
            std::env::temp_dir().join(format!("task_recent_{}.sqlite", uuid::Uuid::new_v4()));
        let conn = open_and_migrate(&db_path).expect("migrate");
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload, created_at, started_at)
             VALUES ('task-old', 'deploy', 'success', '{\"phase\":\"SUCCESS\"}', '2026-03-01 00:00:00', '2026-03-01 00:00:00')",
            [],
        )
        .expect("insert old");
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload, created_at, started_at)
             VALUES ('task-new', 'upgrade', 'running', '{\"phase\":\"UPGRADING_BP\"}', '2026-03-02 00:00:00', '2026-03-02 00:00:00')",
            [],
        )
        .expect("insert new");
        let rows = recent_task_list_with_conn(&conn, 8).expect("recent tasks");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].task_id, "task-new");
        assert_eq!(rows[0].phase.as_deref(), Some("UPGRADING_BP"));
        assert_eq!(rows[1].task_id, "task-old");
    }

    #[test]
    fn tc_task_002_task_log_query_supports_paging_and_keyword() {
        let db_path =
            std::env::temp_dir().join(format!("task_log_query_{}.sqlite", uuid::Uuid::new_v4()));
        let conn = open_and_migrate(&db_path).expect("migrate");
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload, error_msg, created_at, started_at)
             VALUES ('deploy-1', 'deploy', 'success', '{\"phase\":\"DONE\"}', NULL, '2026-03-01 00:00:00', '2026-03-01 00:00:00')",
            [],
        )
        .expect("insert deploy-1");
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload, error_msg, created_at, started_at)
             VALUES ('deploy-2', 'deploy', 'failed', '{\"phase\":\"FAILED\"}', 'boom', '2026-03-02 00:00:00', '2026-03-02 00:00:00')",
            [],
        )
        .expect("insert deploy-2");
        conn.execute(
            "INSERT INTO task (id, task_type, status, payload, error_msg, created_at, started_at)
             VALUES ('upgrade-1', 'upgrade', 'running', '{\"phase\":\"UPGRADING_BP\"}', NULL, '2026-03-03 00:00:00', '2026-03-03 00:00:00')",
            [],
        )
        .expect("insert upgrade-1");

        let page1 = task_log_page_with_conn(
            &conn,
            &TaskLogQueryPayload {
                page: Some(1),
                page_size: Some(2),
                ..TaskLogQueryPayload::default()
            },
        )
        .expect("page1");
        assert_eq!(page1.total, 3);
        assert_eq!(page1.total_pages, 2);
        assert_eq!(page1.items.len(), 2);
        assert_eq!(page1.items[0].task_id, "upgrade-1");

        let filtered = task_log_page_with_conn(
            &conn,
            &TaskLogQueryPayload {
                keyword: Some("deploy".into()),
                status: Some("failed".into()),
                ..TaskLogQueryPayload::default()
            },
        )
        .expect("filtered");
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items.len(), 1);
        assert_eq!(filtered.items[0].task_id, "deploy-2");
        assert_eq!(filtered.items[0].phase.as_deref(), Some("FAILED"));
    }
}
