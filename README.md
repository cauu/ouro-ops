# Ouro Ops

Cardano Stake Pool 控制平面（macOS）。基于 Tauri 2 + React + TypeScript，详见 [PRD](docs/prd/v1.0.md) 与 [详细设计](docs/detail-design/v1.0.md)。

## 环境要求

- Node.js 18+
- Rust 1.80+
- Python 3.11+（Sidecar 使用，需能执行 `sidecar/src/runner_bridge.py`）
- macOS（当前目标平台）
- 目标机部署依赖（Ansible 侧）：
  - `python3 -m pip install ansible-runner pyyaml ansible`
  - `ansible-galaxy collection install community.docker`

## 安装与运行

```bash
# 安装前端依赖
npm install

# 开发模式（启动 Vite + Tauri 窗口，Sidecar 由 Rust 自动拉起 python3 runner_bridge.py）
npm run tauri dev

# 构建前端
npm run build

# 构建应用（若 CI 环境报错可先执行 unset CI）
npm run tauri build
```

## Phase 1 交付物（当前）

- **可运行应用**：`npm run tauri dev` 启动后可见占位页，展示 Sidecar ping 与 DB 版本。
- **SQLite**：首次启动在应用数据目录创建 `ouro_ops.sqlite`，执行迁移 `001_init`（pool / machine / kes_state / task / task_machine / machine_health / audit_log）。
- **Sidecar**：`sidecar/src/runner_bridge.py` 通过 stdin/stdout JSON-RPC 支持 `ping`、`run_playbook`、`shutdown`；无 ansible-runner 时 run_playbook 返回 mock 事件。
- **事件**：Rust 将 Sidecar 事件转发为 Tauri 事件 `task:progress`、`task:completed`、`task:failed`。
- **错误类型**：`src-tauri/src/error.rs` 中 `AppError` 含 1xxx～5xxx 分类。

## 部署默认值（Phase 3）

- 默认镜像：`ghcr.io/blinklabs-io/cardano-node`
- 默认 tag：`latest`
- 覆盖规则：
  - 仅指定 `image_registry`/`cardano_version` 时，使用 `<registry>:<tag>`
  - 指定 `image_digest` 时，优先使用 `<registry>@<digest>`
- 启动语义：对齐 `blinklabs-io/docker-cardano-node` 的 `run` 模式
  - 容器数据库路径：`/data/db`
  - 容器 socket 路径：`/ipc/node.socket`
- 兼容迁移：若存在旧目录 `/opt/cardano/data` 且新目录 `/opt/cardano/db` 为空，部署时会做一次性迁移并记录日志。
- `safe_validation_mode=true` 时只执行只读校验，不变更系统配置、不重启容器。

## 项目结构

```
ouro-ops/
├── src/                 # React 前端
├── src-tauri/           # Tauri Rust 后端
│   ├── src/commands/   # IPC 命令（ping, db_version, run_playbook_test）
│   ├── src/db/          # SQLite 迁移与 repo
│   ├── src/sidecar/     # Sidecar 进程管理与事件转发
│   └── migrations/
├── sidecar/             # Python runner_bridge（JSON-RPC + ansible-runner）
└── docs/
```

## 开发计划

见 [开发计划 v1.0](docs/development-plan/v1.0.md)。Phase 1 已完成实现；验收测试见 [测试用例](docs/test-cases/v1.0.md) 中 TC-DB-001/002、TC-SC-001/002/003、TC-EVT-*、TC-ERR-003。
