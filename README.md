# Ouro Ops

面向 Cardano Stake Pool 的确定性运维 CLI。用户把网站生成的 operation prompt 交给 AI agent；
agent 从已验证的 `ouro-ops` 二进制读取对应 Skill，只向类型化操作传参数，危险写入仍由运维方对
最终 live plan 显式确认。

当前 active 规格是
[`S0020 Agentless Ephemeral Runner`](docs/specs/20260716T1441-S0020-agentless-ephemeral-runner.md)。
S0019 的目标机常驻 CLI、adoption attestation 和 control↔target 版本耦合已退出普通操作路径。

## 当前架构

| 层 | 角色 | 当前职责 |
| --- | --- | --- |
| Agent harness | Codex / Claude Code 等 | 对话、展示 plan、等待运维方批准 |
| Embedded Skills | `ouro-skills/` | 决策框架、停止条件、红线和准确命令 |
| Control CLI | `crates/ouro` 的 `ouro-ops` | pool-spec/凭据/host key、签名策略、plan/confirm/permit、审计 |
| Ephemeral runner | control release 内嵌 Linux/x86_64 静态 runner | 每次操作临时传到目标、校验、执行闭合命令、返回结构化结果并清理 |
| BP / Relay | 现有 `cardano` SSH 账号 + Cardano 容器 | 不需要常驻 Ouro CLI、daemon、gate、attestation 或 Ouro 版本状态 |

一次普通远端操作会创建 run-unique 0700 临时目录，校验 control 选定 runner 的 SHA-256，
通过 `sudo env -i` 执行闭合的 `target` 子命令，限制时间/输出并清理。agent 不能选择 runner、远端
路径、hash 或 sudo argv。

## 运维流程

- 只读 observability 直接从当前节点 tip 返回证据，无 adoption/confirmation 前置条件。
- runtime、KES、upgrade 的 `--plan` 绑定 pool spec、host key、role/network/genesis、签名 OCI
  策略与当前容器状态。
- apply 必须携带 operator 批准的 `candidate_hash` 和一次性 confirm token，并在写前重新生成同一
  candidate；运行时漂移会在 mutation 前拒绝。
- restart/KES/upgrade step 还需要最后生成、30 秒有效的 fleet permit，机制校验 relay quorum 与
  BP-last。
- KES opcert 与 upgrade image archive 先在 control 本地 `inbox preview`，实际 apply 才和 runner
  一次性传输；目标没有持久 inbox。
- troubleshooting 复用 pool spec 中现有 `cardano` 账号。host-key、超时、输出和审计仍受控，但
  必须先运行按角色解释的 typed snapshot；仅针对剩余证据缺口使用 `diag exec`。诊断不是 OS
  机制强制的只读通道；这是 S0020 明确选择的 honest-agent 边界。

完整命令与能力顺序见 [`docs/S0020-operations.md`](docs/S0020-operations.md) 和对应
`ouro-ops skill show <operation>`。

## 快速开始

```bash
cargo build -p ouro

# 校验声明式 pool spec
target/debug/ouro-ops spec validate --spec examples/pool-spec.minimal.yaml

# 固定只读 tip（host/key 从 operator-owned spec 映射）
target/debug/ouro-ops op run --op observability/health \
  --spec <pool-spec> --dispatch <host> --ssh-key creds://<name> \
  --node <id> --param machine=<id>

# 排障基线（BP 会包含 KES/opcert 与 block_production_ready）
target/debug/ouro-ops op run --op troubleshooting/snapshot \
  --spec <pool-spec> --dispatch <host> --ssh-key creds://<name> \
  --node <id> --param machine=<id>

# 仅针对 snapshot 的剩余证据缺口；这里 --dispatch 使用 machine id
target/debug/ouro-ops diag exec --dispatch <id> --spec <pool-spec> -- <diagnostic-command>

# live-state-bound restart plan（不会重启）
target/debug/ouro-ops op run --op runtime/restart --spec <pool-spec> \
  --dispatch <host> --ssh-key creds://<name> --node <id> \
  --param machine=<id> --plan

# 本地 public artifact 预览（不会复制/暂存）
target/debug/ouro-ops inbox preview --type <opcert|image> --file <path>

# 在线验签并选择当前镜像；不带 --from 为部署推荐，带 --from 为升级下一跳
target/debug/ouro-ops release select --platform linux/amd64 \
  --from sha256:<current-image-config-digest>
```

`pool-spec.yaml` 是运维方持有的声明式路由/身份/策略数据，schema 为
[`schemas/pool-spec.schema.json`](schemas/pool-spec.schema.json)。敏感值只允许 `creds://` 引用；
私钥内容不进入 spec、JSON 或模型上下文。

## 质量门

```bash
make test
make python-test
bash ci/l2-integration.sh
cargo clippy -p ouro --lib --tests -- -D warnings
target/debug/ouro-ops manifest verify --against packaging/bundle-manifest.json
```

`deploy/register-submit` 使用同一套 agentless 一次性 runner：先审阅 operator 已签名交易并绑定
候选，精确批准后最多执行一次提交；节点接受不等于链上确认，拒绝或结果歧义都不会自动重试。
`onboard` 和 `adopt` 仅保留给运维方明确要求的 S0019 migration/recovery，不是当前操作的恢复建议
或前置步骤。

项目采用 immutable-spec 交付：每个需求由一份 append-only spec 记录需求、设计、item 计划和验收
证据，item 级提交引用 spec 文件名。索引见 [`docs/README.md`](docs/README.md)。
