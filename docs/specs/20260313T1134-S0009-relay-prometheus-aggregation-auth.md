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
- [ ] p9-3 设计 relay 白名单 API 契约与字段映射表（含空值/时间戳兜底）
- [ ] p9-4 实现 relay 网关 Basic Auth + HTTPS + 白名单 API 路由模板
- [ ] p9-5 实现/修复 monitor 数据源：relay API 优先，local monitor fallback
- [ ] p9-6 实现多 relay 主备切换策略（超时、退避、最新时间戳选优）
- [ ] p9-7 新增现网增量接入 playbook（bootstrap，不重跑 full deploy）
- [ ] p9-8 将观测与认证步骤并入新部署流程（deploy 默认内建）
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
