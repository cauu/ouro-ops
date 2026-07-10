# Ouro Ops

Cardano Stake Pool 运维工具面。产品形态为 **一个确定性 CLI（`ouro`）+ 一组 skills（`ouro-skills/`）**：
agent loop 与交互 UI 复用现成 agent harness（Claude Code / Cowork），不再自研桌面 app。

> 架构自 S0014 起从「Tauri + React + Python sidecar + Ansible」四语言桌面栈转为 CLI + skills。
> 规格与执行标准见 [`docs/specs/20260708T0000-S0014-agent-tooling.md`](docs/specs/20260708T0000-S0014-agent-tooling.md)。
> 旧桌面 / sidecar / Ansible 栈已退役（见该 spec §2.7 退役清单）。

## 四层架构

| 层 | 角色 | 说明 |
| --- | --- | --- |
| Agent Harness | Claude Code / Cowork | agent loop、确认交互、流式日志、会话记录，全部复用 |
| Skills（runbook 层） | `ouro-skills/` | `SKILL.md` 只写决策树与红线；`scripts/` 承载已知操作逻辑（Bash，幂等 + JSON 契约）|
| `ouro` CLI（确定性内核） | `crates/ouro`（Rust 单二进制）| 密钥、签名、配置渲染、审计、审计化脚本执行入口、确定性编排 |
| 目标机器 | BP / Relay | 节点跑容器；两类 SSH principal：受限诊断身份（L3）、经 sudoers allowlist 的执行身份（L1/L2）|

## 安全模型（机制级，非 prompt 约定）

- **写操作唯一入口**：一切写操作只能经 `ouro-ops tool run <skill>/<script>`；该命令建审计上下文、注入 **HMAC 签名的 invocation token**，脚本经 `ouro-ops tool verify-context` 校验后才放行——仅设置 `OURO_AUDIT_ID` 环境变量无法绕过。
- **强制审计**：每次调用记录 append-only 的 `start` / `finish` / `crash` 事件（SQLite）。
- **带外确认令牌**：破坏性动作（`kes push`、`rollback`）需人显式经 `ouro-ops confirm create` 签发的一次性 `tok_` 令牌；agent 无签发权限，不存在可猜测的静态令牌。
- **密钥隔离**：冷钥 / KES skey / VRF 永不进模型上下文；JSON / 审计 / 日志只记录 hash、路径、counter、metadata。
- **确定性编排**：跨机顺序由 `upgrade/scripts/run.sh` 执行（原子 machine lock、relay quorum、BP-last、verify-before-next、失败即停批次）；「BP + 至少一个 relay 在线」不变式由 `spec.upgrade.min_online_relays`（默认 1）机制强制，不可经环境放宽。

## 环境要求

- Rust 1.80+（`cargo` 构建）
- Python 3.11+（L2 脚本与测试用；需 `pyyaml`、`jsonschema`）
- Bash、SSH、目标机侧容器运行时
- macOS / Linux

## 快速开始

```bash
# 构建 CLI
cargo build            # 或 make check

# 校验 pool-spec（只读，输出单行 JSON）
cargo run -- spec validate --spec examples/pool-spec.minimal.yaml

# 渲染某台机器的节点配置 / 拓扑
cargo run -- config render --spec examples/pool-spec.minimal.yaml --machine bp1 --out /tmp/render

# 只读状态与漂移
cargo run -- status --snapshot tests/fixtures/status/healthy-preprod.json \
  --spec examples/pool-spec.minimal.yaml --diff-spec

# 只读池概览（承接原 Delegators 点态数据）
cargo run -- pool overview --spec examples/pool-spec.minimal.yaml

# 经审计入口执行一个 L2 skill 脚本
cargo run -- tool run deploy/preflight --spec examples/pool-spec.minimal.yaml --machine bp1
```

`ouro` 主要子命令：`spec validate`、`config render|apply`、`status`、`pool overview|register-tx`、
`kes generate|counter status|push`、`rollback`、`confirm create`、`tool run|verify-context`、`audit init|log`、
`legacy inspect`。所有 L1/L2 stdout 遵循 [`schemas/tool-output.schema.json`](schemas/tool-output.schema.json) 单行 JSON 契约。

## pool-spec.yaml

用户唯一需提供的声明式目标状态，schema 为 [`schemas/pool-spec.schema.json`](schemas/pool-spec.schema.json)（版本化，
`spec_version: 1`）。样例见 [`examples/pool-spec.minimal.yaml`](examples/pool-spec.minimal.yaml) 与
[`examples/pool-spec.complete.yaml`](examples/pool-spec.complete.yaml)。敏感值只允许 `creds://` 引用（明文不进 spec / 模型 / JSON）。

## 常用命令（Makefile）

```bash
make test    # cargo test
make check   # cargo check
make ci      # bash ci/l2-integration.sh（schema / 脚本 / 安全负向 / parity + cargo test）
make e2e     # bash ci/harness-e2e.sh（harness 式端到端：deploy / upgrade / kes / takeover）
make help    # 列出全部命令
```

## 项目结构

```
ouro-ops/
├── crates/ouro/          # ouro CLI（Rust 单二进制：domain/config/render/status/kes/pool/audit/confirm/ssh/...）
├── ouro-skills/          # L2 skills：SKILL.md 决策层 + scripts/（deploy/upgrade/runtime/observability/...）
│   └── lib/ouro-lib.sh   # 幂等 check-then-act、审计门、JSON 契约、脱敏
├── schemas/              # pool-spec.schema.json、tool-output.schema.json
├── examples/             # pool-spec 样例
├── ci/                   # l2-integration.sh、harness-e2e.sh
├── tests/                # Rust / Python 契约与安全负向测试、fixtures
└── docs/specs/           # immutable-spec 交付文档（当前 active：S0014）
```

## 交付流程

采用 immutable-spec 工作流：每个需求一份 append-only spec，item 级提交引用 spec 文件名。索引见
[`docs/README.md`](docs/README.md)。
