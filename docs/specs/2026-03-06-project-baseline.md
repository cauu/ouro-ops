# Cardano Stake Pool 控制平面项目基线

状态：`superseded`  
日期：`2026-03-06`

## 1. 需求详情

- 背景
  - 该项目是一个面向 Cardano Stake Pool Operator 的 macOS 本地优先控制平面。
  - 仓库中的需求、设计、计划、审查与验证内容此前分散在 `docs/` 下的多个目录中。
  - 项目现在采用 immutable-spec 工作流，因此需要一份活动 spec 作为执行基线。
- 范围
  - 将当前项目基线收敛为一份活动 spec。
  - 保留旧文档作为历史参考，不删除也不重写。
  - 记录已完成的 Phase 1 至 Phase 3 范围，并将 Phase 4 与跨阶段事项保留为后续执行计划。
- 约束
  - 采用 local-first 的 push 模式：远程主机通过 SSH 与 Ansible 管理，不在目标机常驻 agent。
  - SQLite 不得使用外键与级联；关联清理由应用层负责。
  - 最小支持部署拓扑为 `1 relay + 1 bp`。
  - `safe_validation_mode` 必须保持只读，不得修改生产系统。
  - 默认部署镜像为 `ghcr.io/blinklabs-io/cardano-node:latest`，但允许显式 tag 与 digest 覆盖。
  - 同一时间只能存在一个活动 spec。
- 非目标
  - 不重写或删除 `docs/prd`、`docs/detail-design`、`docs/development-plan`、`docs/test-cases`、`docs/test-results`、`docs/review` 下的旧文档。
  - 不引入数据库级外键或级联语义。
  - 不支持多个活动 spec 或并行文档轨道。

## 2. 概要设计

- 架构 / 受影响模块
  - 前端：Tauri + React + TypeScript 页面，覆盖 setup、machine management、deploy，以及后续的 monitoring/KES/upgrade 流程。
  - 后端：基于 Tauri IPC 的 Rust commands、SQLite 持久化、sidecar 进程管理与事件发射。
  - 自动化：Python sidecar 对接 `ansible-runner`；Ansible playbook 与 role 负责 deploy、hardening、takeover、validation，以及未来 upgrade/KES 工作流。
- 数据模型与接口
  - 核心实体保持为 `pool`、`machine`、`kes_state`、`task`、`task_machine`、`machine_health`、`audit_log`。
  - Machine 与 pool、task 的关系由 repository 与 command 逻辑保证，而不是依赖 SQLite 外键。
  - 当前部署基线包含运行中 `cardano-node` 容器探测、迁移到 `/opt/cardano/*` 的 takeover，以及失败后切回 legacy 容器名的回滚。
- 风险与回退策略
  - 旧文档全部保留且视为参考输入。
  - 后续新工作必须先读取本 spec；若范围发生实质变化，应创建新 spec，并在 change log 中标记本 spec 不再活跃。
  - 已部署变更仍采用前向式回退，通过专门的 rollback 工作项与 `git revert` 处理，不重写历史。

## 3. 执行计划

- [x] `p0-1` 在 `docs/specs/` 下创建符合 immutable-spec 的活动 spec
- [x] `p0-2` 增加 `docs` 入口文档，指向活动 spec，并将旧文档标记为历史参考
- [ ] `p4-1` 实现 `commands/monitor.rs`：`monitor_snapshot`、`monitor_start_polling`、`monitor_stop_polling`
- [ ] `p4-2` 实现指标采集与 `healthy`、`warning`、`critical` 阈值映射
- [ ] `p4-3` 实现前端监控 store 与 Dashboard 数据流
- [ ] `p4-4` 实现 `kes_status_all`、`kes_generate`、`kes_import_cert`
- [ ] `p4-5` 增加 `ansible/playbooks/kes-push.yml`
- [ ] `p4-6` 实现 `upgrade_start`、`upgrade_confirm_next`、`upgrade_rollback`
- [ ] `p4-7` 增加 `ansible/playbooks/upgrade.yml` 与 `rollback.yml`
- [ ] `p4-8` 实现滚动升级状态机与 `upgrade:gate`
- [ ] `p4-9` 实现 `KesManager` 页面与轮换流程
- [ ] `p4-10` 实现 `UpgradeWizard` 页面与门控处理
- [ ] `p4-11` 实现 Dashboard 卡片、KES 倒计时与最近任务视图
- [ ] `p4-12` 为 BP 升级与 KES 推送补齐 ticker 确认的高危确认能力
- [ ] `p4-13` 运行并记录完整的 Phase 4 验证
- [ ] `x-1` 统一前端对 `AppError` 的处理与用户可读错误展示
- [ ] `x-2` 确保所有关键操作写入 `audit_log`

## 4. 测试与验收标准

- `TC-DOC-001` `docs/specs/` 下存在一份活动 spec，且包含 requirement details、outline design、execution plan、test and acceptance criteria、execution log、validation evidence、change log。
- `TC-DOC-002` `docs/README.md` 指向活动 spec，并清楚标识旧文档为历史参考。
- `TC-P4-001` 监控 command 与 Dashboard 流程覆盖快照、轮询、持久化和阈值判定。
- `TC-P4-002` KES 流程覆盖状态查询、密钥生成、证书导入校验与推送到 BP。
- `TC-P4-003` 升级流程覆盖 relay 逐台推进、BP 确认门控、回退参数与状态转换。
- `TC-P4-004` 前端流程覆盖 Dashboard 卡片、升级门控确认与 KES 工作流页面。
- `TC-P4-005` 安全确认与回退集成覆盖 dangerous 确认、ticker 确认与 rollback inventory 使用。

## 5. 执行日志（仅追加）

- `2026-03-06` `p0-1` 开始。审阅现有 `docs/` 结构，识别出 PRD、设计、计划、测试用例、测试结果与 review 分散在多个目录，缺少单一活动 spec。
- `2026-03-06` `p0-1` 完成。建立本项目基线 spec，并映射已完成的 Phase 1 至 Phase 3 范围，以及剩余的 Phase 4 与跨阶段工作。
- `2026-03-06` `p0-2` 开始。准备 `docs` 入口文档，用于指向活动 spec，并保留旧文档为历史参考。
- `2026-03-06` `p0-2` 完成。已增加 `docs/README.md` 作为文档入口。
- `2026-03-06` 提交已延后。当前工作区存在无关未提交改动，按 immutable-spec 工作流，本次不自动提交。

## 6. 验证证据（仅追加）

- `TC-DOC-001 | stack: other | command: manual inspection of docs/specs/2026-03-06-project-baseline.md | result: pass | note: active spec contains all required immutable-spec sections`
- `TC-DOC-002 | stack: other | command: manual inspection of docs/README.md | result: pass | note: docs entrypoint points to active spec and classifies legacy docs as historical references`

## 7. 变更记录（仅追加）

- `2026-03-06` 基线来源：`docs/prd/v1.0.md`、`docs/high-level-design/v1.0.md`、`docs/detail-design/v1.0.md`、`docs/development-plan/v1.0.md`、`docs/test-cases/v1.0.md`、`docs/test-results/phase2-v1.0.md`、`docs/test-results/phase3-v1.0.md`、`docs/review/v1.0.md`。
- `2026-03-06` 引导期范围已拆分为 `docs/specs/2026-03-06-phase1-phase3-completed.md`（已完成历史工作）与 `docs/specs/2026-03-06-phase4-active.md`（后续活动范围）。本基线记录仅为可追溯性保留。
- `2026-03-06` 本文档已退役，不再作为执行入口。后续执行只使用 `docs/specs/2026-03-06-phase4-active.md`，历史完成范围只查看 `docs/specs/2026-03-06-phase1-phase3-completed.md`。
