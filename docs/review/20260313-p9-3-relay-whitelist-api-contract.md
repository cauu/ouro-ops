# S0009 p9-3 Relay 白名单 API 契约与指标映射（V1）

日期：2026-03-13
状态：frozen for implementation (`p9-4`/`p9-5`)

## 1. 设计目标
- 客户端不可发送任意 PromQL。
- 仅通过 relay 公网白名单接口查询指标。
- 支持多 relay 主备切换与时间戳选优。
- 字段与现有 `MonitorSnapshot` 兼容，减少前端改造成本。

## 2. 接口契约（V1 轻量模式）

> 说明：V1 采用“多固定端点 + 客户端聚合”，以降低实现成本（无新服务进程）。

### 2.1 认证与通用行为
- 协议：`HTTPS`
- 认证：`Basic Auth`
- 成功：`200`
- 未认证：`401`
- 限流：`429`
- 服务不可用：`503`
- 禁止任意参数：所有端点不接受查询参数；若存在参数返回 `400`。

### 2.2 白名单端点
- `GET /api/ops/v1/telemetry/epoch`
- `GET /api/ops/v1/telemetry/sync-percent`
- `GET /api/ops/v1/telemetry/tip-diff-blocks`
- `GET /api/ops/v1/telemetry/peer-count`
- `GET /api/ops/v1/telemetry/cpu-sys-percent`
- `GET /api/ops/v1/telemetry/mem-live-bytes`
- `GET /api/ops/v1/telemetry/mem-rss-bytes`
- `GET /api/ops/v1/telemetry/mem-heap-bytes`
- `GET /api/ops/v1/telemetry/gc-minor-total`
- `GET /api/ops/v1/telemetry/gc-major-total`

### 2.3 响应结构（统一）
```json
{
  "metric": "cpu_sys_percent",
  "collected_at": "2026-03-13T04:00:00Z",
  "source_relay": "relay-a",
  "series": [
    {
      "node": "bp-1",
      "role": "bp",
      "instance": "10.0.10.12:12798",
      "timestamp": 1773374400,
      "value": 80.79
    }
  ],
  "note": null
}
```

字段说明：
- `metric`：固定枚举，端点对应的指标键。
- `collected_at`：relay 生成的响应时间（ISO8601，UTC）。
- `source_relay`：当前响应 relay 标识（域名或节点名）。
- `series[].timestamp`：Prometheus 样本秒级时间戳。
- `series[].value`：数值；无值时该节点不出现于 `series`。
- `note`：可选，包含降级/异常说明。

## 3. PromQL 白名单映射

| API metric | PromQL (固定模板) | 目标字段 |
|---|---|---|
| `epoch` | `max by(node, role, instance) (cardano_node_metrics_epoch_int)` | `epoch` |
| `sync_percent` | `max by(node, role, instance) (cardano_node_metrics_syncProgress)` | `sync_percent` |
| `tip_diff_blocks` | `max by(node, role, instance) (cardano_node_metrics_chainDensityTipDiff_int)` | `tip_diff_blocks` |
| `peer_count` | `max by(node, role, instance) (cardano_node_metrics_connectedPeers_int)` | `peer_count` |
| `cpu_sys_percent` | `max by(node, role, instance) (cardano_node_resources_cpuSys_percent)` | `cpu_sys_percent` |
| `mem_live_bytes` | `max by(node, role, instance) (cardano_node_resources_memLive_bytes)` | `mem_live_bytes` |
| `mem_rss_bytes` | `max by(node, role, instance) (cardano_node_resources_memRss_bytes)` | `mem_rss_bytes` |
| `mem_heap_bytes` | `max by(node, role, instance) (cardano_node_resources_memHeap_bytes)` | `mem_heap_bytes` |
| `gc_minor_total` | `max by(node, role, instance) (rts_gc_minor_num_gcs)` | `gc_minor_total` |
| `gc_major_total` | `max by(node, role, instance) (rts_gc_major_num_gcs)` | `gc_major_total` |

> 若某网络环境键名差异存在，仅允许在服务端白名单模板内替换，不允许客户端传 query。

## 4. Label 契约
- Prometheus target 必须具备：`node`、`role`。
- 推荐保留：`instance`、`network`。
- `role` 取值：`bp`/`relay`。
- `node` 作为前端主键（`node + role`）用于合并十个指标端点。

## 5. 空值与时间戳兜底策略
- 单指标缺失：该节点在该端点 `series` 中不返回；前端映射为 `null`。
- 端点失败：返回 `5xx` + `note`；前端保持缓存并重试。
- 聚合时间戳：前端按节点选择 10 个指标中“最新样本时间”作为节点 `collected_at`。
- 多 relay 选优：同节点取 `collected_at` 最新的 relay 数据；若时间相同，优先主 relay。

## 6. 安全边界（本阶段）
- 仅 Basic Auth + HTTPS。
- 禁止 query 参数透传。
- Prometheus 原生 API 不对公网暴露。

## 7. 与现有前端字段对齐
最终映射到 `MonitorSnapshot` 字段：
- `epoch`
- `sync_percent`
- `tip_diff_blocks`
- `peer_count`
- `cpu_sys_percent`
- `mem_live_bytes`
- `mem_rss_bytes`
- `mem_heap_bytes`
- `gc_minor_total`
- `gc_major_total`
- `prometheus_source` <- `source_relay`
- `prometheus_note` <- `note`

## 8. 实施输入
- `p9-4`：按本契约生成 Nginx 路由模板与 Basic Auth 配置。
- `p9-5`：monitor 数据源接入上述 10 端点并实现 fallback。
- `p9-6`：在多 relay 列表上实现超时切换与时间戳选优。
