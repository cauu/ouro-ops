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
- [x] `p6-2` 实现 stake pool 是否已注册的链上判断与详情读取
- [ ] `p6-3` 将 Settings 中误导性的本地可编辑链上字段改为只读或迁移
- [ ] `p6-4` 设计并实现 registration 准备流程：参数校验、证书生成、交易草稿
- [ ] `p6-5` 设计并实现 registration 提交流程：签名输入、交易提交与结果回执
- [x] `p6-6` 增加前端注册状态页，支持对 `p6-2` 的链上查询能力进行可视化验证
- [ ] `p6-7` 增加高风险确认、审计记录与验收验证
- [ ] `p6-9` 增加前端注册向导，将 registration 准备与提交链路接入 UI
- [x] `p6-10` 修复链上状态查询在 docker 无直连权限场景下的错误归因，避免第一次失败的 stderr 污染最终提示
- [x] `p6-11` 修复链上注册状态查询在新 cardano-cli 下的兼容性：移除 `stake-snapshot` 依赖，并扩宽 `pool-state / pool-params` 的解析结构
- [x] `p6-12` 兼容 `cardano-cli 10.14.0.0` 的 `sps*` pool 参数结构，确保 relay 查询能返回 registered parameters
- [x] `p6-13` 当 BP 上的 `cardano-cli` 无法执行 `pool-state / pool-params` 时，自动回退到同池 relay 查询链上注册状态
- [x] `p6-14` 根据链上注册参数中的 `metadata_url` 拉取 metadata JSON，并解析其中的 `ticker`
- [x] `p6-15` 对已链上注册的 staking pool，允许用户输入 `pool_id` 完成 workspace 绑定，并将链上 pool 信息持久化到本地数据库
- [x] `p6-16` 每次访问 Dashboard 时，在后台静默刷新已绑定 pool 的最新链上数据并更新本地缓存

## 4. 测试与验收标准

- `TC-P6-001` 能根据 pool id 或注册材料判断当前 pool 是否已链上注册。
- `TC-P6-002` 若已注册，前端可展示链上真实注册信息，且不再误用本地 `ticker/margin/fixed_cost` 作为 truth source。
- `TC-P6-003` 若未注册，系统可生成 registration 证书和交易草稿，并明确缺失的签名材料。
- `TC-P6-004` 注册提交流程具备显式确认、失败可见性和审计记录。
- `TC-P6-005` 不再允许用户将仅本地可写的字段误认为链上已生效配置。
- `TC-P6-006` spec 切换后，`docs/specs/` 根目录应仅保留 `S0006` 作为 active spec，`S0005` 应进入 `completed/`，且 `docs/README.md` 入口一致。
- `TC-P6-007` 后端与前端应暴露统一的链上注册状态查询接口、请求模型与返回模型；在 `p6-2` 落地真实链上查询前，接口至少能完成本地校验并返回一致的占位结构。
- `TC-P6-008` 对已链上注册的 pool，输入 `pool_id` 后应能完成绑定，并将 `pool_id`、ticker、margin、fixed_cost、pledge、reward_account、metadata、owners、relays 持久化到本地数据库。
- `TC-P6-009` 每次访问 Dashboard 时，应 best-effort 静默刷新已绑定 pool 的链上数据；无已绑定 pool 或刷新失败时，不得阻塞 Dashboard 正常加载。

## 5. 执行日志（仅追加）

- `2026-03-09` `p6-0` 初始化 draft spec：将“stake pool 链上注册状态查询与注册流程”收敛为独立草案，等待后续启动执行。
- `2026-03-09T16:02:09+0800` `p6-8` 完成：`S0006` 从 draft 提升为唯一 active spec，并显式承接 `S0005` 已交付的 Phase 4 运行时、监控、KES、升级与审计基线。
- `2026-03-09T16:12:00+0800` `p6-1` 完成：新增 `pool_onchain_status` 查询契约、请求/返回模型和前端 IPC；当前实现先完成本地校验和占位返回，真实 `cardano-cli` 查询逻辑留待 `p6-2`。
- `2026-03-09T16:35:00+0800` `p6-2` 完成：`pool_onchain_status` 现在通过目标 `relay/bp` 机器上的运行中容器执行 `cardano-cli` 查询；优先使用显式 `pool_id`，否则从 `cold.vkey` 推导 pool id，再通过 `query stake-snapshot` 判断是否已链上注册，并用 `query pool-state / pool-params` 读取 on-chain 注册详情。
- `2026-03-09T17:05:00+0800` `p6-6` 完成：新增只读的前端注册状态页，接入 `poolOnchainStatus(...)` IPC，可按 relay/bp 机器查询链上注册状态与注册详情，先用于验证 `p6-2` 的链上查询能力，不引入交易提交或注册向导写路径。
- `2026-03-09T17:20:00+0800` `p6-10` 完成：统一修复 pool / machine / monitor 的 docker SSH 包装器，在回退 `sudo -n docker ...` 前屏蔽第一次无权限直连的 stderr，避免把真实失败原因错误格式化成“当前 SSH 用户无法直接访问 Docker daemon”。 
- `2026-03-09T17:32:00+0800` `p6-11` 完成：移除 `query stake-snapshot` 作为注册判定来源，改为直接使用 `query pool-state / pool-params` 读取注册详情并据此判定是否已链上注册；同时扩宽对嵌套 `poolParams / poolParameters / currentPoolParams` 等结构的解析，以兼容当前 `cardano-cli 10.14.0.0` 输出。 
- `2026-03-09T17:40:00+0800` `p6-12` 完成：根据真实 relay 输出，补充 `spsCost / spsMargin / spsPledge / spsRewardAccount / spsOwners / spsRelays / spsMetadata` 字段解析，并兼容 `single host name` relay 结构，使 registered on-chain 状态下能正确返回 registered parameters。 
- `2026-03-09T18:05:00+0800` `p6-13` 完成：将链上注册状态查询策略从“严格使用用户选中的机器”收敛为“优先使用选中机器；若是 BP 且本机 `cardano-cli` 对 `pool-state / pool-params` 报 era 不兼容，则自动回退到同池 relay 查询”，避免把 BP 的 era/CLI 能力差异暴露给用户。 
- `2026-03-09T18:22:00+0800` `p6-14` 完成：在链上注册参数已解析出 `metadata_url` 的前提下，额外发起 metadata JSON 拉取并提取 `ticker`；metadata 拉取为 best-effort，不影响主查询成功/失败判定。 
- `2026-03-09T20:14:27+0800` `p6-15` 完成：新增 `pool_bind_onchain` 绑定链路，允许用户用已注册的 `pool_id` 将 workspace 绑定到真实链上 pool，并把 `pool_id`、ticker、margin、fixed_cost、pledge、reward_account、metadata、owners、relays 与同步时间持久化到本地 `pool` 表。 
- `2026-03-09T20:14:27+0800` `p6-16` 完成：Dashboard 页面进入时会 best-effort 调用 `pool_refresh_bound_onchain`，在后台静默刷新已绑定 pool 的链上缓存；若尚未绑定或查询失败，则忽略错误，不影响页面加载。 

## 6. 验证证据（仅追加）

- `2026-03-09` `TC-P6-001/002/003/004/005 | stack: other | command: manual repository inspection | result: pass | note: 已确认当前仓库仅覆盖本地 pool 元数据与节点运维链路，尚未实现链上注册查询与注册提交；因此独立起草新 spec，避免继续误用 Settings 中的本地字段。`
- `2026-03-09T16:02:09+0800` `TC-P6-006 | stack: other | command: manual inspection of docs/specs tree and docs/README.md | result: pass | note: S0006 已提升为根目录唯一 active spec，S0005 已转入 completed，README 当前活动入口已切换到 S0006。`
- `2026-03-09T16:12:00+0800` `TC-P6-007 | stack: rust | command: cargo test -q | result: pass | note: tc_pool_006~008 覆盖链上查询契约的请求来源判定、缺失输入提示和机器角色限制；命令注册与静态接口断言通过。`
- `2026-03-09T16:12:00+0800` `TC-P6-007 | stack: node | command: pnpm build | result: pass | note: 前端已暴露 PoolOnchainQueryPayload、PoolOnchainStatus 模型和 poolOnchainStatus IPC，构建通过。`
- `2026-03-09T16:35:00+0800` `TC-P6-001/002 | stack: rust | command: cargo test -q | result: pass | note: tc_pool_009 覆盖已注册 pool 的链上判断与详情映射；tc_pool_010 覆盖从 cold.vkey 推导 pool id 并在 preprod 上执行真实查询命令拼装。`
- `2026-03-09T16:35:00+0800` `TC-P6-001/002 | stack: node | command: pnpm build | result: pass | note: 前端查询契约保持兼容，poolOnchainStatus 仍能消费真实 on-chain 查询结果模型。`
- `2026-03-09T17:05:00+0800` `TC-P6-002 | stack: rust | command: cargo test -q | result: pass | note: 新增前端静态断言 tc_fe_023，确认已接入 On-chain Status 路由、侧栏入口和 poolOnchainStatus 查询页面。`
- `2026-03-09T17:05:00+0800` `TC-P6-002 | stack: node | command: pnpm build | result: pass | note: PoolRegistrationStatus 页面、App 路由和 Sidebar 入口已纳入构建，前端可只读展示链上注册状态与注册详情。`
- `2026-03-09T17:20:00+0800` `TC-P6-001 | stack: rust | command: cargo test -q | result: pass | note: tc_pool_011、tc_mch_012、tc_mon_004 断言 docker SSH 包装器会屏蔽第一次无权限直连的 stderr，再回退到 sudo 执行，避免错误提示被 permission denied 污染。`
- `2026-03-09T17:32:00+0800` `TC-P6-001/002 | stack: rust | command: cargo test -q | result: pass | note: tc_pool_009、tc_pool_010 已切换到 pool-state 判定路径；新增 tc_pool_012 覆盖嵌套 poolParams 结构解析，验证在不依赖 stake-snapshot 的情况下仍可识别已注册 pool 并读取详情。`
- `2026-03-09T17:40:00+0800` `TC-P6-002 | stack: rust | command: cargo test -q | result: pass | note: 新增 tc_pool_013 覆盖 relay 真实 `pool-state / pool-params` 返回的 `sps*` 字段和 `single host name` relay 结构，验证 registered parameters 可被正确映射到前端模型。`
- `2026-03-09T18:05:00+0800` `TC-P6-001/002 | stack: rust | command: cargo test -q | result: pass | note: 新增 tc_pool_014，覆盖 BP 上 `pool-state` 因 era 不兼容报错时，会自动回退到同池 relay 查询并返回完整 registered parameters。`
- `2026-03-09T18:22:00+0800` `TC-P6-002 | stack: rust | command: cargo test -q | result: pass | note: 新增 tc_pool_015，覆盖在已解析出 `metadata_url` 后拉取 metadata JSON 并读取 `ticker` 字段，验证前端可直接看到链上 metadata 中定义的 pool ticker。`
- `2026-03-09T20:14:27+0800` `TC-P6-008/009 | stack: rust | command: cargo test -q | result: pass | note: 新增 tc_db_005 验证 pool 表 on-chain binding 字段迁移到位；新增 tc_fe_024 验证 App 已接入 pool 绑定回写与 Dashboard 的后台静默刷新入口。`
- `2026-03-09T20:14:27+0800` `TC-P6-008/009 | stack: node | command: pnpm build | result: pass | note: PoolRegistrationStatus 的绑定入口、App 的 pool 状态回写，以及 Dashboard 的静默刷新链路均已纳入构建并通过。`

## 7. 变更记录（仅追加）

- `2026-03-09` 基于用户确认，“查询当前矿池是否已链上注册、已注册则读取注册信息、未注册则提供注册功能”被识别为超出当前 Phase 4 运维范围的新需求，单独创建 draft spec 处理。
- `2026-03-09T16:02:09+0800` 用户确认以链上注册能力为新的执行主线，当前 spec 从 draft 提升为 active，并以前序 spec `S0005` 作为已交付基线。
- `2026-03-09T16:50:00+0800` 基于用户确认，将原 `p6-6` 拆分为“前端注册状态页”和“前端注册向导”两个事项，先交付状态页以便直接验证 `p6-2` 的链上查询能力。
- `2026-03-09T19:50:00+0800` 基于用户确认，增加“输入已注册 `pool_id` 后完成 workspace 绑定并持久化链上信息；Dashboard 每次访问时静默刷新最新链上数据”的需求，作为当前 active spec 的追加事项实现。
