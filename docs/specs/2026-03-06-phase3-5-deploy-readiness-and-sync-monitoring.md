# Cardano Stake Pool 控制平面 Phase 3.5 部署就绪与同步监控规范

状态：`active`  
日期：`2026-03-06`

## 1. 需求详情

- 背景
  - 当前部署链路会在容器启动成功后持续等待 `syncProgress == 100.00`，这会把“部署成功”和“完全同步完成”绑定在一起。
  - 对于未迁移数据库的新节点，这会导致部署任务长时间阻塞，即使节点已经成功启动并开始同步。
  - 同步过程目前缺少“同步速度”维度的可视化与监控，无法判断节点是否正常追块、是否卡住、何时能完成同步。
- 范围
  - 调整部署完成判定：节点成功启动、socket 可用、`query tip` 可返回并进入同步状态时，即视为部署成功。
  - 将“完全同步到 100%”从部署成功条件中移出，改由监控链路持续观察。
  - 增加同步速度监控，包括同步进度、区块高度增长速度、趋势与基础状态展示。
  - 为后续 Phase 4 的完整监控体系打基础，但本 spec 只聚焦部署完成判定与同步速度监控。
- 约束
  - 不回退已完成的 Phase 1 至 Phase 3 基础能力。
  - 继续沿用 local-first 架构：Tauri + Rust + SQLite + Python sidecar + Ansible。
  - SQLite 不引入外键与级联。
  - 同一时间只能有一个活动 spec。
- 非目标
  - 不在本 spec 中实现完整的 KES 生命周期管理。
  - 不在本 spec 中实现完整的升级/回退工作流。
  - 不改写历史完成归档或旧参考文档。

## 2. 概要设计

- 架构 / 受影响模块
  - `ansible/roles/cardano-node`：调整部署完成判定，不再等待 `syncProgress == 100.00`。
  - `src-tauri/src/commands/deploy.rs` 与任务状态更新链路：将“已进入同步”视为成功完成的可接受条件。
  - `src-tauri/src/commands/monitor.rs` 或等效监控入口：补充同步速度与同步状态采集。
  - 前端 Dashboard 或部署相关页面：展示同步进度与同步速度。
- 数据模型与接口
  - 可复用 `machine_health` 记录同步进度、区块高度、采样时间与推导出的同步速度。
  - 部署成功判定至少要求：容器启动成功、`/ipc/node.socket` 可用、`cardano-cli query tip` 返回有效 JSON。
  - 同步速度建议按连续两次采样的区块高度差 / 时间差计算，并附带 `syncProgress` 变化。
- 风险与回退策略
  - 若部署成功条件放宽，必须同步增加“节点进入同步态”的检查，避免把异常启动误判为成功。
  - 若同步速度监控实现不稳定，允许先记录原始采样值，再逐步引入速率计算。

## 3. 执行计划

- [x] `p35-1` 调整 `cardano-node` 部署完成判定，使“容器启动 + socket 可用 + query tip 成功”即可完成部署
- [x] `p35-2` 在部署链路中记录节点已进入同步状态的初始观测值
- [x] `p35-3` 扩展监控采集，记录 `syncProgress`、区块高度与采样时间
- [x] `p35-4` 增加同步速度计算逻辑，输出基础速率指标与异常判断依据
- [x] `p35-5` 在前端展示同步速度、同步进度与基础同步状态
- [x] `p35-6` 为部署完成判定和同步监控补充自动化或脚本化验证

## 4. 测试与验收标准

- `TC-P35-001` 未迁移数据库时，节点从零同步，部署任务在节点成功启动并可返回 `query tip` 后可完成，不再阻塞到 `100.00`。
- `TC-P35-002` 已迁移数据库时，部署任务在节点恢复运行并进入同步态后可完成。
- `TC-P35-003` 监控链路可记录并展示 `syncProgress`、区块高度与采样时间。
- `TC-P35-004` 系统可计算并展示同步速度，至少包含一个稳定可读的速率指标。
- `TC-P35-005` 若节点已启动但同步无进展，监控侧能暴露“速度异常或停滞”的基础判断信息。

## 5. 执行日志（仅追加）

- `2026-03-06` 新增本 spec，用于插入在 Phase 3 与 Phase 4 之间的过渡工作。
- `2026-03-06` 需求来源：实际部署过程中发现“等待同步到 100%”使部署任务阻塞过久，且缺少同步速度监控。
- `2026-03-06` 当前工作区存在其他未提交改动，因此本次 spec 文档变更不自动提交。
- `2026-03-06` `p35-1` 完成：Ansible 部署完成条件从“等待 `syncProgress == 100.00`”改为“socket 可用且 `cardano-cli query tip` 成功返回”。
- `2026-03-06` `p35-2` 完成：部署链路在首个 `query tip` 成功后记录 `block`、`syncProgress`、`era`、`hash` 作为初始同步观测值。
- `2026-03-06` `p35-3` 完成：新增 Tauri `monitor_snapshot` 命令，按机器采集 `query tip`，落库到 `machine_health`。
- `2026-03-06` `p35-4` 完成：基于连续采样的区块高度差与时间差计算 `blocks_per_minute`，并增加 5 分钟无进展的停滞判断。
- `2026-03-06` `p35-5` 完成：Dashboard 增加同步监控区域，展示同步进度、块高、速度、状态与异常说明。
- `2026-03-06` `p35-6` 完成：补充 Rust 单测、前端静态断言、Ansible 语法检查与前端构建验证。

## 6. 验证证据（仅追加）

- `2026-03-06` 暂无实现级验证证据。本 spec 创建完成后，后续将按 `TC-P35-*` 逐项追加。
- `2026-03-06` `TC-P35-001 | stack: rust | command: cargo test -q | result: pass | note: tc_dep_006 断言 playbook 已去除对 syncProgress == 100.00 的阻塞等待，仅保留首个 query tip 成功条件`
- `2026-03-06` `TC-P35-002 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: 部署链路语法有效，首个 tip 观测记录步骤已纳入 playbook`
- `2026-03-06` `TC-P35-003 | stack: rust | command: cargo test -q | result: pass | note: tc_mon_001 覆盖 tip 解析；monitor_snapshot 将 syncProgress、block_height 与采样时间写入 machine_health`
- `2026-03-06` `TC-P35-004 | stack: rust | command: cargo test -q | result: pass | note: tc_mon_002 覆盖 blocks_per_minute 计算；Dashboard 展示 Blocks/min`
- `2026-03-06` `TC-P35-005 | stack: rust | command: cargo test -q | result: pass | note: tc_mon_003 覆盖 5 分钟无块高增长的 stalled 判断；Dashboard 展示 stalled/unreachable 状态`
- `2026-03-06` `TC-P35-003 | stack: node | command: pnpm build | result: pass | note: Dashboard 同步监控 UI 编译通过`

## 7. 变更记录（仅追加）

- `2026-03-06` 本 spec 插入在已完成的 Phase 1 至 Phase 3 归档与下一阶段 Phase 4 草稿之间，作为新的唯一活动 spec。
