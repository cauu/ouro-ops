# Cardano Stake Pool 控制平面 Phase 4 活动规范

状态：`active`  
日期：`2026-03-06`

## 1. 需求详情

- 背景
  - Phase 1 至 Phase 3 的基础能力已经完成，并已单独归档。
  - 当前活动范围是 Phase 4 及跨阶段质量项，用于补齐监控、KES、升级、回退和错误处理，使产品达到更完整的可用状态。
- 范围
  - 监控快照与轮询。
  - KES 状态、生成、导入与推送流程。
  - 滚动升级、门控确认与回退。
  - Dashboard、KES、升级相关前端体验。
  - 错误处理与审计覆盖等跨阶段能力。
- 约束
  - 继续沿用既有 local-first 架构：Tauri + Rust + SQLite + Python sidecar + Ansible。
  - 保持 Phase 3 已交付的部署基线，包括 blinklabs 默认镜像、takeover 与 safe validation mode。
  - 关联关系继续由应用层负责，不引入 SQLite 外键与级联。
  - 同一时间只能有一个活动 spec。
- 非目标
  - 不在本 spec 中重新打开已归档的 Phase 1 至 Phase 3 范围。
  - 不改写已有历史参考文档。

## 2. 概要设计

- 架构 / 受影响模块
  - 后端 commands：`monitor`、`kes`、`upgrade`。
  - 前端页面与 store：`Dashboard`、`KesManager`、`UpgradeWizard`、监控状态管理。
  - Ansible playbook：`upgrade.yml`、`rollback.yml`、`kes-push.yml`。
- 数据模型与接口
  - `machine_health` 作为周期性监控快照的主要落库位置。
  - `kes_state` 作为 KES 状态与过期信息的主来源。
  - 升级流程在任务状态机中加入 relay 逐台推进、BP 门控与回退元数据。
- 风险与回退策略
  - 升级与 KES 流程中，所有影响 BP 的动作都必须经过显式确认。
  - 回退仍然采用前向变更模式，依赖 rollback playbook 与验证证据。

## 3. 执行计划

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

- `TC-P4-001` 监控 command 与 Dashboard 流程覆盖快照、轮询、持久化和阈值判定。
- `TC-P4-002` KES 流程覆盖状态查询、密钥生成、证书导入校验与推送到 BP。
- `TC-P4-003` 升级流程覆盖 relay 逐台推进、BP 确认门控、回退参数与状态转换。
- `TC-P4-004` 前端流程覆盖 Dashboard 卡片、升级门控确认与 KES 工作流页面。
- `TC-P4-005` 安全确认与回退集成覆盖 dangerous 确认、ticker 确认与 rollback inventory 使用。

## 5. 执行日志（仅追加）

- `2026-03-06` 从 `docs/development-plan/v1.0.md` 与 `docs/detail-design/v1.0.md` 中提取剩余范围，初始化本 Phase 4 活动 spec。
- `2026-03-06` 当前尚未在本 spec 下启动任何实现项。

## 6. 验证证据（仅追加）

- `2026-03-06` 暂无验证证据。后续会在 Phase 4 各事项实现并验证后逐条追加。

## 7. 变更记录（仅追加）

- `2026-03-06` 当前活动范围已从早期文档收敛记录中拆分出来，后续仅追踪已归档 Phase 1 至 Phase 3 之后的前向工作。
- `2026-03-06` 因新增“部署成功判定解耦与同步速度监控”中间范围，当前文档降为 `draft`。新的执行入口为 `docs/specs/2026-03-06-phase3-5-deploy-readiness-and-sync-monitoring.md`。
- `2026-03-08` 用户确认 Phase 3.6 结项后，本 spec 恢复为唯一活动 spec，继续承接 Phase 4 与跨阶段质量项。
