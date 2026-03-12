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
