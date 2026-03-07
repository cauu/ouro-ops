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
}
