# S0009 p9-10 E2E 验收与回滚预案校验

日期：2026-03-13

## 1. 验收范围
- 已完成项：p9-1 ~ p9-9
- 当前校验项：
  - 端到端链路可执行性（playbook/monitor/dashboard 关键路径）
  - 回滚路径可执行性（观测网关回退）

## 2. 自动化校验结果

### 2.1 Ansible 语法与流程
- `ansible/playbooks/deploy.yml` 语法检查通过。
- `ansible/playbooks/observability-bootstrap.yml` 语法检查通过。
- `ansible/playbooks/observability-rollback.yml` 语法检查通过。

### 2.2 Rust / UI 回归
- `cargo test -q tc_mon_` 通过（21 tests）。
- `cargo test -q tc_dep_018_deploy_playbook_includes_observability_gateway_defaults` 通过。
- `cargo test -q tc_dep_019_observability_rollback_playbook_exists` 通过。
- `pnpm -s build` 通过。

## 3. TC 状态总览
- TC-P9-001: pass（S0009 为唯一 active，S0008 completed/replaced）
- TC-P9-002: pass（网关模板已实现 Basic Auth，未认证可返回 401）
- TC-P9-003: pass（白名单端点拒绝 query 参数，客户端无法透传任意 PromQL）
- TC-P9-004: pass（已提供 `observability-bootstrap.yml` 增量接入）
- TC-P9-005: pass（deploy 已内建 relay 观测网关阶段）
- TC-P9-006: pass（Dashboard 与 monitor 已完成 10 项字段链路映射与展示补强）
- TC-P9-007: pass（relay 不可用时 local fallback + 缓存策略保底）
- TC-P9-008: pass（多 relay 退避与最新时间戳选优已落地）

## 4. 回滚预案

### 4.1 快速回滚（推荐）
1. 执行：
   - `ansible-playbook ansible/playbooks/observability-rollback.yml -i <inventory>`
2. 结果：
   - 移除 `ouro-ops-metrics.conf` 与 htpasswd 文件。
   - 重新校验并 reload nginx。

### 4.2 软回退（保留配置，仅停用）
1. deploy 或后续变更时设置：
   - `enable_ops_observability_gateway=false`
2. 结果：
   - 后续执行不再更新/启用观测网关配置。

## 5. 已知限制
- 当前环境未做真实公网 relay/BP 联调（本地仓库内无法直接连接目标机器），因此运行态连通性需在目标环境按 playbook 执行后复核。
- App 使用 relay API 认证依赖环境变量注入（`OURO_OPS_RELAY_TELEMETRY_*`），需在发布/启动脚本中固化。
