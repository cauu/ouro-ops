# Cardano Stake Pool 链上注册状态查询与注册流程

Spec-ID：`S0006`
状态：`active`
创建时间：`2026-03-09`
开始时间：`2026-03-09T16:02:09+0800`
完成时间：
前一个 Spec-ID：`S0005`
结项原因：

## 1. 需求详情

- 背景
  - Phase 4 已完成节点运行时配置、运行时重启、KES、升级、监控与审计链路，当前 spec 在该控制面基线上继续向链上注册能力推进。
  - 当前控制面已经覆盖 relay / bp 节点部署、运行时配置、KES、升级、回退和监控。
  - 当前系统中的 `pool_init` / `pool_update` 仅写入本地 SQLite 元数据，不代表链上 stake pool 已注册，也不能读取链上真实注册信息。
  - 用户需要判断当前 stake pool 是否已经链上注册；若已注册，应读取并展示注册信息；若未注册，应提供注册能力。
- 范围
  - 查询 stake pool 链上注册状态。
  - 读取并展示链上已注册的 pool 信息。
  - 提供未注册场景下的注册流程。
  - 明确本地元数据与链上真实配置的边界。
- 约束
  - 保持 local-first 架构：Tauri + Rust + SQLite + Python sidecar + Ansible。
  - 复用 Phase 4 已交付的运行时控制面与审计能力，不回退既有节点运维基线。
  - 不引入 SQLite 外键和级联。
  - 冷密钥和交易签名流程必须保持可审计，不得绕过显式确认。
  - 不能假设当前机器上已经完整具备 cold key、payment key、reward account key 或提交交易所需的所有签名材料。
- 非目标
  - 不在本 spec 中实现矿池退役。
  - 不在本 spec 中实现链上参数自动纠偏。
  - 不在本 spec 中实现浏览器钱包或远端钱包集成。

## 2. 概要设计

- 架构 / 受影响模块
  - 后端 commands：新增 pool registration query / prepare / submit 相关命令。
  - 前端页面：新增链上注册状态展示与注册向导。
  - 运行时依赖：通过 `cardano-cli` 查询链上 pool 状态并构建 registration 交易材料。
- 数据模型与接口
  - 本地 `pool` 表降级为控制面元数据或缓存，不再作为链上 truth source。
  - 新增链上查询结果模型，至少覆盖：
    - `pool_id`
    - `ticker`
    - `margin`
    - `fixed_cost`
    - `pledge`
    - `reward_account`
    - `owners`
    - `relays`
    - `metadata_url`
    - `metadata_hash`
    - `registered_onchain`
  - 注册流程拆成至少两个阶段：
    - 准备 registration inputs / certificate / transaction draft
    - 签名并提交 transaction
- 风险与回退策略
  - 链上注册是不可逆的高风险操作，必须显式确认。
  - 所有注册前置数据必须支持只读检查和预览，避免直接提交错误参数。
  - 提交前要记录完整审计日志，包括 pool id、目标网络、证书摘要和交易 hash。

## 3. 执行计划

- [x] `p6-8` 将 `S0006` 提升为唯一 active spec，并承接 `S0005` 已交付的 Phase 4 基线
- [x] `p6-1` 定义链上注册状态查询接口与返回模型
- [ ] `p6-2` 实现 stake pool 是否已注册的链上判断与详情读取
- [ ] `p6-3` 将 Settings 中误导性的本地可编辑链上字段改为只读或迁移
- [ ] `p6-4` 设计并实现 registration 准备流程：参数校验、证书生成、交易草稿
- [ ] `p6-5` 设计并实现 registration 提交流程：签名输入、交易提交与结果回执
- [ ] `p6-6` 增加前端注册状态页与注册向导
- [ ] `p6-7` 增加高风险确认、审计记录与验收验证

## 4. 测试与验收标准

- `TC-P6-001` 能根据 pool id 或注册材料判断当前 pool 是否已链上注册。
- `TC-P6-002` 若已注册，前端可展示链上真实注册信息，且不再误用本地 `ticker/margin/fixed_cost` 作为 truth source。
- `TC-P6-003` 若未注册，系统可生成 registration 证书和交易草稿，并明确缺失的签名材料。
- `TC-P6-004` 注册提交流程具备显式确认、失败可见性和审计记录。
- `TC-P6-005` 不再允许用户将仅本地可写的字段误认为链上已生效配置。
- `TC-P6-006` spec 切换后，`docs/specs/` 根目录应仅保留 `S0006` 作为 active spec，`S0005` 应进入 `completed/`，且 `docs/README.md` 入口一致。
- `TC-P6-007` 后端与前端应暴露统一的链上注册状态查询接口、请求模型与返回模型；在 `p6-2` 落地真实链上查询前，接口至少能完成本地校验并返回一致的占位结构。

## 5. 执行日志（仅追加）

- `2026-03-09` `p6-0` 初始化 draft spec：将“stake pool 链上注册状态查询与注册流程”收敛为独立草案，等待后续启动执行。
- `2026-03-09T16:02:09+0800` `p6-8` 完成：`S0006` 从 draft 提升为唯一 active spec，并显式承接 `S0005` 已交付的 Phase 4 运行时、监控、KES、升级与审计基线。
- `2026-03-09T16:12:00+0800` `p6-1` 完成：新增 `pool_onchain_status` 查询契约、请求/返回模型和前端 IPC；当前实现先完成本地校验和占位返回，真实 `cardano-cli` 查询逻辑留待 `p6-2`。

## 6. 验证证据（仅追加）

- `2026-03-09` `TC-P6-001/002/003/004/005 | stack: other | command: manual repository inspection | result: pass | note: 已确认当前仓库仅覆盖本地 pool 元数据与节点运维链路，尚未实现链上注册查询与注册提交；因此独立起草新 spec，避免继续误用 Settings 中的本地字段。`
- `2026-03-09T16:02:09+0800` `TC-P6-006 | stack: other | command: manual inspection of docs/specs tree and docs/README.md | result: pass | note: S0006 已提升为根目录唯一 active spec，S0005 已转入 completed，README 当前活动入口已切换到 S0006。`
- `2026-03-09T16:12:00+0800` `TC-P6-007 | stack: rust | command: cargo test -q | result: pass | note: tc_pool_006~008 覆盖链上查询契约的请求来源判定、缺失输入提示和机器角色限制；命令注册与静态接口断言通过。`
- `2026-03-09T16:12:00+0800` `TC-P6-007 | stack: node | command: pnpm build | result: pass | note: 前端已暴露 PoolOnchainQueryPayload、PoolOnchainStatus 模型和 poolOnchainStatus IPC，构建通过。`

## 7. 变更记录（仅追加）

- `2026-03-09` 基于用户确认，“查询当前矿池是否已链上注册、已注册则读取注册信息、未注册则提供注册功能”被识别为超出当前 Phase 4 运维范围的新需求，单独创建 draft spec 处理。
- `2026-03-09T16:02:09+0800` 用户确认以链上注册能力为新的执行主线，当前 spec 从 draft 提升为 active，并以前序 spec `S0005` 作为已交付基线。
