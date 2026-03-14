CREATE TABLE IF NOT EXISTS app_config (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    created_at  TEXT DEFAULT (datetime('now')),
    updated_at  TEXT DEFAULT (datetime('now'))
);
