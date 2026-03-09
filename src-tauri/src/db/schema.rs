//! 表结构定义与设计 §6.2 一致，建表 SQL 在 migrations 中

pub const MIGRATION_001: &str = include_str!("../../migrations/001_init.sql");
pub const MIGRATION_002: &str = include_str!("../../migrations/002_machine_health_sync_stage.sql");
pub const MIGRATION_003: &str = include_str!("../../migrations/003_task_runtime_types.sql");
pub const MIGRATION_004: &str = include_str!("../../migrations/004_pool_onchain_binding.sql");
