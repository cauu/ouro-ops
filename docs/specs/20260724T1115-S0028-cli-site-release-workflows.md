# CLI and Site Release Workflows

Spec-ID: S0028
状态: active
创建时间: 2026-07-24T09:54:02+08:00
开始时间: 2026-07-24T11:15:19+08:00
完成时间:
前一个 Spec-ID: S0027
结项原因:

## 1. Requirement Details

### Background

S0025/S0027 已经建立 production-form 静态网站、canonical external Skills、macOS
control CLI 与内嵌 Linux/x86_64 target runner 的成对候选构建，但当前发布边界仍是：

- `.github/workflows/site.yml` 只构建和本地验证，不发布 Cloudflare；
- `.github/workflows/release.yml` 只生成 `release-standard-not-published` 候选，不创建
  release、attestation 或正式安装入口；
- 网站 Prompt 仍绑定 repo-local
  `./target/release-candidate-control/release/ouro-ops`；
- `packaging/SIGNING_IDENTITY`、安装 URL、Homebrew formula 等仍包含 placeholder 或
  已放弃的多渠道假设；
- current `self-update --check` 不读取线上 release，也不构成真实更新流程。

因此 S0029 不能先做生产 E2E：它必须从 Cloudflare production site 复制 Prompt，并
通过正式、可验证的 CLI 分发入口执行，而不是验证仓库内 candidate。

S0018 从未激活，并预设了 Skills 内嵌、四 target runner、cosign + minisign、
Homebrew + npm、自更新 apply 等大范围基础设施。Operator 已接受反方收敛方案；S0018
以 `replaced` 归档，S0028 是当前 CLI/Site 发布的唯一 draft。

### Scope

1. 为 `web/onboarding` 建立最小 Cloudflare assets-only workflow：
   PR 只构建/测试，`main` 自动构建/测试/部署；不增加人工审批。
2. 发布四个 control CLI：
   - `x86_64-unknown-linux-musl`
   - `aarch64-unknown-linux-musl`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
3. 第一阶段远端 target 只支持 Linux/x86_64。所有 control CLI 内嵌同一 exact
   Linux/x86_64 runner；远端不是 x86_64 时，在上传 runner 或 target write 前返回
   typed `unsupported_target_arch`。
4. Release 由维护者在 `main` 手动启动一次，只选择
   `patch|minor|major`。Workflow 自动更新 `Cargo.toml`/`Cargo.lock`、提交 release
   commit/tag，并在新 tag 上构建、attest、发布。
5. GitHub immutable Releases 是唯一 canonical binary source。每次 release 只发布四个
   tarball 与 `SHA256SUMS`，并为 tarball 生成 GitHub artifact attestations。
6. 首次安装和更新使用同一个 GitHub-CLI verified reinstall：下载、验证、解包到
   `$HOME/.local/bin/ouro-ops`，再检查 `version`/`contract`；不使用 sudo。
7. Production Site 只引用正式 `ouro-ops` 和 verified install/update 流程；首次 Site
   上线和提高 `requires-ouro`/contract floor 时必须先发布满足要求的 CLI。
8. 完成一次真实 CLI release 与一次真实 Cloudflare production deploy，输出供 S0029
   使用的 site URL、CLI version/artifact digest 和 runner digest baseline。

### Final Decision Record

| Topic | Decision |
| --- | --- |
| Release initiation | `main` 上一次 `workflow_dispatch`；唯一业务输入为 `patch\|minor\|major` |
| Additional approval | 无 required reviewer、wait timer 或第二次用户确认 |
| Version authority | 当前 root `Cargo.toml`；workflow 自动 bump 并同步 `Cargo.lock` |
| Control platforms | Linux/macOS × x86_64/arm64，共四个 |
| Remote target | 第一阶段仅 Linux/x86_64；arm64 在任何 runner upload/write 前拒绝 |
| Artifact | 四个 single-binary tarball + `SHA256SUMS` |
| Trust | GitHub immutable release + artifact attestation；固定 repo/workflow identity |
| Distribution | 只使用 GitHub Releases；不做 Homebrew/npm |
| Install/update | GitHub CLI verified reinstall 到 `$HOME/.local/bin/ouro-ops`；删除无真实线上能力的 `self-update` stub |
| Bad release | 不覆盖、不降级；发布更高 patch 并更新 latest/site recommendation |
| Site | PR build/test；`main` automatic Cloudflare deploy；无 Preview/PR comment gate |
| Site/CLI ordering | 首发和 CLI floor 提升时先 CLI、后 Site；普通 Site 变更独立发布 |

### Constraints

- CLI 不内嵌 Skills。网站从六个 canonical external Skills 生成 Prompt，CLI 只实现
  contract 和安全机制。
- Site 保持当前单页静态 build，不引入 Astro/Node frontend runtime、Cloudflare
  server logic、数据库或服务端 Prompt 生成。
- 禁止 `pull_request_target`。Fork PR 不得读取 Cloudflare secrets 或执行 deployment。
- `production` environment 只用于 Cloudflare secrets、branch restriction 和 deployment
  audit，不配置 required reviewer/wait timer。
- CLI release 不使用 environment approval 或长期 PAT。Preparation 只获得所需的
  `contents: write`/`actions: write`；publish 只获得所需的
  `contents: write`/`actions: write`/`id-token: write`/`attestations: write`。
- Release workflow 只能 non-force 写入一个 `chore(release): vX.Y.Z` commit 和同名
  tag；不得绕过并发、版本、tracked-diff 或测试门禁。
- Release artifact 必须在真正包含 bumped Cargo files 的 tag commit 上构建和 attest，
  不能在旧 `GITHUB_SHA` 的 modified worktree 上伪装新版本 provenance。
- 远端 architecture discovery 只能由 CLI 通过 strict SSH 执行固定、只读的
  `uname -s`/`uname -m` probe；Agent 不能提供或替换 probe/runner bytes/path/digest。
- ARM target rejection 不得影响 ARM control 管理 x86_64 target；control architecture
  与 remote runner architecture 是两个独立维度。
- `self-update` stub、placeholder signing identities、fake `install.sh`、Homebrew formula
  和未支持的 distribution claims 必须从 live CLI/packaging/docs/site 删除，completed
  historical specs 保留。
- Custom domain 不是本 spec 完成门槛；稳定 Cloudflare Worker production URL 足以完成
  S0029 baseline。

### Activation Prerequisites

以下为一次性外部 wiring，必须在对应 production acceptance 前存在，但不形成每次发布
审批：

1. GitHub repository 启用 immutable releases。
2. Repository Actions/rules 允许 release workflow 使用 scoped `GITHUB_TOKEN`
   non-force 写入 release commit/tag，并显式 dispatch tag-ref publish run。
3. GitHub `production` environment 配置：
   - `CLOUDFLARE_API_TOKEN`
   - `CLOUDFLARE_ACCOUNT_ID`
4. Cloudflare Worker 名称固定为 `ouro-ops-site`；production URL 可被 smoke test 访问。
5. Operator 不向 spec、workflow log 或测试 fixture 提供任何 secret value；验收只检查
   secret 名称/可用性和外部结果。

### Non-goals

- Linux/arm64 remote target runner；出现真实需求时另开 runtime spec。
- Cloudflare PR Preview、PR comment 和 custom domain 接入。
- Homebrew tap、npm wrapper、package-manager auto-publish。
- `self-update apply`、后台检查、自动 binary swap 或自动 downgrade。
- 自管 cosign identity、minisign、独立 Rekor 操作、offline bundle。
- 自定义 `release-manifest.json` 或独立安装 runner。
- S0027/S0029 真实双机 Deploy E2E。
- 发布 Cardano node image layers；signed node-image catalog 保持独立。

## 2. Outline Design

### A. Remote Target Architecture Gate

在任何 ephemeral runner transport 前，CLI 通过现有 strict known_hosts/declared SSH
account 执行一个固定、只读、bounded probe：

```text
uname -s
uname -m
```

只接受 `Linux` + `x86_64|amd64`。其他 OS/architecture 返回 typed blocker：

```json
{
  "reason_code": "unsupported_target_arch",
  "supported": "linux/x86_64",
  "observed": "linux/arm64",
  "target_writes": false,
  "runner_uploaded": false
}
```

所有四个 control binary 内嵌同一 Linux/x86_64 runner bytes；`ouro-ops contract` 继续
报告该 runner platform/digest，不因 control architecture 变化而变化。

### B. Version Preparation

维护者在 GitHub UI 只选择：

- `patch`：`X.Y.Z → X.Y.(Z+1)`
- `minor`：`X.Y.Z → X.(Y+1).0`
- `major`：`X.Y.Z → (X+1).0.0`

Preparation workflow：

1. 只允许从 current `main` 执行，并通过 release concurrency 串行化。
2. 读取 root `Cargo.toml`。已有正式 release 时，Cargo version 必须等于 latest stable；
   尚无 release 时，以当前 Cargo version 为 bootstrap baseline。
3. Repo-owned deterministic helper 计算新版本，只更新 root `Cargo.toml` 和
   `Cargo.lock`；任何其他 tracked diff 都拒绝。
4. 运行 version/helper、Rust 和 packaging contract tests。
5. Push 前重新确认 origin/main 没有前进、tag/release 不存在、版本严格增加。
6. 创建一个 `chore(release): vX.Y.Z` commit 和同名 tag，依次 non-force push。
7. 使用 `GITHUB_TOKEN` 显式 `workflow_dispatch` publish workflow，ref 固定为新 tag。

普通 `GITHUB_TOKEN` push/tag 不会自动触发新 workflow；显式 dispatch 使 publish run 的
`GITHUB_SHA` 等于新 release commit。用户仍只启动一次。

若 commit/tag 已写入但 dispatch/publish 因临时外部错误失败，下一次相同 bump
invocation 必须识别 exact unpublished release commit/tag 并恢复 publish，不再次 bump。
其他 partial/mismatched state fail closed，并给出人工诊断，不重写历史。

### C. Four-platform Publish

Tag-ref publish workflow：

1. 验证 `GITHUB_REF`、tag、Cargo、Cargo.lock 与 binary version 一致。
2. 构建一次 static Linux/x86_64 runner。
3. 以该 exact runner bytes 构建四个 control binaries。
4. 每个 control 在对应原生环境执行 `ouro-ops version` 与 `ouro-ops contract`。
5. 验证四个 control 报告相同、非空的 runner platform/digest。
6. 每个平台生成只含一个 `ouro-ops` 的
   `ouro-ops-vX.Y.Z-<rust-target>.tar.gz`。
7. 生成 `SHA256SUMS`，并为每个 tarball 生成 GitHub artifact attestation。
8. 一次性发布 immutable GitHub Release；同 version/tag/assets 不可替换。

PR/`next` 只运行普通 tests、bump-helper tests 和 packaging source contract，不执行
完整四平台 matrix，也不创建 tag/release/attestation。

### D. Verified Install And Update

Production Site 提供同一套 clone-free 流程用于首次安装和更新：

1. 要求本机已安装 GitHub CLI；缺失时只返回明确前置条件，不修改 PATH/binary。
2. 读取本机 OS/architecture，映射到四个正式 rust targets。
3. 从固定 `cauu/ouro-ops` repository 下载 latest stable 对应 tarball。
4. 使用固定 repository 与 release workflow identity 验证 immutable release asset 和
   artifact attestation。
5. 验证 archive 只含一个 expected `ouro-ops`，再在同目录写临时文件并原子 rename 到
   `$HOME/.local/bin/ouro-ops`；不调用 sudo、不覆盖其他路径。
6. 目标不存在时 fresh install；目标是相同 verified version/digest 时 no-write
   idempotent success；目标是较旧 valid Ouro 时严格向前替换；目标是较新版本、
   prerelease、未知/非 Ouro executable 或无法验证时 fail closed。
7. 使用绝对 `OURO_BIN=$HOME/.local/bin/ouro-ops` 执行 `version`/`contract`，不依赖用户
   当前 PATH；是否另行加入 PATH 不属于发布正确性。

删除现有无真实线上能力的 `self-update` CLI command/help/tests，以及 live
`packaging/install.sh`、`packaging/SIGNING_IDENTITY` 和
`packaging/homebrew/ouro-ops.rb` placeholder。`packaging/RELEASE.md` 只描述本 spec 的
GitHub-CLI verified reinstall，不保留第二套近似流程。

### E. Site Build And Deploy

- PR：checkout → `web/onboarding/build.sh` → Skill/HTML fidelity tests。无 secrets、无
  Cloudflare upload。
- Push `main`：执行相同 build/tests，然后在同一 job/output 上使用固定 Wrangler 4
  发布 `web/onboarding/dist` 到 assets-only Worker。
- Deploy 后对真实 production URL 运行 bounded smoke：HTTP success、CSP、GitHub link、
  六个 canonical Skill payload 和无 repo-local candidate path。

Site 普通文案/样式变更可独立发布；首次上线或提高 `requires-ouro`/contract floor 时，
workflow/test 必须证明满足 floor 的 immutable CLI release 已存在。

若包含新 floor 的 Site change 先进入 `main`，Site workflow 必须在 Cloudflare write
前停止并保留上一 production version。满足 floor 的 CLI publish 完成后，publish
workflow 显式 dispatch Site workflow 的 current `main`；Site 重新 build/test 并自动
发布，不需要第二次用户操作。普通 CLI release 触发同一 dispatch 也是幂等的。

### F. Recovery

- Preparation 在写 commit/tag 前失败：无 repository/release 变化。
- Commit/tag 已写、publish 未完成：幂等恢复相同 version。
- Immutable release 已发布：不覆盖、不删除重发；用更高 patch 向前修复。
- Site deploy 失败：Cloudflare 上一个 production version 保持服务；修复后重新运行
  main deploy。
- 不自动 downgrade CLI，不实现 Fleet/host rollback。

### Primary References

- GitHub artifact attestations:
  `https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations`
- GitHub immutable releases:
  `https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases`
- GitHub release integrity verification:
  `https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity`
- GitHub-token-triggered workflow behavior:
  `https://docs.github.com/en/actions/concepts/security/github_token`
- Ouro-pass Site workflow reference:
  `/Users/caiyufu/Dev/projects/bubble-studio/ouro-pass/.github/workflows/site.yml`

## 3. Execution Plan

- [x] p1-1 [Release Prerequisites] 建立无 secret-value 的 external wiring verifier/runbook，
  能验证 immutable releases、scoped Actions permissions/repository rules、Cloudflare
  environment secret names 和 Worker identity；缺失时 typed report，且不阻塞后续
  repo implementation，真实配置只在 p5-1 production acceptance 前强制通过。
- [x] p1-2 [Remote Architecture Gate] 在所有 ephemeral runner transport 前加入 fixed
  strict-SSH OS/arch probe；只允许 Linux/x86_64，ARM/unknown 在 upload/write 前 typed
  reject，四种 control architecture 行为一致。
- [x] p2-1 [Version Preparation] 实现 deterministic patch/minor/major helper、
  Cargo.toml/Cargo.lock-only release commit/tag、concurrency/CAS gates 和 tag-ref publish
  dispatch；支持 exact partial publish recovery。
- [x] p2-2 [Four-platform Publish] 重构 paired build，发布四个 single-binary tarball、
  `SHA256SUMS`、artifact attestations 和 immutable GitHub Release；PR/next 不运行完整
  matrix。
- [x] p3-1 [Verified Install/Update] 删除 `self-update` stub 和 live placeholder
  distribution artifacts/claims，建立 GitHub-CLI clone-free download/verify、
  `$HOME/.local/bin/ouro-ops` atomic install 流程，覆盖首次安装、同版本 no-write 和严格
  向前更新。
- [x] p4-1 [Site Build/Deploy] 将 Site workflow 收敛为 PR build/test 与 `main`
  build/test/Cloudflare deploy；无 Preview/PR comment/custom-domain gate。
- [x] p4-2 [Site/CLI Contract] 从 production Prompt 删除 repo-local candidate binding，
  加入 verified install/update、CLI-floor-before-Site gate 和 CLI publish 后的幂等 Site
  dispatch，保持 canonical Skill 唯一来源。
- [x] p5-1 [Production Acceptance] 完成一次真实 automatic bump CLI release 和一次
  Cloudflare production deploy，固化 S0029 所需 baseline。
- [x] p5-1-fix1 [Production Workflow Hardening] 修复真实验收发现的 runner PATH/PEP 668
  CI portability、immutable release attestation 传播延迟和 deploy 后缺少自动 production
  smoke，保持 p5-1 直到一次新的单 dispatch 全自动贯通才完成。
- [x] p5-1-fix2 [L2 Distribution Follow-up] 让隔离 venv 使用 canonical
  `requirements-dev.txt`，并保留 Rocky 9 已安装的 `curl-minimal`，消除真实 matrix
  暴露的 pytest 缺失和 curl 包冲突。
- [~] p5-1-fix3 [Lightweight Verified Installer] 将完整 `ouro-install.sh` 作为每个
  immutable GitHub Release 的 checksum-bound、attested asset 发布；Production Site
  只呈现并允许复制一段固定 tag、验证 installer asset 后执行的短 bootstrap，不再
  内嵌或复制完整安装实现。

### Item → TC Mapping

| Item | Acceptance |
| --- | --- |
| p1-1 | TC-1 |
| p1-2 | TC-2 |
| p2-1 | TC-3, TC-4 |
| p2-2 | TC-5, TC-6 |
| p3-1 | TC-7 |
| p4-1 | TC-8 |
| p4-2 | TC-9 |
| p5-1 | TC-10 |
| p5-1-fix1 | TC-11 |
| p5-1-fix2 | TC-12 |
| p5-1-fix3 | TC-13 |

## 4. Test And Acceptance Criteria

- TC-1：不读取 secret value 即可检查 GitHub immutable releases、workflow permissions/
  repository rule、`production` environment secret names 和 `ouro-ops-site` identity；
  configured 与 missing 都返回 typed facts，缺失项给 actionable prerequisite 且不改
  repository/release/site。Verifier/runbook 的测试通过即可完成 p1-1；真实 configured
  状态由 TC-10 强制。
- TC-2：fixed probe 对 Linux x86_64/amd64 选择唯一 embedded runner；Linux arm64、
  非 Linux、unknown/malformed/injected output 在任何 runner upload/target write 前返回
  `unsupported_target_arch`/typed blocker，且 `target_writes=false`、
  `runner_uploaded=false`。四种 control architecture 结果一致。
- TC-3：从 0.1.0 分别验证 patch→0.1.1、minor→0.2.0、major→1.0.0；preparation 只提交
  `Cargo.toml`/`Cargo.lock`，commit message、tag、Cargo/lock version 完全一致。
- TC-4：main 并发前进、额外 tracked diff、已有 tag/release、非单调版本全部在发布前
  失败；tag publish run 的 `GITHUB_SHA` 等于 release commit。Commit/tag 已写而 publish
  未完成时，重跑恢复同一 version，不产生第二次 bump。
- TC-5：四个正式 tarball 分别只含一个 native `ouro-ops`，在对应原生 OS/arch 上执行
  `version`/`contract`；tag、Cargo、binary 和 release version 相同。四个 control 报告
  同一非空 Linux/x86_64 runner digest。
- TC-6：immutable release、`SHA256SUMS` 和每个 tarball attestation 可验证；tampered
  archive、错误 repo/workflow identity、runner mismatch、同版本重发均失败。
- TC-7：在无 repository checkout 的 clean macOS arm64/x86_64 和 Linux
  arm64/x86_64 control 环境完成 fresh install；至少在 macOS arm64 与 Linux x86_64
  完成 N-1→N verified update、同版本同 digest no-write、prerelease/downgrade/未知
  executable 拒绝。安装只写 `$HOME/.local/bin/ouro-ops` 并以绝对路径验证；缺少 GitHub
  CLI 时只给前置条件且不写 binary/PATH。Live CLI/packaging/docs/site 中不存在
  `self-update` stub、placeholder install/signing/Homebrew 第二流程。
- TC-8：PR/fork 只构建测试且无 Cloudflare secrets/upload；`main` 使用 production
  environment 与固定 Wrangler 发布 assets-only Worker。无
  `pull_request_target`、Preview、PR comment 或人工 approval。
- TC-9：真实 production URL 返回预期 HTML/CSP/GitHub link；六个 Prompt 与 canonical
  Skills 字节一致，正式安装/更新流程可执行，不含 repo-local candidate、placeholder
  signing identity、Homebrew/npm 或虚假 self-update claim。首次上线/CLI floor 提升时
  已有满足 floor 的 immutable CLI release；floor 未满足时 Cloudflare 零写入，满足
  floor 的 CLI publish 后自动 dispatch current-main Site deploy，无第二次用户操作。
- TC-10：受控 production run 从一次 patch/minor/major 选择自动产生唯一 release
  commit/tag、四个 verified assets 和 production Site；baseline 包含 source commit、
  Site URL、CLI version/artifact digest、runner digest 与 verification evidence，可供
  S0029 直接引用。此时 TC-1 的 immutable release、repository permissions/rules、
  Cloudflare secret names 和 Worker identity 必须全部为 configured。
- TC-11：missing-`gh` fixture 在 GitHub-hosted runner 上不能误用预装 `gh`；Debian 12
  等 PEP 668 环境通过隔离 venv 运行 L2；release create 后以有界重试等待 immutable
  release attestation；Cloudflare deploy 后使用 action 的 production URL 自动验证
  HTTP/CSP/GitHub link、六个 canonical Skills、verified installer 和无 repo-local
  candidate。对应 workflow source contracts 与完整本地回归均通过。
- TC-12：L2 venv 从 `requirements-dev.txt` 安装完整测试依赖；Rocky 9 不请求与
  `curl-minimal` 冲突的 `curl` 包；Ubuntu 24.04、Debian 12、Rocky 9 三个真实 matrix
  job 全部通过。
- TC-13：每个新 Release 包含 canonical `ouro-install.sh`，其 digest 纳入
  `SHA256SUMS` 且具有固定 release-publish workflow attestation；网站 copy 按钮复制
  的 bootstrap 不超过 20 个非空行，固定一次 latest stable tag，下载并在执行前验证
  对应 installer asset/provenance，再以该 tag 执行。Production HTML 不包含完整
  installer，latest Release 尚无 installer asset 时 Site 在 Cloudflare 零写入处失败；
  发布含 installer 的新 patch 后自动 Site deploy 与独立 production smoke 通过。

Pass/fail：

- TC-1 至 TC-10 全部通过，每个 item 有 evidence 后才能完成。
- 任一未确认 decision、第二发布渠道、placeholder、ARM target runner 隐式执行、
  Agent-supplied probe/runner、old-SHA provenance、错误二次 bump、未验证 binary 安装、
  production Site 先于所需 CLI、Prompt/Skill 分叉、fork secrets、Site Preview/人工审批
  或把 local candidate 当 production，均为 fail。

## 5. Execution Log (append-only)

- 2026-07-24T09:54:02+08:00 draft created：operator 要求在真实 Deploy E2E 前先完善
  CLI/Site 发布 workflow；Site 参考 ouro-pass 直接发布静态产物到 Cloudflare，CLI
  发布链路先讨论后实现。
- 2026-07-24T11:15:19+08:00 S0028 activated after execution-readiness review；no
  implementation item started。
- 2026-07-24T11:18:00+08:00 p1-1 started：implement read-only typed verification for
  GitHub release/environment wiring and the fixed Cloudflare Worker identity。
- 2026-07-24T11:19:04+08:00 p1-1 completed：added the read-only GitHub configuration
  verifier and operator runbook；the live probe reports current missing prerequisites without
  blocking repository implementation or reading secret values。
- 2026-07-24T11:19:30+08:00 p1-2 started：move target OS/architecture discovery ahead
  of every shared ephemeral runner and runner-plus-payload transport。
- 2026-07-24T11:25:06+08:00 p1-2 completed：the shared strict-SSH transport now runs
  one fixed read-only `uname` probe before opening stdin；unsupported and malformed targets return
  one typed refusal with `runner_uploaded=false` and `target_writes=false`。
- 2026-07-24T11:26:00+08:00 p2-1 started：implement deterministic Cargo SemVer
  preparation, fail-closed repository-state checks and exact unpublished-tag recovery。
- 2026-07-24T11:28:19+08:00 p2-1 completed：the one-input preparation workflow now
  performs deterministic Cargo-only bumps, main/CAS/tag/release gates, non-force commit/tag pushes
  and exact unpublished release recovery before tag-ref publish dispatch。
- 2026-07-24T11:29:00+08:00 p2-2 started：implement native four-platform control
  builds around one exact Linux/x86_64 runner, aggregate validation, attestations and immutable
  GitHub Release publication。
- 2026-07-24T11:34:00+08:00 p2-2 completed：added source-only PR/next checks and a
  tag-ref native four-platform workflow that validates one embedded runner identity, creates four
  single-binary archives, attests their canonical checksums and creates one immutable release。
- 2026-07-24T11:35:00+08:00 p3-1 started：replace the legacy distribution stubs with
  one GitHub-CLI verified reinstall command source and deterministic atomic install/update tests。
- 2026-07-24T11:42:16+08:00 p3-1 completed：deleted the CLI stub and placeholder
  install/signing/Homebrew artifacts；the single copyable command source now verifies immutable
  release/assets/attestation/checksum/candidate identity before an atomic user-bin install。
- 2026-07-24T11:43:00+08:00 p4-1 started：activate PR-safe Site validation and
  current-main-only production deployment for the fixed assets-only Cloudflare Worker。
- 2026-07-24T11:44:58+08:00 p4-1 completed：PR/fork runs only deterministic
  build/tests；push or explicit dispatch on current main rebuilds and deploys the fixed
  `ouro-ops-site` assets-only Worker through the reviewer-free `production` environment。
- 2026-07-24T11:46:00+08:00 p4-2 started：bind Site setup and prompts to the single
  verified reinstall source, add the pre-Cloudflare CLI floor and post-release current-main Site
  dispatch。
- 2026-07-24T12:04:00+08:00 p4-2 completed：the generated Site now injects the exact
  canonical verified-reinstall source and binds every Prompt to `$HOME/.local/bin/ouro-ops`；
  current-main Site deployment refuses before Cloudflare when the latest immutable CLI release is
  below any canonical Skill floor，and a completed CLI publish dispatches one current-main Site run。
- 2026-07-24T11:59:35+08:00 timestamp correction：the preceding p4-2 completion entry
  was recorded with an incorrect future wall-clock value；p4-2 completed before this correction。
- 2026-07-24T11:59:35+08:00 p5-1 started：run the mandatory live prerequisite gate
  before publishing or deploying；enabled repository immutable releases，created the reviewer-free
  and zero-wait `production` environment，and restricted it to branch `main`。
- 2026-07-24T11:59:35+08:00 p5-1 paused under the external-evidence exception：
  `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` are not configured as `production`
  environment secret names，and no secret source exists in the local process environment；no secret
  value was requested，read，logged or invented，and no release/Site workflow was dispatched。
- 2026-07-24T15:32:00+08:00 p5-1 resumed：the live prerequisite gate reports every
  required fact configured without reading secret values；fast-forwarded `next` and `main` to
  `94c263a` and dispatched one `patch` preparation run。
- 2026-07-24T15:38:18+08:00 p5-1 first controlled run produced release commit
  `a08cf25f79c9131566b4134579cad743ee21ab6a` and immutable `v0.1.1` with four native
  controls and artifact attestations；the publish run stopped after release creation because
  `gh release verify` raced GitHub's release-attestation propagation，so automatic Site dispatch did
  not run。A later read-only verification passed；a manual current-main Site dispatch deployed
  version `d8e61de2-2f70-45a9-a342-b903f339273b` only to diagnose the remaining path，not
  to claim TC-10。
- 2026-07-24T15:42:00+08:00 p5-1-fix1 started：repair the three production findings
  without replacing or mutating immutable `v0.1.1`。
- 2026-07-24T15:50:10+08:00 p5-1-fix1 completed：missing-`gh` now uses an actually
  empty fixture PATH，L2 uses an isolated venv，release verification retries only the bounded
  attestation-propagation window，and every Cloudflare deploy consumes the action's production URL
  for an exact canonical-source smoke。
- 2026-07-24T15:54:19+08:00 p5-1-fix2 started：the first remote L2 run proved the
  venv dependency list omitted pytest on Ubuntu/Debian and Rocky's preinstalled `curl-minimal`
  conflicts with an explicit `curl` package request；use the repository dependency authority and
  preserve the image-provided curl implementation。
- 2026-07-24T15:55:00+08:00 p5-1-fix2 implementation checkpoint：remote matrix
  evidence requires the corrected workflow to exist on GitHub；commit and push the still-in-progress
  item without marking it complete，then collect all three distribution results before its
  acceptance transition。
- 2026-07-24T15:58:23+08:00 p5-1-fix2 second matrix finding：the venv dependency and
  Rocky curl fixes passed their former failure points；all three images then reached the web
  generator's explicit `node --check` and proved Node was absent from the L2 runtime dependency
  declaration。Add distro `nodejs` packages and keep the item in progress for another real matrix。
- 2026-07-24T16:01:20+08:00 p5-1-fix2 third matrix finding：all Python and web checks
  pass after adding Node；the shared Rust failure is the CLI control-key test's real
  `ssh-keygen` dependency。Declare `openssh-client` on apt images and `openssh-clients` on Rocky，
  then require one final three-distribution matrix。
- 2026-07-24T16:06:41+08:00 p5-1-fix2 completed：the final main matrix
  `30077503263` passed Ubuntu 24.04，Debian 12 and Rocky 9 end to end；each image installed
  the declared Python/Node/SSH runtime and completed the full L2 suite。
- 2026-07-24T16:18:07+08:00 p5-1 completed：one `patch` dispatch prepared source
  `9fe922feb9dc16337360ed8607197264ffcdeb0c` and immutable `v0.1.2`，publish run
  `30077864986` produced and verified all four native controls，then automatically dispatched Site
  run `30078174024`。The Site run deployed Cloudflare version
  `83221c6d-d4a0-45a5-baa4-7be2f411fc65` to
  `https://ouro-ops-site.martincauu.workers.dev` and passed its post-deploy production smoke；
  no second operator dispatch was used。
- 2026-07-24T17:30:57+08:00 p5-1-fix3 started：operator accepted moving the full
  installer out of Production HTML and into each immutable GitHub Release；the website must retain
  a direct copy action for the shorter verified bootstrap。
- 2026-07-24T17:40:42+08:00 p5-1-fix3 implementation checkpoint：release aggregation
  now binds canonical `ouro-install.sh` into checksums and attestation；the Site embeds a 16-line
  bootstrap with an explicit copy action and refuses Cloudflare writes until the latest Release has
  a verified installer asset。Commit/push the in-progress item so a new patch Release can provide
  the mandatory production evidence before completion。

## 6. Validation Evidence (append-only)

- （待执行）
- TC-1 | stack: python | command: `python3 -m unittest -v
  tests.test_s0028_release_prerequisites` | result: pass | note: configured and missing snapshots
  return typed non-mutating facts；`--require-ready` gates only the production acceptance mode。
- TC-1 | stack: other | command: `python3 packaging/release-prerequisites.py --repo
  cauu/ouro-ops` | result: pass | note: live read-only probe reported immutable releases and the
  production environment/secrets as missing；Actions/rules were readable and Worker identity was
  `ouro-ops-site`；no secret value was read。
- TC-2 | stack: rust | command: `cargo test -q` | result: pass | note: 183 unit tests
  cover the fixed probe, Linux x86_64/amd64 acceptance, ARM/non-Linux/malformed refusal and
  write-free typed evidence。
- TC-2 | stack: python | command: `python3 tests/test_s0020_observability.py &&
  python3 tests/test_s0020_stateless_plan.py` | result: pass | note: live fake-SSH transport proves
  unsupported and injected probes never enter the runner-receiving session。
- TC-2 | stack: python | command: `python3 tests/test_s0020_stateless_apply.py &&
  python3 tests/test_s0020_kes_airgap_preflight.py && python3 tests/test_s0019_pipeline.py` |
  result: pass | note: fleet, apply and runner-plus-payload paths retain their existing behavior
  through the shared architecture gate。
- TC-3 | stack: python | command: `python3 tests/test_s0028_release_version.py` | result:
  pass | note: 0.1.0 patch/minor/major produce 0.1.1/0.2.0/1.0.0 and modify only the root Cargo
  package/lock records；workflow commit/tag identity is source-checked。
- TC-4 | stack: python | command: `python3 tests/test_s0028_release_version.py` | result:
  pass | note: partial exact commit/tag state resumes the same release；mismatched partial state is
  blocked and workflow source enforces clean tree, origin/main CAS, absent tag/release and tag-ref
  publish dispatch。
- TC-3 | stack: rust | command: `cargo metadata --locked --no-deps && cargo test --locked
  -q && python3 tests/test_release_candidate.py` | result: pass | note: locked Cargo metadata,
  183 Rust tests and paired release-source contract remain valid after preparation refactor。
- TC-5 | stack: python | command: `python3 tests/test_s0028_release_assets.py` | result:
  pass | note: canonical four-target set, single executable archive shape, version/target
  descriptors and one non-empty shared runner digest are enforced；runner labels map to native
  Linux/macOS x86_64/arm64 hosts。
- TC-6 | stack: python | command: `python3 tests/test_s0028_release_assets.py` | result:
  pass | note: tampered tarball and runner mismatch fail；publish source uses tag-ref identity,
  `actions/attest@v4` over canonical checksums, create-once release and `gh release verify`。
- TC-5 | stack: python | command: `make python-test` | result: pass | note: maintained
  Python release, packaging, Skill, deploy and safety contracts pass with the new publish boundary。
- TC-7 | stack: python | command: `python3 tests/test_s0028_verified_reinstall.py` |
  result: pass | note: clean-home fresh install covers macOS/Linux x86_64/arm64 mappings；macOS
  arm64 and Linux x86_64 cover N-1→N, identical no-write, prerelease/downgrade/unknown/digest and
  verification refusals at the exact `$HOME/.local/bin/ouro-ops` boundary。
- TC-7 | stack: rust | command: `cargo test -q` | result: pass | note: 183 tests pass
  after removing the `self-update` command/help implementation；live placeholder installer,
  signing identity and Homebrew formula are absent。
- TC-8 | stack: python | command: `python3 tests/test_s0028_site_workflow.py` | result:
  pass | note: workflow source proves PR/fork contains no secret/deploy path, production deploy is
  current-main-only and no Preview/comment/custom-domain/manual-approval mechanism exists。
- TC-8 | stack: ui | command: `./web/onboarding/build.sh && python3 -m pytest -q
  tests/test_web_generator.py && python3 tests/test_skill_docs.py` | result: pass | note: 13
  generator/HTTP tests and canonical six-Skill fidelity pass before Cloudflare deployment。
- TC-9 | stack: ui | command: `python3 -m pytest -q tests/test_web_generator.py &&
  python3 tests/test_skill_docs.py` | result: pass | note: all six generated Prompt payloads remain
  byte-exact canonical Skills；the setup block is byte-exact `packaging/verified-reinstall.sh`，
  every CLI invocation uses the verified user-bin path and no repo-local candidate remains。
- TC-9 | stack: python | command: `python3 tests/test_s0028_site_cli_floor.py &&
  python3 tests/test_s0028_site_workflow.py && python3 tests/test_release_surfaces.py` | result:
  pass | note: an insufficient or invalid release floor fails before the Cloudflare action；a
  successful immutable CLI publish dispatches the current-main Site workflow，whose production
  path remains current-main-only。
- TC-9 | stack: other | command: `make python-test && cargo test -q && git diff --check` |
  result: pass | note: the complete maintained Python/integration suite and all 183 Rust tests pass；
  legacy transport failure expectations now reflect the fixed pre-upload architecture probe。
- TC-11 | stack: python | command: `python3 tests/test_s0028_release_assets.py &&
  python3 tests/test_s0028_verified_reinstall.py && python3 tests/test_s0028_site_workflow.py &&
  python3 tests/test_s0028_production_site.py` | result: pass | note: source contracts cover bounded
  release-attestation retry，an actually missing `gh`，Cloudflare deployment URL wiring and exact
  local production-form smoke。
- TC-11 | stack: other | command: `make python-test && cargo test --locked -q && git diff
  --check` | result: pass | note: the full maintained Python/integration suite and all 183 locked
  Rust tests pass after the production portability fixes。
- TC-11 | stack: ui | command: `python3 packaging/verify-production-site.py --url
  https://ouro-ops-site.martincauu.workers.dev` | result: pass | note: the first deployed production
  page returned HTTP success，network-denying CSP，canonical GitHub link，six byte-exact Skills and
  the byte-exact verified installer，with no repo-local candidate；this diagnostic deployment is not
  used as TC-10's automatic baseline。
- TC-12 | stack: python | command: `python3 tests/test_s0028_l2_workflow.py` | result:
  pass | note: the workflow uses the canonical dev-requirements file，preserves Rocky
  `curl-minimal`，and declares Node plus distro-correct SSH client packages。
- TC-12 | stack: other | command: GitHub Actions run `30077503263` | result: pass |
  note: Ubuntu 24.04，Debian 12 and Rocky 9 each passed dependency installation and the complete
  L2 integration suite on main commit `1659066215e1d3ff4141b8c7af433e67ff3e2d08`。
- TC-10 | stack: other | command: GitHub Actions prepare run `30077822758` and publish
  run `30077864986` | result: pass | note: one patch selection created sole source/tag
  `v0.1.2@9fe922feb9dc16337360ed8607197264ffcdeb0c`；the tag-ref workflow executed four
  native controls，attested the canonical checksums，verified the immutable release after bounded
  propagation retry and automatically dispatched Site。
- TC-10 | stack: other | command: `gh release verify v0.1.2 --repo cauu/ouro-ops` |
  result: pass | note: immutable release identity resolves to the source commit；macOS arm64 asset
  digest is `3ee4dbad5a09588e7e4ed2e2f10cfbf07591682e39259ccce35f0d23d95797c0`，
  its artifact attestation and checksum pass，the native binary reports `0.1.2` and all controls
  bind embedded Linux/x86_64 runner digest
  `8e53114e582c4ebd2edbe0a60eef4a8f012dfa26628ece7262b6b198f43217a4`。
- TC-10 | stack: ui | command: automatic GitHub Actions Site run `30078174024` and
  `python3 packaging/verify-production-site.py --url
  https://ouro-ops-site.martincauu.workers.dev` | result: pass | note: current-main source
  `9fe922feb9dc16337360ed8607197264ffcdeb0c` deployed Cloudflare version
  `83221c6d-d4a0-45a5-baa4-7be2f411fc65`；both workflow and independent smoke observed 148711
  exact bytes，network-denying CSP，canonical GitHub link，six byte-exact Skills，the verified
  installer and no repo-local candidate。
- TC-10 | stack: other | command: `python3 packaging/release-prerequisites.py --repo
  cauu/ouro-ops --require-ready` | result: pass | note: before production mutation，immutable
  releases，repository permissions/rules，reviewer-free main-only `production` environment，
  both Cloudflare secret names and fixed `ouro-ops-site` Worker identity were all configured
  without reading secret values。
- TC-10 | stack: other | command: `cargo test --locked -q && python3
  tests/test_s0028_release_assets.py && python3 tests/test_s0028_site_workflow.py && python3
  tests/test_s0028_verified_reinstall.py && python3 tests/test_s0028_l2_workflow.py && python3
  tests/test_s0028_production_site.py && git diff --check` | result: pass | note: all 183 locked
  Rust tests and focused release/Site/install/L2/source-fidelity regressions pass at the final
  `0.1.2` source baseline。
- TC-13 | stack: other | command: `make python-test && cargo test --locked -q && git diff
  --check` | result: pass | note: full maintained integration coverage and all 183 Rust tests pass；
  release fixtures require and tamper-test the fifth `ouro-install.sh` checksum subject，bootstrap
  fixtures prove verification precedes execution/refusal is write-free，and Site source tests bind
  the copy action to the exact 16-line canonical bootstrap while excluding the complete installer。

## 7. Change Requests (append-only)

- 2026-07-24T09:54:02+08:00 operator 明确执行顺序为 S0028 release workflows →
  S0029 full E2E；CLI 方案不得由 agent 在讨论前自行决定。
- 2026-07-24T10:06:06+08:00 operator 确认 control CLI 支持 Linux 和 macOS；draft
  提出 Linux/macOS × x86_64/arm64 四 control artifact。
- 2026-07-24T10:26:44+08:00 operator 移除发布用户审批：维护者主动
  `workflow_dispatch` 后，release/Site 自动 gates 连续执行。
- 2026-07-24T10:48:52+08:00 operator 将 SemVer bump 集成进 release workflow：
  维护者只选择 patch/minor/major，workflow 自动更新 `Cargo.toml`/`Cargo.lock`。
- 2026-07-24T11:05:03+08:00 operator 接受反方收敛建议：四 control artifact 保留；
  remote target 第一阶段只支持 Linux/x86_64 并在 runner upload/write 前拒绝 ARM；
  删除 Site Preview/PR comment/custom-domain gate、自定义 release manifest、全 PR 四平台
  matrix、CLI release environment、Homebrew/npm/self-update apply 和独立签名链；
  安装/更新统一为 GitHub-CLI verified reinstall，S0018 以 replaced 归档。
- 2026-07-24T11:11:58+08:00 final executability audit 收敛最后歧义：删除而非兼容保留
  `self-update` stub 与 live placeholder install/signing/Homebrew artifacts；canonical
  install path 固定为 `$HOME/.local/bin/ouro-ops`；p1-1 以 verifier/runbook 可独立完成，
  external configured 状态延后到 p5-1/TC-10 production acceptance 强制，避免 secrets
  wiring 阻塞 repo implementation。
- 2026-07-24T17:30:57+08:00 operator rejected copying the complete 169-line installer
  from the website as too heavy；accepted publishing canonical `ouro-install.sh` beside the CLI
  assets and keeping only a copyable verified bootstrap on the Site。
