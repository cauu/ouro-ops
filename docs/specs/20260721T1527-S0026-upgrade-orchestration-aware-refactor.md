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
- [ ] p4-1 [Tests] 执行完整回归门，证明 TC-10 且前述 TC 无回归。

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

## 7. Change Requests (append-only)

- 2026-07-21T10:44:00+08:00 upgrade-first；Deploy 留后续 spec；Compose 不得被裸
  docker run 重建。
- 2026-07-21T15:20:08+08:00 删除 systemd 归属证明、字段分类、strategy 框架、
  Compose transaction/candidate/finalize/verify-rebind 和自动回滚；保留 CLI 可信探测、
  run 写安全、Skill 人工指南和用户完成后的普通健康检查。
