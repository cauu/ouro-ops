# S0009 p9-12-fix7 Telemetry Raw Endpoint Contract

## Goal
- Remove legacy per-metric telemetry endpoints.
- Keep only one endpoint that returns full Prometheus vector payload.

## Gateway API
- Method: `GET`
- Path: `/api/ops/v1/telemetry/raw`
- Query params: not allowed (`400` if present)
- Auth: Basic Auth (`ouro_app:<API_KEY>`)
- Upstream query: `/api/v1/query?query={__name__=~".+"}`

## Client Mapping
The Tauri monitor consumes `raw` once per relay and maps these fields locally:
- `epoch` <- `cardano_node_metrics_epoch_int`
- `sync_percent` <- `cardano_node_metrics_syncProgress`
- `tip_diff_blocks` <- `cardano_node_metrics_chainDensityTipDiff_int` or `cardano_node_metrics_blockfetchclient_lateblocks`
- `peer_count` <- `cardano_node_metrics_connectedPeers_int` or `cardano_node_metrics_connectionManager_duplexConns` or `cardano_node_metrics_peerSelection_EstablishedPeers`
- `cpu_sys_percent` <- `cardano_node_resources_cpuSys_percent`
- `mem_live_bytes` <- `cardano_node_resources_memLive_bytes` or `cardano_node_metrics_RTS_gcLiveBytes_int` or `rts_gc_current_bytes_used`
- `mem_rss_bytes` <- `cardano_node_resources_memRss_bytes` or `cardano_node_metrics_Mem_resident_int`
- `mem_heap_bytes` <- `cardano_node_resources_memHeap_bytes` or `cardano_node_metrics_RTS_gcHeapBytes_int`
- `gc_minor_total` <- `rts_gc_minor_num_gcs` or `cardano_node_metrics_RTS_gcMinorNum_int`
- `gc_major_total` <- `rts_gc_major_num_gcs` or `cardano_node_metrics_RTS_gcMajorNum_int`

## Compatibility
- Parser supports both Prometheus vector payload (`data.result`) and legacy series payload (`series`).
- For legacy series payload, `metric_name` falls back to top-level `metric` field.
