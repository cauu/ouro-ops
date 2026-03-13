CREATE TABLE task_new (
    id          TEXT PRIMARY KEY,
    task_type   TEXT NOT NULL CHECK(task_type IN (
                    'deploy', 'upgrade', 'kes_rotation',
                    'rollback', 'health_check', 'hardening',
                    'runtime_config', 'runtime_restart',
                    'observability_bootstrap', 'observability_rollback'
                )),
    status      TEXT NOT NULL DEFAULT 'pending' CHECK(status IN (
                    'pending', 'running', 'paused',
                    'success', 'failed', 'cancelled'
                )),
    payload     TEXT,
    error_msg   TEXT,
    started_at  TEXT,
    finished_at TEXT,
    created_at  TEXT DEFAULT (datetime('now'))
);

INSERT INTO task_new (id, task_type, status, payload, error_msg, started_at, finished_at, created_at)
SELECT id, task_type, status, payload, error_msg, started_at, finished_at, created_at
FROM task;

DROP TABLE task;
ALTER TABLE task_new RENAME TO task;
