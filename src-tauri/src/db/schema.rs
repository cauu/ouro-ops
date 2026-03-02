//! 表结构定义与设计 §6.2 一致，建表 SQL 在 migrations 中

pub const MIGRATION_001: &str = include_str!("../../migrations/001_init.sql");
