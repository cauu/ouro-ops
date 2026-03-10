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
