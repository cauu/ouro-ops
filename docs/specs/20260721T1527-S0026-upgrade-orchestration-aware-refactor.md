# Orchestration-Aware Upgrade Routing

Spec-ID: S0026
状态: active
创建时间: 2026-07-21T10:44:00+08:00
开始时间: 2026-07-21T15:27:12+08:00
完成时间:
前一个 Spec-ID: S0025
结项原因:

## 1. Requirement Details

### Background

一次真实 Upgrade 操作把由 Docker Compose 管理的生产 BP 当作直接 docker run
容器删除并重建。新容器丢失 com.docker.compose.* labels，脱离 Compose 管理，
随后出现 tip 冻结和 ObsoleteNode 等症状。

根因是探针把 orchestration 硬编码为 run，而 upgrade/step 对所有容器都使用
docker rm + docker run。S0026 只修复这条根因链。

### Scope

1. CLI 读取 docker inspect 的 Config.Labels，把目标分为 run、compose 或 unsupported。
2. run：CLI 使用新镜像和升级前相同的受支持容器参数重建，并验证结果。
3. compose：CLI 不重建；Upgrade Skill 告诉用户正确的人工升级步骤。用户完成后，
   agent 复用普通健康观测检查结果。
4. unsupported：CLI 不重建并返回具体原因，Skill 向用户解释。
5. orchestration 只影响 Upgrade 分流，不得使 Compose 上的 KES、观测和诊断回归。
6. 同步 Upgrade Skill、docs/S0020-operations.md 和生成站点。

### Constraints

- CLI 负责可信探测、签名 release/镜像校验、敏感参数处理和所有 Ouro 自动写操作。
- Skill 负责调用顺序、用户解释、批准对话和 Compose 人工指南。
- 所有 Compose 写操作由用户执行；agent 不得通过 CLI、SSH 或 diag exec 代执行。
- upgrade/step 必须在 CLI 内硬性拒绝 orchestration != run。
- env 等敏感值不得进入 agent 上下文；计划只显示脱敏摘要。
- 保持现有签名 N→N+1、preload、confirmation、fleet permit、canary/BP-last 和
  rollback_possible 规则。
- 不引入跨人工步骤的事务、pending 状态、锁或 journal。

### Non-goals

- 不修改 Compose 文件，不执行 docker compose。
- 不新增 upgrade/verify-rebind 或 Compose 专用 operation。
- 不实现 Compose transaction、candidate/finalize、自动校验协议或自动回滚。
- 不扫描 systemd、cron 等外部管理工具；分类只依据 Docker runtime 与 labels。
- 不支持 swarm、k8s、portainer 自动升级。
- 不重构 Deploy，不回填 S0025。
- 不承诺复现任意 Docker 参数；超出 RecreateSpec 的 run 形状在写前拒绝。

## 2. Outline Design

### 2.1 Responsibility Boundary

| 事项 | CLI | Upgrade Skill | 用户 |
| --- | --- | --- | --- |
| 当前状态/编排 | 探测并返回结构化事实 | 调用并选择分支 | 补充缺失的 Compose 路径 |
| 下一版本 | release select 验证唯一签名 N→N+1 | 展示建议并询问是否继续 | 决定 |
| 镜像准备 | upgrade/preload-image | 计划、展示、等待批准、调用 | 批准 |
| run 升级 | upgrade/step 计划、执行、验证 | 驱动批准和 fleet 顺序 | 批准 |
| Compose 升级 | 拒绝自动重建 | 展示人工步骤，完成后再检查 | 修改 Compose 并执行 |
| unsupported | 返回原因且不写容器 | 解释原因和下一步 | 决定后续处理 |

原则：CLI 提供可信事实和受支持的执行机制；Skill 负责对话与工作流；用户执行所有
Compose 写操作。

### 2.2 CLI Observation And Admission

ouro-skills/lib/ouro-probe.sh 停止硬编码 orchestration，改为：

- 存在 com.docker.compose.* 且无冲突标签族：compose。
- 存在 swarm/k8s/portainer 标签、冲突标签或非标准 runtime/socket：unsupported。
- 无已知编排标签：run。

Compose 尽可能输出 project、service、working_dir、config_files、config_hash。
字段残缺时仍是 compose，绝不回退为 run；Skill 询问用户缺失信息。

observability/health 的 container 结果增加：

- orchestration: run | compose | unsupported；
- orchestration_reason；
- compose: 可选的 project/service/working_dir/config_files/config_hash。

CLI 将现有准入拆为：

- base conformity：runtime、rootful、socket、单节点、mount/permission 等公共条件；
- direct-run conformity：base conformity + orchestration == run。

KES、observability、troubleshooting 使用 base conformity；upgrade/step 使用
direct-run conformity。

### 2.3 Run Upgrade

仅 orchestration == run 时，Skill 才调用 upgrade/step。

CLI 的 RecreateSpec 是唯一参数合同：

- name、restart policy、network mode、published ports；
- env、bind mounts、entrypoint、args；
- user、group_add、labels。

CLI 复现当前容器的实际运行值，不要求 agent 区分镜像默认和部署覆盖。无法完整表示的
run 形状在任何 docker rm 前拒绝并说明原因。

CLI 负责 target-side 提取 RecreateSpec、隐藏敏感值、把完整 spec 绑定到批准、apply
前重验漂移、构建固定 argv、执行重建，并验证目标 config digest、支持字段和 readiness。
Skill 继续负责现有 preload/step 的计划、用户批准、confirmation、fleet permit 和
canary relay → remaining relays → BP 顺序。

### 2.4 Compose Manual Handoff

当 orchestration == compose：

1. upgrade/step plan/apply 都在任何 docker rm/run 前返回 manual_compose_required。
2. Skill 使用 CLI 的 Compose facts 和 release select 结果，告诉用户：
   - 当前与推荐版本；
   - 目标 repository@platform-manifest-digest；
   - 需要修改的 project/service/config files；
   - docker compose config 和 docker compose up -d --no-deps <service> 操作模板。
3. Skill 只展示命令，不执行。信息不足时询问用户，不猜路径。
4. 用户完成后通知 agent；agent 重新调用 observability/health，检查：
   - 仍由 Compose 管理；
   - project/service 在可观测时符合预期；
   - image config digest 等于目标；
   - container、socket 和 readiness 正常。

这只是一次新的当前状态检查，不保存 candidate、receipt、baseline 或 pending 状态。
检查失败后继续依赖用户与 agent 对话诊断。

Upgrade Skill 的 red line 改为：

> Agent 不得执行 raw docker/compose 写操作；确认 Compose 管理后，可以向用户展示人工
> Compose 升级命令。目标版本和镜像必须来自 release select，不得使用 latest 或自行
> 选择 digest。

### 2.5 Unsupported And Recovery

以下情况停止自动升级并返回稳定 reason code：

- swarm、k8s、portainer、冲突标签或非标准 runtime/socket；
- run 容器无法生成完整 RecreateSpec；
- 签名 N→N+1、镜像校验、quorum 或 BP-last 规则拒绝。

run 继续使用现有 rollback_possible；不允许逆向恢复时保持 forward recovery/re-sync。
Compose 不做自动回滚，失败后由 agent 根据当前事实和 release 恢复预期与用户继续对话。

接受的边界：无已知编排 labels 的容器按 run 处理，不增加 systemd/cron 推断。

### 2.6 Modules Impacted

- ouro-skills/lib/ouro-probe.sh：orchestration 与 Compose facts。
- crates/ouro/src/supervisor.rs：base/direct-run conformity。
- crates/ouro/src/s0019_cli.rs：健康输出、Upgrade 分流和校验。
- crates/ouro/src/executor.rs：RecreateSpec 的 user/group_add/labels。
- ouro-skills/upgrade/SKILL.md：三分支对话和 Compose 指南。
- docs/S0020-operations.md、站点生成与测试。

## References

- crates/ouro/src/executor.rs
- crates/ouro/src/supervisor.rs
- crates/ouro/src/s0019_cli.rs
- ouro-skills/lib/ouro-probe.sh
- ouro-skills/upgrade/SKILL.md
- docs/S0020-operations.md
- data/allowlist.json
- docs/specs/completed/20260718T2158-S0025-release-ready-external-skills-digest-pinned-operations.md

## 3. Execution Plan

- [x] p1-1 [CLI] 探针和 observability/health 输出 run/compose/unsupported、reason 和
  Compose facts，并补对应 TC-1/TC-2 测试。
- [x] p1-2 [CLI] 拆分 base/direct-run conformity；Compose 上非重建操作保持可用，并补
  对应 TC-3 测试。
- [x] p2-1 [CLI] RecreateSpec 和 docker argv 增加 user、group_add、labels，保持脱敏，
  并补对应 TC-4 测试。
- [x] p2-2 [CLI] upgrade/step 仅允许 run；apply 前重验，成功后验证 digest、参数和
  readiness；其他分支在 mutation 前拒绝，并补对应 TC-5/TC-6 测试。
- [x] p3-1 [Skill] Upgrade Skill 加入三分支、Compose 人工指南、完成后 health 检查和
  不执行 raw Compose 写操作的 red line，并补对应 TC-7/TC-8 测试。
- [x] p3-2 [Docs] 同步 operations、生成站点和对应 TC-9 测试。
- [x] p4-1 [Tests] 执行完整回归门，证明 TC-10 且前述 TC 无回归。

### Item → TC Mapping

| Item | Acceptance |
| --- | --- |
| p1-1 | TC-1, TC-2 |
| p1-2 | TC-3 |
| p2-1 | TC-4 |
| p2-2 | TC-5, TC-6 |
| p3-1 | TC-7, TC-8 |
| p3-2 | TC-9 |
| p4-1 | TC-10 |

## 4. Test And Acceptance Criteria

- TC-1：Compose labels 输出 compose 和可取得的 Compose facts；字段残缺不回退为 run。
- TC-2：无已知编排 labels 输出 run；其他编排、冲突标签或非标准 runtime 输出
  unsupported 和具体原因。
- TC-3：真实上报 compose 后，KES、observability、troubleshooting 不被全局误拒。
- TC-4：RecreateSpec/argv 覆盖 name/restart/network/ports/env/binds/entrypoint/args/
  user/group_add/labels；敏感 env 不出现在 agent 输出。
- TC-5：run apply 前发现 spec 漂移则不写；成功后目标 digest、支持字段和 readiness
  通过。
- TC-6：compose/unsupported 的 upgrade/step plan/apply 在 docker rm/run 前拒绝。
- TC-7：Skill 对 Compose 展示签名版本、不可变镜像、已知 project/service/files 和
  人工 config/up 步骤；信息不足时询问用户。
- TC-8：Skill 不执行 Compose 写操作；用户完成后复用 health 检查，无 transaction、
  finalize 或 verify-rebind。
- TC-9：Skill、operations 和站点对三分支及 CLI/Skill/用户边界描述一致。
- TC-10：现有 release、preload、run canary/BP-last、rollback_possible 和 KES
  回归测试通过。

验收命令：

- cargo test -q
- python3 tests/test_probe.py
- python3 tests/test_s0020_upgrade_workflow.py
- python3 tests/test_s0020_stateless_plan.py
- python3 tests/test_s0020_stateless_apply.py
- python3 tests/test_skill_docs.py
- python3 tests/test_web_generator.py

Pass/fail：TC-1 至 TC-10 全部通过。任一 Compose/unsupported 目标进入 docker rm/run、
run 重建丢失支持字段、敏感 env 进入 agent 输出，或 Compose 上 KES 回归，均为 fail。
每个 item 对应 TC 通过并追加证据后才可完成和提交。

## 5. Execution Log (append-only)

- 2026-07-21T10:44:00+08:00 S0026 以 draft 创建，承接 S0025 §70 的真实 Upgrade 事故。
- 2026-07-21T13:08:00+08:00 至 14:09:00+08:00 draft 评审曾引入 systemd 检查、
  字段分类、strategy 和 Compose 两阶段事务。
- 2026-07-21T15:20:08+08:00 operator 将目标收敛为 run 自动升级、Compose 人工交接、
  其他明确停止；确认后续依赖用户与 agent 对话，不需要事务或跨阶段状态。draft 据此
  重写并明确 CLI / Skill / 用户职责。
- 2026-07-21T15:27:12+08:00 operator 明确要求 activate & execute，并要求每个 item
  完成后立即 commit。promote 前将测试归属收敛到各实现 item，p4-1 只保留最终回归门；
  S0026 promoted 为唯一 active spec，p1-1 开始。
- 2026-07-21T15:31:49+08:00 p1-1 完成：probe 根据容器 labels 输出 run、compose 或
  unsupported，observability/health 透传 reason 和 Compose facts；TC-1、TC-2 通过。
- 2026-07-21T15:35:24+08:00 p1-2 开始：将通用运行约束与仅限 docker run 的重建约束
  分开，避免 Compose 被全局门控误伤。
- 2026-07-21T15:36:59+08:00 p1-2 完成：base conformity 支持 Compose 上的读取、KES
  和普通状态路径，direct-run conformity 仅用于容器重建；TC-3 通过。
- 2026-07-21T15:38:15+08:00 p2-1 开始：扩充可重建参数模型，补齐容器用户、附加组和
  labels，同时维持 plan 输出脱敏。
- 2026-07-21T15:48:48+08:00 p2-1 完成：probe、RecreateSpec 和密封 docker argv 已覆盖
  user、group_add、labels；原有全部运行参数和环境变量脱敏保持有效；TC-4 通过。
- 2026-07-21T15:50:25+08:00 p2-2 开始：收紧 upgrade/step 分流、apply 前漂移检查和
  重建后参数验证，确保非 run 分支在任何重建写操作前停止。
- 2026-07-21T16:03:54+08:00 p2-2 完成：Compose/unsupported plan 与 apply 返回稳定
  reason code，run apply 重验候选和 RecreateSpec，并在 digest、参数、readiness 都通过
  后才完成；TC-5、TC-6 通过。
- 2026-07-21T16:04:52+08:00 p3-1 开始：按 skill-creator 的简洁指令原则，将 Upgrade
  Skill 改为 run 自动、Compose 人工交接、unsupported 停止三分支。
- 2026-07-21T16:07:25+08:00 p3-1 完成：Skill 先读 live orchestration 再分流；Compose
  只展示签名不可变镜像与人工 config/up 命令，等待用户完成后做普通 health 检查；
  TC-7、TC-8 通过。
- 2026-07-21T16:08:14+08:00 p3-2 开始：同步 operations 文档与站点内嵌 canonical
  Upgrade Skill，验证三分支职责描述一致。
- 2026-07-21T16:12:05+08:00 p3-2 完成：operations 明确 CLI/Skill/用户职责，站点生成
  精确内嵌新版 canonical Upgrade Skill；TC-9 通过。
- 2026-07-21T16:16:26+08:00 p4-1 开始：执行 spec 列出的完整 Rust/Python 回归门，并
  额外用 pytest 实际执行站点测试函数。
- 2026-07-21T16:19:59+08:00 p4-1 完成：Rust 177 项、全部指定 Python workflow 与站点
  9 项测试通过；TC-10 及 TC-1 至 TC-9 无回归。所有 execution item 已完成，spec 保持
  active，等待 operator 按流程显式 close。

## 6. Validation Evidence (append-only)

- （待执行）
- TC-1 | stack: python | command: `python3 tests/test_probe.py` | result: pass | note:
  Compose labels 输出 project/service/working_dir/config files/config hash，缺少已知编排
  labels 时输出 run。
- TC-2 | stack: python | command: `python3 tests/test_s0020_observability.py` | result: pass |
  note: observability/health 透传 run/compose 分类；probe 对 Portainer 标签输出 unsupported
  和稳定原因。
- TC-2 | stack: rust/python | command: `cargo fmt --all && python3 tests/test_probe.py &&
  python3 tests/test_s0020_observability.py` | result: pass | note: 补充验证冲突 labels 和
  非标准 runtime 均归一化为 unsupported，并保留稳定 reason。
- TC-3 | stack: rust/python | command: `cargo test -q supervisor && python3
  tests/test_s0020_observability.py && python3 tests/test_s0020_stateless_plan.py` | result: pass |
  note: Compose observation 下 observability、troubleshooting 和 KES plan 均可用，
  direct-run 门仍拒绝 Compose 重建。
- TC-4 | stack: rust/python | command: `cargo test -q executor && python3 tests/test_probe.py &&
  python3 tests/test_s0020_upgrade_workflow.py` | result: pass | note: 重建 argv 保留 name、
  restart、network、ports、env、binds、entrypoint、args、user、group_add、labels；agent
  plan 中敏感 env value 仍被替换为 redaction marker。升级工作流测试因本地监听限制在
  获准的非沙箱环境执行。
- TC-5 | stack: rust/python | command: `cargo test -q supervisor && cargo test -q executor &&
  python3 tests/test_s0020_upgrade_workflow.py` | result: pass | note: 批准后 labels 漂移在首个
  rename/run 前拒绝；成功路径验证目标 digest、完整 RecreateSpec 与 typed readiness。
- TC-6 | stack: python | command: `python3 tests/test_s0020_upgrade_workflow.py` | result: pass |
  note: Compose plan/apply 返回 manual_compose_required，unsupported 返回
  unsupported_orchestration，且拒绝发生在任一 docker 命令前；本地健康监听用例在获准的
  非沙箱环境执行。
- TC-7 | stack: python | command: `python3 tests/test_skill_docs.py` | result: pass | note:
  Compose 分支展示签名版本、repository@manifest、project/service/files 与用户执行的
  config/up 模板；缺失信息时明确询问用户。
- TC-8 | stack: python | command: `python3 tests/test_skill_docs.py` | result: pass | note:
  Agent red line 禁止 raw Docker/Compose 写操作；用户完成后仅复用 observability/health，
  明确不创建 transaction、pending、finalize、receipt 或 verify-rebind。通用
  quick_validate 因本仓库产品 frontmatter 扩展而不适用，仓库专用校验通过。
- TC-9 | stack: python | command: `python3 tests/test_skill_docs.py && python3 -m pytest -q
  tests/test_web_generator.py` | result: pass | note: operations、canonical Skill 与生成站点均
  描述 run/compose/unsupported 三分支；站点 9 项测试通过，本地 HTTP 用例在获准的非沙箱
  环境执行。
- TC-10 | stack: rust/python | command: `cargo test -q`; `python3 tests/test_probe.py`;
  `python3 tests/test_s0020_upgrade_workflow.py`; `python3 tests/test_s0020_stateless_plan.py`;
  `python3 tests/test_s0020_stateless_apply.py`; `python3 tests/test_skill_docs.py`;
  `python3 tests/test_web_generator.py`; `python3 -m pytest -q tests/test_web_generator.py` |
  result: pass | note: Rust 177/177、Upgrade、preload、run canary/BP-last、
  rollback_possible、KES、observability、probe、docs 与站点回归全部通过；需要本地监听的
  workflow/HTTP 测试在获准的非沙箱环境执行。

## 7. Change Requests (append-only)

- 2026-07-21T10:44:00+08:00 upgrade-first；Deploy 留后续 spec；Compose 不得被裸
  docker run 重建。
- 2026-07-21T15:20:08+08:00 删除 systemd 归属证明、字段分类、strategy 框架、
  Compose transaction/candidate/finalize/verify-rebind 和自动回滚；保留 CLI 可信探测、
  run 写安全、Skill 人工指南和用户完成后的普通健康检查。

## 8. Change Request: 直接升级到签名推荐最新版（2026-07-21，append-only）

### 8.1 Requirement Amendment

- 当前运行镜像必须是当前平台签名 release catalog 中的受信镜像；目标必须是该平台
  `recommended` 指向的签名最新版。
- `release select --from <current>` 直接返回签名推荐最新版，不再沿 transitions 选择相邻
  hop；当前已是推荐版本时明确停止。
- `upgrade/preload-image` 和 `upgrade/step` 只接受推荐目标。任意非推荐目标、逆向升级、
  未受信当前镜像或平台不匹配均在 mutation 前拒绝。
- signed transition 不再承担升级准入。只有恰好存在 current → recommended 的 signed
  transition 且其 backward-compatible 为 true 时，才允许自动回滚；缺少该边时升级仍可
  执行，但必须明确失败后采用 forward recovery 或 re-sync。
- Agent Skill、operations、站点提示和本地验收产物必须使用同一语义，不再要求用户按
  `10.5.4-1 → 10.6.4-1 → 10.7.1-3 → 11.0.1-1` 逐跳升级。

### 8.2 Solution Amendment

release catalog 的 `recommended` 是唯一自动升级目标；`allowed` 证明当前和目标的签名
OCI 身份与平台，`transitions` 仅作为可选的精确回滚能力声明保留。CLI 在 plan/apply
两端重新解析同一签名 catalog，验证 current 和 recommended；若有精确 transition，仍
校验该元数据并据此生成 rollback plan，否则输出 `upgrade_transition: null`、
`forward_recovery_or_resync_required`，且不生成 rollback executor plan。无需修改或重签
现有 release catalog。

### 8.3 Execution Plan Amendment

- [x] p5-1 [CLI] 将 release selection、stateless/legacy Upgrade 准入改为直接推荐最新版，
  transition 降级为可选回滚声明，并补 TC-11/TC-12。
- [x] p5-2 [Skill/Docs] 同步 Upgrade Skill、operations 和站点提示，并补 TC-13。
- [x] p5-3 [Tests/Artifacts] 执行完整回归并重建本地 release candidate/网站验收产物，
  证明 TC-14。

| Item | Acceptance |
| --- | --- |
| p5-1 | TC-11, TC-12 |
| p5-2 | TC-13 |
| p5-3 | TC-14 |

### 8.4 Acceptance Amendment

- TC-11：从生产 catalog 中每个低于推荐版的受信 amd64 镜像执行 `release select --from`
  或 upgrade plan，目标均为 11.0.1-1；10.5.4-1 可直接 plan 到 11.0.1-1。
- TC-12：非推荐目标、逆向目标、未知当前镜像被拒绝；无 current → recommended 精确边时
  `upgrade_transition` 为 null、无 rollback plan，失败语义为 forward recovery/re-sync；
  有 backward-compatible 精确边时仍可自动回滚。
- TC-13：Skill、operations 和生成站点明确“直升签名推荐最新版”，不再出现相邻 hop
  准入要求，Compose 人工交接与 run 参数保持规则不变。
- TC-14：原 TC-1 至 TC-10 与新增测试全部通过；本地 release candidate smoke 和站点
  产物重新生成，内含新的直升最新版语义。

### 8.5 Execution Log Amendment (append-only)

- 2026-07-21T16:39:37+08:00 operator 确认官方并不要求按相邻版本链升级，要求执行
  “任意受信当前版本直接升级到签名推荐最新版”；p5-1 开始。
- 2026-07-21T16:48:52+08:00 p5-1 完成：release selection、stateless 与 legacy Upgrade
  均以当前平台 `recommended` 为唯一目标；精确 transition 缺失时仍可 plan，但不生成
  rollback plan，并明确 forward recovery/re-sync。TC-11、TC-12 通过；p5-2 开始。
- 2026-07-21T16:58:31+08:00 p5-2 完成：Upgrade Skill v8、operations、release signing
  指南和网站多语言提示统一为直升签名推荐版；删除相邻 hop 准入语义，保留 transition
  仅描述自动回滚。TC-13 通过；p5-3 开始。
- 2026-07-21T16:59:30+08:00 p5-3 完成：完整 Rust/Python 回归通过；release candidate
  新增并通过 10.5.4-1 直升 11.0.1-1 smoke，网站 dist 重新嵌入 Upgrade Skill v8。
  TC-14 通过，S0026 所有追加 item 完成，保持 active 等待 operator 显式 close。

### 8.6 Validation Evidence Amendment (append-only)

- （待执行）
- TC-11 | stack: rust/python | command: `cargo test -q && python3
  tests/test_release_catalog.py && python3 tests/test_s0020_upgrade_workflow.py` | result: pass |
  note: 生产 catalog 中 10.5.4-1、10.6.4-1、10.7.1-3 均选择并 plan 到 11.0.1-1；
  release selection 不写本地状态。
- TC-12 | stack: rust/python | command: `cargo test -q && python3
  tests/test_s0020_upgrade_workflow.py` | result: pass | note: 非推荐/逆向目标拒绝；10.5/10.6
  直升时 transition/rollback plan 为空且报告 forward recovery/re-sync，10.7 直升保留已签名
  backward-compatible 自动回滚。
- TC-13 | stack: python | command: `python3 tests/test_skill_docs.py && python3 -m pytest -q
  tests/test_web_generator.py` | result: pass | note: canonical Upgrade Skill、operations、签名
  指南、站点 UI 与内嵌 prompt 均明确直升签名推荐最新版；站点 9 项测试通过，本地 HTTP
  用例在获准的非沙箱环境执行。skill-creator 通用 quick_validate 不适用于本仓库带产品
  contract frontmatter 的外部 Skill，使用仓库专用校验。
- TC-14 | stack: rust/python/bash | command: `cargo test -q`; `python3 tests/test_probe.py`;
  `python3 tests/test_s0020_upgrade_workflow.py`; `python3 tests/test_s0020_stateless_plan.py`;
  `python3 tests/test_s0020_stateless_apply.py`; `python3 tests/test_skill_docs.py`; `python3 -m
  pytest -q tests/test_web_generator.py`; `make release-candidate`; `python3
  tests/test_release_candidate.py`; `./web/onboarding/build.sh` | result: pass | note: Rust 177 项、
  全部 workflow/safety/docs/site 回归通过；候选产物 `release-upgrade-select.json` 证明
  10.5.4-1 返回 `upgrade_recommended` 11.0.1-1、transition null；站点 dist 内含 Skill v8。
  需要本地监听或构建缓存/网络的步骤在获准的非沙箱环境执行。

### 8.7 Change Requests Amendment (append-only)

- 2026-07-21T16:39:37+08:00 删除 transition 作为升级准入链路的要求；保留精确 signed
  transition 仅用于自动回滚授权。

## 9. Change Request: 运行时确认每台机器的 SSH 用户（2026-07-21，append-only）

### 9.1 Requirement Amendment

- 官网表单和 canonical Skill 不得假设 SSH 用户为 `cardano`、`root` 或控制机当前用户。
- Agent 完成 mandatory compatibility preflight 后、写入 `pool-spec.yaml` 或首次连接远端前，
  必须先询问用户：所有机器是否共用同一个 SSH 用户名。
- 若共用，Agent 只询问一次用户名并填入全部 `machines[].ssh.user`；若不共用，按机器逐一
  收集用户名并填入对应字段。不得要求密码、私钥内容或其他 secret。
- 官网仍只收集 host；生成的 pool spec 使用明确的逐机占位符，Agent 必须完成上述对话并
  替换全部占位符后才能写文件或发起 SSH。
- CLI 必须使用每台机器在 pool spec 中声明且通过安全语法校验的 `ssh.user`，不再拒绝
  非 `cardano` 用户；host、port、credential reference 和 known-host 校验保持不变。

### 9.2 Solution Amendment

沿用现有 `PoolSpec::validate` 的 `reject_unsafe_username` 作为用户名安全边界。所有 stateless
read/plan/apply、fleet live facts、KES relay evidence 和 diagnostics SSH argv 从目标机器的
`ssh.user` 派生。官网为每台机器生成 `__SSH_USER_<MACHINE_ID>__` YAML 字符串，并在完整
Prompt 中规定“先确认共用或逐机，再替换占位符”的对话步骤。Upgrade Skill 只保留这一
必要决策，不增加用户名表单或持久状态。

### 9.3 Execution Plan Amendment

- [x] p6-1 [CLI] 移除 stateless/diagnostics 的固定 `cardano` 门槛，逐机使用已校验的
  `ssh.user`，并补 TC-15。
- [x] p6-2 [Skill/Site] 更新 Upgrade Skill、官网生成 Prompt、UI 提示和文档，重建站点并
  补 TC-16。
- [x] p6-3 [Artifacts] 重建配对 release candidate，并用非 `cardano` 逐机账号验证候选
  二进制 transport plan，补 TC-17。

| Item | Acceptance |
| --- | --- |
| p6-1 | TC-15 |
| p6-2 | TC-16 |
| p6-3 | TC-17 |

### 9.4 Acceptance Amendment

- TC-15：同一 pool spec 中 BP 与 relay 使用不同安全用户名时，diagnostics、stateless
  read/plan/apply、fleet/KES 辅助连接的 SSH argv 和 principal 均使用对应逐机用户名；SSH
  option injection 等不安全用户名仍被 schema/Rust validation 拒绝。
- TC-16：生成 Prompt 不含 `user: cardano` 或“existing cardano account”；明确先询问是否
  共用用户名、共用时问一次、否则逐机询问，并在所有占位符替换前禁止写 spec/SSH；生成
  站点精确内嵌新版 Upgrade Skill 且相关回归通过。
- TC-17：本地 release candidate 从包含不同 BP/relay SSH 用户名的 pool spec 生成的
  transport plan 使用目标机器声明的 principal，且配对构建、校验和与候选检查通过。

### 9.5 Execution Log Amendment (append-only)

- 2026-07-21T17:18:41+08:00 operator 指出不同机器可能使用不同 SSH 账号，要求 Agent
  执行时先确认是共用账号还是逐机账号；检查发现 CLI 仍硬编码 `cardano`，p6-1 开始。
- 2026-07-21T17:26:00+08:00 p6-1 完成：diagnostics、stateless read/plan/apply、fleet
  live facts 和 KES relay evidence 均使用对应机器的已校验 `ssh.user`；BP/relay 不同账号
  回归通过，TC-15 通过；p6-2 开始。
- 2026-07-21T17:26:33+08:00 p6-2 完成：Upgrade Skill v9 和官网完整 Prompt 在 preflight
  后先询问共用或逐机 SSH 用户名；pool spec 使用逐机 invalid-until-replaced 占位符，所有
  用户名确认前禁止写 spec/SSH。站点重建并在应用内浏览器验证，TC-16 通过。
- 2026-07-21T17:27:30+08:00 p6-3 开始：重建包含 p6-1 CLI 行为的本地配对候选，避免
  实机验收继续使用旧的固定 `cardano` 二进制。
- 2026-07-21T17:31:30+08:00 p6-3 完成：配对 macOS control/Linux runner 候选重建通过；
  候选二进制分别从同一 spec 生成 `bp-admin@192.0.2.1` 与
  `relay-ops@192.0.2.2` transport plan。TC-17 通过，S0026 追加项全部完成并保持 active。

### 9.6 Validation Evidence Amendment (append-only)

- （待执行）
- TC-15 | stack: rust/python | command: `cargo test -q`; `python3
  tests/test_s0020_product_flow.py`; `python3 tests/test_s0020_observability.py`; `python3
  tests/test_s0020_stateless_plan.py`; `python3 tests/test_s0020_stateless_apply.py`; `python3
  tests/test_s0019_dispatch.py` | result: pass | note: BP `bp-admin`、relay `relay-ops` 分别进入
  diagnostics、read/plan/apply、fleet status 和 KES evidence SSH argv/principal；原有 username
  option-injection validation 保持通过。本地健康监听测试在获准的非沙箱环境执行。
- TC-16 | stack: python/ui | command: `python3 tests/test_skill_docs.py`; `python3 -m pytest -q
  tests/test_web_generator.py`; `./web/onboarding/build.sh`; in-app browser generate Upgrade prompt |
  result: pass | note: Prompt 内含 Skill v9、先问共用/多个账号、共用只问一次、否则逐机
  收集、未全部替换前停止；BP/relay 分别出现 `__SSH_USER_BP1__`/
  `__SSH_USER_RELAY1__`，不含 `user: cardano` 或 existing-cardano 文案，浏览器无错误。
- TC-17 | stack: bash/python | command: `make release-candidate`; candidate `op run --op
  observability/health ... --transport-plan` for bp1 and relay1 using
  `valid-distinct-ssh-users.yaml`; `python3 tests/test_pool_spec_schema.py`; `python3
  tests/test_release_candidate.py` | result: pass | note: paired build/checksums pass；候选输出
  principal `bp-admin`/`relay-ops` 和对应 SSH destination，未建立真实 SSH 连接。

### 9.7 Change Requests Amendment (append-only)

- 2026-07-21T17:18:41+08:00 SSH 用户从固定产品假设改为 Agent 对话后写入的逐机 pool
  spec 事实；官网不新增用户名输入字段。

## 10. Change Request: 所有公开 Skill 独立完成 SSH 账号确认（2026-07-21，append-only）

### 10.1 Requirement Amendment

- Upgrade、Runtime、Observability、KES Rotation、Troubleshooting、Deploy 六个 canonical Skill
  必须各自独立包含同一套 SSH 账号确认规则，单独复制任一 Skill 都能完成正确对话。
- 六份规则必须保持一致：mandatory compatibility preflight 后、写 pool spec/解析 credential/
  首次 SSH 前确认全部机器共用还是逐机使用不同用户名；共用只问一次，不共用收集逐机映射；
  全部占位符解析前停止；不询问密码、私钥内容或其他 secret。
- 官网继续精确嵌入 canonical Skill；全局 Prompt 只负责表单上下文、生成逐机占位符和提示执行
  顺序，不得成为任一 Skill 独立使用时不可缺少的行为定义。
- 删除公开 Skill 中仍把 `cardano` 描述为固定 SSH principal 的过时文案。

### 10.2 Solution Amendment

以 Upgrade Skill v9 的 `SSH account discovery` 章节作为唯一文本模板，逐字加入其余五个
公开 Skill，并提升被修改 Skill 的版本。仓库测试抽取六份同名章节并要求完全一致；站点测试
继续要求每个内嵌 payload 与 canonical 文件完全相同，并逐一验证 SSH 对话边界。

### 10.3 Execution Plan Amendment

- [x] p7-1 [Skills/Site] 对齐六个公开 Skill 的 SSH discovery 规则，移除固定 principal
  文案，重建官网并补 TC-18。

| Item | Acceptance |
| --- | --- |
| p7-1 | TC-18 |

### 10.4 Acceptance Amendment

- TC-18：六个 canonical Skill 均包含字节一致的 `SSH account discovery` 章节；每个站点
  payload 与对应 canonical Skill 完全一致并包含该章节；不再出现固定 `cardano` SSH account/
  principal 文案；Skill 文档和站点生成回归通过。

### 10.5 Execution Log Amendment (append-only)

- 2026-07-21T17:38:58+08:00 operator 确认网站与 Skill 应保持同一规则，且任一 Skill
  单独使用也应完整；p7-1 开始。
- 2026-07-21T17:43:33+08:00 p7-1 完成：六个公开 Skill 使用完全一致的 SSH account
  discovery 章节；Runtime v5、Observability v4、KES Rotation v21、Troubleshooting v4、
  Deploy v3，Upgrade 保持 v9；Observability 删除固定 `cardano` SSH principal 文案，站点
  重新生成且内嵌 canonical 副本一致。TC-18 通过，S0026 保持 active 等待 operator 显式
  close。

### 10.6 Validation Evidence Amendment (append-only)

- （待执行）
- TC-18 | stack: python/site | command: `python3 tests/test_skill_docs.py`; `python3 -m pytest -q
  tests/test_web_generator.py`; `./web/onboarding/build.sh` | result: pass | note: 文档测试抽取六份
  `SSH account discovery` 并要求完全一致；站点 9 项测试逐个验证 payload 与 canonical Skill
  完全一致、包含同一对话规则，且本地 HTTP 产物可访问。回环监听用例在获准的非沙箱环境
  执行。

### 10.7 Change Requests Amendment (append-only)

- 2026-07-21T17:38:58+08:00 SSH discovery 从 Upgrade 专属章节提升为六个公开 Skill 的
  一致独立前置规则；网站全局 Prompt 保留组装职责，不再弥补 Skill 行为缺口。

## 11. Change Request: Skill 是 SSH 决策唯一真实来源（2026-07-21，append-only）

### 11.1 Requirement Amendment

- canonical Skill 是 SSH 账号确认行为的唯一真实信息来源。
- 网站 Prompt wrapper 只提供逐机占位符、pool spec 模板和执行顺序上下文，不得再次定义
  “共用或逐机账号”、询问次数、secret 边界或停止条件。
- 网站表单提示不得复述 SSH 决策逻辑，只能说明执行时由内嵌 canonical Skill 确认用户名。
- 生成测试必须同时证明六个内嵌 Skill 仍具备完整规则，以及源 wrapper 只委托 Skill、没有
  第二份行为定义。

### 11.2 Solution Amendment

删除网站 wrapper 和多语言表单提示中的 SSH 决策复述。wrapper 只标记 YAML 中存在逐机
invalid-until-replaced 占位符，并要求严格按内嵌 Skill 的 `SSH account discovery` 章节解析；
完成该 Skill 章节后才进入写 pool spec 的顺序说明。新增源模板级负向测试，防止决策文案
重新进入 wrapper；已有 payload 测试继续验证六个 canonical Skill 的完整且一致规则。

### 11.3 Execution Plan Amendment

- [x] p8-1 [Site/Tests] 将 SSH 决策唯一归属 canonical Skill，简化 wrapper 与多语言提示，
  重建站点并补 TC-19。

| Item | Acceptance |
| --- | --- |
| p8-1 | TC-19 |

### 11.4 Acceptance Amendment

- TC-19：源网站 wrapper 不含共用/逐机询问、用户名次数、secret 或停止策略的重复定义；只
  说明逐机占位符并委托内嵌 Skill 的 `SSH account discovery`。六个生成 payload 仍与 canonical
  Skill 完全一致且包含完整 SSH 规则；多语言 UI 不再复述决策；站点生成与本地 HTTP 回归通过。

### 11.5 Execution Log Amendment (append-only)

- 2026-07-21T17:58:39+08:00 operator 确认修复 review P2，要求 canonical Skill 成为 SSH
  决策唯一真实信息来源；p8-1 开始。
- 2026-07-21T18:00:17+08:00 p8-1 完成：网站 wrapper 仅描述逐机占位符并委托内嵌 Skill
  的 `SSH account discovery`；删除共用/逐机、询问次数、secret 和停止规则复述，四种语言
  UI 也只指向 canonical Skill。源模板负向守卫和完整网站回归通过，TC-19 通过；S0026
  保持 active。

### 11.6 Validation Evidence Amendment (append-only)

- （待执行）
- TC-19 | stack: python/site | command: `python3 tests/test_skill_docs.py`; `python3 -m pytest -q
  tests/test_web_generator.py`; `./web/onboarding/build.sh` | result: pass | note: 六个 Skill 继续
  包含完全一致的完整 SSH 规则；新增测试证明源 wrapper/UI 不含第二份决策策略，只提供逐机
  placeholder 上下文并委托 canonical Skill；站点 10 项测试及本地 HTTP 访问通过。

### 11.7 Change Requests Amendment (append-only)

- 2026-07-21T17:58:39+08:00 网站从“复制一份 SSH 行为规则”改为“提供上下文并显式委托
  canonical Skill”；六个 Skill 的独立规则保持不变。

## 12. Change Request: 保留 direct-run json-file 日志轮转参数（2026-07-22，append-only）

### 12.1 Requirement Amendment

- relay1 实机 `docker inspect` 仅因 `HostConfig.LogConfig` 包含 `json-file` 的
  `max-file=3`、`max-size=50m` 而被 Upgrade recreate 探针拒绝；其余 direct-run 条件均通过。
- Upgrade 不得要求运维删除现有日志轮转策略，也不得在 recreate 时静默丢弃；产品必须把
  这两个参数纳入受支持、可审批、可漂移检测、可回滚的密封模型。
- 仅新增 `json-file` driver 下 `max-file` 与 `max-size` 的封闭子集；其他 driver、其他 log
  option、空键、非法值或非字符串值继续在 mutation 前 fail-closed。
- 已完成的 preload 保持有效且不改变运行容器；修复后重新执行 `upgrade/step --plan` 即可。

### 12.2 Solution Amendment

`ouro-probe.sh` 将支持的日志配置规范化为 `RecreateSpec.log_driver` 与排序后的
`log_options`。默认/空 `json-file` 表示无需显式 argv；存在受支持 option 时密封 executor
生成独立参数数组 `--log-driver json-file --log-opt key=value`。Rust 侧再次校验 driver、键和值，
避免仅依赖 shell 探针。序列化后的完整 `RecreateSpec` 已进入 HMAC candidate binding、apply
前 fresh-observation equality、rollback recreate 与 postcondition equality，因此新增字段自动进入
这些安全边界。Skill 只需说明支持范围和 fail-closed 结果，不承载参数解析。

### 12.3 Execution Plan Amendment

- [x] p9-1 [Probe/CLI/Tests] 扩展 `RecreateSpec` 与密封 executor，支持并验证 `json-file`
  `max-file`/`max-size`，补 TC-20。
- [x] p9-2 [Skill/Docs/Artifacts] 同步 Upgrade Skill/operations，重建配对 candidate 与站点，
  补 TC-21。

| Item | Acceptance |
| --- | --- |
| p9-1 | TC-20 |
| p9-2 | TC-21 |

### 12.4 Acceptance Amendment

- TC-20：探针对 `LogConfig={Type:json-file, Config:{max-file:3,max-size:50m}}` 生成非空
  recreate；最终 plan/executor 包含精确 `--log-driver json-file` 与两个 `--log-opt`，字段进入
  candidate/drift/postcondition/rollback；未知 driver/option 和非法值仍返回 recreate null 或 Rust
  validation refusal，且 mutation 之前无 Docker 写操作。
- TC-21：Upgrade Skill 与 operations 明确 direct-run 会原样保留受支持的 json-file 轮转参数；
  配对 release candidate、站点和回归重新生成并通过，候选 control binary 内嵌本次 Linux
  runner；实机只需重跑 upgrade plan，不要求删除 log-opt。

### 12.5 Execution Log Amendment (append-only)

- 2026-07-22T12:01:52+08:00 relay1 实机确认唯一未建模项为 `json-file` 的
  `max-file=3`/`max-size=50m`；operator 选择产品侧扩展，不改变运行态；p9-1 开始。
- 2026-07-22T12:07:03+08:00 p9-1 完成：probe 与 Rust executor 双层限制
  `json-file`/`max-file`/`max-size`，RecreateSpec、审批 plan、实际 run、HMAC candidate、fresh
  drift、postcondition 和 rollback 共用同一字段；日志策略漂移在首次 Docker mutation 前拒绝，
  未知/非法配置保持 fail-closed。TC-20 通过。
- 2026-07-22T12:07:20+08:00 p9-1 独立提交完成；p9-2 开始同步 Agent 决策文案、公开
  operations 与本地验收产物。
- 2026-07-22T12:11:14+08:00 p9-2 完成：Upgrade Skill v10 与 operations 明确保留受支持
  json-file 轮转策略且不要求运维删除；站点重新嵌入 canonical Skill v10；macOS control 与
  Linux x86_64 runner 配对 candidate 重建，manifest/hash/contract 通过，runner 二进制含 probe
  与 Rust 双层 logging guard。TC-21 通过，S0026 保持 active 等待实机重跑 plan。

### 12.6 Validation Evidence Amendment (append-only)

- （待执行）
- TC-20 | stack: rust/python | command: `cargo test -q`; `python3 tests/test_probe.py`;
  `python3 tests/test_s0020_stateless_plan.py`; `python3 tests/test_s0020_stateless_apply.py`;
  `python3 tests/test_s0020_upgrade_workflow.py` | result: pass | note: Rust 178 项通过；relay1
  日志轮转 fixture 生成 recreate 并在 sealed run 中精确重放两个 log-opt；driver/option/字符串
  类型/值负例返回 null 或 validation refusal；`50m→100m` fresh drift 在 rename/run 前拒绝。
  stateless apply 的本地监听用例在获准的非沙箱环境执行。
- TC-21 | stack: bash/python/site | command: `make release-candidate`; `python3
  tests/test_release_candidate.py`; `python3 tests/test_skill_docs.py`; `./web/onboarding/build.sh`;
  `python3 -m pytest -q tests/test_web_generator.py`; candidate `contract check` | result: pass |
  note: Skill v10/operations/站点一致；配对 candidate 校验和与 descriptor 通过，Linux runner
  strings 含 probe 的 max-file/max-size 分支及 Rust 未支持 driver/option refusal；网站 10 项
  含本地 HTTP 回归通过。Zig 缓存和 HTTP 监听步骤在获准的非沙箱环境执行。

### 12.7 Change Requests Amendment (append-only)

- 2026-07-22T12:01:52+08:00 direct-run 密封模型新增常用 json-file 日志轮转参数；不扩大到
  任意 Docker logging 配置，未知形状继续拒绝。

## 13. Change Request: 信任 BP 10.5.3-1 精确 OCI 身份（2026-07-22，append-only）

### 13.1 Requirement Amendment

- BP 实机当前运行 `ghcr.io/blinklabs-io/cardano-node:10.5.3-1`，其 image config digest 为
  `sha256:ea53539f722c08ced4df221e329438e1f48ae80ef196687753c2583081421905`；该已知官方镜像
  必须进入可信 release catalog，才能从当前版本执行 upgrade。
- 可信项必须记录从官方 GHCR 解析出的完整 linux/amd64 OCI 三元组：index digest
  `sha256:ec379c67d1ef2f0e4478bf3b28ac16db3a62535d6af8f92d6d1e53766a382afb`、platform
  manifest digest `sha256:3f2aa6636cae566d89faf44b4a1640fd1619b715306664c0d3db0b27dcb31dd4`
  与上述 image config digest；不得仅信任 tag 或截断 digest。
- `10.5.3-1` 可直接选择当前签名推荐版本 `11.0.1-1`，不新增强制逐跳升级链；没有显式
  transition 时不承诺自动回滚，失败后按现有 forward recovery/resync 语义处理。
- production catalog 版本从 5 提升到 6，并继续使用 CLI 内置 production Ed25519 公钥签名。
  本次不自动发布或推送 catalog；真实候选 E2E 读取固定 main URL，需在签名提交发布后生效。

### 13.2 Solution Amendment

只修改 `data/releases.json` 的签名 release catalog：在唯一 contract 的 `allowed` 中加入
`10.5.3-1` linux/amd64 完整 OCI 身份、提升 `allowlist_version` 并重新签名；不修改静态
`data/allowlist.json`，不修改 CLI 代码，也不为该版本添加 transition。catalog 回归将它纳入
全部历史 amd64 镜像集合，证明签名验证通过且直接选择 11.0.1-1、transition 为 null。

### 13.3 Execution Plan Amendment

- [x] p10-1 [Catalog/Tests] 加入并签名 BP 10.5.3-1 linux/amd64 OCI 身份，验证直接推荐选择，
  补 TC-22。

| Item | Acceptance |
| --- | --- |
| p10-1 | TC-22 |

### 13.4 Acceptance Amendment

- TC-22：production release catalog v6 以既有 production key 签名并通过 schema/签名校验；
  `10.5.3-1` 的完整 linux/amd64 OCI 三元组唯一存在于 allowed；从其 image config digest
  执行 release selection/upgrade plan 时直接得到签名推荐 `11.0.1-1` 且 transition null；
  tamper 与未知镜像仍 fail-closed。

### 13.5 Execution Log Amendment (append-only)

- 2026-07-22T12:15:54+08:00 operator 提供 BP 当前 `10.5.3-1` config digest 截断展示；从
  官方 GHCR 解析并核对完整 linux/amd64 OCI 三元组，p10-1 开始。
- 2026-07-22T12:18:21+08:00 p10-1 完成：production catalog 提升到 v6，以既有 production
  Ed25519 authority 重新签名；10.5.3-1 linux/amd64 的 index/manifest/config 完整身份进入
  allowed。从该 config 选择时直接返回 11.0.1-1、transition null；不新增升级链或自动回滚
  声明。TC-22 通过，S0026 保持 active；真实候选读取 main 固定 URL，待该签名提交发布后
  才能在远端 E2E 生效。

### 13.6 Validation Evidence Amendment (append-only)

- （待执行）
- TC-22 | stack: rust/python/catalog | command: signer `inspect`/`sign`; `cargo test -q`;
  `python3 tests/test_release_catalog.py`; `python3 tests/test_s0020_upgrade_workflow.py`;
  `OURO_RELEASES_FILE=data/releases.json target/debug/ouro-ops release select --platform
  linux/amd64 --from sha256:ea53539f722c08ced4df221e329438e1f48ae80ef196687753c2583081421905` |
  result: pass | note: production public key
  `3ceb1920f30d3768a7b979c563b4e1738dc7708e8ed6e91d6e32bd7a0df165dd` 匹配内置 trust
  root，canonical SHA-256 `e7e5b0252e76110608b5a2f3d999af1d6ecfec68c6f4bbc9148f7d366068d940`；
  Rust 178 项及两个端到端 Python 套件通过；精确选择输出 policy_version 6、
  `upgrade_recommended` 11.0.1-1、transition null，篡改与未知镜像拒绝回归继续通过。

### 13.7 Change Requests Amendment (append-only)

- 2026-07-22T12:15:54+08:00 production catalog 扩展一个已知 BP 历史镜像身份；推荐目标、
  direct-upgrade 策略、transition 与 CLI 信任根均不改变。
