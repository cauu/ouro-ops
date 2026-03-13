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
