# S0009 p9-12-fix8 RAW Telemetry 指标字典

更新时间：2026-03-13
来源：`GET /api/ops/v1/telemetry/raw`（你提供的样本）

说明：
- 本文用于“前端可展示指标”的选型基线。
- 含义基于 Cardano/EKG/RTS 常见命名规则与当前样本推断；最终以节点 `/metrics` 的 `# HELP`/`# TYPE` 为准。
- 单位未在名称中明确时，按“计数/时长/字节/比例”语义推断。

## 1. 链状态与同步
- `cardano_node_metrics_epoch_int`：当前 epoch 编号。
- `cardano_node_metrics_slotNum_int`：当前全局 slot 编号。
- `cardano_node_metrics_slotInEpoch_int`：当前 epoch 内 slot 编号。
- `cardano_node_metrics_blockNum_int`：当前链上块高（block number）。
- `cardano_node_metrics_density_real`：链密度（最近窗口内出块密度）。
- `cardano_node_metrics_forks_int`：检测到的分叉累计计数。
- `cardano_node_metrics_blockfetchclient_lateblocks`：落后块数量（同步落后程度）。
- `cardano_node_metrics_blockfetchclient_blocksize`：最近拉取区块大小。
- `cardano_node_metrics_blockfetchclient_blockdelay_s`：区块获取延迟（秒）。
- `cardano_node_metrics_blockfetchclient_blockdelay_cdfOne`：区块延迟 CDF（阈值 one）。
- `cardano_node_metrics_blockfetchclient_blockdelay_cdfThree`：区块延迟 CDF（阈值 three）。
- `cardano_node_metrics_blockfetchclient_blockdelay_cdfFive`：区块延迟 CDF（阈值 five）。
- `cardano_node_metrics_served_block_count_int`：节点对外服务的区块总量。
- `cardano_node_metrics_served_block_latest_count_int`：最近窗口内服务区块数量。
- `cardano_node_metrics_served_header_counter_int`：节点对外服务的区块头总量。
- `cardano_node_metrics_nodeStartTime_int`：节点启动时间（unix epoch）。
- `cardano_node_metrics_forging_enabled`：是否启用 forging（1 启用，0 禁用）。

## 2. 交易与内存池
- `cardano_node_metrics_txsInMempool_int`：当前 mempool 交易数。
- `cardano_node_metrics_mempoolBytes_int`：当前 mempool 占用字节数。
- `cardano_node_metrics_txsProcessedNum_int`：已处理交易累计数。
- `cardano_node_metrics_txsSyncDuration_int`：交易同步耗时（按命名推断为时长）。

## 3. 连接管理（connectionManager）
- `cardano_node_metrics_connectionManager_duplexConns`：双向连接总数。
- `cardano_node_metrics_connectionManager_fullDuplexConns`：全双工连接数。
- `cardano_node_metrics_connectionManager_incomingConns`：入站连接数。
- `cardano_node_metrics_connectionManager_outgoingConns`：出站连接数。
- `cardano_node_metrics_connectionManager_unidirectionalConns`：单向连接数。

## 4. Inbound Governor
- `cardano_node_metrics_inboundGovernor_hot`：hot 状态入站连接数。
- `cardano_node_metrics_inboundGovernor_warm`：warm 状态入站连接数。
- `cardano_node_metrics_inboundGovernor_cold`：cold 状态入站连接数。
- `cardano_node_metrics_inboundGovernor_idle`：idle 状态入站连接数。

## 5. Peer Selection（总体状态）
- `cardano_node_metrics_peerSelection_KnownPeers`：已知 peers 总数。
- `cardano_node_metrics_peerSelection_RootPeers`：root peers 总数。
- `cardano_node_metrics_peerSelection_KnownLocalRootPeers`：已知本地 root peers 数。
- `cardano_node_metrics_peerSelection_KnownNonRootPeers`：已知非 root peers 数。
- `cardano_node_metrics_peerSelection_KnownBootstrapPeers`：已知 bootstrap peers 数。
- `cardano_node_metrics_peerSelection_KnownBigLedgerPeers`：已知 big-ledger peers 数。
- `cardano_node_metrics_peerSelection_EstablishedPeers`：已建立连接 peers 总数。
- `cardano_node_metrics_peerSelection_EstablishedLocalRootPeers`：已建立本地 root peers 数。
- `cardano_node_metrics_peerSelection_EstablishedNonRootPeers`：已建立非 root peers 数。
- `cardano_node_metrics_peerSelection_EstablishedBootstrapPeers`：已建立 bootstrap peers 数。
- `cardano_node_metrics_peerSelection_EstablishedBigLedgerPeers`：已建立 big-ledger peers 数。
- `cardano_node_metrics_peerSelection_ActivePeers`：活跃 peers 总数。
- `cardano_node_metrics_peerSelection_ActiveLocalRootPeers`：活跃本地 root peers 数。
- `cardano_node_metrics_peerSelection_ActiveNonRootPeers`：活跃非 root peers 数。
- `cardano_node_metrics_peerSelection_ActiveBootstrapPeers`：活跃 bootstrap peers 数。
- `cardano_node_metrics_peerSelection_ActiveBigLedgerPeers`：活跃 big-ledger peers 数。
- `cardano_node_metrics_peerSelection_warm`：warm peers 总数。
- `cardano_node_metrics_peerSelection_hot`：hot peers 总数。
- `cardano_node_metrics_peerSelection_cold`：cold peers 总数。
- `cardano_node_metrics_peerSelection_warmBigLedgerPeers`：warm big-ledger peers 数。
- `cardano_node_metrics_peerSelection_hotBigLedgerPeers`：hot big-ledger peers 数。
- `cardano_node_metrics_peerSelection_coldBigLedgerPeers`：cold big-ledger peers 数。

## 6. Peer Selection（Promotions / Demotions）
- `cardano_node_metrics_peerSelection_ColdPeersPromotions`：cold peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_ColdNonRootPeersPromotions`：cold non-root peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_ColdBootstrapPeersPromotions`：cold bootstrap peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_ColdBigLedgerPeersPromotions`：cold big-ledger peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_WarmPeersPromotions`：warm peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_WarmNonRootPeersPromotions`：warm non-root peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_WarmLocalRootPeersPromotions`：warm local-root peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_WarmBootstrapPeersPromotions`：warm bootstrap peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_WarmBigLedgerPeersPromotions`：warm big-ledger peers 晋升事件计数。
- `cardano_node_metrics_peerSelection_WarmPeersDemotions`：warm peers 降级事件计数。
- `cardano_node_metrics_peerSelection_WarmNonRootPeersDemotions`：warm non-root peers 降级事件计数。
- `cardano_node_metrics_peerSelection_WarmBootstrapPeersDemotions`：warm bootstrap peers 降级事件计数。
- `cardano_node_metrics_peerSelection_WarmBigLedgerPeersDemotions`：warm big-ledger peers 降级事件计数。
- `cardano_node_metrics_peerSelection_ActivePeersDemotions`：active peers 降级事件计数。
- `cardano_node_metrics_peerSelection_ActiveLocalRootPeersDemotions`：active local-root peers 降级事件计数。
- `cardano_node_metrics_peerSelection_ActiveNonRootPeersDemotions`：active non-root peers 降级事件计数。
- `cardano_node_metrics_peerSelection_ActiveBootstrapPeersDemotions`：active bootstrap peers 降级事件计数。
- `cardano_node_metrics_peerSelection_ActiveBigLedgerPeersDemotions`：active big-ledger peers 降级事件计数。

## 7. Peer Selection Churn（变化量与耗时）
- `cardano_node_metrics_peerSelection_churn_IncreasedKnownPeers`：KnownPeers 增量。
- `cardano_node_metrics_peerSelection_churn_DecreasedKnownPeers`：KnownPeers 减量。
- `cardano_node_metrics_peerSelection_churn_IncreasedKnownPeers_duration`：KnownPeers 增长阶段耗时。
- `cardano_node_metrics_peerSelection_churn_DecreasedKnownPeers_duration`：KnownPeers 减少阶段耗时。

- `cardano_node_metrics_peerSelection_churn_IncreasedActivePeers`：ActivePeers 增量。
- `cardano_node_metrics_peerSelection_churn_DecreasedActivePeers`：ActivePeers 减量。
- `cardano_node_metrics_peerSelection_churn_IncreasedActivePeers_duration`：ActivePeers 增长阶段耗时。
- `cardano_node_metrics_peerSelection_churn_DecreasedActivePeers_duration`：ActivePeers 减少阶段耗时。

- `cardano_node_metrics_peerSelection_churn_IncreasedEstablishedPeers`：EstablishedPeers 增量。
- `cardano_node_metrics_peerSelection_churn_DecreasedEstablishedPeers`：EstablishedPeers 减量。
- `cardano_node_metrics_peerSelection_churn_IncreasedEstablishedPeers_duration`：EstablishedPeers 增长阶段耗时。
- `cardano_node_metrics_peerSelection_churn_DecreasedEstablishedPeers_duration`：EstablishedPeers 减少阶段耗时。

- `cardano_node_metrics_peerSelection_churn_IncreasedKnownBigLedgerPeers`：KnownBigLedgerPeers 增量。
- `cardano_node_metrics_peerSelection_churn_DecreasedKnownBigLedgerPeers`：KnownBigLedgerPeers 减量。
- `cardano_node_metrics_peerSelection_churn_IncreasedKnownBigLedgerPeers_duration`：KnownBigLedgerPeers 增长阶段耗时。
- `cardano_node_metrics_peerSelection_churn_DecreasedKnownBigLedgerPeers_duration`：KnownBigLedgerPeers 减少阶段耗时。

- `cardano_node_metrics_peerSelection_churn_IncreasedActiveBigLedgerPeers`：ActiveBigLedgerPeers 增量。
- `cardano_node_metrics_peerSelection_churn_DecreasedActiveBigLedgerPeers`：ActiveBigLedgerPeers 减量。
- `cardano_node_metrics_peerSelection_churn_IncreasedActiveBigLedgerPeers_duration`：ActiveBigLedgerPeers 增长阶段耗时。
- `cardano_node_metrics_peerSelection_churn_DecreasedActiveBigLedgerPeers_duration`：ActiveBigLedgerPeers 减少阶段耗时。

- `cardano_node_metrics_peerSelection_churn_IncreasedEstablishedBigLedgerPeers`：EstablishedBigLedgerPeers 增量。
- `cardano_node_metrics_peerSelection_churn_DecreasedEstablishedBigLedgerPeers`：EstablishedBigLedgerPeers 减量。
- `cardano_node_metrics_peerSelection_churn_IncreasedEstablishedBigLedgerPeers_duration`：EstablishedBigLedgerPeers 增长阶段耗时。
- `cardano_node_metrics_peerSelection_churn_DecreasedEstablishedBigLedgerPeers_duration`：EstablishedBigLedgerPeers 减少阶段耗时。

## 8. 内存 / GC / RTS（节点进程运行态）
- `cardano_node_metrics_RTS_gcLiveBytes_int`：GC live bytes（存活对象字节）。
- `cardano_node_metrics_RTS_gcHeapBytes_int`：GC heap bytes（堆总字节）。
- `cardano_node_metrics_Mem_resident_int`：进程 RSS 常驻内存字节。
- `cardano_node_metrics_RTS_gcMinorNum_int`：minor GC 次数。
- `cardano_node_metrics_RTS_gcMajorNum_int`：major GC 次数。
- `cardano_node_metrics_RTS_gcticks_int`：GC tick 计数。
- `cardano_node_metrics_RTS_mutticks_int`：mutator tick 计数。
- `cardano_node_metrics_Stat_threads_int`：进程线程数。
- `cardano_node_metrics_Stat_cputicks_int`：CPU tick 计数。

- `rts_gc_current_bytes_used`：当前 GC 使用字节数。
- `rts_gc_current_bytes_slop`：当前 GC slop 字节（保留/碎片空间）。
- `rts_gc_max_bytes_used`：历史最大 GC 使用字节。
- `rts_gc_max_large_bytes_used`：历史最大 large object 区使用字节。
- `rts_gc_max_compact_bytes_used`：历史最大 compact 区使用字节。
- `rts_gc_max_bytes_slop`：历史最大 GC slop 字节。
- `rts_gc_cumulative_bytes_used`：累计使用字节。
- `rts_gc_bytes_allocated`：累计分配字节。
- `rts_gc_bytes_copied`：累计复制字节（GC 拷贝成本）。
- `rts_gc_num_gcs`：GC 总次数。
- `rts_gc_num_bytes_usage_samples`：内存使用采样次数。

- `rts_gc_cpu_ms`：GC 总 CPU 时间（ms）。
- `rts_gc_gc_cpu_ms`：GC 阶段 CPU 时间（ms）。
- `rts_gc_mutator_cpu_ms`：mutator CPU 时间（ms）。
- `rts_gc_wall_ms`：GC 相关总墙钟时间（ms）。
- `rts_gc_gc_wall_ms`：GC 阶段墙钟时间（ms）。
- `rts_gc_mutator_wall_ms`：mutator 墙钟时间（ms）。
- `rts_gc_init_cpu_ms`：GC 初始化 CPU 时间（ms）。
- `rts_gc_init_wall_ms`：GC 初始化墙钟时间（ms）。

- `rts_gc_par_tot_bytes_copied`：并行 GC 总复制字节。
- `rts_gc_par_avg_bytes_copied`：并行 GC 平均复制字节。
- `rts_gc_par_max_bytes_copied`：并行 GC 最大复制字节。
- `rts_gc_par_balanced_bytes_copied`：并行 GC 平衡复制字节。

- `rts_gc_peak_megabytes_allocated`：峰值分配内存（MB）。

- `rts_gc_nm_cpu_ms`：non-moving GC CPU 时间（ms）。
- `rts_gc_nm_elapsed_ms`：non-moving GC 墙钟时间（ms）。
- `rts_gc_nm_max_elapsed_ms`：non-moving GC 最大墙钟时间（ms）。
- `rts_gc_nm_sync_cpu_ms`：non-moving GC 同步 CPU 时间（ms）。
- `rts_gc_nm_sync_elapsed_ms`：non-moving GC 同步墙钟时间（ms）。
- `rts_gc_nm_sync_max_elapsed_ms`：non-moving GC 同步最大墙钟时间（ms）。

## 9. 元信息与构建信息
- `cardano_node_metrics_cardano_build_info`：节点版本/编译器/架构等构建元数据（label 中包含 version、revision、compiler、os 等）。
- `ekg_server_timestamp_ms`：EKG 服务器时间戳（毫秒）。

## 10. 前端展示优先级建议（第一批）
建议优先接入以下指标（高价值、可解释、稳定）：
1. `cardano_node_metrics_epoch_int`
2. `cardano_node_metrics_blockNum_int`
3. `cardano_node_metrics_slotNum_int`
4. `cardano_node_metrics_blockfetchclient_lateblocks`
5. `cardano_node_metrics_peerSelection_EstablishedPeers`
6. `cardano_node_metrics_connectionManager_duplexConns`
7. `cardano_node_metrics_Mem_resident_int`
8. `cardano_node_metrics_RTS_gcLiveBytes_int`
9. `cardano_node_metrics_RTS_gcHeapBytes_int`
10. `cardano_node_metrics_RTS_gcMinorNum_int`
11. `cardano_node_metrics_RTS_gcMajorNum_int`
12. `cardano_node_metrics_txsInMempool_int`
13. `cardano_node_metrics_mempoolBytes_int`
14. `cardano_node_metrics_forks_int`
15. `cardano_node_metrics_forging_enabled`

