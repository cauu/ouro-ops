mod repo;
mod schema;

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use crate::error::AppError;

const MIGRATIONS: &[&str] = &[schema::MIGRATION_001];

/// 执行迁移：user_version 递增，仅执行未执行的迁移
pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    let current: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i32;
        if version > current {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", version)?;
        }
    }
    Ok(())
}

/// 打开应用数据目录下的 SQLite 连接并执行迁移
pub fn open_and_migrate(db_path: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open(db_path)?;
    run_migrations(&conn)?;
    Ok(conn)
}

pub struct DbState(pub Mutex<Connection>);

pub use repo::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn count_rows(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .expect("count rows")
    }

    #[test]
    fn tc_db_001_migration_creates_tables_and_no_fk() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");

        let tables = [
            "pool",
            "machine",
            "kes_state",
            "task",
            "task_machine",
            "machine_health",
            "audit_log",
        ];
        for table in tables {
            assert!(table_exists(&conn, table).expect("table exists"));
        }
        let version = get_user_version(&conn).expect("user version");
        assert!(version >= 1);

        let mut stmt = conn
            .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name IN ('pool','machine','kes_state','task','task_machine','machine_health','audit_log')")
            .expect("prepare sqlite_master query");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query sqlite_master");
        for row in rows {
            let ddl = row.expect("read ddl");
            assert!(
                !ddl.to_uppercase().contains("REFERENCES"),
                "ddl should not contain REFERENCES: {ddl}"
            );
        }
    }

    #[test]
    fn tc_db_002_business_layer_cascade_without_fk() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");

        let pool_id = pool_insert(&conn, "OURO", "preprod", Some(0.02), Some(340000000))
            .expect("insert pool");
        let machine_id = machine_insert(
            &conn,
            pool_id,
            "relay-1",
            "10.0.0.1",
            22,
            "root",
            "relay",
            Some("SHA256:dummy"),
        )
        .expect("insert machine");
        conn.execute(
            "INSERT INTO kes_state (machine_id, kes_period_current, kes_period_max, op_cert_counter)
             VALUES (?1, 1, 2, 3)",
            rusqlite::params![machine_id],
        )
        .expect("insert kes_state");
        conn.execute(
            "INSERT INTO machine_health (machine_id, block_height) VALUES (?1, 100)",
            rusqlite::params![machine_id],
        )
        .expect("insert machine_health");
        let task_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO task (id, task_type, status) VALUES (?1, 'deploy', 'running')",
            rusqlite::params![task_id],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status) VALUES (?1, ?2, 'running')",
            rusqlite::params![task_id, machine_id],
        )
        .expect("insert task_machine");

        pool_delete_cascade(&conn, pool_id).expect("pool cascade delete");

        assert_eq!(count_rows(&conn, "pool"), 0);
        assert_eq!(count_rows(&conn, "machine"), 0);
        assert_eq!(count_rows(&conn, "kes_state"), 0);
        assert_eq!(count_rows(&conn, "machine_health"), 0);
        assert_eq!(count_rows(&conn, "task_machine"), 0);
        // 任务主表与 pool 无直接关联，保留由上层业务继续处理。
        assert_eq!(count_rows(&conn, "task"), 1);
    }

    #[test]
    fn tc_p2_7_machine_repo_insert_delete_list_and_pool_binding() {
        let conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&conn).expect("run migrations");

        let pool_id = pool_insert(&conn, "OURO", "preprod", Some(0.02), Some(340000000))
            .expect("insert pool");
        let pool = pool_get_single(&conn)
            .expect("get pool")
            .expect("pool exists");
        assert_eq!(pool.id, pool_id);

        let relay_id = machine_insert(
            &conn,
            pool.id,
            "relay-1",
            "10.0.0.10",
            22,
            "root",
            "relay",
            Some("SHA256:relay"),
        )
        .expect("insert relay");
        let bp_id = machine_insert(
            &conn,
            pool.id,
            "bp-1",
            "10.0.0.11",
            22,
            "root",
            "bp",
            Some("SHA256:bp"),
        )
        .expect("insert bp");

        let relay = machine_get(&conn, relay_id)
            .expect("get relay")
            .expect("relay exists");
        assert_eq!(relay.pool_id, pool.id);

        conn.execute(
            "INSERT INTO kes_state (machine_id, kes_period_current, kes_period_max, op_cert_counter)
             VALUES (?1, 1, 2, 3)",
            rusqlite::params![relay_id],
        )
        .expect("insert kes_state");
        conn.execute(
            "INSERT INTO machine_health (machine_id, block_height) VALUES (?1, 120)",
            rusqlite::params![relay_id],
        )
        .expect("insert machine_health");
        let task_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO task (id, task_type, status) VALUES (?1, 'deploy', 'running')",
            rusqlite::params![task_id],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_machine (task_id, machine_id, status) VALUES (?1, ?2, 'running')",
            rusqlite::params![task_id, relay_id],
        )
        .expect("insert task_machine");

        let all = machine_list(&conn, None, None).expect("list all");
        assert_eq!(all.len(), 2);
        let only_relay = machine_list(&conn, Some("relay"), None).expect("list relay");
        assert_eq!(only_relay.len(), 1);
        assert_eq!(only_relay[0].id, relay_id);
        let by_network = machine_list(&conn, None, Some("preprod")).expect("list network");
        assert_eq!(by_network.len(), 2);

        machine_delete_cascade(&conn, relay_id).expect("delete relay with cascade");
        assert_eq!(count_rows(&conn, "machine"), 1);
        assert_eq!(count_rows(&conn, "kes_state"), 0);
        assert_eq!(count_rows(&conn, "machine_health"), 0);
        assert_eq!(count_rows(&conn, "task_machine"), 0);
        assert!(machine_get(&conn, relay_id).expect("query relay").is_none());
        assert!(machine_get(&conn, bp_id).expect("query bp").is_some());
    }
}
