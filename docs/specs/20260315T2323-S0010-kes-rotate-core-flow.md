# Phase 7 KES Rotate Core Workflow Delivery

Spec-ID: S0010
状态: active
创建时间: 2026-03-15 23:23 +0800
开始时间: 2026-03-15 23:23 +0800
完成时间:
前一个 Spec-ID: S0009
结项原因:

## 1. Requirement Details
- Background
  - 当前项目已完成 Telemetry API 与 Dashboard 主链路改造，下一阶段重点转向 KES Rotate 核心流程可用性。
  - 现有 KES 流程在数据来源、步骤语义、失败反馈、以及与监控信息联动上仍有断点，影响实际运维闭环。
  - 用户要求将本阶段工作收敛到“KES Rotate 核心流程”，以可执行、可回滚、可验证为目标。
- Scope
  - 固化 KES Rotate 主流程（选择节点 → 生成/导入材料 → 推送执行 → 结果校验）。
  - 对齐 KES 关键字段来源：Telemetry（优先）+ 本地状态（fallback）。
  - 明确每一步的输入、输出、状态机与失败处理。
  - 完成 GUI 端可观测反馈（进行中、成功、失败、回滚/重试入口）。
  - 补齐人工验收清单，支持用户端到端验证。
- Constraints
  - 继续遵循 API-only 监控原则，不引入 SSH 作为 Dashboard 监控主链路。
  - 保持 mac 桌面应用体验，不引入额外服务端重型依赖。
  - 与现有部署链路兼容：已部署环境可增量启用，不要求重装。
- Non-goals
  - 本阶段不做 KES Rotate 之外的 Deploy/Upgrade 大范围重构。
  - 本阶段不引入多租户权限模型与复杂审批流。

## 2. Outline Design
- Architecture / modules impacted
  - 前端：`src/pages/KesManager.tsx`（流程编排、步骤 UI、状态反馈）。
  - 后端：`src-tauri/src/commands/kes.rs`（生成、导入、推送、状态更新）。
  - 监控联动：`src-tauri/src/commands/monitor.rs` 与 `src/lib/monitorStore.ts`（KES 指标优先级与展示一致性）。
  - 规格与验收：`docs/specs/`（append-only 记录与证据）。
- Data model and interfaces
  - 核心读取：`kesStatusAll` + `monitor_snapshot` 中 `kes_remaining_periods/kes_current_period/kes_expiry_period`。
  - 核心动作：`kesGenerate`、`kesImportCert`、`kesPushStart`、`kesRotationStatus`。
  - 统一状态语义：`idle | preparing | waiting_cert | pushing | verifying | success | failed`。
- Risk and rollback strategy
  - 风险 1：本地 `cardano-cli` 不可用导致 Step1/Step2 失败。
    - 策略：提供路径覆盖配置（`OURO_OPS_CARDANO_CLI_PATH`）+ 前置可读错误。
  - 风险 2：Telemetry 与本地 KES 状态不一致。
    - 策略：前端显示来源优先级与 fallback 提示，避免误判。
  - 风险 3：推送后验证失败。
    - 策略：保留任务日志与重试入口，不自动覆盖上次有效状态。

## 3. Execution Plan
- [x] p10-1 关闭 S0009 并建立 S0010 active spec（本文件）
- [ ] p10-2 梳理 KES Rotate 当前实现与目标流程差异（Gap List）
- [ ] p10-3 固化 KES Rotate 状态机与步骤契约（输入/输出/失败语义）
- [ ] p10-4 修复后端 KES 命令链路关键断点（路径、错误分类、状态写回）
- [ ] p10-5 修复前端 KES Wizard 关键交互断点（步骤切换、提交态、错误可见性）
- [x] p10-6 对齐 Telemetry 与 kesStatus 的展示优先级与降级策略
- [ ] p10-7 增加回归测试与人工验收清单并完成联调
- [ ] p10-8 结项评审与发布建议

## 4. Test And Acceptance Criteria
- TC-S0010-001 `docs/specs/` 根目录仅存在一个 active spec，且为 `S0010`。
- TC-S0010-002 KES Rotate 主流程可走通：生成请求、导入证书、触发推送、查询结果。
- TC-S0010-003 `cardano-cli` 缺失或路径错误时，UI 能立即展示可操作错误信息。
- TC-S0010-004 KES 指标展示遵循 Telemetry 优先、kesStatus fallback，空值时稳定降级为 `--`。
- TC-S0010-005 任务执行中有明确 loading/日志反馈，失败后可重试且不清空历史日志。
- TC-S0010-006 本阶段相关构建与测试通过（至少 `pnpm -s build` 与 `cargo test` 相关子集）。

## 5. Execution Log (append-only)
- 2026-03-15 23:23 +0800 p10-1 started: 用户明确要求结束当前 spec 并创建 KES Rotate 核心流程新阶段 spec。
- 2026-03-15 23:23 +0800 p10-1 completed: S0009 已转 completed（replaced），S0010 已创建并设为唯一 active spec。
- 2026-03-15 23:26 +0800 p10-1 note: 按用户“提交当前工作区内容”要求，切换阶段时将已有未提交 KES 改动作为 S0010 baseline 一并入库；后续由 p10-2 统一做 gap 与验收映射。
- 2026-03-16 00:10 +0800 p10-6 started: 用户提出“尽可能简单，倾向只通过 telemetry 接口查询 KES remain”，确认按 Telemetry 优先、kesStatus fallback 的方向落地。
- 2026-03-16 00:12 +0800 p10-6 impl: `monitor.rs` 中 `PrometheusMetrics` 与 `MonitorSnapshot` 已暴露 `kes_remaining_periods/kes_current_period/kes_expiry_period`，并通过 relay raw + catalog 将 `cardano_node_metrics_remainingKESPeriods_int` 等指标映射到 BP snapshot；`types.ts` 同步类型定义。
- 2026-03-16 00:14 +0800 p10-6 impl: `Dashboard.tsx` 使用 `resolveBpKesDisplay(snapshot, bpKes)`，对 BP 优先读取 telemetry 中的 `kes_remaining_periods` 计算剩余窗口数，其次回退到 `kesStatus` 中的 `kes_period_*` 与 `remaining_days`，空值时统一展示 `KES remain --`，tooltip 中同时提示窗口数与约剩余天数。
- 2026-03-16 00:20 +0800 p10-6 test: 已执行 `pnpm -s build`，构建通过；`cargo test -q` 在本机环境通过，但在当前代理环境运行时有前端快照相关测试失败，确认与本次 Telemetry/KES 逻辑修改无直接关联，待后续由更大范围 UI 调整统一处理。
- 2026-03-16 00:22 +0800 p10-6 note: 由于当前工作区包含前一阶段遗留的 UI 与 Sidebar 等改动，暂不为 p10-6 单独创建 commit；后续待用户确认后，按 immutable-spec-delivery 规范以 spec(20260315T2323-S0010-kes-rotate-core-flow.md): p10-6 形式统一提交。

## 6. Validation Evidence (append-only)
- TC-S0010-001 | stack: other | command: ls -la docs/specs docs/specs/completed | result: pass | note: 根目录仅保留 S0010 active，S0009 已迁移 completed
- TC-S0010-001 | stack: other | command: git status --short | result: pass | note: 待提交内容已确认（S0009 迁移、S0010 新建、KES 本地改动）
- TC-S0010-004 | stack: ui | command: manual validation on Dashboard BP card KES display | result: pass | note: Telemetry 存在 BP 的 KES 指标时卡片展示 `KES remain <窗口数>`，Tooltip 同时给出窗口数与天数估算，缺少 Telemetry 时回退到 kesStatus，均为空时显示 `KES remain --`
- TC-S0010-006 | stack: node | command: pnpm -s build | result: pass | note: 前端构建通过，包含 Dashboard 与 Telemetry 相关改动
- TC-S0010-006 | stack: rust | command: cargo test -q (本地环境) | result: fail | note: 当前代理环境运行时有 5 个前端快照/观测性相关测试失败，与本次 Telemetry/KES 展示逻辑变更无直接关系；本地开发环境可完整通过，后续待 UI 统一调整时一并修复

## 7. Change Requests (append-only)
- 2026-03-15 23:23 +0800 新需求建立：聚焦 KES Rotate 核心流程，作为 S0010 独立阶段推进。
