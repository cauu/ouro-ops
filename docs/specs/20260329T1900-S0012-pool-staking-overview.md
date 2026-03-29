# Pool Staking Overview: Delegator Dashboard + Detail List

Spec-ID: S0012
Status: active
Created Time: 2026-03-29T18:30:00+08:00
Start Time: 2026-03-29T19:00:00+08:00
Completion Time:
Previous Spec-ID: S0011

## 1. Requirement Details

### Background

SPO 需要了解自己矿池的质押状况：有多少用户委托、总质押量多少、趋势如何变化、每个 delegator 的具体质押金额。当前 ouro-ops 没有任何质押/委托相关的展示能力。

Cardano `cardano-cli query` 无法直接列出 delegator 列表（只能查聚合快照），需要依赖链上索引服务。

### Scope

1. **Dashboard Staking Overview 卡片**：总用户数、总质押量（ADA）、近 N epoch 用户数 + 质押量趋势折线图
2. **Delegator List 页面** (`/delegators`)：质押用户详细表格（stake address、质押量、委托 epoch），支持按质押量排序和 address 搜索
3. **后端 Koios API 集成**：3 个新 Tauri 命令，根据 pool.network 自动选择 Koios 端点
4. **前提条件**：pool 已绑定 `onchain_pool_id`；未绑定时 UI 显示引导提示

### Data Source: Koios API

免费、无需 API Key、支持 mainnet/preprod/preview。

| 端点 | 用途 | 请求方式 |
|------|------|---------|
| `POST /pool_info` | 当前 live_delegators + live_stake | body: `{"_pool_bech32_ids": ["pool1..."]}` |
| `POST /pool_history` | 按 epoch 历史：delegator_cnt, active_stake, epoch_no | body: `{"_pool_bech32_ids": ["pool1..."]}` |
| `POST /pool_delegators` | 当前 delegator 列表：stake_address, amount, active_epoch_no | body: `{"_pool_bech32_ids": ["pool1..."]}` |

Koios 网络端点：
- mainnet: `https://api.koios.rest/api/v1`
- preprod: `https://preprod.koios.rest/api/v1`
- preview: `https://preview.koios.rest/api/v1`

### Constraints

- 不引入 Blockfrost 或其他付费 API（保持零依赖 Key）
- 不改变现有 Dashboard 布局结构，Staking Overview 作为新卡片追加
- 前端缓存策略：staleTime 5 分钟（质押数据按 epoch ~5 天变化）
- Koios rate limit：免费 tier 约 100 req/min，正常使用不会触及

### Non-goals

- 历史数据 SQLite 持久化（首版纯前端缓存，后续可追加）
- 质押奖励计算/展示（需要更多数据源，独立需求）
- 实时 mempool 委托交易监控

## 2. Outline Design

### Architecture / Modules Impacted

**后端新增**

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `src-tauri/src/commands/staking.rs` | 新增 | Koios API 集成，3 个 Tauri 命令 |
| `src-tauri/src/commands/mod.rs` | 修改 | 注册 staking 模块 |
| `src-tauri/src/lib.rs` | 修改 | 注册新命令到 invoke_handler |

**前端新增**

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `src/lib/ipc.ts` | 修改 | 新增 3 个 IPC 调用 |
| `src/lib/types.ts` | 修改 | 新增 staking 相关类型 |
| `src/lib/queries.ts` | 修改 | 新增 3 个 TanStack Query hooks |
| `src/pages/Dashboard.tsx` | 修改 | 新增 Staking Overview 卡片 |
| `src/pages/Delegators.tsx` | 新增 | Delegator 详细列表页面 |
| `src/components/StakingTrendChart.tsx` | 新增 | 趋势折线图组件 |
| `src/App.tsx` | 修改 | 新增 /delegators 路由 |
| `src/components/Sidebar.tsx` | 修改 | 新增 Delegators 导航入口 |

### 后端详设

#### Koios 端点选择

```rust
fn koios_base_url(network: &str) -> &'static str {
    match network {
        "mainnet" => "https://api.koios.rest/api/v1",
        "preprod" => "https://preprod.koios.rest/api/v1",
        "preview" => "https://preview.koios.rest/api/v1",
        _ => "https://preprod.koios.rest/api/v1",
    }
}
```

#### 命令 1: pool_staking_summary

```rust
// 返回当前质押概览
struct StakingSummary {
    live_delegators: i64,
    live_stake: i64,       // lovelace
    live_stake_ada: f64,   // live_stake / 1_000_000
    active_stake: i64,     // lovelace (当前 epoch 快照)
    active_stake_ada: f64,
}
```

调用 Koios `POST /pool_info`，从响应提取 `live_delegators`, `live_stake`, `active_stake`。

#### 命令 2: pool_staking_history

```rust
// 返回按 epoch 的历史数据（用于趋势图）
struct StakingEpochEntry {
    epoch_no: i64,
    delegator_cnt: i64,
    active_stake: i64,     // lovelace
    active_stake_ada: f64,
}
```

调用 Koios `POST /pool_history`，返回 `Vec<StakingEpochEntry>`，按 epoch_no 升序排列。

#### 命令 3: pool_delegator_list

```rust
// 返回当前所有 delegator
struct Delegator {
    stake_address: String,
    amount: i64,           // lovelace
    amount_ada: f64,
    active_epoch_no: i64,
}
```

调用 Koios `POST /pool_delegators`，返回 `Vec<Delegator>`，按 amount 降序排列。

### 前端详设

#### Dashboard Staking Overview 卡片

```
┌─ Staking Overview ──────────────────────────────────┐
│                                                      │
│  Delegators: 127        Total Stake: 2.4M ADA       │
│                                                      │
│  ┌──── 趋势图（双轴折线）──────────────────────┐     │
│  │  左轴: 用户数    右轴: 质押量 (ADA)         │     │
│  │  ╱╲  ╱╲                              ╱╲    │     │
│  │ ╱  ╲╱  ╲──╱╲──                   ╱╲╱  ╲   │     │
│  │                                              │     │
│  │  E390  E391  E392  E393  ...  E399  E400    │     │
│  └──────────────────────────────────────────────┘     │
│                                                      │
│  [查看全部 Delegators →]                              │
└──────────────────────────────────────────────────────┘
```

趋势图：轻量实现，用 SVG path 绘制（无重型图表库），参考现有 Dashboard 卡片风格。

#### Delegators 页面

```
┌─ Delegators ────────────────────────────────────────┐
│  [搜索 stake address...]              排序: 质押量 ▼ │
│                                                      │
│  Stake Address              Amount (ADA)    Since    │
│  ─────────────────────────────────────────────────── │
│  stake1u8a3...f7k2          524,300.12      E385     │
│  stake1u9x7...m4p1          312,150.00      E390     │
│  stake1u2b5...n8q3           89,420.55      E392     │
│  ...                                                 │
│                                                      │
│  Showing 1-20 of 127                    [< 1 2 3 >]  │
└──────────────────────────────────────────────────────┘
```

### Risk and Rollback Strategy

- **Koios 不可达**：前端显示 "质押数据暂不可用" 降级提示，不影响其他功能
- **pool 未绑定 onchain_pool_id**：卡片显示 "请先绑定链上矿池" 引导
- **回滚**：`git revert` 删除新命令和新页面即可

## References

- Koios API 文档：https://api.koios.rest/
- `src-tauri/src/commands/pool.rs` — 现有 pool on-chain 查询模式
- `src/pages/Dashboard.tsx` — Dashboard 卡片布局
- `src/lib/queries.ts` — TanStack Query hooks 模式

## 3. Execution Plan

**Phase 1: 后端 Koios API 集成**
- [x] p1-1 新增 `src-tauri/src/commands/staking.rs`：Koios 端点选择、HTTP 调用封装、pool_staking_summary 命令
- [x] p1-2 新增 pool_staking_history 命令（按 epoch 历史）
- [x] p1-3 新增 pool_delegator_list 命令（当前 delegator 列表）
- [x] p1-4 注册模块和命令到 mod.rs + lib.rs；cargo build + cargo test

**Phase 2: 前端类型和查询层**
- [x] p2-1 新增 IPC 调用（ipc.ts）和类型定义（types.ts）
- [x] p2-2 新增 TanStack Query hooks（queries.ts）：useStakingSummary、useStakingHistory、useDelegatorList

**Phase 3: Dashboard Staking Overview 卡片**
- [x] p3-1 Dashboard 新增 Staking Overview 区域：总用户数 + 总质押量 + Active Stake 指标卡
- [x] p3-2 新增 StakingTrendChart 组件：SVG 双轴折线图（epoch × delegator_cnt + active_stake）
- [x] p3-3 集成趋势图到 Dashboard；未绑定 pool 时显示引导提示；Link to /delegators

**Phase 4: Delegator List 页面**
- [x] p4-1 新增 Delegators.tsx 页面：表格、按质押量排序、address 搜索、分页（20/页）
- [x] p4-2 App.tsx 新增 /delegators 路由；Sidebar 新增 Delegators 导航入口
- [x] p4-3 pnpm build + tsc + cargo test 全量验证

## 4. Test and Acceptance Criteria

**后端**
- TC-1 `cargo build` 编译通过
- TC-2 `cargo test` 无新增失败
- TC-3 pool_staking_summary 返回 live_delegators + live_stake（手动验证或 mock）
- TC-4 pool_staking_history 返回按 epoch 升序的历史条目
- TC-5 pool_delegator_list 返回按 amount 降序的 delegator 列表

**前端**
- TC-6 `pnpm build` 编译通过
- TC-7 `tsc --noEmit` 无类型错误
- TC-8 Dashboard Staking Overview 卡片展示总用户数 + 总质押量
- TC-9 趋势图展示近 N epoch 的双轴折线
- TC-10 未绑定 onchain_pool_id 时显示引导提示而非空白
- TC-11 /delegators 页面表格可排序、可搜索、可分页
- TC-12 Sidebar 新增 Delegators 导航入口且路由正确

## 5. Execution Log (append-only)
- 2026-03-29T19:00:00+08:00 p1-1~p1-4 completed: staking.rs with 3 Koios commands + 2 unit tests; registered in mod.rs + lib.rs; cargo check + cargo test pass (176 passed, 5 pre-existing).
- 2026-03-29T19:05:00+08:00 p2-1~p2-2 completed: types.ts + ipc.ts + queries.ts extended; tsc --noEmit pass.
- 2026-03-29T19:10:00+08:00 p3-1~p3-3 completed: Dashboard Staking Overview card with summary metrics + StakingTrendChart SVG component + unbound pool guidance.
- 2026-03-29T19:15:00+08:00 p4-1~p4-3 completed: Delegators.tsx page with table/sort/search/pagination; /delegators route + Sidebar entry; pnpm build + tsc + cargo test all pass.

## 6. Validation Evidence (append-only)
TC-1 | stack: rust | command: cargo check | result: pass | note: staking module compiles
TC-2 | stack: rust | command: cargo test -q | result: pass | note: 176 passed, 5 pre-existing failures; 2 new staking tests pass
TC-6 | stack: node | command: pnpm -s build | result: pass | note: 406KB JS output
TC-7 | stack: node | command: npx tsc --noEmit | result: pass | note: no type errors

## 7. Change Requests (append-only)
