# Ouro Ops

让 AI Agent（Grok Bot、Claude Code 等）运维 Cardano 矿池。

- [`ouro-skills/pool-ops`](ouro-skills/pool-ops/SKILL.md)：日常运维。Agent 直接通过 SSH + docker compose 做状态检查、出块就绪判断、排障、重启、改配置和升级；会改动节点的操作先给用户确认。
- [`ouro-skills/kes-rotation`](ouro-skills/kes-rotation/SKILL.md)：KES 轮换。通过 `ouro-ops` CLI 在 BP 上生成新 KES 密钥，生成离线冷签包，校验后安装。
- `ouro-ops` CLI（`crates/ouro`）：目前只有 KES 轮换需要它。

官网（`web/onboarding`）给出一段可以直接发给 Agent 的安装指令，并在 `/skills/<name>/SKILL.md` 发布这两个 Skill。

矿池信息放在 Agent 机器上的 `pool-spec.yaml`（schema：[`schemas/pool-spec.schema.json`](schemas/pool-spec.schema.json)）；矿池特有的情况写在旁边的 `POOL-NOTES.md`。这两个文件都不进仓库。

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
target/debug/ouro-ops inbox preview --type <opcert|tx> --file <path>

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
target/debug/ouro-ops contract
make release-candidate   # macOS；构建并验收配对候选，但不发布
```

Fresh Fleet Deploy 使用 `deploy inspect|apply|check`：先只读识别 supported clean/partial
主机并展示 signed image/change set，获得一次 operator 同意后以单次 Apply 配置
Ubuntu/Chrony/UFW/Compose，最后统一检查 BP/Relay 的静态部署形状与启动 readiness。
`onboard` 和 `adopt` 仅保留给运维方明确要求的 S0019 migration/recovery，不是当前操作的恢复建议
或前置步骤。

项目采用 immutable-spec 交付：每个需求由一份 append-only spec 记录需求、设计、item 计划和验收
证据，item 级提交引用 spec 文件名。索引见 [`docs/README.md`](docs/README.md)。
