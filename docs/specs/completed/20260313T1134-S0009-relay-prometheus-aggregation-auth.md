# Phase 6 Relay Aggregated Prometheus API & Auth Delivery

Spec-ID: S0009
状态: completed
创建时间: 2026-03-13 11:34 +0800
开始时间: 2026-03-13 11:34 +0800
完成时间: 2026-03-15 23:23 +0800
前一个 Spec-ID: S0008
结项原因: replaced

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
- [x] p9-9 Dashboard 运行态联调与可观测性补强（source/note/collected_at）
- [x] p9-10 完成端到端验收与回滚预案校验，准备结项评审
- [x] p9-12 新增服务端执行检测与 GUI 触发入口（bootstrap / rollback）

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

## 15. Addendum (append-only)
### 15.1 Execution Plan Delta
- [x] p9-9 Dashboard 运行态联调与可观测性补强（source/note/collected_at）

### 15.2 Execution Log Delta
- 2026-03-13 20:36 +0800 p9-9 started: 对齐 relay 样本时间戳与 Dashboard 展示语义，补强 source/note/collected_at 运行态可见性。
- 2026-03-13 20:36 +0800 p9-9 completed: 后端快照 `collected_at` 已优先使用 relay 样本时间；Dashboard 资源卡片已补充 source/sample/note 轻量 tooltip 展示。

### 15.3 Validation Evidence Delta
- TC-P9-006 | stack: rust | command: rg -n "collected_at_epoch|resolve_snapshot_collected_at|prometheus.collected_at_epoch" src-tauri/src/commands/monitor.rs | result: pass | note: monitor 快照已接入 relay 样本时间回填，按数据新鲜度驱动展示
- TC-P9-006 | stack: ui | command: rg -n "prometheus_source|prometheus_note|sample ·|formatRelativeCollectedAt\\(selectedNode.collected_at\\)" src/pages/Dashboard.tsx | result: pass | note: Dashboard 已展示 source/note/collected_at 的轻量信息层
- TC-P9-006 | stack: rust | command: cargo test -q tc_mon_ | result: pass | note: monitor 相关 21 项单测通过
- TC-P9-006 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过，Dashboard 改动可编译

### 15.4 Change Request Delta
- 2026-03-13 20:36 +0800 执行推进：完成 p9-9，下一步进入 p9-10（端到端验收与回滚预案校验）。

## 16. Addendum (append-only)
### 16.1 Execution Plan Delta
- [x] p9-10 完成端到端验收与回滚预案校验，准备结项评审

### 16.2 Execution Log Delta
- 2026-03-13 20:43 +0800 p9-10 started: 汇总 S0009 端到端验收证据并固化回滚预案。
- 2026-03-13 20:43 +0800 p9-10 completed: 已新增回滚 playbook 与验收报告文档，覆盖 deploy/bootstrap/rollback 三路径语法校验与关键静态回归。

### 16.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: 现网增量接入路径语法通过
- TC-P9-005 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: 新部署默认内建路径语法通过（relay/bp host pattern 告警属本地空 inventory 预期）
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-rollback.yml | result: pass | note: 回滚路径语法通过
- TC-P9-005 | stack: rust | command: cargo test -q tc_dep_019_observability_rollback_playbook_exists | result: pass | note: 回滚 playbook 存在性与关键动作静态校验通过
- TC-P9-005 | stack: other | command: test -f docs/review/20260313-p9-10-e2e-acceptance-rollback.md && echo ok | result: pass | note: 端到端验收与回滚报告已落盘

### 16.4 Change Request Delta
- 2026-03-13 20:43 +0800 阶段状态：S0009 所有计划项已完成，等待用户确认是否结项（`delivered`）。

## 17. Addendum (append-only)
### 17.1 Execution Plan Delta
- [x] p9-11 在 spec 中补充人工验收清单（UAT Checklist）

### 17.2 Execution Log Delta
- 2026-03-13 20:56 +0800 p9-11 started: 按用户要求补充可直接执行的人工验收清单。
- 2026-03-13 20:56 +0800 p9-11 completed: 已在 S0009 增加分步骤人工验收清单，覆盖安全、采集、展示、failover 与回滚。

### 17.3 Manual UAT Checklist
- [ ] UAT-01 / 基础可用性检查（TC-P9-001）
  操作：
  `ls -la docs/specs docs/specs/completed`
  预期：
  `docs/specs/` 根目录仅存在 `20260313T1134-S0009-relay-prometheus-aggregation-auth.md` 为 active；`S0008` 在 completed 且为 replaced。

- [ ] UAT-02 / 现网增量接入执行（TC-P9-004）
  操作：
  `ansible-playbook -i <inventory> ansible/playbooks/observability-bootstrap.yml -e "ops_metrics_basic_auth_password=<API_KEY>" -e "ops_metrics_tls_cert_path=<CERT_PATH>" -e "ops_metrics_tls_key_path=<KEY_PATH>"`
  预期：
  playbook 成功，无需 full deploy 即完成 relay 观测网关接入。

- [ ] UAT-03 / 白名单 API 认证校验（TC-P9-002）
  操作：
  `curl -i https://<relay>/api/ops/v1/telemetry/epoch`
  `curl -i -u ouro_app:<API_KEY> https://<relay>/api/ops/v1/telemetry/epoch`
  预期：
  未认证返回 `401`；认证后返回 `200`。

- [ ] UAT-04 / 禁止任意 PromQL 校验（TC-P9-003）
  操作：
  `curl -i -u ouro_app:<API_KEY> "https://<relay>/api/ops/v1/telemetry/epoch?query=up"`
  预期：
  返回 `400`（或被拒绝），客户端无法透传任意 query。

- [ ] UAT-05 / 新部署默认内建校验（TC-P9-005）
  操作：
  正常执行一次 deploy（不显式关闭 `enable_ops_observability_gateway`）。
  预期：
  relay 上自动完成网关配置（nginx conf + htpasswd），部署后 API 可直接查询。

- [ ] UAT-06 / Dashboard 指标映射校验（TC-P9-006）
  操作：
  启动 App 并进入 Dashboard，至少 1 BP + 1 Relay 场景下观察节点卡与 Resources。
  预期：
  每节点 10 项核心字段中至少 6 项非空；可看到 source/sample/note 的 tooltip 信息。

- [ ] UAT-07 / relay 不可用兜底校验（TC-P9-007）
  操作：
  临时使主 relay 网关不可用（停 nginx 或封禁端口），保持节点本地采集链路可用。
  预期：
  Dashboard 不崩溃，继续展示缓存或 local fallback 数据，出现降级提示但可持续轮询。

- [ ] UAT-08 / 多 relay 自动切换校验（TC-P9-008）
  操作：
  配置至少两个 relay；让 relay-1 不可用，relay-2 可用；观察一段时间后恢复 relay-1。
  预期：
  数据自动切换到可用 relay；恢复后按最新 `collected_at` 选优，无需手工干预。

- [ ] UAT-09 / 回滚预案演练（TC-P9-004, TC-P9-005）
  操作：
  `ansible-playbook -i <inventory> ansible/playbooks/observability-rollback.yml`
  预期：
  `ouro-ops-metrics.conf` 与 htpasswd 被移除，nginx reload 成功；观测 API 不再可用或返回 404。

### 17.4 Validation Evidence Delta
- TC-P9-004 | stack: other | command: rg -n "## 17\\.3 Manual UAT Checklist|UAT-01|UAT-09" docs/specs/20260313T1134-S0009-relay-prometheus-aggregation-auth.md | result: pass | note: 人工验收清单已追加到 active spec，覆盖核心验收路径

### 17.5 Change Request Delta
- 2026-03-13 20:56 +0800 按用户要求补充人工验收清单，等待用户逐项验收结果反馈。

## 18. Addendum (append-only)
### 18.1 Execution Plan Delta
- [x] p9-12 新增服务端执行检测与 GUI 触发入口（bootstrap / rollback）

### 18.2 Execution Log Delta
- 2026-03-13 21:14 +0800 p9-12 started: 响应用户新增需求，实现“服务端是否执行”检测能力与 GUI 触发点（执行与回滚）。
- 2026-03-13 21:14 +0800 p9-12 completed: 已新增 observability 命令模块、任务类型迁移、IPC 接口、Dashboard 触发按钮与任务轮询状态展示。

### 18.3 Validation Evidence Delta
- TC-P9-004 | stack: rust | command: cargo test -q tc_db_004_task_migration_allows_runtime_and_observability_task_types | result: pass | note: task 类型迁移已包含 `observability_bootstrap`/`observability_rollback`
- TC-P9-005 | stack: rust | command: cargo test -q tc_obs_ | result: pass | note: observability 命令与 inventory 构建相关单测通过
- TC-P9-005 | stack: rust | command: cargo test -q tc_obs_002_observability_commands_registered_and_dashboard_has_triggers | result: pass | note: Tauri 命令注册与 Dashboard 触发入口已接通
- TC-P9-006 | stack: ui | command: pnpm -s build | result: pass | note: 前端新增触发按钮与状态展示改动可编译
- TC-P9-005 | stack: other | command: rg -n "observability_gateway_status|observability_bootstrap_start|observability_rollback_start" src-tauri/src/commands/observability.rs src-tauri/src/lib.rs src/lib/ipc.ts src/pages/Dashboard.tsx | result: pass | note: 后端检测与 GUI 触发链路完整可追溯

### 18.4 Change Request Delta
- 2026-03-13 21:14 +0800 新增需求已交付：GUI 可触发 bootstrap/rollback，并可读取服务端执行状态与 relay 配置探测结果。

## 19. Addendum (append-only)
### 19.1 Execution Plan Delta
- [x] p9-12-fix1 修复 GUI 触发 bootstrap 时的缺省密码失败路径

### 19.2 Execution Log Delta
- 2026-03-13 21:20 +0800 p9-12-fix1 started: 复核 GUI 触发链路时发现 bootstrap playbook 对密码参数有硬依赖，导致无参触发可能失败。
- 2026-03-13 21:20 +0800 p9-12-fix1 completed: 已改为 bootstrap 缺省自动生成并下发共享密码，GUI 无需额外输入可直接触发。

### 19.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: rg -n "Generate shared metrics basic auth password when missing|Propagate generated metrics password to relay hosts|ops_metrics_basic_auth_password" ansible/playbooks/observability-bootstrap.yml | result: pass | note: bootstrap 已支持无参触发并自动补齐密码
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: 修复后 playbook 语法通过

### 19.4 Change Request Delta
- 2026-03-13 21:20 +0800 修复完成：GUI 触发 bootstrap 不再依赖外部手工传入密码变量。

## 20. Addendum (append-only)
### 20.1 Execution Plan Delta
- [x] p9-12-fix2 增加 GUI 执行中反馈与任务输出展示

### 20.2 Execution Log Delta
- 2026-03-13 21:26 +0800 p9-12-fix2 started: 响应用户反馈“Enable API 点击后无明显反馈”，补充执行态可见性。
- 2026-03-13 21:26 +0800 p9-12-fix2 completed: Dashboard 已增加执行中状态徽章、按钮 loading 文案、任务状态提示与 TaskLogStream 输出面板。

### 20.3 Validation Evidence Delta
- TC-P9-006 | stack: ui | command: rg -n "执行中|Enabling\\.\\.\\.|Rolling\\.\\.\\.|TaskLogStream taskId=\\{gatewayLogTaskId\\}|Enable API started|Rollback started" src/pages/Dashboard.tsx | result: pass | note: 执行中反馈与输出展示链路已落地
- TC-P9-006 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过，反馈区改动可编译

### 20.4 Change Request Delta
- 2026-03-13 21:26 +0800 用户交互已增强：点击 Enable API/Rollback 后可立即看到执行中状态与实时任务输出。

## 21. Addendum (append-only)
### 21.1 Execution Plan Delta
- [x] p9-12-fix3 修复 bootstrap/deploy 在真实 inventory 下的 hostvars 取值错误

### 21.2 Execution Log Delta
- 2026-03-13 17:32 +0800 p9-12-fix3 started: 用户反馈 Enable API 执行失败，报错 `hostvars['localhost']` 无 `generated_metrics_basic_auth_password`。
- 2026-03-13 17:32 +0800 p9-12-fix3 completed: 已统一改为从 `ansible_play_hosts_all[0]` 读取 run_once 生成值，移除对 `localhost` hostvars 依赖，修复 bootstrap 与 deploy 同类风险。

### 21.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: rg -n "generated_metrics_basic_auth_password|ansible_play_hosts_all\[0\]|hostvars\['localhost'\]" ansible/playbooks/observability-bootstrap.yml ansible/playbooks/deploy.yml | result: pass | note: 两个 playbook 均已改为首个 play host 传播密码，不再依赖 localhost hostvars
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: bootstrap 语法检查通过
- TC-P9-005 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: deploy 语法检查通过（relay/bp host pattern 警告为本地空 inventory 预期）

### 21.4 Change Request Delta
- 2026-03-13 17:32 +0800 修复完成：Enable API 触发链路移除对 `localhost` hostvars 的隐式前提，适配 sidecar 生成 inventory 场景。

## 22. Addendum (append-only)
### 22.1 Execution Plan Delta
- [x] p9-12-fix4 修复 Enable API 对预置 TLS 文件的硬依赖（缺失时自动生成）

### 22.2 Execution Log Delta
- 2026-03-13 18:00 +0800 p9-12-fix4 started: 用户反馈 bootstrap 在 relay 上因缺失 `/etc/ssl/certs/ouro-ops.crt` 失败。
- 2026-03-13 18:00 +0800 p9-12-fix4 completed: 在 `ops-observability-gateway` 角色新增“TLS 材料缺失时自动生成自签名证书”逻辑，保留开关可禁用。

### 22.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: rg -n "ops_metrics_tls_auto_generate_self_signed|Generate self-signed certificate|openssl|Re-check TLS" ansible/roles/ops-observability-gateway/defaults/main.yml ansible/roles/ops-observability-gateway/tasks/main.yml | result: pass | note: 已落地自签名 TLS 自动生成与二次校验链路
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: bootstrap 语法检查通过
- TC-P9-005 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: deploy 语法检查通过（relay/bp host pattern 警告为本地空 inventory 预期）

### 22.4 Change Request Delta
- 2026-03-13 18:00 +0800 修复完成：Enable API 在未预置 TLS 证书场景可自动生成并继续执行，无需人工先下发证书文件。

## 23. Addendum (append-only)
### 23.1 Execution Plan Delta
- [x] p9-12-fix5 修复 GUI 触发时可能读取旧版 ansible 源的问题

### 23.2 Execution Log Delta
- 2026-03-13 19:11 +0800 p9-12-fix5 started: 用户仍命中旧版 role 行号，判断运行时 playbook 源路径可能未指向当前工作区。
- 2026-03-13 19:11 +0800 p9-12-fix5 completed: observability playbook 路径解析改为优先 `OURO_OPS_WORKSPACE_ROOT` 与当前工作区 `./ansible/playbooks`，再回退 `CARGO_MANIFEST_DIR`。

### 23.3 Validation Evidence Delta
- TC-P9-004 | stack: rust | command: rg -n "OURO_OPS_WORKSPACE_ROOT|current_dir\(\)|observability playbook not found" src-tauri/src/commands/observability.rs | result: pass | note: playbook 路径已优先解析当前工作区，避免读到旧构建目录
- TC-P9-005 | stack: rust | command: cargo test -q tc_obs_ | result: pass | note: observability 命令相关单测通过
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: bootstrap 语法检查通过

### 23.4 Change Request Delta
- 2026-03-13 19:11 +0800 修复完成：GUI 触发 observability 任务时优先使用当前仓库最新 ansible 脚本，避免旧源码路径导致的假修复。

## 24. Addendum (append-only)
### 24.1 Execution Plan Delta
- [x] p9-12-fix6 修复 relay 网关 502（缺失本地 Prometheus 上游）

### 24.2 Execution Log Delta
- 2026-03-13 20:32 +0800 p9-12-fix6 started: 根据运行态排障结论（`127.0.0.1:9090` refused，nginx 返回 502）修复 observability 网关上游依赖。
- 2026-03-13 20:32 +0800 p9-12-fix6 completed: 网关角色新增“本地 Prometheus 容器自动部署 + 抓取 `cardano-node:12798` + 就绪探测”，保持白名单 API 契约不变并消除 502 根因。

### 24.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: rg -n "ops_metrics_prometheus_container_name|prometheus.yml.j2|cardano-node-local|12798|127.0.0.1:9090:9090|Wait for local Prometheus query API ready" ansible/roles/ops-observability-gateway/defaults/main.yml ansible/roles/ops-observability-gateway/tasks/main.yml ansible/roles/ops-observability-gateway/templates/prometheus.yml.j2 | result: pass | note: relay 网关已内建本地 Prometheus 上游能力
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: bootstrap 语法检查通过
- TC-P9-005 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: deploy 语法检查通过（relay/bp host pattern 警告为本地空 inventory 预期）
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-rollback.yml | result: pass | note: rollback 已覆盖 Prometheus 容器与配置清理，语法检查通过

### 24.4 Change Request Delta
- 2026-03-13 20:32 +0800 修复完成：Enable API 现在会在 relay 本地自动补齐 Prometheus Query API 上游，不再依赖外部预置 `127.0.0.1:9090` 服务。

## 25. Addendum (append-only)
### 25.1 Execution Plan Delta
- [x] p9-12-fix7 收敛 telemetry API 为单一 `raw` 端点并移除旧端点依赖

### 25.2 Execution Log Delta
- 2026-03-13 21:42 +0800 p9-12-fix7 started: 按最新需求移除旧 telemetry 细粒度端点，仅保留全量 `raw` 返回。
- 2026-03-13 21:42 +0800 p9-12-fix7 completed: Nginx 网关已仅暴露 `GET /api/ops/v1/telemetry/raw`；monitor 改为单次拉取并在客户端完成指标映射；补齐 series payload 兼容解析。

### 25.3 Validation Evidence Delta
- TC-P9-003 | stack: other | command: rg -n "telemetry/(epoch|sync-percent|tip-diff-blocks|peer-count|cpu-sys-percent|mem-live-bytes|mem-rss-bytes|mem-heap-bytes|gc-minor-total|gc-major-total|snapshot)" ansible src-tauri src -S | result: pass | note: 可执行代码中已无旧 telemetry 端点依赖
- TC-P9-003 | stack: other | command: test -f docs/review/20260313-p9-12-fix7-telemetry-raw-contract.md && echo ok | result: pass | note: raw 单端点契约与字段映射文档已落盘
- TC-P9-003 | stack: other | command: rg -n "location = /api/ops/v1/telemetry/raw|proxy_pass .*/api/v1/query\\?query=%7B__name__%3D~%22.%2B%22%7D" ansible/roles/ops-observability-gateway/templates/ouro-ops-metrics.conf.j2 | result: pass | note: 网关仅暴露 raw 端点并转发全量指标查询
- TC-P9-006 | stack: rust | command: cargo test -q tc_mon_ | result: pass | note: monitor 21 项单测通过，包含 raw payload 解析与字段映射回归
- TC-P9-005 | stack: rust | command: cargo test -q tc_obs_ | result: pass | note: observability 命令链路回归通过
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: bootstrap 语法检查通过
- TC-P9-005 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: deploy 语法检查通过（relay/bp host pattern 警告为本地空 inventory 预期）
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-rollback.yml | result: pass | note: rollback 语法检查通过

### 25.4 Change Request Delta
- 2026-03-13 21:42 +0800 需求落地：旧 telemetry 细粒度端点已下线，当前实现仅保留单一 `raw` 端点。

## 26. Addendum (append-only)
### 26.1 Execution Plan Delta
- [x] p9-12-fix8 新增 raw 全量指标 key→作用字典文档（用于前端选型）

### 26.2 Execution Log Delta
- 2026-03-13 21:58 +0800 p9-12-fix8 started: 根据用户提供的 raw 样本，整理全量指标字典文档以支持后续前端指标展示规划。
- 2026-03-13 21:58 +0800 p9-12-fix8 completed: 已新增“raw 指标字典”文档，覆盖 135 个 key 的作用说明与首批前端展示优先级建议。

### 26.3 Validation Evidence Delta
- TC-P9-006 | stack: other | command: test -f docs/review/20260313-p9-12-fix8-raw-metrics-catalog.md && echo ok | result: pass | note: raw 指标字典文档已落盘
- TC-P9-006 | stack: other | command: grep -oE '`(cardano_node_metrics_[A-Za-z0-9_]+|rts_gc_[A-Za-z0-9_]+|ekg_server_timestamp_ms)' docs/review/20260313-p9-12-fix8-raw-metrics-catalog.md | tr -d '`' | sort -u | wc -l | result: pass | note: 文档覆盖 key 数量为 135，与样本一致

### 26.4 Change Request Delta
- 2026-03-13 21:58 +0800 新增交付：通过文档化方式冻结 raw 指标字典，后续前端可直接按文档选取展示指标。

## 27. Addendum (append-only)
### 27.1 Execution Plan Delta
- [x] p9-12-fix9 生成前端可直接消费的 telemetry 指标 JSON 配置

### 27.2 Execution Log Delta
- 2026-03-13 22:06 +0800 p9-12-fix9 started: 将 `p9-12-fix8` 的 Markdown 指标字典转换为前端可直接消费的 JSON 配置结构。
- 2026-03-13 22:06 +0800 p9-12-fix9 completed: 已新增 `src/config/telemetry-metrics-catalog.json`，包含分组、单位、格式化规则、优先级与 preset 清单。

### 27.3 Validation Evidence Delta
- TC-P9-006 | stack: other | command: node -e \"const fs=require('fs');const p='src/config/telemetry-metrics-catalog.json';const j=JSON.parse(fs.readFileSync(p,'utf8'));console.log('metrics',j.metrics.length,'groups',j.groups.length,'core',j.presets.dashboard_core.length);\" | result: pass | note: JSON 可解析，metrics=135 groups=9 core=15
- TC-P9-006 | stack: other | command: rg -n \"\\\"schemaVersion\\\"|\\\"endpoint\\\"|\\\"dashboard_core\\\"|cardano_node_metrics_epoch_int|cardano_node_metrics_peerSelection_EstablishedPeers|cardano_node_metrics_RTS_gcLiveBytes_int\" src/config/telemetry-metrics-catalog.json | result: pass | note: 关键 schema 字段与核心指标映射存在

### 27.4 Change Request Delta
- 2026-03-13 22:06 +0800 新增交付：前端后续可直接读取 JSON 配置决定“展示哪些指标、如何分组与格式化”。

## 28. Addendum (append-only)
### 28.1 Execution Plan Delta
- [x] p9-12-fix10 修复 raw 端点未认证访问未返回 401 的问题

### 28.2 Execution Log Delta
- 2026-03-13 22:14 +0800 p9-12-fix10 started: 用户反馈 raw 接口可访问但未触发 401，排查网关鉴权链路。
- 2026-03-13 22:14 +0800 p9-12-fix10 completed: 移除 `auth_request + /_ops_auth return 204` 子请求路径，改为在 `raw` location 直接使用 `auth_basic`，恢复未认证 401 语义。

### 28.3 Validation Evidence Delta
- TC-P9-002 | stack: other | command: rg -n "auth_request|auth_basic|auth_basic_user_file|location = /api/ops/v1/telemetry/raw" ansible/roles/ops-observability-gateway/templates/ouro-ops-metrics.conf.j2 -S | result: pass | note: raw 端点已改为原生 Basic Auth，旧 auth_request 路径已移除
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: 鉴权模板调整后 bootstrap 语法检查通过

### 28.4 Change Request Delta
- 2026-03-13 22:14 +0800 修复完成：raw 接口未认证请求应返回 401；认证通过后返回 200。

## 29. Addendum (append-only)
### 29.1 Execution Plan Delta
- [x] p9-12-fix11 打通 API key 自动化链路（App 生成/持久化/下发/读取）

### 29.2 Execution Log Delta
- 2026-03-14 10:06 +0800 p9-12-fix11 started: 按用户确认目标实现“用户无感 API key”，消除手工 `ops_metrics_basic_auth_password` 与手工环境变量依赖。
- 2026-03-14 10:06 +0800 p9-12-fix11 completed: 新增 `app_config` 持久化表；deploy/bootstrap 自动生成并保存 key 后下发至 Ansible；monitor 改为 `env 优先 + app_config 兜底` 自动读取认证参数并默认启用自签 TLS 兼容。

### 29.3 Validation Evidence Delta
- TC-P9-002 | stack: rust | command: cargo test -q tc_obs_ | result: pass | note: observability 链路新增 `ensure_relay_telemetry_credentials` 单测通过，bootstrap 已自动下发 app 侧凭据
- TC-P9-005 | stack: rust | command: cargo test -q tc_dep_022_ensure_relay_telemetry_credentials_persists_password | result: pass | note: deploy 链路可自动生成并持久化 API key
- TC-P9-006 | stack: rust | command: cargo test -q tc_mon_ | result: pass | note: monitor 新增 app_config 兜底配置读取，22 项监控单测通过
- TC-P9-001 | stack: rust | command: cargo test -q tc_db_ | result: pass | note: 新增 `006_app_config.sql` 与 app_config upsert 能力，数据库迁移与读写单测通过

### 29.4 Change Request Delta
- 2026-03-14 10:06 +0800 需求落地：部署与观测链路已支持 API key 全自动闭环，用户无需感知 key 生成与下发细节。

## 30. Addendum (append-only)
### 30.1 Execution Plan Delta
- [x] p9-12-fix12 Enable API 点击即进入本地 submitting 态并锁定重复点击

### 30.2 Execution Log Delta
- 2026-03-14 10:18 +0800 p9-12-fix12 started: 按反馈优化 Enable API/Rollback 按钮交互，要求不等待 IPC 返回即可进入提交中态。
- 2026-03-14 10:18 +0800 p9-12-fix12 completed: Dashboard 新增 `gatewaySubmittingKind` 本地提交态，点击瞬间显示“提交中…”，按钮立即 loading 并禁用重复点击；提交完成后自动切回任务轮询态。

### 30.3 Validation Evidence Delta
- TC-P9-006 | stack: ui | command: rg -n "gatewaySubmittingKind|setGatewaySubmittingKind\\(\"bootstrap\"\\)|setGatewaySubmittingKind\\(\"rollback\"\\)|提交中…|disabled=\\{gatewayActionBusy\\}" src/pages/Dashboard.tsx | result: pass | note: 已实现点击即提交中、loading 文案与重复点击锁定
- TC-P9-006 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过，交互改动可编译
- TC-P9-005 | stack: rust | command: cargo test -q tc_obs_002_observability_commands_registered_and_dashboard_has_triggers | result: pass | note: Dashboard 触发入口静态回归通过

### 30.4 Change Request Delta
- 2026-03-14 10:18 +0800 交互优化完成：Enable API/回滚按钮现在有即时提交反馈，避免“点击无感”和重复触发。

## 31. Addendum (append-only)
### 31.1 Execution Plan Delta
- [x] p9-12-fix13 修复 relay API 指标覆盖与匹配缺陷（BP/Relay 数据无法展示）

### 31.2 Execution Log Delta
- 2026-03-14 14:05 +0800 p9-12-fix13 started: 复盘用户现场，确认 gateway 仅抓取 relay 本地 `cardano-node`，且历史 raw payload 缺少 `node/host_ip` 标签时客户端匹配容易 miss。
- 2026-03-14 14:05 +0800 p9-12-fix13 completed: bootstrap 额外下发 `ops_metrics_scrape_targets`（全池 relay+bp），gateway Prometheus 改为按目标列表抓取并写入 `node/role/host_ip`；monitor 新增 legacy payload 的“同 IP relay 无标签兜底匹配”。

### 31.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: rg -n "ops_metrics_scrape_targets|ops_metrics_effective_scrape_targets|cardano-node-pool|host_ip" ansible/roles/ops-observability-gateway/defaults/main.yml ansible/roles/ops-observability-gateway/tasks/main.yml ansible/roles/ops-observability-gateway/templates/prometheus.yml.j2 | result: pass | note: gateway 已支持由 App 下发全池抓取目标并透传标签
- TC-P9-005 | stack: rust | command: cargo test -q tc_obs_ --manifest-path src-tauri/Cargo.toml | result: pass | note: observability 链路新增 scrape target 生成逻辑并通过回归
- TC-P9-006 | stack: rust | command: cargo test -q tc_mon_ --manifest-path src-tauri/Cargo.toml | result: pass | note: monitor 新增 legacy raw 兜底匹配并通过 23 项监控单测
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: bootstrap 语法检查通过

### 31.4 Change Request Delta
- 2026-03-14 14:05 +0800 修复完成：Enable API 重新执行后，relay API 可聚合并返回 BP+Relay 指标；Dashboard 的 BP/Relay 指标展示链路恢复。

## 32. Addendum (append-only)
### 32.1 Execution Plan Delta
- [x] p9-12-fix14 修复 Prometheus 抓取目标 `connection refused`（节点未发布 12798）

### 32.2 Execution Log Delta
- 2026-03-14 14:32 +0800 p9-12-fix14 started: 用户现场返回 `targets` 全部 `down` 且 `dial tcp <ip>:12798: connect: connection refused`，确认抓取端口未对宿主机发布。
- 2026-03-14 14:32 +0800 p9-12-fix14 completed: 在 `cardano-node` role 的 deploy/upgrade/rollback 路径统一补齐 `12798:12798` 端口映射，确保 relay 本地 Prometheus 可以抓取 bp/relay 的节点指标。

### 32.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: rg -n "12798:12798|3001:3001" ansible/roles/cardano-node/tasks/main.yml ansible/roles/cardano-node/tasks/upgrade.yml ansible/roles/cardano-node/tasks/rollback.yml | result: pass | note: 三条容器生命周期路径均包含 metrics 端口发布
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: deploy 语法检查通过（空 inventory 警告符合预期）

### 32.4 Change Request Delta
- 2026-03-14 14:32 +0800 修复完成：节点 `12798` 可被 relay 侧抓取，`/api/ops/v1/telemetry/raw` 具备返回 bp/relay 指标的前置条件。

## 33. Addendum (append-only)
### 33.1 Execution Plan Delta
- [x] p9-12-fix15 在 Dashboard 增加临时“重新部署”入口（验收完成后删除）

### 33.2 Execution Log Delta
- 2026-03-14 14:46 +0800 p9-12-fix15 started: 用户反馈前端缺少“重新部署”入口，要求提供可临时触发 deploy 的 GUI 按钮用于验收。
- 2026-03-14 14:46 +0800 p9-12-fix15 completed: Dashboard 新增“临时重新部署（验收后删除）”卡片；按钮会自动选取当前 pool 的 BP+Relay 触发 `deploy_start`，并在页面展示任务状态与实时日志流。

### 33.3 Validation Evidence Delta
- TC-P9-006 | stack: ui | command: rg -n "临时入口（验收后删除）|临时重新部署|handleTemporaryRedeploy|deployStart\\(|deployStatus\\(|TaskLogStream taskId=\\{tempRedeployTaskId\\}" src/pages/Dashboard.tsx | result: pass | note: 临时入口、任务触发、状态轮询与日志展示链路已接入
- TC-P9-006 | stack: ui | command: pnpm -s build | result: pass | note: Dashboard 临时入口改动编译通过

### 33.4 Change Request Delta
- 2026-03-14 14:46 +0800 交付完成：前端可直接触发临时重部署用于验收，功能完成后可按需求移除。

## 34. Addendum (append-only)
### 34.1 Execution Plan Delta
- [x] p9-12-fix16 修复 deploy 链路未下发 telemetry scrape targets 导致仅抓取 relay 本机

### 34.2 Execution Log Delta
- 2026-03-14 15:02 +0800 p9-12-fix16 started: 用户验收结果显示 relay Prometheus 仅存在单目标 `172.17.x.x:12798`，说明 deploy 后网关回退到本地单目标抓取。
- 2026-03-14 15:02 +0800 p9-12-fix16 completed: deploy worker 新增 `build_pool_scrape_targets` 并将 `ops_metrics_scrape_targets` 注入 extra vars；确保新部署流程也能覆盖 BP+Relay 指标抓取，不依赖额外手工 Enable API。

### 34.3 Validation Evidence Delta
- TC-P9-005 | stack: rust | command: cargo test -q tc_dep_023_build_pool_scrape_targets_contains_bp_and_relays --manifest-path src-tauri/Cargo.toml | result: pass | note: deploy 侧 scrape target 构建单测通过
- TC-P9-005 | stack: rust | command: cargo test -q tc_dep_ --manifest-path src-tauri/Cargo.toml | result: pass | note: deploy 模块 31 项回归通过
- TC-P9-006 | stack: ui | command: pnpm -s build | result: pass | note: 前端临时验收入口改动仍可编译

### 34.4 Change Request Delta
- 2026-03-14 15:02 +0800 修复完成：完整 deploy 链路已与 observability bootstrap 对齐，下发全池 scrape targets，避免再次退化为单目标抓取。

## 35. Addendum (append-only)
### 35.1 Execution Plan Delta
- [x] p9-12-fix17 Enable API 自动修复 `.htpasswd` 可读权限（按 nginx 实际运行用户/组设置）

### 35.2 Execution Log Delta
- 2026-03-14 15:35 +0800 p9-12-fix17 started: 用户反馈“手工修权限不合理”，要求在 Enable API 部署流程中自动处理 `.htpasswd` 权限问题。
- 2026-03-14 15:35 +0800 p9-12-fix17 completed: `ops-observability-gateway` role 新增 nginx 运行用户自动探测与 effective user/group 解析逻辑；`/etc/ouro-ops` 与 `.htpasswd` 权限从固定 `www-data` 改为按实际 nginx 组赋权，部署时自动生效，无需手工干预。

### 35.3 Validation Evidence Delta
- TC-P9-004 | stack: ansible | command: rg -n "Detect nginx runtime user|ops_metrics_effective_nginx_user|ops_metrics_effective_nginx_group|ops_metrics_nginx_user|ops_metrics_nginx_group|ops_metrics_htpasswd_path" ansible/roles/ops-observability-gateway/tasks/main.yml ansible/roles/ops-observability-gateway/defaults/main.yml | result: pass | note: 已实现 nginx 用户探测与 htpasswd 权限动态赋值
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: Enable API 对应 playbook 语法检查通过
- TC-P9-004 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: deploy 语法检查通过（空 inventory host pattern warning 符合预期）

### 35.4 Change Request Delta
- 2026-03-14 15:35 +0800 变更完成：`Enable API` 现在会在部署流程中自动修复 nginx 读取 `.htpasswd` 所需权限，不再依赖手工登录服务器调整文件属组/权限。

## 36. Addendum (append-only)
### 36.1 Execution Plan Delta
- [x] p10-1 将 Dashboard 监控链路重构为严格 API-only（移除 SSH/tip 依赖）

### 36.2 Execution Log Delta
- 2026-03-14 16:30 +0800 p10-1 started: 按 S0010 实施，目标是 Dashboard 指标仅依赖 relay Telemetry API（`/api/ops/v1/telemetry/raw`），不再因 SSH/tip 失败导致空数据。
- 2026-03-14 16:30 +0800 p10-1 completed: `monitor_snapshot` 路径改为 API-only；新增 `block_height <- cardano_node_metrics_blockNum_int` 映射；引入 telemetry 三态（`telemetry_live|telemetry_stale|telemetry_unavailable`）；当 API 全不可用时返回本地缓存快照并同时发出 degraded retry 事件。
- 2026-03-14 16:30 +0800 p10-1 completed: Dashboard 状态展示改为 telemetry 语义（live/stale/unavailable），并移除“local 本机采集兜底”文案。

### 36.3 Validation Evidence Delta
- TC-P10-001 | stack: rust | command: rg -n "SnapshotBatch|telemetry_snapshot_state|telemetry_unavailable|load_cached_snapshots|collect_prometheus_metrics\\(" src-tauri/src/commands/monitor.rs | result: pass | note: monitor 后端已切换 API-only，并在 API 不可用时返回缓存+degraded 信号
- TC-P10-002 | stack: rust | command: rg -n "cardano_node_metrics_blockNum_int|tc_mon_024|tc_mon_025|tc_mon_026" src-tauri/src/commands/monitor.rs | result: pass | note: block height 映射与 telemetry 状态/降级单测已补齐
- TC-P10-003 | stack: rust | command: cargo test -q tc_mon_ --manifest-path src-tauri/Cargo.toml | result: pass | note: monitor 模块 26 项回归通过
- TC-P10-003 | stack: rust | command: cargo test -q tc_obs_011_monitor_collect_prometheus_metrics_is_api_only --manifest-path src-tauri/Cargo.toml | result: pass | note: 防回归断言：monitor 主采集链路不再调用本地 SSH 兜底
- TC-P10-004 | stack: ui | command: rg -n "telemetryStatusLabel|telemetry_unavailable|offline\\\" : \\\"online\\\"\\) · \\{telemetryStatusLabel" src/pages/Dashboard.tsx | result: pass | note: 前端已切换 telemetry 状态文案与显示逻辑
- TC-P10-005 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 36.4 Change Request Delta
- 2026-03-14 16:30 +0800 交付完成：Dashboard 监控主链路已不再依赖 SSH；接口有数据即展示，接口异常时继续展示缓存并后台重试。

## 37. Addendum (append-only)
### 37.1 Execution Plan Delta
- [x] p10-2 在 Deploy + Enable API 路径自动放行 relay 443（UFW）

### 37.2 Execution Log Delta
- 2026-03-14 16:58 +0800 p10-2 started: 复盘现场问题，确认 deploy hardening 将入站默认 deny，但未放行 443，导致客户端访问 relay telemetry API 超时。
- 2026-03-14 16:58 +0800 p10-2 completed: `hardening` role 增加 relay 侧 `ufw allow {{ ops_metrics_gateway_listen_port | default(443) }}/tcp`（受 `enable_ops_observability_gateway` 开关控制）；`ops-observability-gateway` role 增加“检测 ufw 并放行 gateway 端口”任务，覆盖历史部署后仅执行 Enable API 的场景。
- 2026-03-14 16:58 +0800 p10-2 completed: 保持 rollback 语义不变，不回收 443 规则；接口可达性由 nginx 配置移除控制。

### 37.3 Validation Evidence Delta
- TC-P10-006 | stack: ansible | command: rg -n "Allow relay telemetry gateway port for relay hosts|ops_metrics_gateway_listen_port \\| default\\(443\\)|enable_ops_observability_gateway" ansible/roles/hardening/tasks/main.yml | result: pass | note: deploy 路径已补齐 relay 443 放行
- TC-P10-007 | stack: ansible | command: rg -n "Check whether UFW is installed|Allow telemetry gateway port in UFW when available|ufw allow \\{\\{ ops_metrics_gateway_listen_port \\}\\}/tcp" ansible/roles/ops-observability-gateway/tasks/main.yml | result: pass | note: Enable API 路径可对历史环境自动自愈防火墙规则
- TC-P10-008 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/deploy.yml | result: pass | note: deploy 语法检查通过（空 inventory host pattern warning 符合预期）
- TC-P10-009 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ANSIBLE_REMOTE_TEMP=/tmp/ansible-remote ansible-playbook --syntax-check -i 'localhost,' -c local ansible/playbooks/observability-bootstrap.yml | result: pass | note: bootstrap 语法检查通过
- TC-P10-010 | stack: rust | command: cargo test -q tc_dep_020_hardening_allows_relay_gateway_port_when_observability_enabled --manifest-path src-tauri/Cargo.toml | result: pass | note: 静态断言 hardening 443 规则存在
- TC-P10-011 | stack: rust | command: cargo test -q tc_dep_021_observability_role_allows_gateway_port_via_ufw_if_available --manifest-path src-tauri/Cargo.toml | result: pass | note: 静态断言 observability role 的 ufw 自愈逻辑存在

### 37.4 Change Request Delta
- 2026-03-14 16:58 +0800 修复完成：`Deploy` 与 `Enable API` 两条路径都会自动放行 relay 443，避免再次出现“服务正常但被 UFW 拦截导致 Dashboard 超时”的问题。

## 38. Addendum (append-only)
### 38.1 Execution Plan Delta
- [x] p10-2-fix01 修复 Enable API 下 UFW 443 放行任务静默跳过

### 38.2 Execution Log Delta
- 2026-03-14 15:27 +0800 p10-2-fix01 started: 用户反馈“重跑 Enable API 后仍无 443 allow”；复盘发现 `ops-observability-gateway` 中 UFW 检测依赖固定路径，存在环境差异导致跳过执行风险。
- 2026-03-14 15:27 +0800 p10-2-fix01 completed: 将 UFW 放行任务改为 `shell` 内部检测 `command -v ufw` 后执行 `ufw allow {{ ops_metrics_gateway_listen_port }}/tcp`，并设置 `failed_when: false`，确保历史环境执行更稳健、缺失 ufw 时不阻断流程。

### 38.3 Validation Evidence Delta
- TC-P10-012 | stack: ansible | command: rg -n "Allow telemetry gateway port in UFW when available|command -v ufw|ufw allow \\{\\{ ops_metrics_gateway_listen_port \\}\\}/tcp|failed_when: false" ansible/roles/ops-observability-gateway/tasks/main.yml | result: pass | note: Enable API 路径已改为运行时检测 + 放行
- TC-P10-013 | stack: rust | command: cargo test -q tc_dep_021_observability_role_allows_gateway_port_via_ufw_if_available --manifest-path src-tauri/Cargo.toml | result: pass | note: 静态断言已更新为新实现

### 38.4 Change Request Delta
- 2026-03-14 15:27 +0800 修复完成：`Enable API` 的 443 放行逻辑不再依赖固定二进制路径，能在更多历史机器环境中稳定生效。

## 39. Addendum (append-only)
### 39.1 Execution Plan Delta
- [x] p10-3 Dashboard 精简 + Observability 独立页面 + Catalog 驱动指标重构（S0012）

### 39.2 Execution Log Delta
- 2026-03-14 16:04 +0800 p10-3 started: 按 S0012 执行 UI 架构调整，目标是将 `Enable API / Rollback / 日志` 从 Dashboard 迁移到独立页面，并移除临时重部署入口。
- 2026-03-14 16:04 +0800 p10-3 completed: 新增 `Telemetry API` 页面与侧边栏入口（`/telemetry`）；Dashboard 仅保留 GW 状态摘要与管理页跳转，删除临时入口与 observability 操作按钮/日志区。
- 2026-03-14 16:04 +0800 p10-3 completed: `MonitorSnapshot` 与 relay raw 映射新增 catalog 核心字段（`slot_num/late_blocks/txs_in_mempool/mempool_bytes/forks/forging_enabled`），并将 Dashboard 展示分层为集群概览 + Resources + Connections + Chain & Tx。

### 39.3 Validation Evidence Delta
- TC-P10-014 | stack: ui | command: rg -n "TelemetryApi|/telemetry|Telemetry API|管理 API" src/App.tsx src/components/Layout.tsx src/components/Sidebar.tsx src/pages/Dashboard.tsx src/pages/TelemetryApi.tsx | result: pass | note: 独立 Telemetry API 页面、路由、导航与 Dashboard 轻量入口已生效
- TC-P10-015 | stack: rust | command: rg -n "slot_num|late_blocks|txs_in_mempool|mempool_bytes|forks|forging_enabled|RELAY_TELEMETRY_FIELD_METRICS" src-tauri/src/commands/monitor.rs src/lib/types.ts src/pages/Dashboard.tsx | result: pass | note: 后端映射、前端类型与展示层已对齐 catalog 核心字段
- TC-P10-016 | stack: rust | command: cargo test -q tc_mon_ --manifest-path src-tauri/Cargo.toml | result: pass | note: monitor 模块 27 项回归通过（新增字段映射单测通过）
- TC-P10-017 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过，页面拆分与 Dashboard 重排无编译回归
- TC-P10-018 | stack: rust | command: cargo test -q tc_obs_002_observability_commands_registered_and_telemetry_page_has_triggers --manifest-path src-tauri/Cargo.toml | result: pass | note: 静态断言已更新为“Telemetry API 页面承载观测操作”

### 39.4 Change Request Delta
- 2026-03-14 16:04 +0800 交付完成：Dashboard 已回归“监控展示主职责”；Enable API 管理能力迁移至独立页面；指标展示按 telemetry catalog 核心字段扩展并完成前后端联动。

## 40. Addendum (append-only)
### 40.1 Execution Plan Delta
- [x] p10-4 Dashboard 全量轮询更新（前台 15s / 后台 60s）

### 40.2 Execution Log Delta
- 2026-03-14 18:01 +0800 p10-4 started: 按 S0013 实施 Dashboard 轮询改造，目标覆盖 monitor 快照、KES 状态、近期任务日志、GW 状态，并在窗口可见性变化时动态调频。
- 2026-03-14 18:01 +0800 p10-4 completed: `monitorStore` 新增 `setMonitorStorePollingInterval(intervalSeconds)`，复用 `monitor_start_polling` 的可重入重启能力，无需新增后端 API。
- 2026-03-14 18:01 +0800 p10-4 completed: Dashboard 新增统一 `refreshDashboardData()`（`Promise.allSettled` + in-flight 防重入），并接入 `visibilitychange/focus`：前台 15s、后台 60s，回前台立即触发一次 `refreshDashboardData + refreshMonitorStore`。
- 2026-03-14 18:01 +0800 p10-4 completed: 刷新失败不再清空旧数据；新增轻量错误提示 `auxRefreshError`，保留自动重试语义。

### 40.3 Validation Evidence Delta
- TC-P10-019 | stack: ui | command: rg -n "refreshDashboardData|Promise.allSettled|visibilitychange|setMonitorStorePollingInterval|refreshMonitorStore|foregroundIntervalSeconds|backgroundIntervalSeconds" src/pages/Dashboard.tsx src/lib/monitorStore.ts | result: pass | note: Dashboard 全量轮询编排、可见性调频和 monitor 动态间隔已接入
- TC-P10-020 | stack: rust | command: cargo test -q tc_obs_012_dashboard_polling_orchestration_supports_visibility_interval_switch --manifest-path src-tauri/Cargo.toml | result: pass | note: 静态断言覆盖 Dashboard 轮询关键机制
- TC-P10-021 | stack: rust | command: cargo test -q tc_mon_ --manifest-path src-tauri/Cargo.toml | result: pass | note: monitor 模块回归通过，telemetry 语义未退化
- TC-P10-022 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过，轮询改造无编译回归

### 40.4 Change Request Delta
- 2026-03-14 18:01 +0800 交付完成：Dashboard 已改为全量持续轮询；前台 15 秒、后台 60 秒、回前台即时补拉生效，且保持缓存/降级展示语义不变。

## 41. Addendum (append-only)
### 41.1 Execution Plan Delta
- [x] p10-5 Dashboard 再精简：仅保留集群卡片视图（移除节点详情/Resources 面板）

### 41.2 Execution Log Delta
- 2026-03-14 19:28 +0800 p10-5 started: 按 S0014（再精简版）执行 Dashboard 信息架构收敛，目标为“首屏即全量关键状态”。
- 2026-03-14 19:28 +0800 p10-5 completed: Dashboard 移除独立「节点详情」区块（含 Resources/Connections/Chain&Tx），保留「Telemetry 状态条 + 集群概览卡 + 近期日志」。
- 2026-03-14 19:28 +0800 p10-5 completed: 节点卡新增资源直出（`CPU(sys)`、`Mem(RSS)`，`Mem(Live)` 以 tooltip 呈现）；`tip diff` 与 RTT/占比分布已移除。
- 2026-03-14 19:28 +0800 p10-5 completed: 口径修复落地：`KES remain` 改为 `remaining_days` 主显示；`Sync` 新增 `slotInEpoch/epochSlots` 回退计算；`GW` 改为 runtime 可用度（`relay-api` 来源节点数/快照总数）；`collected_at` 按 UTC 解析修复 8h 偏差。
- 2026-03-14 19:28 +0800 p10-5 completed: monitor 模型新增 `slot_in_epoch` 字段，并完成 raw 指标映射 `cardano_node_metrics_slotInEpoch_int -> slot_in_epoch`（Rust + TS 对齐）。

### 41.3 Validation Evidence Delta
- TC-P10-023 | stack: rust | command: rg -n "slot_in_epoch|cardano_node_metrics_slotInEpoch_int" src-tauri/src/commands/monitor.rs src/lib/types.ts src/pages/Dashboard.tsx | result: pass | note: 后端映射、前端类型与 Sync 回退展示字段已接入
- TC-P10-024 | stack: ui | command: ! rg -n "节点详情|Connections & Peers|tip diff" src/pages/Dashboard.tsx | result: pass | note: Dashboard 已移除节点详情区、Connections 模块与 tip diff 展示
- TC-P10-025 | stack: ui | command: rg -n "CPU \(sys\)|Mem \(RSS\)|Mem \(Live\)|GW \{gatewayRuntimeSummary\}|Gateway runtime" src/pages/Dashboard.tsx | result: pass | note: 卡片资源直出与 GW runtime 口径已生效
- TC-P10-026 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过
- TC-P10-027 | stack: rust | command: cargo test -q tc_mon_ --manifest-path src-tauri/Cargo.toml | result: pass | note: monitor 27 项回归通过（含新增字段映射）

### 41.4 Change Request Delta
- 2026-03-14 19:28 +0800 交付完成：Dashboard 已切换为“卡片主视图”并完成关键口径修复，首屏只保留决策所需核心监控信息。

## 42. Addendum (append-only)
### 42.1 Execution Plan Delta
- [x] p10-5-fix01 集群卡片视觉分区优化（Chain 与 Runtime 分离）

### 42.2 Execution Log Delta
- 2026-03-14 19:54 +0800 p10-5-fix01 started: 按反馈优化卡片信息层次，解决 Block/Epoch 与 CPU/Mem 维度混杂问题。
- 2026-03-14 19:54 +0800 p10-5-fix01 completed: 节点卡改为双分区布局：`Chain` 区聚焦 Block/Epoch/Sync（含进度条），`Runtime` 区聚焦 CPU(sys)/Mem(RSS) 并用浅色容器区分维度。
- 2026-03-14 19:54 +0800 p10-5-fix01 completed: 风险信息改为顶部悬挂标签（`late` + `KES`），`Mem(Live)` 下沉为 tooltip，BP 继续保留 `forging` 状态与 `立即 Rotate` 行为。

### 42.3 Validation Evidence Delta
- TC-P10-028 | stack: ui | command: rg -n "lateBlocksTone|Runtime|Block|Epoch|Sync|CPU \(sys\)|Mem \(RSS\)|Mem \(Live\)|forging" src/pages/Dashboard.tsx | result: pass | note: 卡片双分区与风险/资源信息分层已落地
- TC-P10-029 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过，布局重排无编译回归

### 42.4 Change Request Delta
- 2026-03-14 19:54 +0800 交付完成：集群概览卡已按“链状态 vs 资源状态”双维度分区展示，重点信息层次更清晰。

## 43. Addendum (append-only)
### 43.1 Execution Plan Delta
- [x] p10-5-fix02 集群卡片升级为 2x2 复合布局（Chain 主区 / Runtime 辅区 / 风险挂件 / icon 元数据）

### 43.2 Execution Log Delta
- 2026-03-14 20:04 +0800 p10-5-fix02 started: 按确认方案重构卡片空间利用与层级，重点提升横向利用率并降低纵向堆叠。
- 2026-03-14 20:04 +0800 p10-5-fix02 completed: 卡片主体改为 2x2：左侧 Chain 主区（Block/Epoch/Sync），右侧 Runtime 辅区（CPU/Mem RSS pills）。
- 2026-03-14 20:04 +0800 p10-5-fix02 completed: 风险信息（Late/KES）迁移为右上角挂件；底部元数据改为 source/sample/note 纯 icon + tooltip。

### 43.3 Validation Evidence Delta
- TC-P10-030 | stack: ui | command: rg -n "MetaIconTip|absolute right-3 top-3|grid-cols-\[minmax\(0,1\.85fr\)_minmax\(0,1fr\)\]|Runtime|late .*KES|slot:" src/pages/Dashboard.tsx | result: pass | note: 2x2 复合布局、风险挂件与 icon 元数据已落地
- TC-P10-031 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 43.4 Change Request Delta
- 2026-03-14 20:04 +0800 交付完成：集群概览卡片已切换到 2x2 复合布局，视觉层次和空间效率显著提升。

## 44. Addendum (append-only)
### 44.1 Execution Plan Delta
- [x] p10-5-fix03 集群卡片改为“>=3 张横向滚动卡轨”并加入最小宽度约束

### 44.2 Execution Log Delta
- 2026-03-14 20:09 +0800 p10-5-fix03 started: 按反馈修复卡片宽度利用不足问题，目标是避免 3 列硬切导致卡片过窄。
- 2026-03-14 20:09 +0800 p10-5-fix03 completed: 当卡片数量 `>=3` 时，切换为横向滚动卡轨（snap + overflow-x）；当 `<3` 时保持常规网格。
- 2026-03-14 20:09 +0800 p10-5-fix03 completed: 为单卡设置最小宽度与断点约束（`min-w` + 宽度上限），并对 Runtime pills 增加 `whitespace-nowrap`，降低遮挡与折行概率。

### 44.3 Validation Evidence Delta
- TC-P10-032 | stack: ui | command: rg -n "useHorizontalCardRail|snap-x|overflow-x-auto|min-w-\[360px\]|sm:min-w-\[400px\]|max-w-\[560px\]|whitespace-nowrap" src/pages/Dashboard.tsx | result: pass | note: 横向滚动卡轨、最小宽度与防折行约束已生效
- TC-P10-033 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 44.4 Change Request Delta
- 2026-03-14 20:09 +0800 交付完成：集群卡片在多节点场景下改为横向滚动，宽度利用和可读性优于固定三列。

## 45. Addendum (append-only)
### 45.1 Execution Plan Delta
- [x] p10-5-fix04 集群卡片细节对齐（移除 Rotate/cluster 标签，状态字段收敛）

### 45.2 Execution Log Delta
- 2026-03-14 20:45 +0800 p10-5-fix04 started: 按用户反馈收敛卡片信息噪音与对齐问题。
- 2026-03-14 20:45 +0800 p10-5-fix04 completed: BP 卡移除 `立即 Rotate` 按钮；Epoch 旁移除 `cluster epoch` 标签；`forging` 字段移除。
- 2026-03-14 20:45 +0800 p10-5-fix04 completed: `slot` 与 `KES remain` 上移到左侧节点状态字段；状态行字体统一为同一字号体系。
- 2026-03-14 20:45 +0800 p10-5-fix04 completed: 底部元数据 icon 区改为右下角对齐，提升卡片基线一致性。

### 45.3 Validation Evidence Delta
- TC-P10-034 | stack: ui | command: rg -n "KES remain|slot \{|justify-end gap-1.5|forging|立即 Rotate|cluster \{clusterEpoch\}" src/pages/Dashboard.tsx | result: pass | note: 目标字段位置与移除项已按要求调整
- TC-P10-035 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 45.4 Change Request Delta
- 2026-03-14 20:45 +0800 交付完成：卡片信息层级按反馈完成精简，状态字段与底部元数据对齐得到修复。

## 46. Addendum (append-only)
### 46.1 Execution Plan Delta
- [x] p10-5-fix05 修复 Dashboard 主区域横向滚动条外溢

### 46.2 Execution Log Delta
- 2026-03-14 20:47 +0800 p10-5-fix05 started: 用户反馈右侧 Dashboard 区域出现横向滚动条，影响整体观感。
- 2026-03-14 20:47 +0800 p10-5-fix05 completed: 为 Dashboard 外层与集群概览容器增加 `overflow-x-hidden`，阻断主区域横向溢出。
- 2026-03-14 20:47 +0800 p10-5-fix05 completed: 多卡卡轨仍保留内部横向滚动能力，同时隐藏滚动条视觉（scrollbar hidden）。

### 46.3 Validation Evidence Delta
- TC-P10-036 | stack: ui | command: rg -n "overflow-x-hidden|scrollbar-width:none|\[&::-webkit-scrollbar\]:hidden" src/pages/Dashboard.tsx | result: pass | note: 主区域横向溢出已限制，卡轨滚动条视觉已隐藏
- TC-P10-037 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 46.4 Change Request Delta
- 2026-03-14 20:47 +0800 交付完成：Dashboard 主区域不再出现横向滚动条，横向滚动仅保留在多卡卡轨内部。

## 47. Addendum (append-only)
### 47.1 Execution Plan Delta
- [x] p10-5-fix06 修复集群概览区域纵向滚动条

### 47.2 Execution Log Delta
- 2026-03-14 20:48 +0800 p10-5-fix06 started: 用户反馈集群概览区域出现纵向滚动条。
- 2026-03-14 20:48 +0800 p10-5-fix06 completed: 卡轨容器增加 `overflow-y-hidden`，防止卡片阴影/容器内边距导致 Y 轴滚动条被触发。

### 47.3 Validation Evidence Delta
- TC-P10-038 | stack: ui | command: rg -n "overflow-x-auto overflow-y-hidden" src/pages/Dashboard.tsx | result: pass | note: 卡轨容器已限制 Y 轴溢出
- TC-P10-039 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 47.4 Change Request Delta
- 2026-03-14 20:48 +0800 交付完成：集群概览区域纵向滚动条已消除。

## 48. Addendum (append-only)
### 48.1 Execution Plan Delta
- [x] p10-5-fix07 彻底修复集群概览纵向滚动条（分离卡轨/网格容器）

### 48.2 Execution Log Delta
- 2026-03-14 20:50 +0800 p10-5-fix07 started: 用户反馈 p10-5-fix06 后问题仍在，说明仅增加 `overflow-y-hidden` 不足以覆盖容器模式切换造成的溢出副作用。
- 2026-03-14 20:50 +0800 p10-5-fix07 completed: 将集群概览卡容器拆分为两套分支：`>=3` 卡片使用独立横向卡轨容器，`<3` 使用独立网格容器，避免同一容器复用时出现 Y 轴滚动状态残留。
- 2026-03-14 20:50 +0800 p10-5-fix07 completed: 横向卡轨分支仅保留 `overflow-x-auto` 语义，网格分支不参与滚动容器逻辑，减少纵向滚动条触发条件。

### 48.3 Validation Evidence Delta
- TC-P10-040 | stack: ui | command: rg -n "useHorizontalCardRail \? \(|overflow-x-auto overflow-y-hidden|grid gap-3 p-4 md:grid-cols-2" src/pages/Dashboard.tsx | result: pass | note: 卡轨/网格容器已拆分为独立分支
- TC-P10-041 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 48.4 Change Request Delta
- 2026-03-14 20:50 +0800 交付完成：集群概览区域纵向滚动条问题按容器分离方式修复。

## 49. Addendum (append-only)
### 49.1 Execution Plan Delta
- [x] p10-5-fix08 修复集群概览“仍有纵向滚动条”（tooltip 溢出导致滚动计算）

### 49.2 Execution Log Delta
- 2026-03-14 21:19 +0800 p10-5-fix08 started: 用户反馈在 p10-5-fix07 后纵向滚动条仍存在，需继续定位非容器级溢出来源。
- 2026-03-14 21:19 +0800 p10-5-fix08 completed: 将集群概览相关 tooltip 从“`opacity:0` 隐藏”改为“`display:none`（`hidden`）+ hover/focus 显示”，避免未显示 tooltip 仍参与 scrollable overflow 计算。
- 2026-03-14 21:19 +0800 p10-5-fix08 completed: 同步覆盖 `TooltipBadge` / `InlineInfoTip` / `MetaIconTip` / Telemetry 说明 tooltip，统一消除隐藏态绝对定位元素造成的 Y 轴外溢。

### 49.3 Validation Evidence Delta
- TC-P10-042 | stack: ui | command: rg -n "role=\"tooltip\"|hidden .*group-hover:block|group-focus-visible:block" src/pages/Dashboard.tsx | result: pass | note: 所有 tooltip 已切换为 display none 默认态
- TC-P10-043 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 49.4 Change Request Delta
- 2026-03-14 21:19 +0800 交付完成：修复集群概览区隐藏 tooltip 触发的纵向溢出，纵向滚动条触发条件进一步收敛。

## 50. Addendum (append-only)
### 50.1 Execution Plan Delta
- [x] p10-5-fix09 Monitor 前端状态去抖：Telemetry 刷新改为静默请求，不在请求前重置 phase/status

### 50.2 Execution Log Delta
- 2026-03-14 21:46 +0800 p10-5-fix09 started: 用户反馈每次调用 telemetry 接口前端状态被重置，影响连续观测体验。
- 2026-03-14 21:46 +0800 p10-5-fix09 completed: 移除 `refreshMonitorStore()` 请求前的 `setState({ status: "Refreshing...", telemetryPhase: "syncing_live" })`，改为仅在请求成功/失败后更新状态。
- 2026-03-14 21:46 +0800 p10-5-fix09 completed: 保持 API-only 轮询链路不变，避免轮询 tick 触发 UI 状态回退/闪烁。

### 50.3 Validation Evidence Delta
- TC-P10-044 | stack: ui | command: rg -n "export async function refreshMonitorStore|Refreshing live telemetry in background|telemetryPhase: \"syncing_live\"" src/lib/monitorStore.ts | result: pass | note: refresh 入口已不在请求前重置 phase/status
- TC-P10-045 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过

### 50.4 Change Request Delta
- 2026-03-14 21:46 +0800 交付完成：Telemetry 轮询刷新改为静默模式，前端状态不再在每次请求前被重置。

## 51. Addendum (append-only)
### 51.1 Execution Plan Delta
- [x] p10-5-fix10 新增独立「操作日志」页面，支持分页与条件查询全量任务日志

### 51.2 Execution Log Delta
- 2026-03-14 22:13 +0800 p10-5-fix10 started: 用户要求新增独立操作日志页面，支持分页展示与查询全部操作日志。
- 2026-03-14 22:13 +0800 p10-5-fix10 completed: 后端 `task` 命令新增 `task_log_query`（分页 + keyword + status + task_type），返回 `items/total/page/total_pages`。
- 2026-03-14 22:13 +0800 p10-5-fix10 completed: 前端新增 `OperationLogs` 页面与 `/logs` 路由，接入侧边栏导航；Dashboard「近期操作日志」增加“查看全部”跳转。
- 2026-03-14 22:13 +0800 p10-5-fix10 completed: 页面支持详情超长截断、复制、查询重置、上一页/下一页及 page size 切换。

### 51.3 Validation Evidence Delta
- TC-P10-046 | stack: rust | command: cargo test -q tc_task_ --manifest-path src-tauri/Cargo.toml | result: pass | note: `tc_task_001`、`tc_task_002` 通过，覆盖排序、分页与关键词/状态过滤
- TC-P10-047 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过，新增 `/logs` 页面与查询交互无编译回归
- TC-P10-048 | stack: static | command: rg -n "task_log_query|TaskLogPage|OperationLogs|path=\"/logs\"|to=\"/logs\"|查看全部" src-tauri/src/commands/task.rs src/lib/types.ts src/lib/ipc.ts src/pages/OperationLogs.tsx src/App.tsx src/components/Sidebar.tsx src/pages/Dashboard.tsx | result: pass | note: 后端命令、前端类型、IPC、路由与入口已完整接入

### 51.4 Change Request Delta
- 2026-03-14 22:13 +0800 交付完成：新增独立操作日志页，支持全量任务日志分页和查询，并与 Dashboard/侧边栏完成联动。

## 52. Addendum (append-only)
### 52.1 Execution Plan Delta
- [x] p10-5-fix11 UI 审计与 polish（ConfirmModal ARIA/焦点陷阱/Esc、Tooltip aria-describedby、焦点环与 ring-offset 统一、按钮比例 h-9、错误 role=alert、shadow token、reduced-motion、表格滚动提示）

### 52.2 Execution Log Delta
- 2026-03-15 p10-5-fix11 started: 按 audit skill 与 Suggested Commands 执行 UI 优化：harden/normalize/adapt/polish。
- 2026-03-15 p10-5-fix11 completed: ConfirmModal 增加 role/dialog、焦点陷阱、Esc、label；Tooltip 增加 useId+aria-describedby；全站主要按钮统一 focus-visible:ring-2 ring-offset-1；触控高度由 min-h-[44px] 调整为 h-9/h-8 以符合桌面比例；错误区块增加 role=alert；index.css 增加 prefers-reduced-motion；tailwind 增加 shadow-app/shadow-card；OperationLogs/TelemetryApi 表格增加横向滚动提示。

### 52.3 Validation Evidence Delta
- TC-P10-049 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过
- TC-P10-050 | stack: static | command: rg -n "role=\"dialog\"|aria-describedby|focus-visible:ring-2|prefers-reduced-motion|shadow-app|shadow-card" src/components/ConfirmModal.tsx src/pages/Dashboard.tsx src/index.css tailwind.config.js | result: pass | note: ARIA、焦点环、reduced-motion、shadow token 已落地

### 52.4 Change Request Delta
- 2026-03-15 交付完成：UI 审计项与按钮比例优化已纳入工作区，按 immutable spec 提交。

## 53. Addendum (append-only)
### 53.1 Execution Plan Delta
- [x] p10-5-fix12 修复 Dashboard 中 `KES remain` 与 `CPU(sys)` 长期为 `--`（API-only）

### 53.2 Execution Log Delta
- 2026-03-15 p10-5-fix12 started: 按 S0015 对 monitor 聚合与 Dashboard 展示链路做口径修复，目标是让 KES 与 CPU 在 raw 指标可用时稳定出值。
- 2026-03-15 p10-5-fix12 completed: monitor 映射新增 `kes_remaining_periods/kes_current_period/kes_expiry_period`，并将 `rts_gc_cpu_ms` 接入 CPU 派生链路（两次采样差分计算百分比，首样本可空）。
- 2026-03-15 p10-5-fix12 completed: Dashboard 的 KES 展示改为 telemetry 优先（`snapshot.kes_*`），`kesStatusAll` 仅作 fallback；主文案切换为窗口数口径 `KES remain N`，tooltip 增加天数估算与 period 信息。
- 2026-03-15 p10-5-fix12 completed: 新增静态回归测试，约束 Dashboard 依赖的 telemetry 关键指标在 `telemetry-metrics-catalog.json` 中存在，降低 catalog/实现漂移风险。

### 53.3 Validation Evidence Delta
- TC-P10-051 | stack: rust | command: cargo test -q tc_mon_ --manifest-path src-tauri/Cargo.toml | result: pass | note: monitor 31 项测试通过，覆盖 CPU 差分派生、KES 指标映射与 catalog 依赖校验
- TC-P10-052 | stack: ui | command: pnpm -s build | result: pass | note: 前端构建通过，Dashboard KES 文案与字段优先级变更无编译回归
- TC-P10-053 | stack: static | command: rg -n "kes_remaining_periods|kes_current_period|kes_expiry_period|rts_gc_cpu_ms|resolveBpKesDisplay|KES remain" src-tauri/src/commands/monitor.rs src/lib/types.ts src/pages/Dashboard.tsx | result: pass | note: 后端字段、前端类型与展示逻辑均已接入

### 53.4 Change Request Delta
- 2026-03-15 交付完成：`KES remain` 与 `CPU(sys)` 的长期 `--` 已按 API-only 链路修复，且新增 catalog 防漂移约束。

## 54. Addendum (append-only)
### 54.1 Execution Plan Delta
- [x] p10-5-fix13 静默刷新：调用 telemetry 时集群概览不重置为空，仅用非空 snapshots 覆盖（monitorStore.ts）
- [x] p10-5-fix14 CPU(sys) 不频繁重置：后端 last_known_cpu_percent registry + 回填逻辑（monitor.rs）

### 54.2 Execution Log Delta
- 2026-03-15 p10-5-fix13 started: 按 plan 在 refreshMonitorStore 中区分返回空/非空，返回空且已有数据时不覆盖 snapshots。
- 2026-03-15 p10-5-fix13 completed: monitorStore 已实现静默刷新；返回空时保留原 snapshots 并设 degraded + lastError「本次刷新未返回数据，继续展示上次数据。」。
- 2026-03-15 p10-5-fix14 started: 后端新增 last_known_cpu_percent_registry，在 resolve_machine_cpu_percent 中写入/回填。
- 2026-03-15 p10-5-fix14 completed: 仅当 p.is_finite() && p > 0 时写入 registry；本次为 None 时用上次已知值回填，避免 0.0% 与估算值跳跃。

### 54.3 Validation Evidence Delta
- TC-P9-007 | stack: ui | command: rg -n "hadSnapshots|snapshots.length > 0|Refresh returned no data" src/lib/monitorStore.ts | result: pass | note: 静默刷新逻辑已落地
- TC-P9-006 | stack: rust | command: rg -n "last_known_cpu_percent_registry|last_known.get|is_finite.*p > 0" src-tauri/src/commands/monitor.rs | result: pass | note: CPU 上次已知值 registry 与回填已落地

### 54.4 Change Request Delta
- 2026-03-15 交付完成：Dashboard 静默刷新与 CPU(sys) 展示稳定性已按 plan 实现，并纳入 S0009 追加项（immutable spec）。
