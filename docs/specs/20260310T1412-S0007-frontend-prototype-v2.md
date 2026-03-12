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
- [x] p7-9 再次切换风格：图标优先、左右布局、减少页面并收敛到 dashboard
- [x] p7-10 回退 p7-9：恢复到上一个可用视觉版本（p7-8）

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
- TC-10 核心页面采用左右布局，核心信息以 dashboard 为主承载。
- TC-11 页面数量减少，原型主流程不出现滚动条。
- TC-12 当用户否定当前版本时，可快速回退到上一个已确认版本并保持原型可用。

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
- 2026-03-10 21:30 +0800 p7-9 started: 图标优先与 dashboard 集中化改版。
- 2026-03-10 21:38 +0800 p7-9 completed: 重构 dashboard 与页面集合，压缩为核心页面集合并控制视口内布局。
- 2026-03-10 22:03 +0800 p7-10 started: 按用户要求回退到上一个版本。
- 2026-03-10 22:08 +0800 p7-10 completed: 恢复 `prototype/s0007` 到 p7-8 版本基线并保留 spec 追溯。

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
- TC-10 | stack: ui | command: manual review of prototype/s0007/index.html structure | result: pass | note: dashboard 采用左右主布局并承载 machine/deploy/pool/settings核心模块
- TC-11 | stack: ui | command: test \"$(find prototype/s0007 -maxdepth 1 -name '*.html' | wc -l | tr -d ' ')\" -eq 3 | result: pass | note: 核心原型页面减少到 dashboard/setup/setup-onboarding
- TC-11 | stack: ui | command: manual review of styles.css viewport rules | result: pass | note: body 与 shell 设置为视口内布局，避免主流程滚动条
- TC-11 | stack: ui | command: find prototype/s0007 -maxdepth 1 -name '*.html' | wc -l | result: pass | note: 结果为 3，已删除 deploy/machines/pool/settings 子页
- TC-10 | stack: ui | command: rg -n \"dashboard-shell|side-panel|main-panel|icon-grid\" prototype/s0007/index.html prototype/s0007/styles.css | result: pass | note: 图标优先与左右布局样式已生效
- TC-11 | stack: ui | command: rg -n \"height:\\s*100vh|overflow:\\s*hidden\" prototype/s0007/styles.css | result: pass | note: 桌面主流程使用视口内布局避免滚动条
- TC-12 | stack: ui | command: find prototype/s0007 -maxdepth 1 -name '*.html' | wc -l | result: pass | note: 回退后恢复为 7 个核心原型页面（p7-8 状态）
- TC-12 | stack: ui | command: test -f prototype/s0007/deploy.html && test -f prototype/s0007/machines.html && test -f prototype/s0007/pool.html && test -f prototype/s0007/settings.html | result: pass | note: p7-9 删除的页面已恢复
- TC-4 | stack: ui | command: rg -n \"<script|fetch\\(|invoke\\(|tauri|axios|XMLHttpRequest\" prototype/s0007/*.html | result: pass | note: 回退后仍保持纯静态原型

## 7. Change Requests (append-only)
- 2026-03-10 18:18 +0800 需求修正：初始化流程不依赖用户登录；首次打开需判断 pool 是否存在，不存在提示创建，存在则直接进入首页看板。
- 2026-03-10 18:27 +0800 体验修正：现有交互过于传统 SaaS，要求提升现代感与轻量化。
- 2026-03-10 18:45 +0800 Onboarding 修正：无 pool 时使用“新建 pool 图标”提示创建，点击进入参数填写流程。
- 2026-03-10 19:06 +0800 设计修正：要求使用 frontend-design 切换到另一种设计风格。
- 2026-03-10 21:30 +0800 设计修正：减少文字与页面，图标替代文案，核心信息集中到 dashboard 且采用左右布局。
- 2026-03-10 22:03 +0800 用户反馈当前版本不可接受，要求立即回退到上一个版本。

## 8. Addendum (append-only)
### 8.1 Execution Plan Delta
- [x] p7-11 参考截图重构风格并收敛为 5 个核心页面（onboarding/dashboard/deploy/kes-rotate/upgrade-image）

### 8.2 Acceptance Delta
- TC-13 页面集合严格收敛为 5 个核心页面，且互相可跳转。
- TC-14 页面内容完整覆盖初始化、部署、注册/更新 Pool、KES Rotate、升级镜像五条核心流程。
- TC-15 新版视觉风格参考给定截图（左侧导航 + 顶部操作条 + 卡片化主内容），并保持浅色与专业/克制基调。
- TC-16 原型仍为纯 HTML/CSS 静态实现，不引入业务逻辑脚本。

### 8.3 Execution Log Delta
- 2026-03-10 22:24 +0800 p7-11 started: 按截图风格重构 S0007 页面与信息架构。
- 2026-03-10 22:36 +0800 p7-11 completed: 完成 5 页重构、流程映射、旧页面收敛与 README 更新。

### 8.4 Validation Evidence Delta
- TC-13 | stack: ui | command: find prototype/s0007 -maxdepth 1 -name '*.html' | sort | result: pass | note: 页面为 onboarding/index/deploy/kes-rotate/upgrade-image 共 5 页
- TC-13 | stack: ui | command: rg -n "href=\"\./(onboarding|index|deploy|kes-rotate|upgrade-image)\.html\"" prototype/s0007/*.html | result: pass | note: 五页均具备互跳导航
- TC-14 | stack: ui | command: rg -n "初始化流程|添加 BP|Relay|unsigned|signed|KES|升级镜像" prototype/s0007/*.html | result: pass | note: 五条核心流程均已映射到页面内容
- TC-15 | stack: ui | command: manual review of prototype/s0007/styles.css and page layout | result: pass | note: 左侧导航、顶部工具条、卡片化主体与浅色专业风格已统一
- TC-16 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007 || true | result: pass | note: 无脚本调用，保持纯静态原型

### 8.5 Change Request Delta
- 2026-03-10 22:24 +0800 需求新增：参考截图调整整体风格，并将页面收敛到 onboarding/dashboard/deploy/kes-rotate/upgrade-image 五个核心页。

## 9. Addendum (append-only)
### 9.1 Execution Plan Delta
- [x] p7-12 将部署/KES Rotate/升级镜像改为分步骤向导流程（stepper）

### 9.2 Acceptance Delta
- TC-17 deploy 页面采用明确 stepper，按步骤呈现并包含上一步/下一步区。
- TC-18 kes-rotate 页面采用明确 stepper，按步骤呈现并包含上一步/下一步区。
- TC-19 upgrade-image 页面采用明确 stepper，按步骤呈现并包含上一步/下一步区。

### 9.3 Execution Log Delta
- 2026-03-10 22:43 +0800 p7-12 started: 三个流程页改造为分步骤向导结构。
- 2026-03-10 22:49 +0800 p7-12 completed: deploy/kes-rotate/upgrade-image 全部完成 stepper 化与导航动作区统一。

### 9.4 Validation Evidence Delta
- TC-17 | stack: ui | command: rg -n "向导步骤|Step 1|Step 2|Step 3|上一步|下一步" prototype/s0007/deploy.html | result: pass | note: deploy 页面已为分步骤向导
- TC-18 | stack: ui | command: rg -n "向导步骤|Step 1|Step 2|Step 3|Step 4|上一步|下一步" prototype/s0007/kes-rotate.html | result: pass | note: kes-rotate 页面已为分步骤向导
- TC-19 | stack: ui | command: rg -n "向导步骤|Step 1|Step 2|Step 3|上一步|下一步" prototype/s0007/upgrade-image.html | result: pass | note: upgrade-image 页面已为分步骤向导
- TC-16 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007 || true | result: pass | note: 分步骤改造后仍为纯静态原型

### 9.5 Change Request Delta
- 2026-03-10 22:43 +0800 需求新增：部署、KES Rotate、升级镜像需要采用分步骤流程展示。

## 10. Addendum (append-only)
### 10.1 Execution Plan Delta
- [x] p7-13 按审计优先级修正原型可访问性与语义质量（真实搜索控件、焦点样式、触达尺寸、table/stepper 语义）

### 10.2 Acceptance Delta
- TC-20 五个页面均提供 skip-link，支持键盘快速跳转主内容。
- TC-21 顶部搜索区改为真实 `input[type=search]`，不再使用伪控件。
- TC-22 deploy / kes-rotate / upgrade-image 的 stepper 增加语义状态（`aria-current=step` + 文本状态）。
- TC-23 数据网格改为语义 table（`table/thead/tbody/th`）。
- TC-24 交互可达性基线修正：可操作按钮使用 pointer，关键交互最小触达 44px，统一 `:focus-visible`。
- TC-25 保持纯静态原型，不引入业务逻辑脚本。

### 10.3 Execution Log Delta
- 2026-03-10 23:02 +0800 p7-13 started: 执行审计建议的高优先级与中优先级设计修正。
- 2026-03-10 23:11 +0800 p7-13 completed: 完成可访问性、语义结构与交互基线统一改造。

### 10.4 Validation Evidence Delta
- TC-20 | stack: ui | command: rg -n "skip-link" prototype/s0007/*.html | result: pass | note: 五个页面均存在 skip-link
- TC-21 | stack: ui | command: rg -n "search-input|type=\"search\"" prototype/s0007/*.html | result: pass | note: 顶部搜索区已改为真实搜索输入
- TC-22 | stack: ui | command: rg -n "aria-current=\"step\"" prototype/s0007/deploy.html prototype/s0007/kes-rotate.html prototype/s0007/upgrade-image.html | result: pass | note: 三个分步骤页面均标记当前步骤
- TC-23 | stack: ui | command: rg -n "<table class=\"data-table\"|<thead>|<tbody>|<th scope=\"col\"" prototype/s0007/*.html | result: pass | note: 数据展示区已改为语义表格
- TC-24 | stack: ui | command: rg -n "cursor:\s*pointer|min-height:\s*44px|:focus-visible" prototype/s0007/styles.css | result: pass | note: 交互尺寸与焦点样式已统一
- TC-25 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007 || true | result: pass | note: 仍为纯 HTML/CSS 静态原型

### 10.5 Change Request Delta
- 2026-03-10 23:02 +0800 需求新增：按审计建议的优先级流程对原型进行设计与可访问性修正。

## 11. Addendum (append-only)
### 11.1 Execution Plan Delta
- [x] p7-14 将部署流程页重构为单步骤聚焦向导（single-step focused wizard）

### 11.2 Acceptance Delta
- TC-26 部署页顶部保留全量水平 stepper，并标记 completed/current/upcoming。
- TC-27 主内容区仅展示当前步骤（Step 2）的详细内容与动作，不展示 Step 1/3/4 详细区块。
- TC-28 页面仅保留轻量已完成摘要（可选），信息密度明显下降。
- TC-29 保持现有企业化轻量风格（浅色背景、白卡片、蓝灰边框、圆角、清晰层级）。

### 11.3 Execution Log Delta
- 2026-03-10 23:18 +0800 p7-14 started: 按 single-step wizard 要求重构部署流程页信息架构。
- 2026-03-10 23:23 +0800 p7-14 completed: 部署页实现“仅当前步骤内容 + 顶部 stepper + 轻量摘要”布局。

### 11.4 Validation Evidence Delta
- TC-26 | stack: ui | command: rg -n "step-node done|step-node current|step-node\">" prototype/s0007/deploy.html | result: pass | note: stepper 保留完整步骤并具备状态层级
- TC-27 | stack: ui | command: rg -n "Step 2 · Deployment Parameters|保存草稿|确认参数|上一步|下一步" prototype/s0007/deploy.html | result: pass | note: 主区域仅保留当前步骤内容与动作
- TC-27 | stack: ui | command: rg -n "Step 1 · 添加 BP / Relay 机器|Step 3 / Step 4|Post-deploy registration path|data-table" prototype/s0007/deploy.html || true | result: pass | note: 已移除非当前步骤详细区块
- TC-28 | stack: ui | command: rg -n "已完成摘要|Step 1 · 添加机器|下一步预告" prototype/s0007/deploy.html | result: pass | note: 仅保留轻量摘要，无展开明细
- TC-29 | stack: ui | command: manual review of prototype/s0007/deploy.html with styles.css tokens | result: pass | note: 风格保持浅色企业化卡片体系与清晰视觉层级

### 11.5 Change Request Delta
- 2026-03-10 23:18 +0800 需求新增：部署流程页改为 single-step focused wizard，避免同时显示多步骤详情。

## 12. Addendum (append-only)
### 12.1 Execution Plan Delta
- [x] p7-15 将 KES Rotate 与升级镜像页面重构为单步骤聚焦向导（single-step focused wizard）

### 12.2 Acceptance Delta
- TC-30 KES Rotate 页面仅展示当前步骤详细内容，顶部 stepper 仅用于显示 completed/current/upcoming 状态。
- TC-31 升级镜像页面仅展示当前步骤详细内容，顶部 stepper 仅用于显示 completed/current/upcoming 状态。
- TC-32 两页均不再展示非当前步骤的详细区块，仅保留轻量已完成摘要。
- TC-33 保持现有企业化浅色风格与低认知负担布局。

### 12.3 Execution Log Delta
- 2026-03-10 23:31 +0800 p7-15 started: 优化 KES Rotate 与升级镜像页面为单步骤聚焦向导。
- 2026-03-10 23:36 +0800 p7-15 completed: 两页已完成单步骤重构，移除多步骤明细并保留轻量摘要。

### 12.4 Validation Evidence Delta
- TC-30 | stack: ui | command: rg -n "Step 2 · 冷环境生成 node.cert|aria-current=\"step\"|已完成摘要|上一步|下一步" prototype/s0007/kes-rotate.html | result: pass | note: KES 页面主区仅保留当前步骤与动作
- TC-31 | stack: ui | command: rg -n "Step 2 · 执行滚动升级|aria-current=\"step\"|已完成摘要|上一步|下一步" prototype/s0007/upgrade-image.html | result: pass | note: 升级页面主区仅保留当前步骤与动作
- TC-32 | stack: ui | command: rg -n "分步骤执行详情|冷环境脚本示例|Step 1 · 版本对比|Step 2 / Step 3 · 升级执行与完成|执行日志（示例）" prototype/s0007/kes-rotate.html prototype/s0007/upgrade-image.html || true | result: pass | note: 非当前步骤详细区块已移除
- TC-33 | stack: ui | command: manual review of prototype/s0007/kes-rotate.html and upgrade-image.html with shared styles.css | result: pass | note: 保持浅色卡片化企业风格与清晰层级
- TC-25 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007 || true | result: pass | note: 仍为纯静态原型

### 12.5 Change Request Delta
- 2026-03-10 23:31 +0800 需求新增：KES Rotate 和升级镜像流程均需按单步骤聚焦向导进行优化。

## 13. Addendum (append-only)
### 13.1 Execution Plan Delta
- [x] p7-16 基于 macOS 原生范式重构全部 5 个页面（titlebar/source list/toolbar/inspector），统一流程页与主视图交互语言

### 13.2 Acceptance Delta
- TC-34 五个页面统一采用 macOS 窗口骨架：titlebar（含 traffic lights）+ sidebar + main content + inspector。
- TC-35 Sidebar 改为 source list 风格，Toolbar 改为轻量搜索与上下文动作，移除 Web 风格重 Header 叠层。
- TC-36 Deploy / KES Rotate / Upgrade 保持单步骤聚焦主区，仅顶部展示进度状态（completed/current/upcoming）。
- TC-37 KES 页面代码区采用终端语义块（terminal head + mono body），Deploy 参数区采用更接近原生 Form 的行式布局。
- TC-38 原型仍为纯 HTML/CSS 静态实现，不引入业务逻辑脚本。
- TC-39 原型说明文档同步更新为 macOS 化设计方向与结构说明。

### 13.3 Execution Log Delta
- 2026-03-10 23:44 +0800 p7-16 started: 执行全页面 macOS 化重构（窗口结构、导航、工具栏、主区与检查器统一）。
- 2026-03-10 23:53 +0800 p7-16 completed: 完成 5 页结构重写与样式系统替换，并更新 README 设计说明。

### 13.4 Validation Evidence Delta
- TC-34 | stack: ui | command: rg -n "window-shell|titlebar|traffic-lights|workspace|sidebar|inspector" prototype/s0007/onboarding.html prototype/s0007/index.html prototype/s0007/deploy.html prototype/s0007/kes-rotate.html prototype/s0007/upgrade-image.html | result: pass | note: 五页均具备统一 macOS 窗口骨架
- TC-35 | stack: ui | command: rg -n "toolbar-search|toolbar-chip|nav-group|nav-item" prototype/s0007/*.html && rg -n -- "backdrop-filter" prototype/s0007/styles.css | result: pass | note: source list 与轻量 toolbar 结构到位，材质化层级存在
- TC-36 | stack: ui | command: rg -n "progress-track|aria-current=\"step\"|Step 2|STEP 2" prototype/s0007/deploy.html prototype/s0007/kes-rotate.html prototype/s0007/upgrade-image.html | result: pass | note: 三个流程页均为单步骤主区 + 顶部进度轨
- TC-37 | stack: ui | command: rg -n "terminal-block|terminal-head|terminal-body|form-row|form-label|form-value" prototype/s0007/kes-rotate.html prototype/s0007/deploy.html prototype/s0007/styles.css | result: pass | note: 终端语义块与行式参数表单均已生效
- TC-38 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007 || true | result: pass | note: 仍无脚本调用
- TC-39 | stack: ui | command: sed -n '1,220p' prototype/s0007/README.md | result: pass | note: README 已更新为 macOS-native 原型设计说明

### 13.5 Change Request Delta
- 2026-03-10 23:44 +0800 需求新增：基于 Apple HIG/macOS 设计范式，优化所有页面并统一为原生桌面风格。

## 14. Addendum (append-only)
### 14.1 Execution Plan Delta
- [x] p7-17 将 KES Rotate 页面重构为沉浸式 Focused Wizard（隐藏侧栏/右栏，聚焦冷环境交互主线）

### 14.2 Acceptance Delta
- TC-40 KES 页面进入专注模式：不再显示全局 sidebar 与 inspector，仅保留最小标题栏与居中主任务区。
- TC-41 进度展示收敛为轻量步骤指示（focused-steps），移除冗余摘要面板。
- TC-42 Step 2 以命令区为绝对视觉锚点，提供复制命令主动作与上下文 badge（input/output/env）。
- TC-43 上传交互改为 Drag & Drop 主入口（含 Choose File），并明确“校验通过后自动进入 Step 3”。
- TC-44 防呆反馈改为非阻塞提示语义，KES 倒计时固定在顶部工具区常驻提醒。
- TC-45 保持纯静态原型，不引入业务逻辑脚本。

### 14.3 Execution Log Delta
- 2026-03-10 23:59 +0800 p7-17 started: 按 Focused Wizard 思路重构 KES Step 2 页面。
- 2026-03-11 00:06 +0800 p7-17 completed: 完成沉浸式布局、上传主线与防呆提示重构。

### 14.4 Validation Evidence Delta
- TC-40 | stack: ui | command: rg -n "focused-workspace|focused-content|wizard-shell" prototype/s0007/kes-rotate.html prototype/s0007/styles.css | result: pass | note: 页面已切换为单主区专注布局
- TC-40 | stack: ui | command: rg -n "<aside class=\"sidebar\"|<aside class=\"inspector\"" prototype/s0007/kes-rotate.html || true | result: pass | note: KES 页面未再包含 sidebar/inspector
- TC-41 | stack: ui | command: rg -n "focused-steps|Step 2 / 4|aria-current=\"step\"" prototype/s0007/kes-rotate.html | result: pass | note: 进度展示已收敛为轻量步骤轨
- TC-42 | stack: ui | command: rg -n "terminal-block|Copy Command|badge-pill|Input:|Output:|Run In:" prototype/s0007/kes-rotate.html prototype/s0007/styles.css | result: pass | note: 命令区与上下文提示成为主视觉锚点
- TC-43 | stack: ui | command: rg -n "focused-dropzone|Choose File|拖拽|自动流转到 Step 3" prototype/s0007/kes-rotate.html | result: pass | note: 上传主入口与自动流转说明已就位
- TC-44 | stack: ui | command: rg -n "toolbar-chip warn|KES Window|nonblocking-hint|network mismatch" prototype/s0007/kes-rotate.html prototype/s0007/styles.css | result: pass | note: 紧迫感指标常驻顶部，错误提示为非阻塞语义
- TC-45 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007/kes-rotate.html prototype/s0007/styles.css || true | result: pass | note: 仍为纯静态实现

### 14.5 Change Request Delta
- 2026-03-10 23:59 +0800 需求新增：按照 Focused Wizard 思路重做 KES 页面，突出冷热机交互主线并降低决策负担。

## 15. Addendum (append-only)
### 15.1 Execution Plan Delta
- [x] p7-18 重构初始化与 onboarding 流程：Welcome Window 引导 + 无侧栏沉浸式 Deploy Wizard

### 15.2 Acceptance Delta
- TC-46 onboarding 页面改为轻量 Welcome Window（无 sidebar/inspector），聚焦“开始部署”主入口。
- TC-47 Welcome Window 提供清晰视觉层级：Hero 图形、标题/副标题、主次 CTA 与“已部署直达 Dashboard”路径。
- TC-48 deploy 页面改为无侧栏沉浸式向导布局，居中主视图承载步骤、表单、操作区。
- TC-49 沉浸式部署页面提供轻量进度提示（Step 1/3 + focused-steps）与加载状态（spinner + 状态文本）。
- TC-50 沉浸式部署页面提供中断防护文案（关闭/取消时的原生确认框策略）。
- TC-51 保持纯 HTML/CSS 静态原型，不引入业务逻辑脚本。

### 15.3 Execution Log Delta
- 2026-03-11 00:16 +0800 p7-18 started: 按 Welcome -> Immersive Deploy 思路重构初始化与 onboarding。
- 2026-03-11 00:24 +0800 p7-18 completed: onboarding/deploy/styles/README 已完成流程与布局调整。

### 15.4 Validation Evidence Delta
- TC-46 | stack: ui | command: rg -n "welcome-shell|welcome-window|focused-workspace" prototype/s0007/onboarding.html prototype/s0007/styles.css | result: pass | note: onboarding 已切换为 Welcome Window 单主区布局
- TC-46 | stack: ui | command: rg -n "<aside class=\"sidebar\"|<aside class=\"inspector\"" prototype/s0007/onboarding.html || true | result: pass | note: onboarding 页面无 sidebar/inspector
- TC-47 | stack: ui | command: rg -n "hero-mark|welcome-title|welcome-subtitle|开始部署|导入已有配置|直接进入 Dashboard" prototype/s0007/onboarding.html prototype/s0007/styles.css | result: pass | note: Welcome 视觉层级与主次 CTA 完整
- TC-48 | stack: ui | command: rg -n "Immersive|focused-workspace|wizard-shell wide" prototype/s0007/deploy.html prototype/s0007/styles.css | result: pass | note: deploy 已切换为无侧栏沉浸式向导主区
- TC-49 | stack: ui | command: rg -n "Step 1 / 3|focused-steps|spinner|loading-row" prototype/s0007/deploy.html prototype/s0007/styles.css | result: pass | note: 轻量进度与加载态存在
- TC-50 | stack: ui | command: rg -n "guard-alert|继续部署|退出部署|中断当前流程" prototype/s0007/deploy.html prototype/s0007/styles.css | result: pass | note: 中断防护策略文案已落位
- TC-51 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007/onboarding.html prototype/s0007/deploy.html prototype/s0007/styles.css || true | result: pass | note: 仍为纯静态实现

### 15.5 Change Request Delta
- 2026-03-11 00:16 +0800 需求新增：初始化采用 Welcome Window，点击开始部署后进入无侧栏沉浸式部署流程。

## 16. Addendum (append-only)
### 16.1 Execution Plan Delta
- [x] p7-19 精简 onboarding/deploy 冗余文案，并收敛 deploy 进度条视觉占比以突出当前表单

### 16.2 Acceptance Delta
- TC-52 onboarding 页面移除非必要说明段落，保留核心引导与主次 CTA。
- TC-53 deploy 页面去除大体量进度模块，替换为紧凑型单行步骤提示。
- TC-54 deploy 页面视觉重心回归当前步骤输入表单区。
- TC-55 仍保持纯 HTML/CSS 静态原型。

### 16.3 Execution Log Delta
- 2026-03-11 00:32 +0800 p7-19 started: 调整初始化与 deploy 页面文案密度与进度视觉权重。
- 2026-03-11 00:36 +0800 p7-19 completed: onboarding 文案精简完成，deploy 进度区改为紧凑单行。

### 16.4 Validation Evidence Delta
- TC-52 | stack: ui | command: rg -n "welcome-points" prototype/s0007/onboarding.html || true | result: pass | note: 冗余说明块已移除
- TC-52 | stack: ui | command: rg -n "开始部署|导入已有配置|直接进入 Dashboard" prototype/s0007/onboarding.html | result: pass | note: 核心引导与主次 CTA 保留
- TC-53 | stack: ui | command: rg -n "step-inline|mini current|Step 1 添加机器" prototype/s0007/deploy.html prototype/s0007/styles.css | result: pass | note: 进度改为紧凑单行提示
- TC-53 | stack: ui | command: rg -n "focused-steps" prototype/s0007/deploy.html || true | result: pass | note: deploy 页面已不再使用大体量步骤块
- TC-54 | stack: ui | command: rg -n "当前步骤输入|form-grid|form-row" prototype/s0007/deploy.html | result: pass | note: 当前步骤表单仍为主内容区核心
- TC-55 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007/onboarding.html prototype/s0007/deploy.html prototype/s0007/styles.css || true | result: pass | note: 未引入脚本逻辑

### 16.5 Change Request Delta
- 2026-03-11 00:32 +0800 需求新增：去掉原型中不必要文案，并收敛 deploy 进度条视觉占比。

## 17. Addendum (append-only)
### 17.1 Execution Plan Delta
- [x] p7-20 完成 Deployment Wizard：Step1 动态 Relay 设计（>=1）并补齐 Step2/Step3 页面设计稿

### 17.2 Acceptance Delta
- TC-56 Step1 提供动态 Relay 列表设计：支持“+ 添加 Relay”，并明确“至少 1 台 Relay”约束与移除规则。
- TC-57 Step2（部署参数）设计完整，包含参数表单与操作区（上一步/保存草稿/确认参数）。
- TC-58 Step3（执行与校验）设计完整，包含执行态、状态列表、中断防护与完成动作。
- TC-59 deploy 进度提示保持紧凑，不抢占表单视觉重心。
- TC-60 保持纯 HTML/CSS 静态原型。

### 17.3 Execution Log Delta
- 2026-03-11 00:43 +0800 p7-20 started: 按新需求补齐 deploy step1 动态 relay 设计，并完成 step2/step3 设计。
- 2026-03-11 00:48 +0800 p7-20 completed: deploy 页面三步设计完成，样式补充 relay 列表与紧凑步骤提示。

### 17.4 Validation Evidence Delta
- TC-56 | stack: ui | command: rg -n "Relay Machines \(>=1\)|\+ 添加 Relay|required|移除" prototype/s0007/deploy.html prototype/s0007/styles.css | result: pass | note: Step1 已具备动态 relay 设计与最小约束表达
- TC-57 | stack: ui | command: rg -n "Step 2 · 部署参数|deploy parameters form|保存草稿|确认参数" prototype/s0007/deploy.html | result: pass | note: Step2 参数区与操作区完整
- TC-58 | stack: ui | command: rg -n "Step 3 · 执行与校验|loading-row|flow-state|guard-alert|完成并进入 Dashboard" prototype/s0007/deploy.html prototype/s0007/styles.css | result: pass | note: Step3 执行/校验/防中断与完成动作完整
- TC-59 | stack: ui | command: rg -n "step-inline|mini current|mini done" prototype/s0007/deploy.html prototype/s0007/styles.css | result: pass | note: 进度条为紧凑单行提示
- TC-60 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007/deploy.html prototype/s0007/styles.css || true | result: pass | note: 无业务逻辑脚本

### 17.5 Change Request Delta
- 2026-03-11 00:43 +0800 需求新增：Deployment Wizard Step1 支持动态添加 Relay（>=1），并完成 Step2 与 Step3 的设计。

## 14. Addendum (append-only)
### 14.1 Execution Plan Delta
- [x] p7-17 将 Deployment Wizard 重构为 4 个独立视图（单步聚焦）并统一底部 Action Bar

### 14.2 Acceptance Delta
- TC-40 Deploy 流程拆分为 4 个独立页面，任一页面仅展示当前步骤详细内容。
- TC-41 顶部仅保留轻量水平 stepper，状态包含 completed/current/upcoming。
- TC-42 四个步骤页均具备固定底部 Action Bar：左侧 `取消`，右侧 `上一步/下一步(或确认)`；Step 4 不提供 `上一步`。
- TC-43 Step 1 支持 Relay 动态增减视觉与 `>=1` 约束表达。
- TC-44 Step 2/Step 3 分别完成参数配置与只读确认视图设计，字段覆盖节点配置与部署关键参数。
- TC-45 原型说明同步更新，明确 Deploy 已拆分为四个独立视图。

### 14.3 Execution Log Delta
- 2026-03-11 10:45 +0800 p7-17 started: 依据 4 视图向导要求重构 deploy 页面结构。
- 2026-03-11 11:03 +0800 p7-17 completed: 完成 deploy step1~step4 独立页面、动作栏统一与文档同步。

### 14.4 Validation Evidence Delta
- TC-40 | stack: ui | command: test -f prototype/s0007/deploy.html && test -f prototype/s0007/deploy-step2.html && test -f prototype/s0007/deploy-step3.html && test -f prototype/s0007/deploy-step4.html | result: pass | note: Deploy 4 个独立步骤视图均存在
- TC-41 | stack: ui | command: rg -n "wizard-stepline|pill-step done|pill-step current" prototype/s0007/deploy*.html | result: pass | note: 四个步骤页均使用顶部轻量 stepper 并带状态
- TC-42 | stack: ui | command: rg -n "action-bar|取消|上一步|下一步|确认并部署|进入 Dashboard" prototype/s0007/deploy*.html | result: pass | note: 底部动作栏结构完整，Step4 无上一步按钮
- TC-43 | stack: ui | command: rg -n "Relay Nodes|\+ Add Relay|− Remove|required" prototype/s0007/deploy.html | result: pass | note: Step1 明确支持 Relay 动态增减和最小一台约束
- TC-44 | stack: ui | command: rg -n "Image & Network|Runtime Options|节点配置摘要|部署参数摘要" prototype/s0007/deploy-step2.html prototype/s0007/deploy-step3.html | result: pass | note: Step2/Step3 关键字段与确认视图完整
- TC-45 | stack: ui | command: rg -n "deploy-step2.html|deploy-step3.html|deploy-step4.html|Deploy Wizard is split into 4 independent views" prototype/s0007/README.md | result: pass | note: 原型说明已同步

### 14.5 Change Request Delta
- 2026-03-11 10:45 +0800 需求新增：Deployment Wizard 必须改为 4 个独立视图，且固定底部动作栏与单步聚焦交互。

## 18. Addendum (append-only)
### 18.1 Correction Note
- 2026-03-11 11:08 +0800 记录纠偏：上一追加段落误复用历史编号（p7-17 / TC-40..45）。本次交付以 `p7-21` 与 `TC-61..66` 为唯一追踪编号。

### 18.2 Execution Plan Delta
- [x] p7-21 将 Deployment Wizard 重构为 4 个独立视图（单步聚焦）并统一底部 Action Bar

### 18.3 Acceptance Delta
- TC-61 Deploy 流程拆分为 4 个独立页面，任一页面仅展示当前步骤详细内容。
- TC-62 顶部仅保留轻量水平 stepper，状态包含 completed/current/upcoming。
- TC-63 四个步骤页均具备固定底部 Action Bar：左侧 `取消`，右侧 `上一步/下一步(或确认)`；Step 4 不提供 `上一步`。
- TC-64 Step 1 支持 Relay 动态增减视觉与 `>=1` 约束表达。
- TC-65 Step 2/Step 3 分别完成参数配置与只读确认视图设计，字段覆盖节点配置与部署关键参数。
- TC-66 原型说明同步更新，明确 Deploy 已拆分为四个独立视图。

### 18.4 Execution Log Delta
- 2026-03-11 10:45 +0800 p7-21 started: 依据 4 视图向导要求重构 deploy 页面结构。
- 2026-03-11 11:03 +0800 p7-21 completed: 完成 deploy step1~step4 独立页面、动作栏统一与文档同步。

### 18.5 Validation Evidence Delta
- TC-61 | stack: ui | command: test -f prototype/s0007/deploy.html && test -f prototype/s0007/deploy-step2.html && test -f prototype/s0007/deploy-step3.html && test -f prototype/s0007/deploy-step4.html | result: pass | note: Deploy 4 个独立步骤视图均存在
- TC-62 | stack: ui | command: rg -n "wizard-stepline|pill-step done|pill-step current" prototype/s0007/deploy*.html | result: pass | note: 四个步骤页均使用顶部轻量 stepper 并带状态
- TC-63 | stack: ui | command: rg -n "action-bar|取消|上一步|下一步|确认并部署|进入 Dashboard" prototype/s0007/deploy*.html | result: pass | note: 底部动作栏结构完整，Step4 无上一步按钮
- TC-64 | stack: ui | command: rg -n "Relay Nodes|\+ Add Relay|− Remove|required" prototype/s0007/deploy.html | result: pass | note: Step1 明确支持 Relay 动态增减和最小一台约束
- TC-65 | stack: ui | command: rg -n "Image & Network|Runtime Options|节点配置摘要|部署参数摘要" prototype/s0007/deploy-step2.html prototype/s0007/deploy-step3.html | result: pass | note: Step2/Step3 关键字段与确认视图完整
- TC-66 | stack: ui | command: rg -n "deploy-step2.html|deploy-step3.html|deploy-step4.html|Deploy Wizard is split into 4 independent views" prototype/s0007/README.md | result: pass | note: 原型说明已同步

### 18.6 Change Request Delta
- 2026-03-11 10:45 +0800 需求新增：Deployment Wizard 必须改为 4 个独立视图，且固定底部动作栏与单步聚焦交互。

## 19. Addendum (append-only)
### 19.1 Execution Plan Delta
- [x] p7-22 Dashboard 顶层集群概览增加 BP/Relay 的 Epoch 与同步进度展示

### 19.2 Acceptance Delta
- TC-67 顶层集群概览新增全局同步摘要（Cluster Epoch、BP/Relay 漂移、最慢节点）。
- TC-68 节点矩阵中每个 BP/Relay 卡片均展示 Epoch 与 Sync 进度条。
- TC-69 BP 卡片显示高危同步与 tip diff 信息，支持一眼识别风险。
- TC-70 保持纯 HTML/CSS 静态原型，不引入业务逻辑脚本。

### 19.3 Execution Log Delta
- 2026-03-12 00:04 +0800 p7-22 started: 按需求补充 Dashboard 顶层 Epoch 与同步进度信息。
- 2026-03-12 00:08 +0800 p7-22 completed: 完成集群同步摘要、节点 Epoch/Sync 卡片与样式更新。

### 19.4 Validation Evidence Delta
- TC-67 | stack: ui | command: rg -n "Cluster Epoch|BP vs Relay Drift|Slowest Node" prototype/s0007/index.html | result: pass | note: 集群同步总览字段已落地
- TC-68 | stack: ui | command: rg -n "node-matrix|Epoch|Sync|progress-mini" prototype/s0007/index.html prototype/s0007/styles.css | result: pass | note: BP/Relay 卡片均包含 Epoch 与 Sync 进度展示
- TC-69 | stack: ui | command: rg -n "BP · 98.41%|critical|tip diff" prototype/s0007/index.html | result: pass | note: BP 风险态与差异信息可见
- TC-70 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007/index.html prototype/s0007/styles.css || true | result: pass | note: 保持纯静态原型

### 19.5 Change Request Delta
- 2026-03-12 00:04 +0800 需求新增：顶层集群概览需展示当前 BP/Relay 实际同步 Epoch 与进度。

## 20. Addendum (append-only)
### 20.1 Execution Plan Delta
- [x] p7-23 重构 Dashboard 常态监控结构：移除 Onboard 菜单、增加节点 Tabs 详情与近期操作日志

### 20.2 Acceptance Delta
- TC-71 Sidebar 在已部署态隐藏 Onboard 菜单，保留 Dashboard 与高频运维入口。
- TC-72 Dashboard 主视图重构为三段：顶部集群概览、中部节点 Tabs 详情、底部近期操作日志。
- TC-73 节点详情区提供 BP/Relay Tabs（分段控件风格）与资源/连接可视化。
- TC-74 近期操作日志以审计列表展示时间、操作类型、目标节点、状态、详情。
- TC-75 顶层继续保留 BP/Relay 的 Epoch 与 Sync 进度，同时展示 KES 高危提示与 CTA。
- TC-76 保持纯 HTML/CSS 静态原型，不引入业务逻辑脚本。

### 20.3 Execution Log Delta
- 2026-03-12 00:10 +0800 p7-23 started: 按反馈补齐 Dashboard 其余模块改造。
- 2026-03-12 00:15 +0800 p7-23 completed: 完成 Sidebar、节点详情 Tabs、审计日志与顶层告警协同改造。

### 20.4 Validation Evidence Delta
- TC-71 | stack: ui | command: rg -n "Onboarding|Deploy" prototype/s0007/index.html || true | result: pass | note: Sidebar 已无 Onboard 菜单，Deploy 未作为常态菜单展示
- TC-71 | stack: ui | command: rg -n "Dashboard|Nodes|KES Rotate|Upgrade|Pool|Activity|Settings" prototype/s0007/index.html | result: pass | note: 高/中频运维菜单结构已替换
- TC-72 | stack: ui | command: rg -n "集群概览|节点详情|近期操作日志" prototype/s0007/index.html | result: pass | note: 主区三段结构齐全
- TC-73 | stack: ui | command: rg -n "tab-segment|seg-btn|node-details-grid|sparkline|dist-list" prototype/s0007/index.html prototype/s0007/styles.css | result: pass | note: Tabs 与资源/连接可视化样式到位
- TC-74 | stack: ui | command: rg -n "audit-table|时间|操作|目标|状态|详情" prototype/s0007/index.html prototype/s0007/styles.css | result: pass | note: 审计日志表格结构完整
- TC-75 | stack: ui | command: rg -n "Cluster Epoch|KES Risk|remain 3|立即 Rotate|Epoch|Sync" prototype/s0007/index.html | result: pass | note: 顶层同步与 KES 高危信息同时可见
- TC-76 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007/index.html prototype/s0007/styles.css || true | result: pass | note: 保持纯静态原型

### 20.5 Change Request Delta
- 2026-03-12 00:10 +0800 需求新增：除顶层概览外，Dashboard 其余部分也需按 macOS 监控场景完成重构。

## 21. Addendum (append-only)
### 21.1 Execution Plan Delta
- [x] p7-24 移除 Dashboard 右侧「高危提醒」和「快捷动作」面板

### 21.2 Acceptance Delta
- TC-77 Dashboard 页面不再渲染右侧 inspector 区域中的「高危提醒」「快捷动作」模块。
- TC-78 Dashboard 主布局调整为仅侧边栏 + 主内容两列，不保留空白右栏。
- TC-79 保持纯 HTML/CSS 静态原型，不引入业务逻辑脚本。

### 21.3 Execution Log Delta
- 2026-03-12 09:44 +0800 p7-24 started: 按需求移除 Dashboard 右侧辅助面板。
- 2026-03-12 09:46 +0800 p7-24 completed: 删除右侧两块内容并收敛为双列布局。

### 21.4 Validation Evidence Delta
- TC-77 | stack: ui | command: rg -n "高危提醒|快捷动作|右侧检查器" prototype/s0007/index.html || true | result: pass | note: 右侧两块内容与检查器已移除
- TC-78 | stack: ui | command: rg -n "workspace no-inspector|workspace\.no-inspector" prototype/s0007/index.html prototype/s0007/styles.css | result: pass | note: Dashboard 已切换为两列布局
- TC-79 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007/index.html prototype/s0007/styles.css || true | result: pass | note: 保持纯静态原型

### 21.5 Change Request Delta
- 2026-03-12 09:44 +0800 需求新增：去掉 Dashboard 右侧「高危提醒」与「快捷动作」。

## 22. Addendum (append-only)
### 22.1 Execution Plan Delta
- [x] p7-25 移除 Dashboard 顶部「集群看板」标题文案

### 22.2 Acceptance Delta
- TC-80 Dashboard 主内容区域不再显示「集群看板」标题。
- TC-81 保持其余 Dashboard 内容与结构不变。
- TC-82 保持纯 HTML/CSS 静态原型，不引入业务逻辑脚本。

### 22.3 Execution Log Delta
- 2026-03-12 09:47 +0800 p7-25 started: 按需求移除 Dashboard 页面主标题。
- 2026-03-12 09:48 +0800 p7-25 completed: 删除「集群看板」标题，保留副标题与其余结构。

### 22.4 Validation Evidence Delta
- TC-80 | stack: ui | command: rg -n "集群看板" prototype/s0007/index.html || true | result: pass | note: 页面内已无该标题文本
- TC-81 | stack: ui | command: rg -n "集群概览|节点详情|近期操作日志" prototype/s0007/index.html | result: pass | note: 主体分区结构保持不变
- TC-82 | stack: ui | command: rg -n "<script|fetch\(|invoke\(|tauri|axios|XMLHttpRequest" prototype/s0007/index.html || true | result: pass | note: 仍为纯静态原型

### 22.5 Change Request Delta
- 2026-03-12 09:47 +0800 需求新增：去掉 Dashboard 的「集群看板」标题。
