-- ==================== 矿池配置 ====================
CREATE TABLE IF NOT EXISTS pool (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ticker      TEXT NOT NULL UNIQUE,
    network     TEXT NOT NULL CHECK(network IN ('mainnet', 'preprod', 'preview')),
    margin      REAL,
    fixed_cost  INTEGER,
    created_at  TEXT DEFAULT (datetime('now')),
    updated_at  TEXT DEFAULT (datetime('now'))
);

-- ==================== 主机信息 ====================
CREATE TABLE IF NOT EXISTS machine (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    pool_id             INTEGER NOT NULL,
    name                TEXT NOT NULL,
    ip                  TEXT NOT NULL,
    ssh_port            INTEGER DEFAULT 22,
    ssh_user            TEXT DEFAULT 'root',
    role                TEXT NOT NULL CHECK(role IN ('relay', 'bp', 'archive')),
    ssh_key_fingerprint TEXT,
    os_version          TEXT,
    cardano_version     TEXT,
    image_registry      TEXT DEFAULT 'ghcr.io/blinklabs-io/cardano-node',
    image_digest        TEXT,
    sort_order          INTEGER DEFAULT 0,
    created_at          TEXT DEFAULT (datetime('now')),
    updated_at          TEXT DEFAULT (datetime('now')),
    UNIQUE(pool_id, ip)
);

-- ==================== KES 状态 ====================
CREATE TABLE IF NOT EXISTS kes_state (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    machine_id          INTEGER NOT NULL,
    kes_period_current  INTEGER,
    kes_period_max      INTEGER,
    op_cert_counter     INTEGER,
    expiry_date         TEXT,
    last_checked_at     TEXT DEFAULT (datetime('now')),
    UNIQUE(machine_id)
);

-- ==================== 任务记录 ====================
CREATE TABLE IF NOT EXISTS task (
    id          TEXT PRIMARY KEY,
    task_type   TEXT NOT NULL CHECK(task_type IN (
                    'deploy', 'upgrade', 'kes_rotation',
                    'rollback', 'health_check', 'hardening'
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

-- ==================== 任务-主机关联 ====================
CREATE TABLE IF NOT EXISTS task_machine (
    task_id     TEXT NOT NULL,
    machine_id  INTEGER NOT NULL,
    status      TEXT DEFAULT 'pending',
    log_path    TEXT,
    PRIMARY KEY (task_id, machine_id)
);

-- ==================== 主机健康快照 ====================
CREATE TABLE IF NOT EXISTS machine_health (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    machine_id          INTEGER NOT NULL,
    block_height        INTEGER,
    sync_progress       REAL,
    peers_count         INTEGER,
    mempool_size        INTEGER,
    cpu_percent         REAL,
    memory_percent      REAL,
    disk_used_percent   REAL,
    missed_slots_24h    INTEGER DEFAULT 0,
    collected_at        TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_health_time ON machine_health(machine_id, collected_at);

-- ==================== 审计日志 ====================
CREATE TABLE IF NOT EXISTS audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    action      TEXT NOT NULL,
    detail      TEXT,
    created_at  TEXT DEFAULT (datetime('now'))
);
