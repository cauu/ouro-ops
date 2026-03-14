export interface DbVersionResult {
  user_version: number;
  tables: Record<string, boolean>;
}

export interface Pool {
  id: number;
  ticker: string;
  network: "mainnet" | "preprod" | "preview";
  margin: number | null;
  fixed_cost: number | null;
  onchain_pool_id: string | null;
  onchain_registered: boolean;
  pledge: number | null;
  reward_account: string | null;
  metadata_url: string | null;
  metadata_hash: string | null;
  owners: string[];
  relays: PoolOnchainRelay[];
  onchain_synced_at: string | null;
  kes_expiry_date: string | null;
  created_at: string;
  updated_at: string;
}

export interface PoolInitPayload {
  ticker: string;
  network: "mainnet" | "preprod" | "preview";
  margin?: number;
  fixed_cost?: number;
}

export interface PoolUpdatePayload {
  ticker?: string;
  margin?: number;
  fixed_cost?: number;
}

export interface PoolOnchainQueryPayload {
  machine_id: number;
  pool_id?: string;
  cold_vkey_path?: string;
}

export interface PoolBindOnchainPayload {
  machine_id: number;
  pool_id: string;
}

export interface PoolRegistrationPreparePayload {
  machine_id: number;
  pool_id: string;
  certificate_path: string;
  payment_addr_path: string;
}

export interface PoolRegistrationSubmitPayload {
  machine_id: number;
  pool_id: string;
  confirm_pool_id: string;
  tx_signed_path: string;
}

export interface PoolOnchainRelay {
  address: string;
  port: number;
}

export interface PoolOnchainRegistration {
  pool_id: string | null;
  ticker: string | null;
  margin: number | null;
  fixed_cost: number | null;
  pledge: number | null;
  reward_account: string | null;
  owners: string[];
  relays: PoolOnchainRelay[];
  metadata_url: string | null;
  metadata_hash: string | null;
}

export interface PoolOnchainStatus {
  machine_id: number;
  machine_name: string;
  network: "mainnet" | "preprod" | "preview" | string;
  query_source: "pool_id" | "cold_vkey" | "unresolved" | string;
  pool_id: string | null;
  cold_vkey_path: string | null;
  registered_onchain: boolean;
  registration: PoolOnchainRegistration | null;
  missing_requirements: string[];
  note: string;
}

export interface PoolRegistrationTxDraft {
  kind: string;
  certificate_path: string | null;
  required_deposit: number | null;
  payment_address: string | null;
  tx_body_path: string | null;
  offline_signing_required: boolean;
  command_preview: string;
}

export interface PoolRegistrationPrepareResult {
  machine_id: number;
  machine_name: string;
  network: "mainnet" | "preprod" | "preview" | string;
  pool_id: string | null;
  registration_relays: PoolOnchainRelay[];
  certificate_generated: boolean;
  certificate_path: string | null;
  missing_requirements: string[];
  tx_draft: PoolRegistrationTxDraft;
  note: string;
}

export interface PoolRegistrationSubmitResult {
  machine_id: number;
  machine_name: string;
  network: "mainnet" | "preprod" | "preview" | string;
  pool_id: string;
  submitted: boolean;
  tx_body_path: string | null;
  tx_signed_path: string | null;
  tx_hash: string | null;
  tx_inputs: string[];
  missing_requirements: string[];
  note: string;
}

export interface Machine {
  id: number;
  pool_id: number;
  name: string;
  ip: string;
  port: number;
  ssh_user: string;
  role: "relay" | "bp" | "archive";
  network: "mainnet" | "preprod" | "preview";
  ssh_key_fingerprint: string | null;
  os_version: string | null;
  cardano_version: string | null;
  image_registry: string;
  image_digest: string | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface MachineAddPayload {
  name: string;
  ip: string;
  port: number;
  ssh_user: string;
  role: "relay" | "bp" | "archive";
  network: "mainnet" | "preprod" | "preview";
  ssh_key_fingerprint: string;
}

export interface MachineFilter {
  role?: "relay" | "bp" | "archive";
  network?: "mainnet" | "preprod" | "preview";
}

export interface RuntimeProbeMount {
  destination: string;
  source: string;
}

export interface RuntimeProbe {
  container_present: boolean;
  container_name: string;
  image_ref: string | null;
  ports: string[];
  mounts: RuntimeProbeMount[];
  managed_by_compose: boolean;
  bp_key_files_present: boolean;
  db_mount_source: string | null;
  keys_mount_source: string | null;
}

export interface PreflightReport {
  ssh_ok: boolean;
  os_version: string;
  disk_available_gb: number;
  memory_total_gb: number;
  disk_iops: number;
  warnings: string[];
}

export interface SshKeyInfo {
  bits: number | null;
  fingerprint: string;
  comment: string;
  key_type: string;
}

export interface DeployPayload {
  machine_ids: number[];
  cardano_version: string;
  image_registry: string;
  image_digest?: string;
  network: "mainnet" | "preprod" | "preview";
  enable_swap: boolean;
  swap_size_gb: number;
  enable_chrony: boolean;
  enable_hardening: boolean;
  safe_validation_mode?: boolean;
  takeover_existing_node?: boolean;
  restore_snapshot?: boolean;
  restore_snapshot_relay?: boolean;
  restore_snapshot_bp?: boolean;
}

export interface UpgradePayload {
  target_version: string;
  image_registry: string;
  image_digest?: string;
  machine_ids: number[];
  auto_continue: boolean;
}

export interface UpgradeGateEvent {
  task_id: string;
  completed_machine: string;
  next_machine: string;
  is_bp: boolean;
  message: string;
}

export interface TaskMachineStatus {
  machine_id: number;
  status: string;
}

export interface DeployTaskStatus {
  task_id: string;
  task_type: string;
  status: "pending" | "running" | "success" | "failed" | "cancelled" | string;
  payload: Record<string, unknown> | null;
  error_msg: string | null;
  started_at: string | null;
  finished_at: string | null;
  created_at: string;
  machine_statuses: TaskMachineStatus[];
}

export interface RecentTaskSummary {
  task_id: string;
  task_type: string;
  status: string;
  phase: string | null;
  error_msg: string | null;
  machine_count: number;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
}

export interface ObservabilityTaskPayload {
  machine_ids?: number[];
}

export interface ObservabilityLastTask {
  task_id: string;
  status: string;
  finished_at: string | null;
}

export interface ObservabilityRelayProbe {
  machine_id: number;
  machine_name: string;
  ip: string;
  configured: boolean;
  gateway_conf_present: boolean;
  htpasswd_present: boolean;
  nginx_running: boolean;
  note: string | null;
}

export interface ObservabilityGatewayStatus {
  relay_total: number;
  configured_relays: number;
  playbook_executed: boolean;
  last_bootstrap: ObservabilityLastTask | null;
  last_rollback: ObservabilityLastTask | null;
  relays: ObservabilityRelayProbe[];
}

export interface TaskLogEvent {
  task_id: string;
  stream: "stdout" | "stderr" | string;
  line: string;
  timestamp: string;
}

export interface MonitorSnapshot {
  machine_id: number;
  machine_name: string;
  role: "relay" | "bp" | "archive" | string;
  network: "mainnet" | "preprod" | "preview" | string;
  block_height: number | null;
  sync_progress: number | null;
  blocks_per_minute: number | null;
  health_level: "healthy" | "warning" | "critical" | string;
  status: "syncing" | "synced" | "stalled" | "unreachable" | "unknown" | string;
  sync_stage:
    | "snapshot_restoring"
    | "restore_failed"
    | "restore_timeout"
    | "fallback_syncing"
    | "syncing"
    | "synced"
    | "unknown"
    | "unreachable"
    | string;
  restore_snapshot_requested: boolean;
  stalled: boolean;
  collected_at: string;
  note: string | null;
  epoch: number | null;
  slot_num: number | null;
  slot_in_epoch: number | null;
  sync_percent: number | null;
  tip_diff_blocks: number | null;
  late_blocks: number | null;
  peer_count: number | null;
  cpu_sys_percent: number | null;
  mem_live_bytes: number | null;
  mem_rss_bytes: number | null;
  mem_heap_bytes: number | null;
  gc_minor_total: number | null;
  gc_major_total: number | null;
  txs_in_mempool: number | null;
  mempool_bytes: number | null;
  forks: number | null;
  forging_enabled: number | null;
  prometheus_source: string | null;
  prometheus_note: string | null;
}

export interface MonitorPollingStatus {
  running: boolean;
  interval_seconds: number;
}

export interface KesStatus {
  machine_id: number;
  machine_name: string;
  kes_period_current: number | null;
  kes_period_max: number | null;
  remaining_days: number | null;
  severity: "healthy" | "warning" | "critical" | string;
  expiry_date: string | null;
  op_cert_counter: number | null;
}

export interface KesSignRequest {
  machine_id: number;
  kes_vkey_path: string;
  counter_value: number;
  instructions: string;
}
