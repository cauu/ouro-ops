# Compose Fleet Deploy Real-host E2E

Spec-ID: S0029
状态: draft
创建时间: 2026-07-24T09:54:02+08:00
开始时间:
完成时间:
前一个 Spec-ID: S0028
结项原因:

## 1. Requirement Details

### Background

S0027 已完成 Fleet Deploy 的 CLI、fixed target executor、canonical Skill、网站入口、
确定性 integration regressions 和 destructive-opt-in 双机 harness，但没有两台满足
signed resource policy 的 fresh Ubuntu 主机，因此真实 TC-18 从未通过。

本 spec 完整承接该未完成验收。它必须在 S0028 完成后执行，使 E2E 从 production
Cloudflare 网站复制 canonical Prompt，并使用正式发布且可验证的 CLI artifact，而非
repo-local release candidate。S0027 中的本地容量 probe、harness syntax pass 和模拟
SSH 均只能作为准备证据，不能替代真实 E2E。

### Scope

1. 准备至少两台满足 signed resource policy 的 fresh Ubuntu 22.04/24.04 host：
   1 bootstrap non-producing BP + 至少 1 Relay。
2. 使用逐机声明的 SSH account/credential，覆盖不同机器不同 SSH 用户；缺失 host key
   由用户在真实 TTY 中逐机确认 trust，Agent 不代确认。
3. 从 S0028 production site 复制 Deploy Prompt，通过 S0028 正式 CLI release 执行
   pool spec → trust → Inspect → operator confirmation → Apply → final Check。
4. 真实观察 Blink Labs image 的 Mithril restore/replay startup；初始单次 Check 可以
   pending，但 bounded 重试的最终结果必须 all-ready。
5. 验证 Relay public P2P、BP 无 public P2P、所有 metrics host-loopback only、
   bootstrap BP node-ready/non-producing、不同 SSH 用户和 signed exact image。
6. 再次运行 Deploy，必须得到 `already_deployed` 且无 target write；再执行 S0026
   Upgrade detection，必须识别 Compose ownership 并正确 handoff 给用户。
7. 保留完整审计 evidence，但不得采集 SSH private key、KES/VRF/opcert 内容或其他
   secrets。

### Constraints

- 依赖 S0028 状态为 completed，并使用其记录的 production site/CLI baseline。
- 所有 target writes 需要本次明确 destructive opt-in；未授权不得执行 Apply。
- 目标必须真实满足 signed resource policy，不允许伪造 `/proc`/disk facts、降低门槛、
  换非 signed image 或把两个 container 塞进不合规单机冒充双机。
- Check 保持单次 bounded read-only 调用。Harness 可以在外层按明确 deadline 重复
  Check，但不得改变 CLI 为内部同步等待或把 pending 当 success。
- 复用 `fixtures/e2e/s0027/run.sh` 的安全边界作为起点；激活时将 fixture/文档命名迁移
  到 S0029，避免完成态 S0027 继续看起来像 active execution owner。
- 不在本 spec 配置 BP KES/VRF/opcert 或启用 forging；不提交 pool registration。

### Non-goals

- 不重新设计或实现 Deploy。
- 不修复主机/网络供应商的外部容量、路由或权限问题。
- 不把 Mithril/replay startup 改为 CLI 内同步等待。
- 不扩大正式 CLI/Site 发布范围；发布问题回到 S0028。

## 2. Outline Design

```text
S0028 production baseline
        ↓
production Site Prompt → verified released CLI
        ↓
user-only SSH trust → read-only clean Inspect
        ↓
one operator confirmation → one Apply
        ↓
one immediate Check (ready or genuine startup pending)
        ↓
bounded external re-checks → all-ready
        ↓
already_deployed/no-write + S0026 Compose handoff
```

Harness 的 `prepare` 阶段只生成/校验 operation spec、展示 trust 命令和检查本地工具；
`run` 阶段需要显式 host list、SSH accounts、production baseline 和 write authorization。
每次 Check 的 JSON 独立保存，便于区分真实 pending、静态 failed 和最终 ready。

## 3. Execution Plan

- [ ] p1-1 [Baseline/Hosts] 验证 S0028 production site/CLI baseline，并对两台 fresh
  Ubuntu host 做资源、网络、SSH account、clean-state 和 write authorization 预检。
- [ ] p1-2 [Harness Migration] 将 S0027 harness 迁移为 S0029 owner，绑定 production
  Prompt/CLI identity、deadline、evidence redaction 和不可将 pending 当 pass 的门禁。
- [ ] p2-1 [Trust/Inspect] 用户逐机完成 interactive SSH trust，运行 clean Inspect，
  保存 signed image recommendation、change set 和无 target writes evidence。
- [ ] p2-2 [Apply] 在一次明确确认后执行真实 Relay → bootstrap BP Apply，记录逐机
  actions/failures，且不插入中间 readiness。
- [ ] p3-1 [Readiness] Apply 后运行一次 Check；如 pending，由 harness 外层 bounded
  重复单次 Check，真实观察 Mithril/replay 并最终达到 all-ready。
- [ ] p3-2 [Postconditions] 验证 public/private ports、P2P、metrics、lifecycle、
  exact image、不同 SSH 用户、already_deployed no-write 和 S0026 Compose handoff。
- [ ] p4-1 [Evidence/Close] 汇总 production baseline、target facts、时间线和脱敏日志，
  仅在最终 all-ready 且所有安全后置条件通过时结项。

### Item → TC Mapping

| Item | Acceptance |
| --- | --- |
| p1-1 | TC-1, TC-2 |
| p1-2 | TC-3 |
| p2-1 | TC-4 |
| p2-2 | TC-5 |
| p3-1 | TC-6 |
| p3-2 | TC-7, TC-8 |
| p4-1 | TC-9 |

## 4. Test And Acceptance Criteria

- TC-1：S0028 已 completed；E2E 记录 production site URL、source commit、CLI
  version/artifact digest、runner digest 和 verification evidence，且 Prompt 来自该
  deployed site。
- TC-2：至少 1 BP + 1 Relay 是不同的 fresh Ubuntu 22.04/24.04 host，分别满足 signed
  CPU/RAM/disk policy；SSH 用户可以不同且按 pool spec 逐机生效。
- TC-3：harness 需要显式 destructive opt-in、fresh-host proof 和 deadline；prepare/
  syntax/simulated SSH 成功不能产生 E2E pass。
- TC-4：用户在真实 TTY 中逐机确认缺失 host keys；clean Inspect 为 applicable、
  `target_writes=false`，unknown deployment/data、资源不足或 trust 变化会阻止 Apply。
- TC-5：一次 Apply 连续尝试所有 Relay 后再启动 bootstrap BP，无中间 health/check/
  sleep/poll；使用 signed exact image、deterministic Compose/ownership 和 selective
  mounts。
- TC-6：至少保存一次真实 Mithril restore 或 replay 引起的 `pending`（若首次 Check
  已 ready，则保存可证明实际启动路径的容器 evidence）；最终 bounded Check 必须
  all-ready，最终 pending 或任一 failed 均不能通过。
- TC-7：Relay P2P 仅在声明 public endpoint 开放；BP 无 public P2P；12798 metrics
  仅 host loopback 可达；BP/Relay socket/tip/P2P/peer 与 topology 符合 S0027 contract。
- TC-8：bootstrap BP 报告 node-ready、
  `forging_readiness=not_applicable`/`block_production=disabled`；再次 Deploy 返回
  `already_deployed` 且 `target_writes=false`；S0026 识别 Compose 并给出人工升级流程。
- TC-9：证据可从 production Prompt 追溯到 release 和目标结果，且不包含 SSH private
  key、KES/VRF/opcert 内容；S0027 TC-18 只在此处真实通过后才算完成其后继验收。

Pass/fail：

- TC-1 至 TC-9 全部通过才能结项。
- 本地容器、fake SSH、prepare、syntax、降低资源门槛、非 signed image、最终 pending、
  public BP P2P/metrics、Agent 代确认 trust、already_deployed 有写入或泄露 secrets，
  均为 fail。

## 5. Execution Log (append-only)

- 2026-07-24T09:54:02+08:00 draft created：承接 S0027 p5-1b/TC-18；按 operator 决定
  排在 S0028 正式发布之后执行。

## 6. Validation Evidence (append-only)

- （待执行）

## 7. Change Requests (append-only)

- 2026-07-24T09:54:02+08:00 operator 要求完整 E2E 独立成 spec，且在 CLI/Site
  release workflow 完善之后再验收。
