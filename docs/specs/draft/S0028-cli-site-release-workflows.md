# CLI and Site Release Workflows

Spec-ID: S0028
状态: draft
创建时间: 2026-07-24T09:54:02+08:00
开始时间:
完成时间:
前一个 Spec-ID: S0027
结项原因:

## 1. Requirement Details

### Background

S0025/S0027 已经建立 production-form 静态网站、canonical external Skills、macOS
control CLI 与内嵌 Linux/x86_64 target runner 的成对候选构建，但当前发布边界仍是：

- `.github/workflows/site.yml` 只构建和本地验证，不上传 artifact、不访问 Cloudflare；
- `.github/workflows/release.yml` 只生成 `release-standard-not-published` 候选，不创建
  tag、GitHub Release、签名、正式安装入口或发布元数据；
- 网站 Prompt 仍绑定 repo-local
  `./target/release-candidate-control/release/ouro-ops`，不能作为生产 E2E 的入口；
- `packaging/SIGNING_IDENTITY`、安装 URL、Homebrew/npm 等仍含 placeholder 或未落地
  假设。

因此 S0029 的真实 Deploy E2E 不能先开始：它需要验证用户从正式 Cloudflare 网站复制
Prompt，并通过正式可信 CLI 渠道执行，而不是验证一个只在仓库内成立的候选路径。

旧 draft S0018 仅作为历史输入，不是本 spec 的执行来源。它预先选择了四平台、
reproducible byte identity、cosign + minisign、Homebrew + npm + install script、
self-update apply 等方案，其中部分已与当前“CLI 不内嵌 Skills、control/runner 成对”
边界不一致。S0028 激活前必须重新讨论并明确取舍，不得直接继承这些假设。

### Goals

1. 为 `web/onboarding` 建立可审计的 Cloudflare assets-only 发布 workflow：
   所有 PR 都构建验证；同仓 PR 可发布 preview；`main` 通过无人工审批的 production
   environment 自动发布；fork PR 不接触 secrets。
2. 保持 canonical Skill 唯一来源：站点发布的 HTML 必须由
   `web/onboarding/build.sh` 从六个 canonical Skills 生成并通过既有 fidelity tests。
3. 讨论并冻结 CLI 的发布触发、版本、目标平台、artifact、签名/来源证明、安装入口、
   promotion/revocation/rollback 和 control/runner 配对规则，再据此实现正式 workflow。
4. 让 production site 上的 Prompt 只引用已经发布、可验证且受支持的 CLI 使用方式；
   网站和 CLI 的发布顺序必须避免 Prompt 指向不存在或未受信的版本。
5. 形成供 S0029 消费的 production release baseline：固定网站 URL、CLI version、
   CLI artifact identity、runner digest 和发布证据。

### Constraints

- Site 参考 `ouro-pass/.github/workflows/site.yml` 的安全结构，但不引入 Astro/Node
  runtime；Ouro Ops 保持当前单页静态 build。
- 禁止 `pull_request_target`。Fork PR 只运行无 secrets 的 build/test；preview 仅允许
  same-repository PR。
- build 与 deploy 使用同一个短期 artifact，production 不重新生成一份未经同等验证的
  HTML。
- production deploy 只允许 `main`，使用 GitHub `production` environment 和最小
  permissions；Cloudflare token/account ID 只来自 repository/environment secrets。
- `release` 和 `production` environment 不配置 required reviewer、wait timer 或第二次
  人工批准；它们只承担 branch restriction、secret isolation 和 deployment audit。
- CLI 正式 artifact 必须保持 control binary 与其内嵌 runner 的 digest 配对；发布流程
  不得从网络或另一 job 临时替换 runner。
- CLI 不内嵌 Skills。网站 Prompt 与 canonical Skills 的发布是 Site track，CLI 只实现
  固定 contract/安全执行面。
- 在 CLI 决策项经 operator 明确确认前，S0028 保持 draft，不得 activate 或实现
  signing/distribution/self-update 的推测方案。
- 不因“旧文档已经写过”而自动采用 S0018 的渠道、平台或信任模型；冲突以 S0028 经
  operator 确认后的决策为准。

### CLI Decisions Required Before Activation

1. **Release trigger and version authority**：已确认使用维护者主动
   `workflow_dispatch`，触发后不再等待用户/required-reviewer 审批；仍需冻结
   `next → main → tag` 的确切顺序与 SemVer 来源。
2. **Supported control platforms**：第一阶段只发布当前已验证的 macOS control
   （arm64/x86_64 中哪些），还是同时支持 Linux control；target runner 是否第一阶段
   只支持 Linux/x86_64。
3. **Artifact contract**：tarball/裸 binary/installer 中哪些为正式入口；是否发布
   runner 作为独立 evidence，但禁止独立安装。
4. **Trust model**：GitHub artifact attestations / Sigstore keyless、checksums、固定
   signing identity、离线签名分别需要哪些；是否现在就需要 minisign/Rekor。
5. **Distribution channels**：第一阶段是否仅 GitHub Releases，还是同时提供 Homebrew、
   install script、npm wrapper；每个入口由谁维护。
6. **Update policy**：只做人工安装新版本，还是同时实现 `self-update apply`；撤销坏版本
   时使用 release removal、signed revocation metadata 还是 security floor。
7. **Promotion**：release candidate 如何在无第二次人工审批的前提下自动晋升
   production，以及失败或误发时允许什么恢复动作。
8. **Site/CLI coupling**：Prompt 使用稳定命令还是明确版本；CLI release 与 production
   site deploy 是同一 release gate 还是先 CLI、后 Site 的两阶段 promotion。

### Proposed Minimal CLI Release (awaiting operator confirmation)

已确认的产品边界：

- control CLI 同时支持 Linux 和 macOS。
- release 由维护者主动触发，但 workflow 启动后没有 required reviewer、environment
  approval 或第二次用户确认；通过自动 gates 后直接发布。

建议的第一阶段方案：

1. **平台矩阵**
   - 发布四个 control artifact：
     `x86_64-unknown-linux-musl`、`aarch64-unknown-linux-musl`、
     `x86_64-apple-darwin`、`aarch64-apple-darwin`。
   - “control CLI 可运行的平台”与“可管理的远端 target runner 平台”分开声明。
     当前 fixed runner 只有 Linux/x86_64；第一阶段可以继续只承诺 Linux/x86_64
     target，不能因为发布了 Linux/arm64 control 就暗示远端 ARM 节点已受支持。
   - 若产品要求管理 Linux/arm64 target，应单独扩展为两个 embedded runner，并让
     control 根据只读可信 host facts 选择 exact runner/digest；不得在 release workflow
     中把未配对 runner 当作普通下载依赖临时替换。
2. **版本与触发**
   - `Cargo.toml` 是 SemVer 唯一来源。
   - `next` 继续做集成，只有已进入 `main` 的 exact commit 可以发布。
   - release 通过 `workflow_dispatch` 输入 version 和 exact main commit；workflow
     校验 version 与 `Cargo.toml` 一致，自动创建对应 `vX.Y.Z` draft release，上传
     全部 assets 并通过自动 gates 后直接 publish，不等待 environment 人工批准。
   - repository 启用 GitHub immutable releases；release 发布后 tag 与 assets 不允许
     原地替换。失败版本用更高 patch version 向前修复。
3. **Artifact contract**
   - 每个平台只发布一个 `ouro-ops-vX.Y.Z-<target>.tar.gz`，包内保持单一
     `ouro-ops` binary。
   - 同一 release 附带 `SHA256SUMS` 和 `release-manifest.json`。Manifest 绑定
     source commit、Cargo version、四个 control digest、每个 control 的 contract
     version、embedded runner platform/digest 和 workflow identity。
   - runner bytes 可以作为 build evidence 保存，但不是用户可独立安装的正式产品。
4. **Trust**
   - 使用 GitHub immutable release + GitHub artifact attestation，固定官方 repository
     `cauu/ouro-ops` 和 release workflow identity；用户可用 `gh release verify-asset`
     与 `gh attestation verify --repo cauu/ouro-ops --signer-workflow ...` 验证。
   - `SHA256SUMS` 负责传输/文件一致性，不把同源 checksum 误称为独立真实性证明。
   - 第一阶段不再叠加自管 cosign identity、minisign key 和 Rekor 操作；GitHub
     attestation 已提供 OIDC/Sigstore provenance，避免同时维护两套未被用户实际使用的
     信任流程。只有明确的 GitHub-independent/offline 威胁模型出现时再增加独立签名。
5. **Distribution**
   - GitHub Releases 是唯一 canonical binary source。
   - 官方安装流程使用 GitHub CLI 下载、验证 attestation/immutable release，再安装到
     user-owned bin directory；禁止以未经验证的 `curl | sh` 作为正式入口。
   - 第一阶段不发布 npm wrapper，也不建立 Homebrew tap。Homebrew 是后续便利渠道，
     其 formula 只能引用同一 GitHub immutable asset/digest，不能形成第二套 build。
6. **Update/recovery**
   - 保留现有 `self-update --check`，第一阶段不实现自动 download/swap。
   - 用户通过重复同一验证安装流程升级；坏版本不原地替换、不静默降级，发布修复版并
     更新 latest/site recommendation。
7. **Promotion**
   - PR/`next` 只做完整四平台 dry build、contract test 和 packaging test，不创建 tag/
     release。
   - main release workflow 先完成 runner/control pairing、tests、manifest/checksums/
     attestations，再创建完整 draft；publish job 只发布该 exact draft，且不配置
     required reviewer。
8. **Site coupling**
   - production Prompt 使用稳定 `ouro-ops` 命令和正式 verified-install 流程，不绑定
     repo-local build path。
   - 首次上线以及任何提高 `requires-ouro`/contract floor 的变更都必须先发布满足要求
     的 CLI，再发布 Site；纯文案/样式变更无需重新发布 CLI。

该方案刻意把 Homebrew、npm、自更新 apply 和独立离线签名留在后续需求中。它们不是
不允许，而是在 canonical GitHub release 链路尚未跑通前不会增加第一阶段的发布面。

### Primary References

- GitHub artifact attestations:
  `https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations`
- GitHub immutable releases:
  `https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases`
- GitHub release integrity verification:
  `https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity`
- Homebrew tap maintenance (deferred convenience channel):
  `https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap`

### Non-goals

- 不在本 spec 执行 S0027/S0029 的真实双机 Deploy E2E。
- 不改变 Deploy/Upgrade/KES/Observability 等 operation 行为或 canonical Skill 流程。
- 不发布 Cardano node 镜像层；signed node-image catalog 仍是独立 artifact。
- 不默认实现 S0018 罗列的所有渠道、自更新、离线 bundle 或四平台矩阵。
- 不引入新的前端框架、Cloudflare runtime API、数据库或服务端 Prompt 生成。

## 2. Outline Design

### Site Track

`build` job 在所有相关 PR 和 `main` push 上 checkout、执行
`./web/onboarding/build.sh`，运行 site/Skill fidelity tests，并把
`web/onboarding/dist` 上传为保留期很短的 artifact。

`preview` job 只在 same-repository PR 上下载该 artifact，使用固定 Wrangler 4 版本
执行 `versions upload`，并以单一 marker 更新 PR preview comment。Fork PR 不运行该
job。

`deploy` job 只在 `main` push 上、经过 `production` environment 后下载完全相同的
artifact，再执行 `wrangler deploy`。README 记录一次性 Cloudflare token/account、
environment、首发和 custom domain 配置，以及本地与 production smoke test。

### CLI Track

先用一份 decision record 固化“CLI Decisions Required Before Activation”。实现只能
从已确认的最小渠道开始，并保持：

```text
source commit + version
        ↓
locked control build ── embeds ── exact Linux runner bytes
        ↓
contract/digest/catalog tests
        ↓
agreed signing + provenance
        ↓
agreed release channel
        ↓
production site Prompt references supported install/run contract
```

发布证据至少能回答：source commit 是什么、用户下载的 control artifact 是什么、
内嵌 runner digest 是什么、谁/什么 workflow 发布、如何验证、哪个版本可被 S0029
验收。

### Relationship to Existing Drafts

- S0018 保留为历史方案输入，不与 S0028 并行执行。
- S0028 激活时必须先把已确认决策写入本 spec，并将 S0018 标记为被 S0028 替代，
  以消除两个 CLI release spec 同时可执行的歧义。
- S0029 依赖 S0028 完成，只消费 S0028 产出的正式网站/CLI baseline。

## 3. Execution Plan

- [ ] p1-1 [Current Boundary Audit] 审计现有 site/release workflows、candidate、
  packaging placeholders、branch flow 和 S0018 假设，形成保留/删除/待决定清单。
- [ ] p1-2 [CLI Decision Freeze] 与 operator 逐项确认 trigger/version/platform/
  artifact/trust/channel/update/promotion/site coupling，并把选择和理由写入本 spec；
  同时正式标记 S0018 被替代。
- [ ] p2-1 [Site Build/Preview] 实现无 secrets 的通用 build gate、短期 artifact 和
  same-repo Cloudflare preview；fork PR 只有 build。
- [ ] p2-2 [Site Production] 实现 `main` + `production` environment 的 Cloudflare
  deploy、custom-domain 文档和 deployed-page smoke/fidelity 验收。
- [ ] p3-1 [CLI Build/Publish] 按 p1-2 确认的最小平台/artifact/channel实现正式 CLI
  workflow，并保留 control/embedded-runner 的 exact digest pairing。
- [ ] p3-2 [CLI Trust/Recovery] 按 p1-2 确认的签名、provenance、verification、
  revocation/rollback 选择实现 fail-closed gates 和 operator runbook。
- [ ] p3-3 [Site/CLI Promotion] 将 production Prompt 从 repo-local candidate 切换为
  已发布 CLI contract，保证 CLI 先可获取/验证、Site 后 promotion。
- [ ] p4-1 [Release Acceptance] 执行一次 dry run 和一次受控 production release，
  固化 S0029 所需的 site URL、CLI version/artifact identity、runner digest 与证据。

### Item → TC Mapping

| Item | Acceptance |
| --- | --- |
| p1-1 | TC-1 |
| p1-2 | TC-2 |
| p2-1 | TC-3, TC-4 |
| p2-2 | TC-5 |
| p3-1 | TC-6 |
| p3-2 | TC-7 |
| p3-3 | TC-8 |
| p4-1 | TC-9 |

## 4. Test And Acceptance Criteria

- TC-1：当前已实现、placeholder、未发布和 S0018 假设被逐项分类；不得把 candidate
  validation 或预置 Wrangler config 误报为正式发布。
- TC-2：八类 CLI 决策均有 operator 明确选择、理由和最小第一阶段边界；S0018 被标记
  为 replaced，不再存在两个可激活的 release spec。
- TC-3：任意相关 PR 都从 canonical Skills 构建 production-form HTML 并通过 fidelity
  tests；fork PR 无法读取 Cloudflare secrets 且不执行 preview。
- TC-4：same-repo PR 的 preview 使用 build job 的同一 artifact，可重复更新同一条 PR
  comment；workflow 不使用 `pull_request_target`，permissions 最小。
- TC-5：`main` production job 只部署已验证 artifact，并受 `production` environment
  保护；真实 Cloudflare URL 返回预期 HTML/CSP/GitHub link，复制出的六个 Prompt 与
  canonical Skills 精确一致。
- TC-6：正式 CLI artifact 来自 locked source/version，contract 与内嵌 runner digest
  匹配；已确认平台可安装执行，未声明平台不被误报支持。
- TC-7：用户可以按已确认 trust model 独立验证 artifact/source/provenance；tampered、
  wrong version/identity/runner pairing 均拒绝，坏版本恢复不要求静默 downgrade。
- TC-8：production site 不再引用 repo-local release candidate；其 CLI 命令在站点上线
  前已真实可用并可验证，不存在 Site 先发布、CLI 尚不可获取的窗口。
- TC-9：dry run 不创建正式 release/production deploy；受控 production run 生成唯一
  baseline，包含 source commit、site URL、CLI version/artifact digest、runner digest、
  verification evidence，S0029 可直接引用。

Pass/fail：

- 所有 item 对应 TC 通过并有 evidence 后才能完成。
- 任一 fork PR 获得 secrets、production 重建未验证 HTML、Prompt/Skill 分叉、CLI 与
  runner 不配对、placeholder 被当作 trust anchor、未确认的 S0018 假设被直接实现，
  或 production Site 先于其引用的 CLI 可用，均为 fail。

## 5. Execution Log (append-only)

- 2026-07-24T09:54:02+08:00 draft created：operator 要求在真实 Deploy E2E 前先完善
  CLI/Site 发布 workflow；Site 参考 ouro-pass 直接发布静态产物到 Cloudflare，CLI
  发布链路先讨论后实现。

## 6. Validation Evidence (append-only)

- （待执行）

## 7. Change Requests (append-only)

- 2026-07-24T09:54:02+08:00 operator 明确执行顺序为 S0028 release workflows →
  S0029 full E2E；CLI 方案不得由 agent 在讨论前自行决定。
- 2026-07-24T10:06:06+08:00 operator 确认 control CLI 支持 Linux 和 macOS；架构、
  target runner、artifact、trust、distribution、update、promotion 和 Site coupling
  尚未确认。Draft 加入一套以 GitHub immutable release + artifact attestation 为主链、
  暂缓多渠道和自更新的最小建议，等待 operator 评审。
- 2026-07-24T10:26:44+08:00 operator 认可移除用户审批步骤：CLI 仍由维护者主动
  `workflow_dispatch`，但开始后不再等待 release-environment required reviewer；
  Site 的 `main` production deploy 同样不增加人工审批。Environment 只保留 branch/
  secret/audit 边界，所有 publish gates 自动执行。
