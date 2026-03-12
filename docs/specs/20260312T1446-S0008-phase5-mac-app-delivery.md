# Phase 5 Mac App Delivery (Prototype To Product UI)

Spec-ID: S0008
状态: active
创建时间: 2026-03-12 14:46 +0800
开始时间: 2026-03-12 14:46 +0800
完成时间:
前一个 Spec-ID: S0007
结项原因:

## 1. Requirement Details
- Background
  - `S0007` 已完成 pool-centric 的多页面静态原型，信息架构、关键文案和交互细节已通过多轮修正。
  - 当前阶段目标从“静态原型”切换到“可运行的 mac 桌面应用实现”，需要将原型落入现有 Tauri + React 工程。
  - 现有能力已具备链上绑定/解绑与 Dashboard 基线（`S0006`），下一阶段应优先打通日常运维主流程（Dashboard / KES Rotate / Upgrade）并接入实时监控数据。
- Scope
  - 以 `prototype/s0007` 为准，完成 mac 应用主界面的实现对齐（视觉、布局、信息架构、交互语义）。
  - 保持 pool-centric 组织方式，移除“机器资产中心”心智，不再以机器列表作为主入口。
  - 完成 Dashboard 的生产化信息结构：
    - BP 卡片承载关键风险指标（含 KES remain）
    - 节点详情 tab（可切换、可访问、样式一致）
    - 轻量 telemetry 提示（缓存优先 + 后台静默刷新 + tooltip）
  - 实现 telemetry 数据链路：本地缓存优先展示，后台静默加载 Prometheus 最新数据，失败时降级并自动重试。
  - 接入容器内 nview 服务可提供的 Prometheus 接口（先实现查询与映射能力，再逐步扩充指标）。
  - 对齐 Deploy/KES/Upgrade 主流程关键约束：
    - Deploy Step1 使用手动节点输入，并在该步完成机器创建
    - `takeover` 默认关闭，其他两个开关默认开启
    - 初始化不使用 mithril（避免耗时）
- Constraints
  - 平台限定为 mac 桌面应用（Tauri），优先桌面交互质量与稳定性。
  - 保持 local-first，不引入破坏当前架构的外部依赖。
  - 不做历史回滚式改造，所有变更通过增量实现与可回退提交完成。
  - 不突破 `S0006` 的安全边界（冷/热环境职责、审计可追溯）。
- Non-goals
  - 不在本阶段重做链上注册主链路的业务规则。
  - 不在本阶段新增“机器资产中心”页面或恢复机器中心化 IA。
  - 不在本阶段实现全量自动化运维编排，仅交付与原型一致的主流程能力与必要数据闭环。

## 2. Outline Design
- Architecture / modules impacted
  - 前端页面：`Dashboard`、`Deploy`、`KES Rotate`、`Upgrade` 相关页面与组件。
  - 前端状态层：监控数据 store（缓存态/刷新态/失败态）与页面消费协议。
  - 后端 commands：Prometheus 查询聚合命令、必要的 Deploy Step1 机器创建接口编排。
  - 审计链路：关键操作继续写入审计日志，保证可追踪性。
- Data model and interfaces
  - telemetry 状态模型最少包含：
    - `cache_ready`（已渲染本地缓存）
    - `syncing_live`（后台静默刷新）
    - `degraded_retrying`（拉取失败，显示缓存并等待下一轮轮询）
  - Prometheus 读取接口先聚焦 Dashboard 所需核心指标（当前原型字段集合），采用可扩展映射结构，避免硬编码散落在组件层。
  - Dashboard KPI 以 BP 卡片和节点详情为 primary source，减少重复指标区。
- Risk and rollback strategy
  - 风险 1：Prometheus 指标口径与页面字段不一致。
    - 策略：先做“字段映射表 + 兜底空值策略 + 数据戳”并记录缺口。
  - 风险 2：改造 IA 影响既有用户路径。
    - 策略：逐页替换并保留最小可回退提交粒度，避免一次性大改。
  - 风险 3：Deploy/KES/Upgrade 改造引入流程中断。
    - 策略：分流程独立验收与回归，每个流程单独可运行后再合并入口。

## 3. Execution Plan
- [x] p8-1 启动 S0008 active spec，完成阶段目标与验收基线冻结
- [ ] p8-2 落地 pool-centric IA：收敛导航与入口，移除机器中心化主流程暴露
- [ ] p8-3 实现 Dashboard 结构对齐：BP 卡片主承载、节点详情 tab、tooltip/标签体系统一
- [ ] p8-4 实现 telemetry 三态体验：缓存优先、后台静默刷新、失败降级重试
- [ ] p8-5 接入 Prometheus 查询能力并完成 Dashboard 指标映射（含时间戳与空值兜底）
- [ ] p8-6 实现 Deploy Step1 手动节点输入 + 机器创建，落实默认开关策略与无 mithril 初始化约束
- [ ] p8-7 实现 KES Rotate 向导生产化页面与关键风险闸门（含 KES remain 关联操作）
- [ ] p8-8 实现 Upgrade 向导生产化页面与 BP gate / rollback 提示
- [ ] p8-9 打通审计与回归：关键操作日志、错误提示一致性、文案与状态对齐
- [ ] p8-10 完成 mac 桌面场景验收与回归测试，准备阶段结项评审

## 4. Test And Acceptance Criteria
- TC-P8-001 `docs/specs/` 根目录仅保留 `S0008` 作为 active spec，`S0007` 进入 completed，文档入口一致。
- TC-P8-002 应用主流程信息架构符合 pool-centric；不再以机器中心页面作为主入口。
- TC-P8-003 Dashboard 关键指标布局与 S0007 对齐：顶部不重复堆叠，BP 卡片承载关键风险，KES remain 与 Rotate 操作并置。
- TC-P8-004 节点详情 tab 满足功能与样式一致性：可切换、等宽、文本垂直居中、容器不过度占宽。
- TC-P8-005 telemetry 三态可见且不扰动主流程：缓存优先、静默刷新、失败降级并自动重试。
- TC-P8-006 Prometheus 接口可稳定返回 Dashboard 所需字段；字段缺失时有可见兜底而不致页面崩溃。
- TC-P8-007 Deploy Step1 支持手动输入节点并在该步完成机器创建；默认策略符合已确认约束（takeover off，其他两个 on）。
- TC-P8-008 KES Rotate 与 Upgrade 流程具备可执行向导结构、关键风险提示与可追溯审计。
- TC-P8-009 在 mac 桌面窗口（常见宽度）下主流程可用、无明显布局破坏或关键遮挡。

## 5. Execution Log (append-only)
- 2026-03-12 14:46 +0800 p8-1 started: 基于 S0007 结项结果启动下一阶段交付规划。
- 2026-03-12 14:46 +0800 p8-1 completed: 创建 S0008 active spec，并冻结 Phase 5 执行计划与验收标准。

## 6. Validation Evidence (append-only)
- TC-P8-001 | stack: other | command: ls docs/specs && ls docs/specs/completed | result: pass | note: S0008 为根目录唯一 active spec，S0007 已归档 completed
- TC-P8-001 | stack: other | command: manual review of docs/README.md | result: pass | note: active/completed 入口与目录状态一致

## 7. Change Requests (append-only)
- 2026-03-12 14:46 +0800 新阶段需求：结束 S0007，并基于其成果制定下一阶段（mac 应用实现）最合理执行规划。
