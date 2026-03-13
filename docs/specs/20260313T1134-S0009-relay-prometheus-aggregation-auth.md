# Phase 6 Relay Aggregated Prometheus API & Auth Delivery

Spec-ID: S0009
状态: active
创建时间: 2026-03-13 11:34 +0800
开始时间: 2026-03-13 11:34 +0800
完成时间:
前一个 Spec-ID: S0008
结项原因:

## 1. Requirement Details
- Background
  - `S0008` 的 `p8-5` 在静态验收（代码存在性）通过，但运行态验收失败：Dashboard 未稳定展示 Prometheus 指标。
  - 当前已有部署已完成，不适合重跑整套部署流程；需要增量接入观测能力。
  - 新目标是以 relay 为统一观测出口，支持多 relay 高可用查询，并将该能力纳入后续新部署默认流程。
- Scope
  - 设计并落地“relay 聚合 Prometheus 查询”能力：App 通过 relay 公网接口获取 BP/Relay 指标。
  - 安全控制本阶段仅覆盖认证能力（最小成本方案优先），先实现 Basic Auth + HTTPS。
  - 为“已部署环境”提供增量接入 playbook（不重跑完整 deploy）。
  - 为“新部署环境”内建观测与认证配置步骤（默认启用）。
  - 修复 Dashboard Prometheus 指标链路，完成运行态可验收闭环。
- Constraints
  - 保持 mac 客户端现有架构，不引入重型中心化依赖。
  - 不把 BP Prometheus 直接暴露公网；BP 仅对 relay 内网可达。
  - 不开放任意 PromQL 给客户端；客户端仅调用白名单 API。
- Non-goals
  - 本阶段不实现完整零信任体系（mTLS、复杂 RBAC、WAF 策略）
  - 本阶段不重构 Dashboard 视觉结构，仅修复数据来源与可用性。

## 2. Outline Design
- Architecture / modules impacted
  - Relay：Prometheus 抓取 BP/Relay 指标（内网）。
  - Relay 网关（Nginx）：暴露白名单查询 API 与 Basic Auth。
  - 后端 monitor：新增/切换到 relay 聚合 API 数据源，保留本地 monitor fallback。
  - 前端 Dashboard：消费统一快照字段并展示 source/note/时间戳。
  - Ansible：新增增量接入 playbook，并将观测/认证步骤并入新部署流程。
- Data model and interfaces
  - 白名单 API（示例）：
    - `GET /api/ops/v1/telemetry/snapshot`
  - 响应字段（与现有 `MonitorSnapshot` 对齐）：
    - `epoch` `sync_percent` `tip_diff_blocks` `peer_count`
    - `cpu_sys_percent` `mem_live_bytes` `mem_rss_bytes` `mem_heap_bytes`
    - `gc_minor_total` `gc_major_total`
    - `collected_at` `prometheus_source` `prometheus_note`
  - 聚合标识：按 `node/role` label 归并，避免多节点指标混淆。
- Risk and rollback strategy
  - 风险 1：relay 接口不可达或认证配置错误导致数据中断。
    - 策略：App 侧保留 monitor fallback + 缓存回退；接口切换支持灰度开关。
  - 风险 2：多 relay 返回数据不一致。
    - 策略：以 `collected_at` 最新为准，并记录 source 便于排查。
  - 风险 3：增量改造影响已运行节点。
    - 策略：单独 playbook、可回滚配置、逐节点滚动执行。

## 3. Execution Plan
- [x] p9-1 启动 S0009 active spec，冻结目标与验收标准
- [x] p9-2 输出 p8-5 失败运行态根因报告（端点、映射、fallback 分支）
- [x] p9-3 设计 relay 白名单 API 契约与字段映射表（含空值/时间戳兜底）
- [x] p9-4 实现 relay 网关 Basic Auth + HTTPS + 白名单 API 路由模板
- [x] p9-5 实现/修复 monitor 数据源：relay API 优先，local monitor fallback
- [x] p9-6 实现多 relay 主备切换策略（超时、退避、最新时间戳选优）
- [x] p9-7 新增现网增量接入 playbook（bootstrap，不重跑 full deploy）
- [x] p9-8 将观测与认证步骤并入新部署流程（deploy 默认内建）
- [ ] p9-9 Dashboard 运行态联调与可观测性补强（source/note/collected_at）
- [ ] p9-10 完成端到端验收与回滚预案校验，准备结项评审

## 4. Test And Acceptance Criteria
- TC-P9-001 `docs/specs/` 根目录仅保留 `S0009` 为 active；`S0008` 进入 completed 且结项原因为 `replaced`。
- TC-P9-002 relay 白名单 API 在无认证时返回 `401`，认证通过返回 `200`。
- TC-P9-003 客户端无法提交任意 PromQL（query 参数被拒绝或不可达）。
- TC-P9-004 已部署环境执行增量 playbook 后，无需 full deploy 即可查询到 BP/Relay 指标。
- TC-P9-005 新部署流程默认完成观测与认证配置，部署完成即可可查询。
- TC-P9-006 Dashboard 在至少 1 BP + 1 Relay 场景中，10 项核心 Prometheus 字段中每节点至少 6 项非空。
- TC-P9-007 relay 不可用时，Dashboard 继续展示缓存/兜底数据，不崩溃。
- TC-P9-008 多 relay 场景下可自动切换，并以最新 `collected_at` 数据驱动展示。

## 5. Execution Log (append-only)
- 2026-03-13 11:34 +0800 p9-1 started: 基于 S0008 运行态失败反馈启动新阶段，收敛 relay 聚合 + 认证 + 部署内建目标。
- 2026-03-13 11:34 +0800 p9-1 completed: S0009 active spec 已创建并冻结计划/验收基线。

## 6. Validation Evidence (append-only)
- TC-P9-001 | stack: other | command: ls -la docs/specs docs/specs/completed | result: pass | note: S0008 已迁移 completed，S0009 为唯一 active spec

## 7. Change Requests (append-only)
- 2026-03-13 11:34 +0800 新需求建立：在已部署环境最小成本接入 relay 聚合观测与认证能力，并纳入新部署默认流程。

## 8. Addendum (append-only)
### 8.1 Execution Plan Delta
- [x] p9-2 输出 p8-5 失败运行态根因报告（端点、映射、fallback 分支）

### 8.2 Execution Log Delta
- 2026-03-13 11:51 +0800 p9-2 started: 复核 p8-5 的采集端点、指标映射和 fallback 分支，定位运行态失败根因。
- 2026-03-13 11:51 +0800 p9-2 completed: 形成根因报告并给出 P0/P1 修复优先级与 S0009 后续执行输入。

### 8.3 Validation Evidence Delta
- TC-P9-006 | stack: other | command: rg -n "collect_prometheus_metrics|map_prometheus_metrics|nview:9090|cardano-node:12798|host:12798|host:12788" src-tauri/src/commands/monitor.rs | result: pass | note: 已定位运行态失败核心链路（端点候选与映射逻辑）
- TC-P9-006 | stack: other | command: nl -ba ansible/roles/cardano-node/tasks/main.yml | sed -n '350,390p' | result: pass | note: 已确认部署默认仅映射 3001，12798/12788 未对外映射
- TC-P9-006 | stack: other | command: nl -ba docs/specs/completed/20260312T1446-S0008-phase5-mac-app-delivery.md | sed -n '214,226p' | result: pass | note: 已确认 p8-5 验收主要为静态存在性检查
- TC-P9-006 | stack: other | command: test -f docs/review/20260313-p8-5-prometheus-runtime-root-cause.md && echo ok | result: pass | note: 根因报告文档已生成并可追溯

### 8.4 Change Request Delta
- 2026-03-13 11:51 +0800 执行推进：按 S0009 计划完成 p9-2，下一步进入 p9-3（白名单 API 契约与字段映射冻结）。

## 9. Addendum (append-only)
### 9.1 Execution Plan Delta
- [x] p9-3 设计 relay 白名单 API 契约与字段映射表（含空值/时间戳兜底）

### 9.2 Execution Log Delta
- 2026-03-13 11:53 +0800 p9-3 started: 冻结 relay 白名单 API 契约，明确无任意 PromQL 的端点模型与字段映射策略。
- 2026-03-13 11:53 +0800 p9-3 completed: 输出 V1 轻量契约（10 个固定端点 + 统一响应结构 + label/空值/时间戳兜底 + 多 relay 选优规则）。

### 9.3 Validation Evidence Delta
- TC-P9-003 | stack: other | command: rg -n "不接受查询参数|若存在参数返回|禁止 query 参数透传" docs/review/20260313-p9-3-relay-whitelist-api-contract.md | result: pass | note: 契约已明确禁用任意 PromQL 入参
- TC-P9-006 | stack: other | command: rg -n "telemetry/epoch|telemetry/sync-percent|telemetry/tip-diff-blocks|telemetry/peer-count|telemetry/cpu-sys-percent|telemetry/mem-live-bytes|telemetry/mem-rss-bytes|telemetry/mem-heap-bytes|telemetry/gc-minor-total|telemetry/gc-major-total" docs/review/20260313-p9-3-relay-whitelist-api-contract.md | result: pass | note: 10 项核心字段端点与映射已冻结
- TC-P9-008 | stack: other | command: rg -n "多 relay 选优|最新的 relay 数据|source_relay" docs/review/20260313-p9-3-relay-whitelist-api-contract.md | result: pass | note: 多 relay 切换与时间戳选优策略已固化
- TC-P9-003 | stack: other | command: test -f docs/review/20260313-p9-3-relay-whitelist-api-contract.md && echo ok | result: pass | note: 契约文档已落盘，可作为 p9-4/p9-5 直接输入

### 9.4 Change Request Delta
- 2026-03-13 11:53 +0800 执行推进：完成 p9-3 契约冻结，下一步进入 p9-4（Nginx Basic Auth + 白名单 API 模板实现）。

## 10. Addendum (append-only)
### 10.1 Execution Plan Delta
- [x] p9-4 实现 relay 网关 Basic Auth + HTTPS + 白名单 API 路由模板

### 10.2 Execution Log Delta
- 2026-03-13 11:59 +0800 p9-4 started: 新增 relay 网关角色，落地 Basic Auth + HTTPS + 固定查询白名单路由。
- 2026-03-13 11:59 +0800 p9-4 completed: 已实现 `ops-observability-gateway` 角色（defaults/tasks/handlers/template），覆盖认证、TLS 材料校验、Nginx 配置渲染与 10 个 telemetry 白名单端点。

### 10.3 Validation Evidence Delta
- TC-P9-002 | stack: other | command: rg -n "auth_basic|auth_request|ops_metrics_htpasswd_path|ssl_certificate|ssl_certificate_key" ansible/roles/ops-observability-gateway/templates/ouro-ops-metrics.conf.j2 ansible/roles/ops-observability-gateway/tasks/main.yml | result: pass | note: 认证与 HTTPS 核心配置已落地
- TC-P9-003 | stack: other | command: rg -n "if \(\$args != \"\"\) \{ return 400; \}|api/v1/query\?query=" ansible/roles/ops-observability-gateway/templates/ouro-ops-metrics.conf.j2 | result: pass | note: 白名单端点已禁用任意 query 参数透传
- TC-P9-006 | stack: other | command: rg -n "telemetry/epoch|telemetry/sync-percent|telemetry/tip-diff-blocks|telemetry/peer-count|telemetry/cpu-sys-percent|telemetry/mem-live-bytes|telemetry/mem-rss-bytes|telemetry/mem-heap-bytes|telemetry/gc-minor-total|telemetry/gc-major-total" ansible/roles/ops-observability-gateway/templates/ouro-ops-metrics.conf.j2 | result: pass | note: 10 项核心指标端点已在网关模板中固化
- TC-P9-002 | stack: other | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local /tmp/ops-observability-syntax.yml | result: pass | note: 角色语法检查通过

### 10.4 Change Request Delta
- 2026-03-13 11:59 +0800 执行推进：完成 p9-4，下一步进入 p9-5（monitor 数据源切换为 relay API 优先 + local fallback）。

## 11. Addendum (append-only)
### 11.1 Execution Plan Delta
- [x] p9-5 实现/修复 monitor 数据源：relay API 优先，local monitor fallback

### 11.2 Execution Log Delta
- 2026-03-13 20:08 +0800 p9-5 started: 在 monitor 采集链路新增 relay 白名单 API 客户端，保留本地 SSH 采集作为 fallback。
- 2026-03-13 20:08 +0800 p9-5 completed: 已完成 relay API 优先 + local fallback 的数据源切换，并补充 Prometheus 向量解析与匹配单测。

### 11.3 Validation Evidence Delta
- TC-P9-006 | stack: rust | command: rg -n "relay_telemetry_config_from_env|collect_relay_prometheus_metrics|collect_local_prometheus_metrics|collect_prometheus_metrics\\(conn, machine\\)" src-tauri/src/commands/monitor.rs | result: pass | note: monitor 已切换为 relay API 优先并保留 local fallback
- TC-P9-006 | stack: rust | command: rg -n "OURO_OPS_RELAY_TELEMETRY_USERNAME|OURO_OPS_RELAY_TELEMETRY_PASSWORD|RELAY_TELEMETRY_ENDPOINTS|parse_relay_metric_samples" src-tauri/src/commands/monitor.rs | result: pass | note: relay API 认证配置与白名单端点映射已接入
- TC-P9-007 | stack: rust | command: cargo test -q tc_mon_ | result: pass | note: 19 项 monitor 单测通过，包含 relay 解析/匹配与 fallback 相关逻辑

### 11.4 Change Request Delta
- 2026-03-13 20:08 +0800 执行推进：完成 p9-5，下一步进入 p9-6（多 relay 主备切换、超时退避与最新时间戳选优）。

## 12. Addendum (append-only)
### 12.1 Execution Plan Delta
- [x] p9-6 实现多 relay 主备切换策略（超时、退避、最新时间戳选优）

### 12.2 Execution Log Delta
- 2026-03-13 20:16 +0800 p9-6 started: 在 relay 采集链路增加多 relay 自动切换与失败退避控制。
- 2026-03-13 20:16 +0800 p9-6 completed: 已实现多 relay 探测、失败 backoff、以及按最新时间戳选优的数据返回策略。

### 12.3 Validation Evidence Delta
- TC-P9-008 | stack: rust | command: rg -n "relay_failure_backoff_registry|relay_mark_backoff|relay_clear_backoff|collect_relay_metrics_from_single_relay|deferred_relays|latest_timestamp" src-tauri/src/commands/monitor.rs | result: pass | note: 多 relay 切换、失败退避与最新时间戳选优逻辑已落地
- TC-P9-007 | stack: rust | command: rg -n "if attempts.is_empty\\(\\) \\{|relay api unavailable after failover attempts|relay failover active" src-tauri/src/commands/monitor.rs | result: pass | note: failover 失败兜底与降级提示路径已覆盖
- TC-P9-008 | stack: rust | command: cargo test -q tc_mon_ | result: pass | note: 21 项 monitor 单测通过，含 backoff/relay 解析相关测试

### 12.4 Change Request Delta
- 2026-03-13 20:16 +0800 执行推进：完成 p9-6，下一步进入 p9-7（已部署环境增量接入 playbook）。

## 13. Addendum (append-only)
### 13.1 Execution Plan Delta
- [x] p9-7 新增现网增量接入 playbook（bootstrap，不重跑 full deploy）

### 13.2 Execution Log Delta
- 2026-03-13 20:22 +0800 p9-7 started: 为已部署环境新增观测能力增量接入 playbook，避免重跑 full deploy。
- 2026-03-13 20:22 +0800 p9-7 completed: 已新增 `observability-bootstrap.yml` 并补充静态测试，覆盖 relay 目标与网关角色引用。

### 13.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: 增量 bootstrap playbook 语法通过，可独立执行
- TC-P9-004 | stack: rust | command: cargo test -q tc_dep_013_observability_bootstrap_playbook_targets_relay_and_gateway_role | result: pass | note: 已验证 playbook 目标主机组与网关角色绑定

### 13.4 Change Request Delta
- 2026-03-13 20:22 +0800 执行推进：完成 p9-7，下一步进入 p9-8（将观测与认证步骤并入新部署流程）。

## 14. Addendum (append-only)
### 14.1 Execution Plan Delta
- [x] p9-8 将观测与认证步骤并入新部署流程（deploy 默认内建）

### 14.2 Execution Log Delta
- 2026-03-13 20:29 +0800 p9-8 started: 将 relay 观测网关配置步骤并入 `deploy.yml`，并默认启用。
- 2026-03-13 20:29 +0800 p9-8 completed: 已在 deploy 流程新增 relay 网关阶段（含共享密码生成/下发 + role 执行），并补充静态回归测试。

### 14.3 Validation Evidence Delta
- TC-P9-005 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: deploy playbook 语法通过（本地语法检查会提示 relay/bp host pattern 未匹配，属预期）
- TC-P9-005 | stack: rust | command: cargo test -q tc_dep_018_deploy_playbook_includes_observability_gateway_defaults | result: pass | note: 已验证 deploy 默认包含观测网关阶段与默认开关逻辑

### 14.4 Change Request Delta
- 2026-03-13 20:29 +0800 执行推进：完成 p9-8，下一步进入 p9-9（Dashboard 运行态联调与 source/note/collected_at 补强）。
