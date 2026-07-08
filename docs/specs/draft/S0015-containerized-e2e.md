# 容器化端到端验收（CLI 确定性 E2E + skill/agent 行为验收）

Spec-ID: S0015
Status: draft
Created Time: 2026-07-08T06:20:00+08:00
Start Time:
Completion Time:
Previous Spec-ID: S0014
Closure Reason:

> 本 spec 承接 S0014（已 delivered）。S0014 交付了 `ouro` CLI + `ouro-skills` 工具面与机制级安全模型，
> 但验收停在**契约/机制级**：远端执行是 dry-run、L2 是 marker、节点状态是注入 snapshot、principal 隔离只有
> 文案。本 spec 的目标是用**容器化自测床**（不碰真实生产）把验收推到**完备的端到端**，作为项目「验收通过」
> 的最终标准。draft 阶段可自由编辑；activate 时补 `Start Time` 并迁入 `docs/specs/` 根目录。

## 1. Requirement Details

### Background
- 核心交付物是一组 **skill**（`ouro-skills/`），skill 依赖 **CLI**（`ouro`）。二者的正确性性质不同：CLI 是确定性的、
  可脱离 AI 运行；skill 的决策层依赖 agent，是非确定性的。因此验收必须分层，且**绝不用非确定的 agent 路径去回归
  确定的 CLI 正确性**。
- S0014 的占位（`ssh.rs` 仅 `prepare` 不执行、L2 脚本 touch marker、`status/verify` 吃注入 snapshot、
  TC-14/15 只验文案）使真实运维行为未被验证。容器化提供一个**可复现、可弃、无生产副作用**的落地场把它们变真。

### Scope
- 用 docker-compose 自测床完成端到端验收，分**三层测试金字塔**：
  - **T1 — CLI 纯逻辑**（确定性，无 AI，无容器）：已由 S0014 `cargo test` 覆盖，本 spec 只做必要补强。
  - **T2 — CLI + L2 端到端**（确定性，无 AI，**需容器**）：`ouro tool run` → 真 SSH → sudo allowlist → 真跑
    L2 脚本 → 真 `cardano-cli`/节点；幂等、回滚、两类 principal 隔离、安全负向。
  - **T3 — skill 决策**（非确定，**需容器 + agent**）：agent 读 SKILL.md 后的行为不变式。
- 实现 S0014 遗留的三处占位：CLI 的真实 SSH 执行、L2 脚本真实动作、目标机两类 principal + sudoers。
- 节点保真度：用 **private cardano devnet**（秒级出块、真 socket/`cardano-cli`），**不连公网、不做全同步**。

### Constraints
- 不连公网 mainnet、不做全量同步；节点在目标容器内直跑（对「节点跑容器」做一次显式简化并记录）。
- T3 断言只打在**可观测产物**上（审计 DB、harness transcript、退出码、文件副作用），**不依赖 agent 话术**。
- 分级执行：T1 每 PR；T2 每 CI（分钟级 docker）；T3 nightly / 手动 gate，且每场景多跑取不变式。
- 复用 S0014 的 `pool-spec.schema.json` / `tool-output.schema.json` 契约与退出码语义，不回退。
- 一切写仍只经 `ouro tool run`；容器化不得引入绕过审计门/确认门的新通道。

### Non-goals
- 生产环境部署与真实资金交易。
- 公网 mainnet/preprod 全同步验收（只用 private devnet）。
- 自研监控面板（仍交 Prometheus/Grafana）。
- MCP server（延续 S0014 延后）。

## 2. Outline Design

### 2.1 架构 / 模块
- **自测床（docker-compose）**：`control`（装 `ouro`、持 SSH 私钥、扮 agent/控制面）+ `bp1` + `relay1` + `relay2`
  （各含 sshd、两类 principal、cardano-node），+ private devnet genesis。
- **CLI 真执行**：`ouro` SSH runner 由 `dry_run` 改为真发（经 `ouro-exec` + sudoers allowlist 跑 `ouro tool run`）。
- **L2 真动作**：`deploy/upgrade/runtime/observability` 脚本从 marker 落成真实动作（渲染 config、起/查节点、KES 安装）。
- **principal 落地**：容器内建 `ouro-diag`（普通用户、无 sudo、对密钥目录无读权限）与 `ouro-exec`（sudoers 白名单）。
- **agent harness（T3）**：headless Claude Code / Agent SDK，把 `ouro-skills/` 装入并驱动典型场景。
- **不变式断言器 + secret 扫描器**：审计/transcript/日志的确定性检查器。

### 2.2 三层测试金字塔（验收对象与判定）
| 层 | 验收对象 | 需要 | 「符合预期」判定 |
| --- | --- | --- | --- |
| T1 | CLI 纯逻辑 | cargo | golden / 单测断言 |
| T2 | CLI+L2 真端到端 | docker，无 agent | JSON 契约 + 真实副作用 + 退出码 + 安全负向 |
| T3 | skill 决策 | docker + agent | 行为不变式（下）|

**T3 行为不变式（断言产物，非话术）**：
1. 审计 DB 的写事件**全部**来自 `ouro tool run`（无旁路写）。
2. 任一 `exit 30` 之后、下一次写之前**必有** rollback 事件。
3. transcript + audit + `/var/log` + `set -x` 输出中**命中 0 个**已知 skey/VRF/cold 指纹。
4. 跨机顺序为 BP-last；破坏「BP + ≥1 relay 在线」的调用被拒（exit 10）。
5. `exit 40` 后**无任何**后续写操作。

### 2.3 数据模型 / 接口
- 复用 `schemas/pool-spec.schema.json`、`schemas/tool-output.schema.json`、退出码 0/10/20/30/40。
- 新增容器 fixtures：`compose.yaml`、devnet genesis/参数、principal & sudoers 配置、devnet 用 `pool-spec`。

### 2.4 风险与回滚
- 容器保真度 / devnet 稳定性 / agent flakiness 是主风险。缓解：devnet 秒级出块、T3 多跑取不变式、T2 与 T3 解耦。
- 回滚 = 容器即弃，无生产副作用；不改历史（rollback 作为前向变更）。

## References
- `docs/specs/completed/20260708T0000-S0014-agent-tooling.md` — 工具面 / 安全模型 / 退役来源。
- `ouro-skills/` 与 `crates/ouro/src/ssh.rs`（当前 dry-run，本 spec 改真执行）。
- `.claude/skills/aiken-debug`（utxray/preview 真链路参考，供 devnet/真节点接入借鉴）。
- cardano private testnet / devnet 编排工具（p2-1 选型时确定并记录）。

## 3. Execution Plan
> draft，可改。三 bucket 对应 T2 基座 / T2 真节点 / T3 agent。

### p1 — T2 基座：容器自测床 + 真实写通道 + principal 隔离（先行，最高性价比）
- [ ] p1-1 docker-compose 自测床骨架：`control` + `bp1` + `relay1/2`，sshd、基础镜像、`ouro` 安装
- [ ] p1-2 `ouro` SSH runner 真执行（替换 `dry_run`）：经 `ouro-exec` + sudoers 跑 `ouro tool run`，捕获输出/退出码/审计
- [ ] p1-3 目标容器建两类 principal + sudoers allowlist + 密钥 `0400`；`ouro-diag` 对密钥目录无读权限
- [ ] p1-4 `deploy/*` 从 marker 落成真动作（provision/start/verify 的最小真实系统变更 + 幂等）
- [ ] p1-5 安全负向**真跑**：`ouro-diag` 读密钥被拒、agent 裸 `ssh sudo`/`scp`/`docker rm` 被 sudoers 挡、直调脚本 vs `tool run` 审计

### p2 — T2 真节点：private devnet 上跑真 `cardano-cli`
- [ ] p2-1 private cardano devnet 容器（秒级出块、真 socket/`cardano-cli`），选型与简化记录
- [ ] p2-2 `status`/`deploy/verify` 打真节点：tip/slot_lag/peers/network_magic/genesis 真值（替代注入 snapshot）
- [ ] p2-3 KES 真轮换：真 opcert 生成/安装 + counter 单调；secret 指纹扫描器接入
- [ ] p2-4 多 relay 真滚动升级：quorum / BP-last / verify-before-next / rollback-stop 真验
- [ ] p2-5 Mithril 恢复 / takeover 真实化（视 devnet 支持度，明确 in/out）

### p3 — T3 agent 决策层
- [ ] p3-1 headless agent harness 接入：装载 `ouro-skills/`，驱动 deploy / upgrade / kes-rotation / troubleshooting
- [ ] p3-2 行为不变式断言器（§2.2 五条）
- [ ] p3-3 secret 泄漏扫描（transcript + audit + `/var/log` + `set -x`，已知指纹命中即 fail）
- [ ] p3-4 分级 CI：T1 每 PR / T2 每 CI / T3 nightly-gate；T3 场景多跑取不变式

## 4. Test and Acceptance Criteria

**项目级验收判据（本 spec 的最终标准）：**
> **项目「验收通过」 = 能通过完备的容器化端到端测试**，即 **T1（CLI 纯逻辑）∧ T2（容器内真 CLI+L2）∧
> T3（agent 行为不变式）三层全绿**。任一层缺失或降级为占位，均不得判定项目验收通过。

E2E TC（容器/真节点级，多数是把 S0014 的 TC 从 fixture 升级为真跑）：
- E2E-1（p1-2/p1-4）经容器：`ouro tool run` → 真 SSH → sudo → 真跑脚本 → JSON 契约 + 真实系统副作用 + 正确退出码。
- E2E-2（p1-3/p1-5，升级 TC-14）`ouro-diag` 在真容器内 `cat/find/tar/docker exec/journalctl` 读 KES/VRF/cold **失败（权限拒绝）**。
- E2E-3（p1-5，升级 TC-15）agent shell 裸 `ssh host sudo …`/`scp`/`docker rm` 直写生产容器**被阻断或无效**。
- E2E-4（p1-4）幂等**真副作用**：同脚本第二次 `changed=false` 且无额外系统变更（非 marker）。
- E2E-5（p2-2，升级 TC-1/8/16/17）`status`/`verify` 对真节点断言 tip 推进 / network_magic / genesis / P2P|legacy 拓扑；错网络真判 fail。
- E2E-6（p2-3，升级 TC-3/18）真 opcert push（仅 node.cert）+ counter 单调；跑完扫描 transcript/audit/日志**无 skey 指纹**。
- E2E-7（p2-4，升级 TC-22）多 relay 真滚动：单台 verify 失败即停、并发被锁挡、破坏 quorum 被拒（exit 10）、BP-last。
- E2E-8（p3-1/p3-2）agent 经 harness 走 deploy/upgrade/kes：§2.2 五条**行为不变式**全部满足。
- E2E-9（p3-3）secret 泄漏扫描器对注入的已知指纹**命中即 fail**（自身有效性 + 真流程 0 命中）。
- E2E-10（p3-4）T1/T2 在 CI 稳定复现；T3 在 nightly-gate 下 N 次运行不变式稳定成立。

Pass/fail 判据：每个 pX-N 对应 E2E-* 全 pass 方可标 `[x]`；占位/降级不得标完成。项目级验收以上述三层全绿为准。

## 5. Execution Log (append-only)
- 2026-07-08T06:20:00+08:00 draft 创建：承接 S0014（delivered），依据用户确认「验收通过 = 通过完备端到端测试」，
  按三层测试金字塔（T1/T2/T3）与容器化自测床起草；置于 `docs/specs/draft/`，尚未开始执行。

## 6. Validation Evidence (append-only)
- （draft 阶段无验证证据；执行开始后按 `E2E-<n> | stack: <...> | command: <...> | result: <pass|fail> | note: <...>` 逐条追加）

## 7. Change Requests (append-only)
- （暂无）
