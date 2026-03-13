# p8-5 运行态失败根因报告（Prometheus 指标未在 Dashboard 展示）

日期：2026-03-13
范围：`S0008/p8-5` 在运行态验收失败（前端不能稳定展示 Prometheus 数据）

## 1. 现象
- Dashboard `Resources` 区域多个指标长期为 `--`（`CPU(sys)`、`Mem`、`GC`）。
- telemetry 轮询可运行，但 Prometheus 字段经常为空。
- `p8-5` 在 spec 中通过的是“代码存在性验收”，非运行态验证。

## 2. 直接证据
- `p8-5` 验收仅使用 `rg` 静态检查：
  - `docs/specs/completed/20260312T1446-S0008-phase5-mac-app-delivery.md:221-223`
- Prometheus 采集端点候选与映射逻辑：
  - `src-tauri/src/commands/monitor.rs:314-362`
  - `src-tauri/src/commands/monitor.rs:223-311`
- 部署时 `cardano-node` 只映射了 `3001`，未映射 `12798/12788`：
  - `ansible/roles/cardano-node/tasks/main.yml:363-365`
- 节点配置声明了 `hasPrometheus 12798 / hasEKG 12788`：
  - `ansible/roles/cardano-node/templates/config-mainnet-10.5.4-1.json.j2:92-96`

## 3. 根因分析

### 根因 A（P0）：验收策略不正确，导致“静态通过、运行态失败”
- 问题：`p8-5` 验收标准没有校验真实数据路径（端点可达、字段非空比例、实际渲染）。
- 影响：实现中端点/映射偏差未被提前发现。

### 根因 B（P0）：采集端点策略与实际部署形态存在偏差
- 当前候选顺序：`nview:9090 -> cardano-node:12798 -> host:12798 -> host:12788`。
- 现实：部署中明确稳定存在的是 `cardano-node` 容器；`nview` 并非确定存在且端口未纳入部署契约。
- 结果：优先尝试 `nview` 带来高失败率和错误噪声，后续端点也可能因网络/映射不可达失败。

### 根因 C（P0）：映射键名与真实指标口径可能不一致
- `map_prometheus_metrics` 使用了大量候选键名，但缺少“运行态探针样本驱动”校准。
- 一旦抓到的数据键名不在候选列表内，`mapped.has_any_value()` 为假，最终返回空字段。

### 根因 D（P1）：fallback 分支会吞掉 Prometheus 可见性
- 在 tip 查询失败且未命中 restore 恢复分支时，直接返回大量 `None`（且 `prometheus_note` 也可能缺失具体上下文）。
- 用户在前端看到的只是 `--`，无法判断是端点不可达、映射失败还是阶段性异常。

### 根因 E（P1）：前端缺少 Prometheus 采集来源/失败信息的显式展示
- Dashboard 资源卡渲染了数值，但未在节点详情中显式展示 `prometheus_source/prometheus_note`。
- 运行态排障成本高，影响验收效率。

## 4. 修复方案（按优先级）

### P0（必须先做）
1. 修正验收标准为运行态：
   - 至少 1 BP + 1 Relay 场景，每节点 10 个核心字段中 >=6 项非空。
   - 验证 `collected_at` 与数据更新时间链路。
2. 调整采集优先级：
   - `cardano-node` 主路径优先，`nview` 作为可选增强路径，不作为首选。
3. 建立真实样本驱动映射：
   - 对每个网络环境抓一份 `/metrics` 样本，生成映射白名单表并纳入测试。

### P1（高优）
4. 优化 fallback：
   - tip 失败时仍尽力执行 Prometheus 采集并附带 `prometheus_note` 细分原因。
5. 前端可观测性：
   - 在节点详情增加 `source/note` 轻量展示（tooltip 即可）。

### P2（后续演进）
6. 统一 relay 聚合 API（S0009 主目标）：
   - App 不直接做多端点 SSH 探测，改为 relay 白名单接口聚合返回。

## 5. 对 S0009 的输入
- 本报告对应 `S0009/p9-2`，后续 `p9-3 ~ p9-5` 按以下顺序推进：
  1) API 契约与字段映射冻结
  2) relay 白名单接口与认证上线
  3) monitor 数据源切换为 relay 优先 + local fallback

