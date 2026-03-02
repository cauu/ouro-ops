/**
 * 与 Rust 结构体一致的 TS 类型（Phase 1 仅占位）
 */

export interface DbVersionResult {
  user_version: number;
  tables: Record<string, boolean>;
}
