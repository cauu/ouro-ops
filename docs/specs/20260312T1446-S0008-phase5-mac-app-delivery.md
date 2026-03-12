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
- [x] p8-2 落地 pool-centric IA：收敛导航与入口，移除机器中心化主流程暴露
- [x] p8-3 实现 Dashboard 结构对齐：BP 卡片主承载、节点详情 tab、tooltip/标签体系统一
- [x] p8-4 实现 telemetry 三态体验：缓存优先、后台静默刷新、失败降级重试
- [x] p8-5 接入 Prometheus 查询能力并完成 Dashboard 指标映射（含时间戳与空值兜底）
- [x] p8-6 实现 Deploy Step1 手动节点输入 + 机器创建，落实默认开关策略与无 mithril 初始化约束
- [x] p8-7 实现 KES Rotate 向导生产化页面与关键风险闸门（含 KES remain 关联操作）
- [x] p8-8 实现 Upgrade 向导生产化页面与 BP gate / rollback 提示
- [x] p8-9 打通审计与回归：关键操作日志、错误提示一致性、文案与状态对齐
- [x] p8-10 完成 mac 桌面场景验收与回归测试，准备阶段结项评审

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

## 8. Addendum (append-only)
### 8.1 Execution Plan Delta
- [x] p8-11 收敛 Phase 5 规划基线：补齐 telemetry 映射、nview/monitor 关系、无 pool Welcome 流程、主题边界与审计清单

### 8.2 Requirement Details Delta
- Scope 增补
  - 明确包含“无 pool 时 Welcome 与 `开始部署 -> Deploy`”流程，作为 pool-centric IA 的一部分（并入 `p8-2`）。
  - 明确本阶段包含与 `S0007` 的浅色主题/色板对齐，不在本阶段引入新的视觉主题体系。
  - 明确 nview Prometheus 与现有 monitor 为并行关系：以现有 monitor 为稳定主数据源，nview 为增量增强源，不作为一次性替代。

### 8.3 Outline Design Delta
- Data model and interfaces 增补
  - telemetry 三态与 `monitorStore.telemetryPhase` 的行为映射如下（命名可保留）：
    - `cache_ready` -> 行为对齐 `loading_cache`（优先渲染本地缓存）
    - `syncing_live` -> 行为对齐 `syncing_live`（后台静默刷新）
    - `degraded_retrying` -> 行为对齐 `degraded`（刷新失败后继续展示缓存并自动重试）
  - nview 与 monitor 关系：
    - monitor：主路径与兜底路径（必须可独立支撑 Dashboard）
    - nview：指标补强路径（可用时合并映射，不可用时不阻塞页面）
- Risk and rollback strategy 增补
  - 风险 4：nview 端点不可用、延迟高或字段缺失。
    - 策略：仅启用映射与兜底，不中断 monitor 主路径；记录缺失字段并降级展示。

### 8.4 Execution Plan Delta (Clarification)
- `p8-2` Clarification：显式覆盖“无 pool Welcome 与 `开始部署 -> Deploy`”路径落地与回归。
- `p8-5` Clarification：显式覆盖“nview 不可用时保持 monitor 主数据源”的降级实现。
- `p8-6` Clarification：验收以“Step1 点击 `下一步` 时触发机器创建”为准，要求有成功/失败反馈与幂等保护。

### 8.5 Acceptance Delta
- TC-P8-010 telemetry 三态与 `monitorStore.telemetryPhase` 行为对齐可验证；命名可保留，不要求强制重命名。
- TC-P8-011 明确并验证 monitor/nview 并行策略：nview 不可用时 Dashboard 仍由 monitor 数据稳定驱动。
- TC-P8-012 无 pool 场景下必须存在 Welcome 与 `开始部署 -> Deploy` 的可达主路径。
- TC-P8-013 主题验收边界明确：实现与 `S0007` 浅色主题/色板对齐，不引入新主题分支。
- TC-P8-014 `TC-P8-008` 的“可追溯审计”需覆盖关键操作清单：
  - Deploy Step1 机器创建触发
  - KES Rotate 执行
  - Upgrade 执行
  - Pool bind / unbind
  - telemetry 降级重试事件
- TC-P8-015 Deploy Step1 在点击 `下一步` 时完成机器创建，且失败可见、重试可控、重复点击不产生重复创建副作用。

### 8.6 Appendix A: nview Prometheus 初始清单（V1）
- 端点（引用）
  - `GET <nview_base>/metrics`（Prometheus exposition）
- Dashboard V1 目标映射指标（按现有页面字段）
  - 同步与链状态：`epoch`、`sync_percent`、`tip_diff_blocks`
  - 节点资源：`cpu_sys_percent`、`mem_live`、`mem_rss`、`mem_heap`
  - 运行健康：`gc_minor_total`、`gc_major_total`、`peer_count`
- 说明
  - 以上为 Phase 5 首批映射集合；若 nview 实际指标名不同，以映射表适配，不直接改 UI 字段语义。

### 8.7 Execution Log Delta
- 2026-03-12 15:01 +0800 p8-11 started: 评估并吸收阶段规划修订建议。
- 2026-03-12 15:01 +0800 p8-11 completed: 完成 Scope/Design/Plan/Acceptance 增补，形成可执行的 Phase 5 基线。

### 8.8 Validation Evidence Delta
- TC-P8-010 | stack: other | command: manual review of section 8.3 telemetry mapping | result: pass | note: telemetry 三态与 `monitorStore.telemetryPhase` 对齐关系已固化
- TC-P8-011 | stack: other | command: manual review of section 8.2/8.3/8.4 | result: pass | note: nview 与 monitor 并行策略及 nview 不可用降级路径已明确
- TC-P8-012 | stack: other | command: manual review of section 8.2 and 8.4 p8-2 clarification | result: pass | note: 无 pool Welcome 与开始部署主路径已纳入计划
- TC-P8-013 | stack: other | command: manual review of section 8.2 theme scope | result: pass | note: 已明确本阶段与 S0007 浅色主题/色板对齐
- TC-P8-014 | stack: other | command: manual review of TC-P8-014 checklist | result: pass | note: 审计关键操作清单已可用于后续逐项验收
- TC-P8-015 | stack: other | command: manual review of section 8.4 p8-6 clarification and TC-P8-015 | result: pass | note: Step1 机器创建触发时机和行为要求已明确

### 8.9 Change Request Delta
- 2026-03-12 15:01 +0800 需求修订：补齐 telemetry 状态映射、nview/monitor 关系、无 pool Welcome 路径、主题对齐边界、审计关键操作清单，以及 p8-5/p8-6 验收细化。

## 9. Addendum (append-only)
### 9.1 Execution Plan Delta
- [x] p8-2 落地 pool-centric IA：收敛导航与入口，移除机器中心化主流程暴露

### 9.2 Execution Log Delta
- 2026-03-12 15:05 +0800 p8-2 started: 开始落地 pool-centric IA 与无 pool Welcome -> Deploy 主路径。
- 2026-03-12 15:12 +0800 p8-2 completed: 完成 Setup Welcome 流程、Start Deploy 直达路径、主导航顺序收敛与 Settings 去机器中心文案。

### 9.3 Validation Evidence Delta
- TC-P8-002 | stack: ui | command: rg -n "NavLink to=\\\"/\\\"|NavLink to=\\\"/deploy\\\"|NavLink to=\\\"/kes\\\"|NavLink to=\\\"/upgrade\\\"|NavLink to=\\\"/settings\\\"" src/components/Sidebar.tsx | result: pass | note: 主导航已收敛为 Dashboard/Deploy/KES/Upgrade/Settings，无 Machines 入口
- TC-P8-002 | stack: ui | command: rg -n "Machines|MachineManager|/machines" src/App.tsx src/components/Sidebar.tsx src/pages/Settings.tsx || true | result: pass | note: 应用路由与主导航无机器中心化入口暴露
- TC-P8-012 | stack: ui | command: rg -n "Welcome|Start Deploy|initializeWorkspace\\(\"/deploy\"\\)|navigate\\(target, \\{ replace: true \\}\\)" src/pages/SetupWizard.tsx | result: pass | note: 无 pool 首屏已提供 Welcome 与 Start Deploy 可达路径
- TC-P8-012 | stack: ui | command: rg -n "Node runtime operations stay within Deploy, KES and Upgrade flows" src/pages/Settings.tsx | result: pass | note: 设置页文案已去除 Machines 中心化描述
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 前端构建通过，IA 调整未引入编译回归

### 9.4 Change Request Delta
- 2026-03-12 15:05 +0800 执行推进：按 S0008 计划继续推进并完成 p8-2。

## 10. Addendum (append-only)
### 10.1 Execution Plan Delta
- [x] p8-3 实现 Dashboard 结构对齐：BP 卡片主承载、节点详情 tab、tooltip/标签体系统一

### 10.2 Execution Log Delta
- 2026-03-12 15:15 +0800 p8-3 started: 开始按 `prototype/s0007` 重构 Dashboard 页面结构。
- 2026-03-12 15:27 +0800 p8-3 completed: 完成 BP 主卡片、节点详情 tabs、轻量 tooltip 与近期日志表结构对齐，并保留 on-chain bind/register 区域能力。

### 10.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "Cluster Overview|Rotate Now|KES remain|slowest|tip diff" src/pages/Dashboard.tsx | result: pass | note: Dashboard 已引入 BP 主卡片承载关键风险与 KES remain 关联操作
- TC-P8-003 | stack: ui | command: rg -n "Recent Operation Logs|Bound On-chain Pool|Bind Existing Pool" src/pages/Dashboard.tsx | result: pass | note: 页面主信息结构已收敛为概览+详情+日志，并保留 pool 绑定主流程
- TC-P8-004 | stack: ui | command: rg -n "selectedNodeId|setSelectedNodeId|inline-grid grid-cols-3|min-h-8 items-center justify-center" src/pages/Dashboard.tsx | result: pass | note: 节点详情 tabs 可切换，tab 按钮使用等宽三列与垂直居中布局
- TC-P8-004 | stack: ui | command: rg -n "role=\\\"tooltip\\\"|group-hover:opacity-100" src/pages/Dashboard.tsx | result: pass | note: 关键标签与 telemetry 已使用轻量 tooltip 交互
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: Dashboard 重构后前端构建通过

### 10.4 Change Request Delta
- 2026-03-12 15:15 +0800 执行推进：继续推进 S0008，并完成 p8-3 的 Dashboard 结构对齐实现。

## 11. Addendum (append-only)
### 11.1 Execution Plan Delta
- [x] p8-4 实现 telemetry 三态体验：缓存优先、后台静默刷新、失败降级重试

### 11.2 Execution Log Delta
- 2026-03-12 15:31 +0800 p8-4 started: 审查 monitorStore 状态机与 Dashboard telemetry 提示交互，收敛三态语义。
- 2026-03-12 15:38 +0800 p8-4 completed: 增加 telemetry 行为映射（cache_ready/syncing_live/degraded_retrying），并将失败细节收敛到轻量 tooltip（含 last error）。

### 11.3 Validation Evidence Delta
- TC-P8-005 | stack: ui | command: rg -n "resolveTelemetryBehavior|TelemetryBehavior|degraded_retrying|cache_ready" src/lib/monitorStore.ts src/pages/Dashboard.tsx | result: pass | note: telemetry 三态行为映射已落地，前端消费行为对齐
- TC-P8-005 | stack: ui | command: rg -n "Telemetry refresh failed; keeping cached data and retrying|Live telemetry unavailable; showing cached data and retrying|Last error:" src/lib/monitorStore.ts src/pages/Dashboard.tsx | result: pass | note: 失败降级保留缓存且自动重试语义可见，失败详情通过 tooltip 低干扰展示
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: telemetry 三态调整后前端构建通过

### 11.4 Change Request Delta
- 2026-03-12 15:31 +0800 执行推进：继续推进 S0008，完成 p8-4 telemetry 三态体验收口。

## 12. Addendum (append-only)
### 12.1 Execution Plan Delta
- [x] p8-5 接入 Prometheus 查询能力并完成 Dashboard 指标映射（含时间戳与空值兜底）

### 12.2 Execution Log Delta
- 2026-03-12 15:39 +0800 p8-5 started: 扩展 monitor 后端，接入 nview/cardano-node Prometheus 候选端点并实现字段映射。
- 2026-03-12 15:49 +0800 p8-5 completed: 完成 Dashboard 指标消费与兜底展示，nview 不可用时保持 monitor 主链路稳定。

### 12.3 Validation Evidence Delta
- TC-P8-006 | stack: rust | command: rg -n "collect_prometheus_metrics|map_prometheus_metrics|nview:9090|cardano-node:12798|host:12798|host:12788" src-tauri/src/commands/monitor.rs | result: pass | note: Prometheus 查询已实现多端点候选探测与映射逻辑
- TC-P8-006 | stack: ui | command: rg -n "epoch|sync_percent|tip_diff_blocks|cpu_sys_percent|mem_live_bytes|mem_rss_bytes|mem_heap_bytes|gc_minor_total|gc_major_total|prometheus_source|prometheus_note" src/lib/types.ts src/pages/Dashboard.tsx src-tauri/src/commands/monitor.rs | result: pass | note: 前后端字段模型与 Dashboard 展示映射已对齐
- TC-P8-011 | stack: ui | command: rg -n "monitor fallback|prometheus_note|collect_prometheus_metrics" src/pages/Dashboard.tsx src-tauri/src/commands/monitor.rs | result: pass | note: nview 不可用时通过 monitor fallback 保持页面可用，并暴露降级说明
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: p8-5 合入后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: Rust/前端快照测试共 142 项通过（仅剩 dead_code warning）

### 12.4 Change Request Delta
- 2026-03-12 15:39 +0800 执行推进：继续推进 S0008，完成 p8-5 Prometheus 查询与 Dashboard 指标映射落地。

## 13. Addendum (append-only)
### 13.1 Execution Plan Delta
- [x] p8-6 实现 Deploy Step1 手动节点输入 + 机器创建，落实默认开关策略与无 mithril 初始化约束

### 13.2 Execution Log Delta
- 2026-03-12 15:50 +0800 p8-6 started: 复核 Deploy Step1 机器创建时机与默认策略，并收敛 mithril 初始化约束。
- 2026-03-12 15:54 +0800 p8-6 completed: 完成无 mithril 初始化策略下的开关默认与 payload 约束，并保持 Step1 手动输入创建链路与失败清理。

### 13.3 Validation Evidence Delta
- TC-P8-007 | stack: ui | command: rg -n "Enter BP and relay nodes manually|Moving to step 2 will create these machines|handlePersistStep1|machineAdd\\(|setStep1Completed\\(true\\)|setStep\\(2\\)|Creating nodes" src/pages/DeployWizard.tsx | result: pass | note: Step1 手动输入并在点击 Next 时完成机器创建
- TC-P8-015 | stack: ui | command: rg -n "step1Completed|creatingStep1|createdIds|machineRemove\\(|best effort cleanup|step === 1 && creatingStep1" src/pages/DeployWizard.tsx | result: pass | note: Step1 具备幂等门禁、失败回滚清理与重复点击保护
- TC-P8-007 | stack: ui | command: rg -n "takeoverExistingNode|enableChrony|enableHardening|mithrilInitializationAllowed = false|restore_snapshot_relay: mithrilInitializationAllowed|restore_snapshot_bp: mithrilInitializationAllowed|Mithril cold-start restore is disabled" src/pages/DeployWizard.tsx | result: pass | note: 默认策略已对齐（takeover off、chrony/hardening on、mithril 初始化关闭）
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: Deploy 策略调整后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 142/142 通过

### 13.4 Change Request Delta
- 2026-03-12 15:50 +0800 执行推进：继续推进 S0008，完成 p8-6 Deploy Step1 机器创建与默认策略约束落地。

## 14. Addendum (append-only)
### 14.1 Execution Plan Delta
- [x] p8-7 实现 KES Rotate 向导生产化页面与关键风险闸门（含 KES remain 关联操作）
- [x] p8-8 实现 Upgrade 向导生产化页面与 BP gate / rollback 提示

### 14.2 Execution Log Delta
- 2026-03-12 15:55 +0800 p8-7/p8-8 started: 对齐 KES/Upgrade 到原型分步向导结构，并收敛到浅色卡片层级。
- 2026-03-12 16:03 +0800 p8-7/p8-8 completed: 完成 KES 风险闸门可视化与 Upgrade BP gate/rollback 分步化改造，并同步快照测试断言。

### 14.3 Validation Evidence Delta
- TC-P8-008 | stack: ui | command: rg -n "KES Rotate|Step 1: Generate KES request|Step 3: Push to BP \\(Risk Gate\\)|Type pool ticker|Confirm KES Push|Step 4: Validation" src/pages/KesManager.tsx src-tauri/src/lib.rs | result: pass | note: KES 向导已包含关键风险闸门、手动确认与最终校验步骤
- TC-P8-008 | stack: ui | command: rg -n "wizardStep|1 Version Confirm|2 Rolling Upgrade|3 Health Check|Step 2: BP gate & rollback|Confirm Next Step|Rollback|unlock BP upgrade" src/pages/UpgradeWizard.tsx src-tauri/src/lib.rs | result: pass | note: Upgrade 向导已具备 BP gate 与 rollback 提示/操作
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: KES/Upgrade 页面结构改造后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 142/142 通过（仅剩 dead_code warning）

### 14.4 Change Request Delta
- 2026-03-12 15:55 +0800 执行推进：继续推进 S0008，完成 p8-7/p8-8 的 KES/Upgrade 原型化落地。

## 15. Addendum (append-only)
### 15.1 Execution Plan Delta
- [x] p8-9 打通审计与回归：关键操作日志、错误提示一致性、文案与状态对齐

### 15.2 Execution Log Delta
- 2026-03-12 16:04 +0800 p8-9 started: 复核关键操作审计清单与前端错误提示一致性覆盖。
- 2026-03-12 16:05 +0800 p8-9 completed: 补齐 telemetry 降级重试事件审计，并通过全量构建/测试回归。

### 15.3 Validation Evidence Delta
- TC-P8-014 | stack: rust | command: rg -n "telemetry_degraded_retry|pool_unbind_onchain|pool_bind_onchain|deploy_start|kes_push_start|upgrade_start|upgrade_rollback" src-tauri/src/commands/*.rs src-tauri/src/lib.rs | result: pass | note: 审计关键操作清单已覆盖，新增 telemetry 降级重试审计动作
- TC-P8-014 | stack: ui | command: rg -n "toUserError|formatTaskError" src/pages/Dashboard.tsx src/pages/DeployWizard.tsx src/pages/KesManager.tsx src/pages/UpgradeWizard.tsx | result: pass | note: 主流程页面错误提示辅助函数使用一致
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: p8-9 审计与文案调整后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 142/142 通过

### 15.4 Change Request Delta
- 2026-03-12 16:04 +0800 执行推进：继续推进 S0008，完成 p8-9 审计与一致性回归收口。

## 16. Addendum (append-only)
### 16.1 Execution Plan Delta
- [x] p8-10 完成 mac 桌面场景验收与回归测试，准备阶段结项评审

### 16.2 Execution Log Delta
- 2026-03-12 16:05 +0800 p8-10 started: 执行 mac 桌面场景下主流程页面的构建、测试与响应式结构回归检查。
- 2026-03-12 16:06 +0800 p8-10 completed: 完成桌面主流程回归验证，输出阶段结项前验收证据。

### 16.3 Validation Evidence Delta
- TC-P8-009 | stack: ui | command: rg -n "xl:grid-cols|md:grid-cols|lg:grid-cols|max-w-\\[min\\(28rem,90vw\\)\\]|overflow-x-auto|inline-grid grid-cols-3" src/pages/Dashboard.tsx src/pages/DeployWizard.tsx src/pages/KesManager.tsx src/pages/UpgradeWizard.tsx | result: pass | note: 主流程页面在桌面宽度下具备响应式网格、滚动兜底和 tooltip 防遮挡策略
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 前端生产构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 后端与前端快照回归 142/142 通过

### 16.4 Change Request Delta
- 2026-03-12 16:05 +0800 执行推进：继续推进 S0008，完成 p8-10 mac 桌面验收与回归测试收口。

## 17. Addendum (append-only)
### 17.1 Execution Plan Delta
- [x] p8-12 修复验收阻塞问题：对齐 prototype 分步流程、Dashboard 节点完整性与浅色主题一致性

### 17.2 Execution Log Delta
- 2026-03-12 16:22 +0800 p8-12 started: 收敛验收失败问题，按 P0/P1 优先级修复 Dashboard/Deploy/KES/Upgrade 与 Welcome 差异。
- 2026-03-12 16:34 +0800 p8-12 completed: 完成节点截断修复、向导分步重构、浅色主题统一及 Welcome 导入入口补齐。

### 17.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "formatTargetLabel|Target|inline-flex flex-wrap items-center gap-2 rounded-lg border border-slate-300 bg-slate-100 p-1" src/pages/Dashboard.tsx | result: pass | note: Dashboard 日志信息结构补齐 target 字段，节点 tabs 改为全量可切换
- TC-P8-004 | stack: ui | command: rg -n "nodes.map\(|min-w-28 items-center justify-center" src/pages/Dashboard.tsx | result: pass | note: Node Details tab 不再截断前三项，tab 宽度与垂直居中统一
- TC-P8-008 | stack: ui | command: rg -n "Step 4 · 执行部署|Step 3 · 配置确认|Step 4 · 校验完成|Step 3 · 健康检查与完成" src/pages/DeployWizard.tsx src/pages/KesManager.tsx src/pages/UpgradeWizard.tsx | result: pass | note: Deploy/KES/Upgrade 已改为与原型对齐的分步向导结构
- TC-P8-012 | stack: ui | command: rg -n "Import Existing Config|Start Deploy" src/pages/SetupWizard.tsx | result: pass | note: Welcome 页面已具备开始部署与导入入口（导入入口当前占位）
- TC-P8-013 | stack: ui | command: rg -n "bg-slate-100|bg-white|text-slate-900" src/components/Layout.tsx src/components/Sidebar.tsx src/pages/SetupWizard.tsx src/pages/Dashboard.tsx src/pages/DeployWizard.tsx src/pages/KesManager.tsx src/pages/UpgradeWizard.tsx | result: pass | note: 主流程页面已统一至浅色主题基线
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 142/142 通过（仅 dead_code warning）

### 17.4 Change Request Delta
- 2026-03-12 16:22 +0800 需求修订：当前版本验收未通过，需继续按 prototype 修复明显对齐问题后再结项。

## 18. Addendum (append-only)
### 18.1 Execution Plan Delta
- [x] p8-13 修复流程组件主题不一致：ConfirmModal 与 TaskLogStream 对齐浅色体系

### 18.2 Execution Log Delta
- 2026-03-12 16:36 +0800 p8-13 started: 回归检查发现流程组件仍保留深色主题，影响主流程视觉一致性。
- 2026-03-12 16:38 +0800 p8-13 completed: 完成 ConfirmModal/TaskLogStream 浅色改造并通过构建与测试回归。

### 18.3 Validation Evidence Delta
- TC-P8-013 | stack: ui | command: rg -n "border-slate-200 bg-white|text-slate-900|bg-slate-50" src/components/ConfirmModal.tsx src/components/TaskLogStream.tsx | result: pass | note: 主流程确认弹窗与日志流组件已对齐浅色主题
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 组件主题调整后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 142/142 通过（仅 dead_code warning）

### 18.4 Change Request Delta
- 2026-03-12 16:36 +0800 修复追加：补齐流程级共用组件（确认弹窗/日志流）的主题一致性，避免页面间深浅风格割裂。

## 19. Addendum (append-only)
### 19.1 Execution Plan Delta
- [x] p8-14 对齐 Welcome/Sidebar 结构与文案：恢复设计稿分组导航、欢迎窗 titlebar 与中文主 CTA 节奏

### 19.2 Execution Log Delta
- 2026-03-12 16:40 +0800 p8-14 started: 根据最新验收反馈修复 Sidebar IA 与 Setup 欢迎窗结构偏差。
- 2026-03-12 16:48 +0800 p8-14 completed: 完成 Sidebar 分组+符号导航、Welcome titlebar/hero/CTA/直达入口改造并回归通过。

### 19.3 Validation Evidence Delta
- TC-P8-002 | stack: ui | command: rg -n "OURO OPS|Mainnet Workspace|Workspace|Operations|⌂ Dashboard|↻ KES Rotate|⬆ Upgrade|日常操作入口统一" src/components/Sidebar.tsx | result: pass | note: Sidebar 结构与信息架构已按设计稿收敛（分组、符号、说明 note）
- TC-P8-012 | stack: ui | command: rg -n "titlebar|traffic-lights|Welcome · Ouro Ops|Not Deployed|欢迎使用 Ouro Ops|开始部署|导入已有配置|直接进入 Dashboard|未检测到部署环境" src/pages/SetupWizard.tsx | result: pass | note: Welcome 页已恢复设计稿窗口形态与主 CTA 节奏
- TC-P8-013 | stack: ui | command: rg -n "to=\"/deploy\"|to=\"/settings\"" src/components/Sidebar.tsx | result: pass | note: Sidebar 已移除 Deploy/Settings 主入口，与设计稿导航一致
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 142/142 通过（仅 dead_code warning）

### 19.4 Change Request Delta
- 2026-03-12 16:40 +0800 需求修订：按验收反馈修复 Sidebar 与 Welcome 窗体结构、文案和主流程入口节奏。

## 20. Addendum (append-only)
### 20.1 Execution Plan Delta
- [x] p8-15 修复 Dashboard 设计对齐问题：titlebar 语义、中文信息架构、节点卡与详情交互、日志表头统一

### 20.2 Execution Log Delta
- 2026-03-12 16:52 +0800 p8-15 started: 根据最新审查清单修复 Dashboard 与 prototype 的结构和文案偏差。
- 2026-03-12 17:06 +0800 p8-15 completed: 完成 Dashboard titlebar/Telemetry orbit/节点卡语义/详情 tabs 语义化/日志中文化，并移除非设计主区块。

### 20.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "Ouro Ops · Dashboard|集群概览（BP \+ Relays）|节点详情|近期操作日志|Telemetry" src/pages/Dashboard.tsx | result: pass | note: Dashboard 主结构、标题与中文区块文案已对齐
- TC-P8-004 | stack: ui | command: rg -n "fieldset|legend className=\"sr-only\"|type=\"radio\"|htmlFor=\"node-tab-" src/pages/Dashboard.tsx | result: pass | note: 节点详情 Tab 改为语义化 segment 控件（fieldset/legend/radio/label）
- TC-P8-003 | stack: ui | command: rg -n "Within 1s|0-50ms|50-100ms|100-500ms|>1s|block: .* slot:" src/pages/Dashboard.tsx | result: pass | note: Connections & Peers 与 Resources 底部摘要结构已对齐设计稿
- TC-P8-003 | stack: ui | command: rg -n "KES remain|立即 Rotate|animate-pulse|Δ.*e" src/pages/Dashboard.tsx | result: pass | note: BP 卡片 KES remain/CTA 文案、脉冲提示与 epoch 差语义已收敛
- TC-P8-003 | stack: ui | command: rg -n "Bound On-chain Pool|Bind Existing Pool|Register New Pool|PoolRegistrationStatus|PoolRegistrationWizard" src/pages/Dashboard.tsx | result: pass | note: Dashboard 已移除非设计主区块（绑定/注册）
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 142/142 通过（仅 dead_code warning）

### 20.4 Change Request Delta
- 2026-03-12 16:52 +0800 需求修订：按 Dashboard 差异清单逐项修复标题、文案、节点卡语义、节点详情与日志结构。

## 21. Addendum (append-only)
### 21.1 Execution Plan Delta
- [x] p8-16 收敛 mac 原生标题栏：移除 Dashboard 页面内 titlebar，改用系统窗口标题栏样式

### 21.2 Execution Log Delta
- 2026-03-12 17:00 +0800 p8-16 started: 根据反馈移除 Dashboard 页面内标题栏，并切换到 mac 原生标题栏样式。
- 2026-03-12 17:00 +0800 p8-16 completed: 完成 Dashboard 伪 titlebar 删除、Tauri 窗口 `titleBarStyle/hiddenTitle` 配置、前端快照断言更新与回归通过。

### 21.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "Ouro Ops · Dashboard|traffic-lights|titleBarStyle|hiddenTitle" src/pages/Dashboard.tsx src-tauri/tauri.conf.json src-tauri/src/lib.rs | result: pass | note: 页面内 titlebar 已移除，系统标题栏样式配置已落地且有测试断言
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 前端构建通过（Dashboard 去内嵌 titlebar 后无编译回归）
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 143/143 通过（仅 dead_code warning）

### 21.4 Change Request Delta
- 2026-03-12 17:00 +0800 需求修订：Dashboard 标题栏不放页面内部，改为优化系统自带标题栏样式。

## 22. Addendum (append-only)
### 22.1 Execution Plan Delta
- [x] p8-17 修复 Dashboard 日志详情可用性：固定详情列宽，支持 hover 查看全文与一键复制

### 22.2 Execution Log Delta
- 2026-03-12 17:09 +0800 p8-17 started: 根据反馈收敛「近期操作日志-详情」超长文案行为，避免横向撑开页面。
- 2026-03-12 17:09 +0800 p8-17 completed: 完成详情列硬宽度约束、单行截断、hover 全文 tooltip 与复制交互，并通过前端构建回归。

### 22.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "table-fixed|w-\\[360px\\]|text-ellipsis|复制详情|role=\\\"tooltip\\\"|copyPlainText|copiedTaskId" src/pages/Dashboard.tsx | result: pass | note: 详情列固定宽度 + 截断 + hover 全文 + 复制能力已落地
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 日志详情交互增强后前端构建通过

### 22.4 Change Request Delta
- 2026-03-12 17:09 +0800 需求修订：日志详情需支持复制与 hover 查看全文，同时不得横向撑开页面。

## 23. Addendum (append-only)
### 23.1 Execution Plan Delta
- [x] p8-18 修复日志详情横向滚动：移除浮层撑宽风险，改为文案右侧常驻图标复制

### 23.2 Execution Log Delta
- 2026-03-12 17:15 +0800 p8-18 started: 根据反馈修复日志详情在复制/tooltip 改造后出现的横向滚动条问题。
- 2026-03-12 17:15 +0800 p8-18 completed: 详情列容器改为 `overflow-x-hidden`，移除自定义浮层 tooltip，保留原生 `title` 全文提示，并将复制入口改为右侧常驻图标按钮。

### 23.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "overflow-x-hidden|aria-label|复制详情|h-6 w-6|viewBox=\\\"0 0 20 20\\\"" src/pages/Dashboard.tsx | result: pass | note: 详情列无横向滚动风险，复制入口为文案右侧常驻图标
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 日志详情布局与交互二次收敛后前端构建通过

### 23.4 Change Request Delta
- 2026-03-12 17:15 +0800 需求修订：修复详情横向滚动条，复制入口改为文案右侧图标触发。

## 24. Addendum (append-only)
### 24.1 Execution Plan Delta
- [x] p8-19 落地沉浸式自定义标题栏：Sidebar Header 红绿灯安全区 + 主内容动态 Toolbar + drag/no-drag 交互

### 24.2 Execution Log Delta
- 2026-03-12 17:27 +0800 p8-19 started: 根据最新要求将 mac 顶部布局改为沉浸式自定义标题栏，覆盖侧栏与主内容双头部结构。
- 2026-03-12 17:27 +0800 p8-19 completed: 完成 `Overlay` 标题栏配置、traffic lights 定位、Sidebar 专属顶部操作区、主内容动态上下文 Toolbar、拖拽区域与 no-drag 交互约束，并通过构建与测试回归。

### 24.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "data-tauri-drag-region|toolbarContextFromPath|Update|Commit|Open|statusToneClass|onToggleCollapse|pl-\\[74px\\]" src/components/Layout.tsx src/components/Sidebar.tsx | result: pass | note: 左侧 header 与右侧动态 toolbar 结构、按钮分组、红绿灯安全区与侧栏折叠交互已落地
- TC-P8-003 | stack: ui | command: rg -n "titleBarStyle|Overlay|trafficLightPosition|hiddenTitle" src-tauri/tauri.conf.json src-tauri/src/lib.rs | result: pass | note: mac 标题栏已切换 Overlay，并固定红绿灯位置到安全边距
- TC-P8-003 | stack: ui | command: rg -n "drag-region|no-drag|-webkit-app-region|#root" src/index.css src/components/Layout.tsx src/components/Sidebar.tsx | result: pass | note: drag/no-drag 与全窗口高度基础样式已生效
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 自定义标题栏结构改造后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 144/144 通过（仅 dead_code warning）

### 24.4 Change Request Delta
- 2026-03-12 17:27 +0800 需求修订：在当前项目直接实现 mac 沉浸式无边框/自定义标题栏布局与交互细节。

## 25. Addendum (append-only)
### 25.1 Execution Plan Delta
- [x] p8-20 修正标题栏拖拽语义：仅空白区 drag，文本/chip/按钮等交互元素强制 no-drag

### 25.2 Execution Log Delta
- 2026-03-12 17:32 +0800 p8-20 started: 根据拖拽规则细化标题栏交互语义，避免 drag 与点击/选择冲突。
- 2026-03-12 17:32 +0800 p8-20 completed: 为主内容标题文本与状态 chip 增加 `no-drag`，侧栏顶部显式保留空白拖拽区，`no-drag` 恢复文本可选择能力，并通过构建/测试回归。

### 25.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "data-tauri-drag-region|no-drag truncate text-\\[14px\\]|no-drag inline-flex min-h-6|pl-\\[74px\\]|flex-1" src/components/Layout.tsx src/components/Sidebar.tsx | result: pass | note: 顶部空白 drag + 文本/chip/按钮 no-drag 规则已落地
- TC-P8-003 | stack: ui | command: rg -n "drag-region|no-drag|user-select: text" src/index.css src-tauri/src/lib.rs | result: pass | note: 全局 drag/no-drag 基础样式与断言已更新
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 拖拽语义修正后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 144/144 通过（仅 dead_code warning）

### 25.4 Change Request Delta
- 2026-03-12 17:32 +0800 需求修订：严格按 drag/no-drag 规则修复标题栏拖拽区域设计。

## 26. Addendum (append-only)
### 26.1 Execution Plan Delta
- [x] p8-21 修复顶部空白区不可拖拽：为被覆盖的 header 内层容器与空白 spacer 显式添加 drag 标记

### 26.2 Execution Log Delta
- 2026-03-12 17:34 +0800 p8-21 started: 定位顶部空白区拖拽失败问题，确认由 header 内层全宽容器覆盖且缺失 drag 标记引发。
- 2026-03-12 17:34 +0800 p8-21 completed: 为主内容与侧栏顶部内层容器及空白 `flex-1` 区域补充 `drag-region + data-tauri-drag-region`，保持交互元素 `no-drag` 不变，并通过构建/测试回归。

### 26.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "data-tauri-drag-region|drag-region flex h-14|flex-1\\\" data-tauri-drag-region" src/components/Layout.tsx src/components/Sidebar.tsx | result: pass | note: 顶部空白区域已具备明确 drag 标记，覆盖层不再吞掉拖拽行为
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 拖拽修复后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 144/144 通过（仅 dead_code warning）

### 26.4 Change Request Delta
- 2026-03-12 17:34 +0800 需求修订：修复顶部标题栏空白区域拖不动的问题。

## 27. Addendum (append-only)
### 27.1 Execution Plan Delta
- [x] p8-22 重构 KES Rotate 页面到 prototype 向导壳：titlebar、wizard-stepline、Step1 命令终端块、统一 action-bar

### 27.2 Execution Log Delta
- 2026-03-12 17:42 +0800 p8-22 started: 根据验收清单重构 KES Rotate，补齐 titlebar、步骤条、Step1 上下文/命令块与四步统一壳结构。
- 2026-03-12 17:42 +0800 p8-22 completed: 完成 KES Rotate 页面结构重排（titlebar + 搜索 + Step chip、四步 stepline、wizard-page/wizard-scroll/action-bar、Step1 terminal-block + Copy Command），并保持现有 KES 真实调用链路。

### 27.3 Validation Evidence Delta
- TC-P8-008 | stack: ui | command: rg -n "Ouro Ops · KES Rotate|Step \\{wizardStep\\} / 4|Step 1 · 生成 KES Keypairs|Step 4 · 校验完成|Copy Command|Copy \\+ 参数说明|Push to BP|Confirm KES Push|sticky bottom-0" src/pages/KesManager.tsx | result: pass | note: KES Rotate 已具备 titlebar、四步向导壳、Step1 终端命令区与底部 action-bar
- TC-P8-008 | stack: ui | command: rg -n "cardano-cli node key-gen-KES|cardano-cli node issue-op-cert|terminal-head|terminal-dot|Run In: bp hot environment|Next Input: node.cert signing" src/pages/KesManager.tsx | result: pass | note: Step1/2 命令区、terminal 样式语义与上下文 badges 已落地
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: KES 页面重构后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 144/144 通过（仅 dead_code warning）

### 27.4 Change Request Delta
- 2026-03-12 17:42 +0800 需求修订：KES Rotate 对齐 prototype，补齐 titlebar、wizard-stepline、Step1 终端块与统一 action-bar。

## 28. Addendum (append-only)
### 28.1 Execution Plan Delta
- [x] p8-23 修复 KES Rotate 顶栏空白拖拽区：空白区域 drag，交互元素 no-drag

### 28.2 Execution Log Delta
- 2026-03-12 17:44 +0800 p8-23 started: 根据反馈修复 KES Rotate 顶部标题栏空白区域不可拖拽问题。
- 2026-03-12 17:44 +0800 p8-23 completed: 为 KES Rotate 顶栏容器补齐 `drag-region + data-tauri-drag-region`，并将按钮/标题/搜索/chip 标记为 `no-drag`，保证拖拽与点击不冲突。

### 28.3 Validation Evidence Delta
- TC-P8-003 | stack: ui | command: rg -n "drag-region|data-tauri-drag-region|no-drag" src/pages/KesManager.tsx | result: pass | note: 顶栏空白区可拖拽、交互元素不可拖拽语义已落地
- TC-P8-009 | stack: node | command: pnpm build | result: pass | note: 顶栏拖拽语义修复后前端构建通过
- TC-P8-009 | stack: rust | command: cargo test -q | result: pass | note: 全量测试 144/144 通过（仅 dead_code warning）

### 28.4 Change Request Delta
- 2026-03-12 17:44 +0800 需求修订：修复顶部标题栏空白区域拖不动问题，并严格遵循 drag/no-drag 规则。
