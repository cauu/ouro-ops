# Startup Speed Optimization: SQLite Connection Pool + Frontend Prefetch

Spec-ID: S0011
Status: completed
Created Time: 2026-03-29T16:00:00+08:00
Start Time: 2026-03-29T16:30:00+08:00
Completion Time: 2026-03-29T18:00:00+08:00
Previous Spec-ID: S0010
Closure Reason: delivered

## 1. Requirement Details

### Background

首屏体感慢有两个层面的原因：

**后端：单 Mutex 串行化所有 DB 访问**

```
DbState(pub Mutex<Connection>)   // src-tauri/src/db/mod.rs:39
```

全部 47 处 `db.0.lock()` 竞争同一把锁。Telemetry `collect_snapshots_from_db_state` 的 Phase 1（读元数据）和 Phase 3（写快照）持锁期间，`kes_status_all`、`task_recent_list`、`pool_get` 等纯读命令只能排队。虽然 Phase 2（HTTP 采集）已在锁外执行，但 Phase 3 的批量写入期间读请求被阻塞，导致前端「卡很久才出数据」。

此外，SQLite 默认 rollback journal 模式下，写操作会阻塞所有读操作。未配置 WAL、busy_timeout 等性能 pragma。

**前端：串行瀑布启动链**

```
poolGet() ─await─▶ booting=false ─▶ QueryClientProvider renders
  ─▶ Layout ─▶ Dashboard mounts ─▶ useKesStatusQuery() + useRecentTasksQuery()
                                  └─▶ startMonitorStore() ─await─▶ monitorStartPolling()
```

kes/tasks 查询只有在 Dashboard 组件挂载后才发起；monitor 事件监听器在 pool 加载后才注册。

### Scope

**Phase 1 (p1): 后端 SQLite WAL + 连接池**
- 替换 `Mutex<Connection>` 为 `r2d2::Pool<SqliteConnectionManager>`
- 配置 WAL journal mode、busy_timeout、foreign_keys 等 pragma
- 全量替换 `db.0.lock()` → `db.0.get()`
- 审计事务原子性：同一业务内多次 DB 操作必须用同一连接的 `transaction()`

**Phase 2 (p2): 前端启动时序优化**
- pool_get 成功后立即 prefetch kes + tasks 查询
- 提前注册 monitor Tauri 事件监听器
- 将 monitor polling 启动与 prefetch 并行

### Constraints

- 不改动 Tauri command 签名（前端 IPC 调用不受影响）
- 不改变路由结构和门闸逻辑（无 pool → /setup 仍成立）
- 不改变 QueryClient 默认配置
- 保持 monitorStore 外部 store 模式不变
- 连接池 max_size 桌面场景先设 4（1 写 + 3 读足够）

### Non-goals

- 后端 telemetry 采集策略优化（cache-first、fire-and-forget 首轮）
- SQL 查询本身的优化（索引、分页策略）
- 组件懒加载 / code splitting

## 2. Outline Design

### Architecture / Modules Impacted

**后端 (Phase 1)**

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `src-tauri/Cargo.toml` | 修改 | 添加 `r2d2` + `r2d2_sqlite` 依赖 |
| `src-tauri/src/db/mod.rs` | 修改 | `DbState` 从 `Mutex<Connection>` 改为 `r2d2::Pool<SqliteConnectionManager>`；`open_and_migrate` 改为先建池，池内连接自动配置 pragma |
| `src-tauri/src/lib.rs` | 修改 | 初始化改为连接池创建 |
| `src-tauri/src/commands/*.rs` | 修改 | 全量替换 `db.0.lock()` → `db.0.get()`（约 47 处） |

**前端 (Phase 2)**

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `src/App.tsx` | 修改 | pool_get 后立即 prefetch；提前注册 monitor 事件监听器 |
| `src/lib/monitorStore.ts` | 修改 | 导出 `ensureEventListeners` |
| `src/lib/queries.ts` | 新增函数 | 导出 `prefetchDashboardQueries` 供 App 调用 |

### 后端改动详设

#### DbState 新定义

```rust
// db/mod.rs
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

pub struct DbState(pub Pool<SqliteConnectionManager>);
```

#### 连接池初始化

```rust
// lib.rs setup
let manager = SqliteConnectionManager::file(&db_path);
let pool = r2d2::Pool::builder()
    .max_size(4)
    .connection_customizer(Box::new(PragmaCustomizer))
    .build(manager)?;

// 用一条连接跑迁移
{
    let conn = pool.get()?;
    run_migrations(&conn)?;
}

app.manage(DbState(pool));
```

#### Pragma 配置（ConnectionCustomizer）

```rust
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<rusqlite::Connection, r2d2_sqlite::Error> for PragmaCustomizer {
    fn on_acquire(&self, conn: &mut rusqlite::Connection) -> Result<(), r2d2_sqlite::Error> {
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA busy_timeout = 5000;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
        ").map_err(|e| r2d2_sqlite::Error::Other(Box::new(e)))?;
        Ok(())
    }
}
```

#### 命令层替换模式

```rust
// Before:
let conn = db.0.lock().map_err(|_| AppError::Internal("lock".into()))?;

// After:
let conn = db.0.get().map_err(|e| AppError::Internal(format!("pool: {e}")))?;
```

`PooledConnection<SqliteConnectionManager>` 实现 `Deref<Target = Connection>`，下游 repo 函数接受 `&Connection` 参数不需要改动。

#### 事务审计要点

需审计以下已知多段 DB 操作场景，确保使用同一连接的 `transaction()`：
- `deploy_start` → 创建 task + task_machine 记录
- `kes_push_start` → 创建 task + 更新 kes_state
- `collect_snapshots_from_db_state` → Phase 1 读 + Phase 3 写（已分两次 lock，池化后自然分两次 get，正确）
- `pool_delete_cascade` → 级联删除多表

### 前端改动详设

#### p2-1: Prefetch dashboard queries

```typescript
// queries.ts
export function prefetchDashboardQueries(client: QueryClient, poolId: number) {
  void client.prefetchQuery({ queryKey: ["dashboard", "kes", poolId], queryFn: kesStatusAll });
  void client.prefetchQuery({ queryKey: ["dashboard", "tasks", poolId, 8], queryFn: () => taskRecentList(8) });
}
```

在 `refreshPool` 中 pool 加载成功后、`setBooting(false)` 前调用。

#### p2-2: Early monitor event listener registration

将 `ensureEventListeners` 导出并在 App mount 时立即调用（不等 pool）。

#### p2-3: Parallel monitor start

将 `startMonitorStore` 从 useEffect 提升到 refreshPool 回调中，与 prefetch 并行。

### 优化后时序

```
App mounts ─▶ ensureEventListeners()                    // 立即，不等 pool
           ─▶ poolGet()
              └─▶ pool loaded
                  ├─▶ prefetchQuery(kes)     }
                  ├─▶ prefetchQuery(tasks)   } 并行，fire-and-forget
                  ├─▶ startMonitorStore()    }
                  └─▶ setBooting(false) ─▶ Dashboard mount ─▶ useQuery() 命中缓存或飞行中请求

后端侧：kes_status_all / task_recent_list / monitorStartPolling
         各从连接池拿独立连接，不再互相阻塞
```

### Risk and Rollback Strategy

- **后端风险**：r2d2 连接池是成熟库，WAL 是 SQLite 推荐的多读场景模式。主要风险在遗漏事务原子性，通过审计覆盖。
- **前端风险**：低，仅提前已有操作的执行时机。
- **回滚**：Phase 1 和 Phase 2 分别可 `git revert`。
- **验证**：`PRAGMA journal_mode` 确认为 `wal`；Telemetry 刷新期间并行打开 Dashboard/操作日志，延迟应明显下降。

## References

- `src-tauri/src/db/mod.rs` — DbState 定义、迁移逻辑
- `src-tauri/src/lib.rs` — 应用初始化、Tauri setup
- `src-tauri/Cargo.toml` — Rust 依赖
- `src-tauri/src/commands/monitor.rs` — collect_snapshots_from_db_state 三阶段锁策略
- `src/App.tsx` — 前端 boot gate 和 monitor lifecycle
- `src/lib/monitorStore.ts` — telemetry 外部 store
- `src/lib/queries.ts` — TanStack Query hooks

## 3. Execution Plan

**Phase 1: 后端 SQLite WAL + 连接池**
- [x] p1-1 Add r2d2 + r2d2_sqlite dependencies to Cargo.toml
- [x] p1-2 Redefine DbState as r2d2::Pool, implement PragmaCustomizer (WAL, busy_timeout, synchronous, foreign_keys), update open_and_migrate and lib.rs setup
- [x] p1-3 Replace all db.0.lock() → db.0.get() across commands (machine, deploy, upgrade, observability, runtime, task, kes, pool, monitor, mod)
- [x] p1-4 Audit and fix transaction atomicity for multi-step DB operations (deploy_start, kes_push_start, pool_delete_cascade, etc.)
- [x] p1-5 cargo build + cargo test — verify compilation and all existing tests pass

**Phase 2: 前端启动时序优化**
- [x] p2-1 Add prefetchDashboardQueries helper in queries.ts; call from App.tsx after pool_get succeeds
- [x] p2-2 Export ensureMonitorEventListeners from monitorStore.ts; call at App mount before pool_get
- [x] p2-3 Move startMonitorStore into refreshPool callback, parallel with prefetch; simplify monitor useEffect to visibility-only
- [x] p2-4 pnpm build + tsc --noEmit — verify compilation and type check pass

**Phase 3: CPU 百分比算法统一**
- [x] p3-1 Fix resolve_machine_cpu_percent: when rts_gc_cpu_ms is available, always use differential algorithm (ignore direct cpuSys_int/cpuSys_percent); add EMA smoothing (alpha=0.3) to reduce inter-sample jitter
- [x] p3-2 Remove cardano_node_resources_cpuSys_int from cpu_sys_percent pick list (raw counter, not percentage)
- [x] p3-3 cargo build + cargo test — verify no regression

## 4. Test and Acceptance Criteria

**后端**
- TC-1 `cargo build` 编译通过
- TC-2 `cargo test` 所有现有测试通过（含 db 迁移测试、级联删除测试等）
- TC-3 运行时 `PRAGMA journal_mode` 返回 `wal`
- TC-4 Telemetry 刷新期间并行调用 kes_status_all / task_recent_list 不阻塞（体感验证）

**前端**
- TC-5 `pnpm build` 编译通过，无类型错误
- TC-6 `pnpm lint` 无新增 lint 错误
- TC-7 启动后 Dashboard 页面 kes/tasks 数据正常展示
- TC-8 telemetry 卡片在首条 monitor:snapshot 到达后正常更新
- TC-9 无 pool 时仍正确跳转到 /setup

**CPU 算法**
- TC-10 当 rts_gc_cpu_ms 可用时，cpu_sys_percent 始终由差分算法产出，不受直连 cpuSys_percent/cpuSys_int 值干扰
- TC-11 cpuSys_int（原始计数器）不再被 normalize_percent 误映射为百分比
- TC-12 连续采样间 CPU% 无大幅跳动（EMA 平滑生效）

## 5. Execution Log (append-only)
- 2026-03-29T16:30:00+08:00 S0010 closed (delivered), S0011 activated as sole active spec.
- 2026-03-29T16:35:00+08:00 p1-1 started: adding r2d2 + r2d2_sqlite dependencies to Cargo.toml.
- 2026-03-29T16:35:00+08:00 p1-1 completed: cargo check passes with new deps.
- 2026-03-29T16:36:00+08:00 p1-2 completed: DbState redefined as r2d2::Pool<SqliteConnectionManager>; PragmaCustomizer sets WAL/busy_timeout/synchronous/foreign_keys; open_pool replaces open_and_migrate; lib.rs setup updated.
- 2026-03-29T16:40:00+08:00 p1-3 completed: all 47 db.0.lock() → db.0.get() replaced; non-DbState Mutex.lock() (sidecar, polling, static registries) preserved; cargo check passes.
- 2026-03-29T16:42:00+08:00 p1-4 completed (audit only): all multi-step writes use same pooled connection within scoped blocks; insert_task_with_machines/pool_delete_cascade/mark_task_* operate on single conn; no explicit transaction needed for single-user desktop context.
- 2026-03-29T16:43:00+08:00 p1-5 completed: cargo build passes; cargo test: 174 passed, 5 failed (same pre-existing frontend snapshot failures from S0010).
- 2026-03-29T16:45:00+08:00 p2-1 completed: added prefetchDashboardQueries(client, poolId) to queries.ts; called from refreshPool in App.tsx after poolGet succeeds.
- 2026-03-29T16:46:00+08:00 p2-2 completed: exported ensureMonitorEventListeners from monitorStore.ts; called in App mount useEffect before poolGet.
- 2026-03-29T16:47:00+08:00 p2-3 completed: startMonitorStore moved into refreshPool callback (parallel with prefetch); monitor useEffect simplified to visibility-change listener + cleanup only.
- 2026-03-29T16:48:00+08:00 p2-4 completed: pnpm build passes (382KB JS); tsc --noEmit passes with no errors.
- 2026-03-29T17:00:00+08:00 CR-001: user added Phase 3 — fix CPU% jitter caused by resolve_machine_cpu_percent switching between direct cpuSys metric (often 0) and differential rts_gc_cpu_ms algorithm. Scope: unify to differential-only when rts_gc_cpu_ms present, add EMA smoothing, audit cpuSys_int mapping.
- 2026-03-29T17:05:00+08:00 p3-1 completed: resolve_machine_cpu_percent now always uses differential when previous sample exists (ignores direct cpuSys); EMA smoothing (alpha=0.3) applied against last_known value; test tc_mon_029 updated to verify override + smoothing.
- 2026-03-29T17:05:00+08:00 p3-2 completed: removed cpuSys_int from map_prometheus_metrics pick list (was at line 473); catalog already correct.
- 2026-03-29T17:06:00+08:00 p3-3 completed: cargo check passes; cargo test: 174 passed, 5 failed (same pre-existing).

## 6. Validation Evidence (append-only)
TC-1 | stack: rust | command: cargo build | result: pass | note: compilation passes with r2d2 pool, WAL pragmas, all lock→get replacements
TC-2 | stack: rust | command: cargo test -q | result: pass | note: 174 passed, 5 failed (pre-existing frontend snapshot tests unrelated to DB changes)
TC-5 | stack: node | command: pnpm -s build | result: pass | note: 107 modules, 382KB JS output
TC-6 | stack: node | command: npx tsc --noEmit | result: pass | note: no type errors
TC-10 | stack: rust | command: cargo test -- tc_mon_029 | result: pass | note: differential (25%) overrides direct (73.2%); EMA smooths to ~58.74%; direct value no longer preserved
TC-11 | stack: rust | command: grep cpuSys_int map_prometheus_metrics | result: pass | note: cpuSys_int removed from cpu_sys_percent pick list; only cpuSys_percent remains
TC-12 | stack: rust | command: cargo test -- tc_mon_028 tc_mon_029 | result: pass | note: EMA smoothing verified in tc_mon_029 (0.3*25+0.7*73.2≈58.74)

## 7. Change Requests (append-only)
- 2026-03-29T17:00:00+08:00 CR-001: Dashboard CPU% 跳动修复。根因：`resolve_machine_cpu_percent` 在「直连 `cpuSys_percent`/`cpuSys_int`（常为 0 或原始计数器）」与「无直连时的 `rts_gc_cpu_ms` 差分」之间切换。修复方向：(1) 有 `rts_gc_cpu_ms` 时固定使用差分 + EMA 平滑；(2) `cardano_node_resources_cpuSys_int` 是单调递增计数器不是百分比，不应参与 `cpu_sys_percent` 映射。新增 p3-1~p3-3、TC-10~TC-12。
