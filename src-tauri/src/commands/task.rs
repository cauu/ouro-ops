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
        let phase = payload
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .and_then(|value| {
                value
                    .get("phase")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            });
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

#[tauri::command]
pub async fn task_recent_list(
    limit: Option<i64>,
    db: State<'_, DbState>,
) -> Result<Vec<RecentTaskSummary>, AppError> {
    let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;
    recent_task_list_with_conn(&conn, limit.unwrap_or(8).clamp(1, 20))
}

#[cfg(test)]
mod tests {
    use super::recent_task_list_with_conn;
    use crate::db::open_and_migrate;

    #[test]
    fn tc_task_001_recent_task_list_sorts_latest_first() {
        let db_path = std::env::temp_dir().join(format!(
            "task_recent_{}.sqlite",
            uuid::Uuid::new_v4()
        ));
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
}
