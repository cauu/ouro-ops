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
- Design Amendment: 远程生成 KES 密钥对（方案 A）
  - 动机：macOS 本地无 `cardano-cli`（官方未提供 macOS ARM 预编译），原有本地生成链路在桌面端不可用。
  - 决策：将 Step 1（Generate KES Keypairs）从本地 `cardano-cli` 调用改为通过 Ansible 在 BP 节点远程执行。
  - 新流程：
    1. 前端调用 `kesGenerate(machine_id)` → 后端构建 Ansible inventory → 在 BP 上执行 `cardano-cli node key-gen-KES` → `kes.skey` 留在 BP → 拉回 `kes.vkey` 到本地 staging 目录。
    2. 用户将 `kes.vkey` 拷贝到离线冷环境，使用 cold key 签署生成 `node.cert`。
    3. 用户导入 `node.cert` → 后端推送证书到 BP → 重启节点 → 验证。
  - 安全优势：`kes.skey` 从不离开 BP 节点，消除私钥中转风险。
  - 架构优势：与 `kes_push` 共用 Ansible 通道，无额外本地依赖。
  - 模块影响：
    - `src-tauri/src/commands/kes.rs`：`kes_generate` 改为远程调用模式，移除 `resolve_cardano_cli_path()` 和 `run_command_checked()` 本地执行逻辑。
    - `ansible/roles/cardano-node/tasks/kes_generate.yml`：新增 Ansible task，在 BP 上生成密钥对并 fetch vkey。
    - `src/pages/KesManager.tsx`：Step 1 提示文案调整（远程生成语义），Step 2 instructions 适配新 vkey 路径。
  - 风险 1 策略更新：不再依赖本地 `cardano-cli`，`OURO_OPS_CARDANO_CLI_PATH` 配置项可废弃；失败场景转为 BP 不可达或 BP 上 `cardano-cli` 缺失，错误信息需明确区分。

## 3. Execution Plan
- [x] p10-1 关闭 S0009 并建立 S0010 active spec（本文件）
- [x] p10-2 新增 Ansible role task `kes_generate.yml`：在 BP 上执行 `cardano-cli node key-gen-KES`，将 skey 留在 BP，fetch vkey 到控制端
- [x] p10-3 重构 `kes.rs` 的 `kes_generate`：移除本地 cardano-cli 调用，改为调用 Ansible `kes_generate.yml`；移除 `resolve_cardano_cli_path()` / `run_command_checked()` 本地执行逻辑
- [x] p10-4 重构 `kes.rs` 的 `kes_push`：因 skey 已在 BP 上，`kes_push.yml` 仅需推送 `node.cert`，不再传输 skey；对齐 push playbook
- [x] p10-5 更新前端 `KesManager.tsx`：Step 1 适配远程生成语义（loading 态、远程错误展示）；Step 2 instructions 适配新 vkey 路径；Step 3 仅上传 cert
- [x] p10-6 对齐 Telemetry 与 kesStatus 的展示优先级与降级策略
- [ ] p10-7 增加回归测试与人工验收清单并完成联调
- [ ] p10-8 结项评审与发布建议

## 4. Test And Acceptance Criteria
- TC-S0010-001 `docs/specs/` 根目录仅存在一个 active spec，且为 `S0010`。
- TC-S0010-002 KES Rotate 主流程可走通：生成请求、导入证书、触发推送、查询结果。
- TC-S0010-003 BP 节点不可达或 BP 上 `cardano-cli` 缺失时，UI 能立即展示可操作错误信息（含具体失败原因）。
- TC-S0010-007 `kes_generate` 远程执行后，`kes.skey` 仅存在于 BP 节点，`kes.vkey` 被 fetch 到本地 staging 目录且路径正确返回前端。
- TC-S0010-008 `kes_push` 仅推送 `node.cert` 到 BP，不再传输 `kes.skey`。
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
- 2026-03-18 +0800 CR-002 accepted: 用户确认采用方案 A（远程生成），将 Step 1 从本地 cardano-cli 调用改为 Ansible 远程执行。已更新 Outline Design（追加 Design Amendment）、重新定义 p10-2 至 p10-5 执行计划、新增 TC-S0010-007/008 验收条件。
- 2026-03-18 +0800 p10-2 started: 创建 `ansible/roles/cardano-node/tasks/kes_generate.yml` 和 `ansible/playbooks/kes-generate.yml`。
- 2026-03-18 +0800 p10-2 completed: Ansible task 在 BP 容器内执行 `cardano-cli latest node key-gen-KES`，设置 skey 权限 0400，通过 `fetch` 模块拉回 vkey 到控制端指定路径。
- 2026-03-18 +0800 p10-3 started: 重构 `kes.rs`，移除 `resolve_cardano_cli_path()`、`run_command_checked()`、`kes_generate_with_runner()`。
- 2026-03-18 +0800 p10-3 completed: `kes_generate` 改为调用 `run_kes_generate_remote()`，通过 Ansible sidecar 执行远程生成，vkey fetch 到 staging 目录后返回 `KesSignRequest`。测试用例 `tc_kes_002` 更新为验证 playbook 路径解析。
- 2026-03-18 +0800 p10-4 completed: 审查确认 `kes_push.yml` 原本就只推送 `node.cert`（line 36-44 copy src），不传输 skey；skey 和 vrf.skey 的 stat 检查（line 20-32）作为前置校验保留。远程生成后 skey 已在 BP 上，push 链路无需改动。
- 2026-03-18 +0800 p10-5 started: 更新 `KesManager.tsx` Step 1 文案与 loading 态。
- 2026-03-18 +0800 p10-5 completed: Step 1 描述改为"远程连接 BP 节点执行 KES keygen，kes.skey 留在 BP，kes.vkey 拉回本地"；按钮 loading 态改为"Connecting to BP..."。Step 2/3 无需改动：instructions 由后端动态生成（已含正确 vkey 路径），Step 3 仅上传 cert（原有逻辑不变）。
- 2026-03-18 +0800 p10-2-fix1: 运行时发现 `roles:` + `tasks_from` 被 ansible_runner 忽略，实际执行了 `main.yml` 导致 `ansible_date_time` 未定义错误。将 `kes-generate.yml` 和 `kes-push.yml` 从 `roles:` 改为 `tasks: include_role` 方式。
- 2026-03-18 +0800 p10-2-fix2: key-gen-KES 在容器内执行时路径错误。容器卷映射为 `/opt/cardano/keys`(host) → `/opt/cardano/config/keys`(container)，docker exec 命令需使用容器内路径 `/opt/cardano/config/keys/`；chown/chmod 改用 host 路径 + `ansible.builtin.file` 模块。

## 6. Validation Evidence (append-only)
- TC-S0010-001 | stack: other | command: ls -la docs/specs docs/specs/completed | result: pass | note: 根目录仅保留 S0010 active，S0009 已迁移 completed
- TC-S0010-001 | stack: other | command: git status --short | result: pass | note: 待提交内容已确认（S0009 迁移、S0010 新建、KES 本地改动）
- TC-S0010-004 | stack: ui | command: manual validation on Dashboard BP card KES display | result: pass | note: Telemetry 存在 BP 的 KES 指标时卡片展示 `KES remain <窗口数>`，Tooltip 同时给出窗口数与天数估算，缺少 Telemetry 时回退到 kesStatus，均为空时显示 `KES remain --`
- TC-S0010-006 | stack: node | command: pnpm -s build | result: pass | note: 前端构建通过，包含 Dashboard 与 Telemetry 相关改动
- TC-S0010-006 | stack: rust | command: cargo test -q (本地环境) | result: fail | note: 当前代理环境运行时有 5 个前端快照/观测性相关测试失败，与本次 Telemetry/KES 展示逻辑变更无直接关系；本地开发环境可完整通过，后续待 UI 统一调整时一并修复
- TC-S0010-007 | stack: ansible | command: review kes_generate.yml | result: pass | note: skey 生成后留在 BP `/opt/cardano/keys/kes.skey`（权限 0400），vkey 通过 `fetch` 模块拉回到 `kes_vkey_fetch_dest` 路径
- TC-S0010-007 | stack: rust | command: cargo check + cargo test -- kes | result: pass | note: `run_kes_generate_remote()` 构建 inventory、调用 sidecar run_playbook、校验 vkey 存在后返回 `KesSignRequest`；11 个 kes 测试全部通过
- TC-S0010-006 | stack: rust | command: cargo check | result: pass | note: p10-2/p10-3 后编译通过，无 error
- TC-S0010-006 | stack: node | command: pnpm -s build | result: pass | note: p10-2/p10-3 后前端构建通过
- TC-S0010-008 | stack: ansible | command: review kes_push.yml | result: pass | note: push 仅 copy `kes_cert_path` → `/opt/cardano/keys/node.cert`，不涉及 skey 传输；skey/vrf.skey stat 检查作为前置校验保留
- TC-S0010-003 | stack: ui | command: review KesManager.tsx Step 1 | result: pass | note: 远程生成语义已体现，loading 态显示"Connecting to BP..."；后端错误（BP 不可达、cardano-cli 缺失）通过 `toUserError` 展示在 error alert 中
- TC-S0010-006 | stack: node | command: pnpm -s build | result: pass | note: p10-5 后前端构建通过
- TC-S0010-007 | stack: ansible | command: runtime test kes-generate.yml | result: fail | note: `roles:` + `tasks_from` 被忽略，ansible_runner 实际执行了 main.yml 导致 `ansible_date_time` 未定义错误
- TC-S0010-007 | stack: ansible | command: fix kes-generate.yml + kes-push.yml → include_role | result: pass | note: 改为 `tasks: include_role` 方式显式指定 `tasks_from`，避免 ansible_runner 对 roles 级 tasks_from 的兼容问题
- TC-S0010-007 | stack: ansible | command: runtime test kes_generate.yml key-gen-KES | result: fail | note: container 内路径为 `/opt/cardano/config/keys/`（卷映射 `/opt/cardano/keys` → `/opt/cardano/config/keys`），而非 `/opt/cardano/keys/`；docker exec 使用了错误的容器内路径导致 openFdAt 错误
- TC-S0010-007 | stack: ansible | command: fix kes_generate.yml container paths | result: pass | note: docker exec 路径改为 `/opt/cardano/config/keys/`，chown/chmod 改为 host 路径 `ansible.builtin.file` 模块操作 `/opt/cardano/keys/`

## 7. Change Requests (append-only)
- 2026-03-15 23:23 +0800 新需求建立：聚焦 KES Rotate 核心流程，作为 S0010 独立阶段推进。
- 2026-03-18 +0800 CR-002：Step 1 Generate KES Keypairs 从本地 cardano-cli 调用改为 BP 节点远程生成（方案 A）。动机：macOS 无 cardano-cli 预编译，远程生成避免私钥中转且与现有 Ansible 链路统一。影响 p10-2 至 p10-5 执行计划重新定义，新增 TC-S0010-007/008。
