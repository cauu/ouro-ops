ALTER TABLE pool ADD COLUMN onchain_pool_id TEXT;
ALTER TABLE pool ADD COLUMN onchain_registered INTEGER NOT NULL DEFAULT 0;
ALTER TABLE pool ADD COLUMN pledge INTEGER;
ALTER TABLE pool ADD COLUMN reward_account TEXT;
ALTER TABLE pool ADD COLUMN metadata_url TEXT;
ALTER TABLE pool ADD COLUMN metadata_hash TEXT;
ALTER TABLE pool ADD COLUMN owners_json TEXT;
ALTER TABLE pool ADD COLUMN relays_json TEXT;
ALTER TABLE pool ADD COLUMN onchain_synced_at TEXT;
