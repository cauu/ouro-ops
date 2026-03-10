# Phase 4 Frontend Prototype (Pure HTML)

Spec-ID: S0007
状态: active
创建时间: 2026-03-10 14:12 +0800
开始时间: 2026-03-10 14:12 +0800
完成时间:
前一个 Spec-ID: S0006
结项原因:

## 1. Requirement Details
- Background
  - 当前版本核心能力已具备，需要基于最新 Design Context 先完成高保真原型，聚焦前端产品化体验。
- Scope
  - 使用 `frontend-design` 思路设计新版本 prototype。
  - 使用纯 HTML（可多页面），不包含业务逻辑。
  - 建立统一视觉语言（浅色主题、专业/克制/极简、Cardano 色盘）。
  - 为后续 React 实装提供静态蓝图。
- Constraints
  - 不接入后端、不调用真实接口。
  - 页面可独立打开浏览。
  - 兼容桌面与移动端浏览。
- Non-goals
  - 不改动现有业务代码行为。
  - 不实现交互状态机与数据持久化。

## 2. Outline Design
- Architecture / modules impacted
  - 新增 `prototype/s0007/` 目录，包含独立 HTML 页面与共享 CSS。
  - 页面覆盖核心流程：Dashboard、Machines、Deploy、Pool、Settings。
- Data model and interfaces
  - 无真实数据模型变更，仅使用静态展示数据块。
  - 通过页面内导航链接模拟信息架构。
- Risk and rollback strategy
  - 风险低：原型文件与业务代码隔离。
  - 回滚策略：删除 `prototype/s0007/` 目录与对应 spec 项目提交。

## 3. Execution Plan
- [x] p7-1 建立 S0007 active spec 与验收基线
- [x] p7-2 设计并实现共享视觉样式（浅色、Cardano 品牌化、响应式）
- [x] p7-3 交付多页面纯 HTML prototype（无业务逻辑）
- [x] p7-4 基于 onboard 改造登录页原型（强调首次价值与可跳过）
- [x] p7-5 修正初始化流程原型：移除登录，改为首次创建 pool 引导
- [x] p7-6 交互现代化改版：从传统 SaaS 结构切换到任务优先的轻量交互
- [x] p7-7 Onboarding 空状态引导：无 pool 时图标提示创建并进入参数填写流程
- [x] p7-8 使用 frontend-design 切换到全新视觉风格（保持浅色与专业基调）

## 4. Test And Acceptance Criteria
- TC-1 原型文件结构完整，页面可独立打开并互相跳转。
- TC-2 视觉符合 Design Context（专业、克制、极简；浅色主题；Cardano 色板）。
- TC-3 页面在桌面与窄屏下可读、可用，无明显布局破坏。
- TC-4 不引入业务逻辑与后端依赖。
- TC-5 登录页具备 onboarding 关键要素：价值主张、时间预期、可跳过路径、首个行动入口。
- TC-6 初始化流程符合产品约束：无登录依赖；未创建 pool 进入 setup；已创建 pool 进入看板。
- TC-7 交互风格符合“专业/克制/极简”的现代桌面体验：弱化厚重表单与表格，强调任务流与主行动。
- TC-8 无 pool 场景下展示“新建 pool 图标 + 创建 CTA”，点击进入 onboarding 参数页。
- TC-9 原型整体视觉风格与上一版显著不同，且符合专业/克制/极简与浅色约束。

## 5. Execution Log (append-only)
- 2026-03-10 14:12 +0800 p7-1 started: 初始化 S0007 active spec。
- 2026-03-10 14:15 +0800 p7-1 completed: 建立 active spec、更新 docs 入口。
- 2026-03-10 14:16 +0800 p7-2 started: 设计共享视觉样式（字体、颜色、布局、响应式规则）。
- 2026-03-10 14:18 +0800 p7-2 completed: 交付 `prototype/s0007/styles.css`，完成浅色 Cardano 风格基础。
- 2026-03-10 14:18 +0800 p7-3 started: 构建多页面原型结构与信息架构链接。
- 2026-03-10 14:22 +0800 p7-3 completed: 交付 dashboard/machines/deploy/pool/settings 五个静态页面。
- 2026-03-10 15:12 +0800 p7-4 started: 使用 onboard 思路改造登录页原型。
- 2026-03-10 15:15 +0800 p7-4 completed: 新增 `login.html` 与登录/首访引导样式。
- 2026-03-10 18:18 +0800 p7-5 started: 根据需求修正初始化流程原型，去除登录依赖。
- 2026-03-10 18:21 +0800 p7-5 completed: 删除 `login.html`，新增 `setup.html` 并更新原型说明。
- 2026-03-10 18:27 +0800 p7-6 started: 根据反馈进行交互现代化改版。
- 2026-03-10 18:35 +0800 p7-6 completed: 重构主页与核心页面交互结构，弱化传统 SaaS 观感。
- 2026-03-10 18:45 +0800 p7-7 started: 调整 onboarding 空状态与创建流程入口。
- 2026-03-10 18:48 +0800 p7-7 completed: setup 页改为空状态引导，并新增 setup-onboarding 参数页。
- 2026-03-10 19:06 +0800 p7-8 started: 根据新指令切换原型视觉风格。
- 2026-03-10 19:14 +0800 p7-8 completed: 重写共享样式为新视觉体系并完成页面回归。

## 6. Validation Evidence (append-only)
- TC-1 | stack: other | command: ls docs/specs && test -f docs/specs/20260310T1412-S0007-frontend-prototype-v2.md | result: pass | note: active spec 文件存在且唯一
- TC-1 | stack: ui | command: find prototype/s0007 -maxdepth 1 -type f | result: pass | note: 原型文件结构完整（5 个 HTML + 1 CSS + README）
- TC-2 | stack: ui | command: manual review of prototype/s0007/styles.css and pages | result: pass | note: 视觉遵循专业/克制/极简、浅色主题、Cardano 蓝系主色
- TC-3 | stack: ui | command: manual review of responsive rules in styles.css | result: pass | note: 提供 <=980px 与 <=640px 布局收敛规则
- TC-4 | stack: ui | command: rg -n \"<script|fetch\\(|invoke\\(|tauri|axios|XMLHttpRequest\" prototype/s0007 | result: pass | note: 页面无业务逻辑与后端依赖
- TC-5 | stack: ui | command: manual review of prototype/s0007/login.html | result: pass | note: 包含价值主张、2 分钟预期、Skip 路径与首个进入动作
- TC-4 | stack: ui | command: rg -n \"<script|fetch\\(|invoke\\(|tauri|axios|XMLHttpRequest\" prototype/s0007/login.html | result: pass | note: 登录页仍为纯静态原型
- TC-6 | stack: ui | command: manual review of src/App.tsx bootstrap routing logic | result: pass | note: 已实现无登录依赖，按 pool 是否存在在 `/setup` 与 `/` 间分流
- TC-6 | stack: ui | command: test -f prototype/s0007/setup.html && test ! -f prototype/s0007/login.html | result: pass | note: 原型入口改为 setup，移除登录页
- TC-4 | stack: ui | command: rg -n \"<script|fetch\\(|invoke\\(|tauri|axios|XMLHttpRequest\" prototype/s0007/setup.html | result: pass | note: setup 页面为纯静态原型
- TC-7 | stack: ui | command: manual review of prototype/s0007/*.html interaction hierarchy | result: pass | note: 以任务流和主行动为中心，减少表格与密集表单占比
- TC-7 | stack: ui | command: manual review of dashboard/deploy/machines/pool/settings prototypes | result: pass | note: 结构从“表格+表单主导”改为“行动项+状态流主导”
- TC-3 | stack: ui | command: manual review of responsive behavior in styles.css and nav/actions wrapping | result: pass | note: 窄屏下导航与按钮可换行并保持触达
- TC-4 | stack: ui | command: rg -n \"<script|fetch\\(|invoke\\(|tauri|axios|XMLHttpRequest\" prototype/s0007/*.html | result: pass | note: 现代化改版后仍保持纯静态
- TC-8 | stack: ui | command: manual review of prototype/s0007/setup.html | result: pass | note: 无 pool 空状态包含新建图标和 Create Pool CTA
- TC-8 | stack: ui | command: test -f prototype/s0007/setup-onboarding.html && rg -n \"Pool Parameters|Create and Enter Dashboard\" prototype/s0007/setup-onboarding.html | result: pass | note: 创建按钮进入参数填写 onboarding 页
- TC-4 | stack: ui | command: rg -n \"<script|fetch\\(|invoke\\(|tauri|axios|XMLHttpRequest\" prototype/s0007/setup.html prototype/s0007/setup-onboarding.html | result: pass | note: onboarding 改动仍为纯静态
- TC-9 | stack: ui | command: manual review of prototype/s0007/styles.css visual tokens and component treatment | result: pass | note: 新版改为编辑化轻奢风格，版式与配色明显区别于上一版
- TC-4 | stack: ui | command: rg -n \"<script|fetch\\(|invoke\\(|tauri|axios|XMLHttpRequest\" prototype/s0007/*.html | result: pass | note: 风格改版后仍无业务逻辑脚本

## 7. Change Requests (append-only)
- 2026-03-10 18:18 +0800 需求修正：初始化流程不依赖用户登录；首次打开需判断 pool 是否存在，不存在提示创建，存在则直接进入首页看板。
- 2026-03-10 18:27 +0800 体验修正：现有交互过于传统 SaaS，要求提升现代感与轻量化。
- 2026-03-10 18:45 +0800 Onboarding 修正：无 pool 时使用“新建 pool 图标”提示创建，点击进入参数填写流程。
- 2026-03-10 19:06 +0800 设计修正：要求使用 frontend-design 切换到另一种设计风格。
