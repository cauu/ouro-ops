#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod db;
mod error;
mod keychain;
mod sidecar;

use std::sync::Arc;
use std::sync::Mutex;
use tauri::Manager;

use crate::commands::monitor::MonitorPollingState;
pub use db::DbState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let path = app.path().app_data_dir().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            let db_path = path.join("ouro_ops.sqlite");
            let conn = db::open_and_migrate(&db_path).map_err(|e| e.to_string())?;
            app.manage(DbState(Mutex::new(conn)));
            app.manage(MonitorPollingState(Mutex::new(None)));

            let sidecar_state =
                sidecar::spawn_sidecar(app_handle.clone()).map_err(|e| e.to_string())?;
            {
                let mut runner = sidecar_state.runner.lock().map_err(|_| "lock")?;
                let r = runner.as_mut().ok_or("runner")?;
                r.ping().map_err(|e| e.to_string())?;
            }
            app.manage(Mutex::new(Some(Arc::new(sidecar_state))));

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<Mutex<Option<Arc<sidecar::SidecarState>>>>() {
                    if let Ok(mut guard) = state.lock() {
                        if let Some(s) = guard.take() {
                            if let Ok(mut runner) = s.runner.lock() {
                                if let Some(ref mut r) = *runner {
                                    let _ = r.shutdown();
                                }
                            }
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::db_version,
            commands::run_playbook_test,
            commands::pool::pool_init,
            commands::pool::pool_get,
            commands::pool::pool_update,
            commands::pool::pool_onchain_status,
            commands::pool::pool_bind_onchain,
            commands::pool::pool_registration_prepare,
            commands::pool::pool_registration_submit,
            commands::pool::pool_refresh_bound_onchain,
            commands::pool::pool_unbind_onchain,
            commands::machine::machine_add,
            commands::machine::machine_remove,
            commands::machine::machine_list,
            commands::machine::ssh_agent_list_keys,
            commands::machine::ssh_agent_add_key,
            commands::machine::machine_preflight,
            commands::machine::machine_runtime_probe,
            commands::deploy::deploy_start,
            commands::deploy::deploy_status,
            commands::deploy::deploy_cancel,
            commands::monitor::monitor_snapshot,
            commands::monitor::monitor_start_polling,
            commands::monitor::monitor_stop_polling,
            commands::observability::observability_gateway_status,
            commands::observability::observability_bootstrap_start,
            commands::observability::observability_bootstrap_status,
            commands::observability::observability_rollback_start,
            commands::observability::observability_rollback_status,
            commands::kes::kes_status_all,
            commands::kes::kes_generate,
            commands::kes::kes_import_cert,
            commands::kes::kes_push_start,
            commands::kes::kes_rotation_status,
            commands::kes::kes_prepare_bundle,
            commands::runtime::runtime_apply_config,
            commands::runtime::runtime_config_status,
            commands::runtime::runtime_restart,
            commands::runtime::runtime_restart_status,
            commands::task::task_recent_list,
            commands::task::task_log_query,
            commands::upgrade::upgrade_start,
            commands::upgrade::upgrade_status,
            commands::upgrade::upgrade_confirm_next,
            commands::upgrade::upgrade_rollback,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod frontend_tests {
    #[test]
    fn tc_fe_001_redirects_to_setup_when_pool_missing() {
        let app = include_str!("../../src/App.tsx");
        assert!(app.contains("path=\"/setup\""));
        assert!(app.contains("<Navigate to=\"/setup\" replace />"));
        assert!(app.contains("pool ? <Layout pool={pool} /> : <Navigate to=\"/setup\" replace />"));
    }

    #[test]
    fn tc_fe_002_sidebar_links_match_routes() {
        let sidebar = include_str!("../../src/components/Sidebar.tsx");
        assert!(sidebar.contains("to=\"/\""));
        assert!(sidebar.contains("to=\"/kes\""));
        assert!(sidebar.contains("to=\"/logs\""));
        assert!(sidebar.contains("to=\"/upgrade\""));
        assert!(!sidebar.contains("to=\"/deploy\""));
        assert!(!sidebar.contains("to=\"/settings\""));
        assert!(sidebar.contains("⌂ Dashboard"));
        assert!(!sidebar.contains("Machines"));
        assert!(sidebar.contains("↻ KES Rotate"));
        assert!(sidebar.contains("⬆ Upgrade"));
        assert!(sidebar.contains("Mainnet Workspace"));
        assert!(sidebar.contains("Workspace"));
        assert!(sidebar.contains("Operations"));
        assert!(sidebar.contains("日常操作入口统一"));
    }

    #[test]
    fn tc_fe_003_task_log_stream_filters_by_task_id() {
        let file = include_str!("../../src/components/TaskLogStream.tsx");
        assert!(file.contains("event.payload.task_id !== taskId"));
    }

    #[test]
    fn tc_obs_011_monitor_collect_prometheus_metrics_is_api_only() {
        let monitor = include_str!("commands/monitor.rs");
        assert!(monitor.contains("fn collect_prometheus_metrics"));
        assert!(!monitor.contains("let local_metrics = collect_local_prometheus_metrics"));
    }

    #[test]
    fn tc_fe_004_deploy_wizard_step_submit() {
        let app = include_str!("../../src/App.tsx");
        let deploy = include_str!("../../src/pages/DeployWizard.tsx");
        assert!(app.contains("path=\"/deploy\""));
        assert!(app.contains("path=\"/kes\""));
        assert!(app.contains("path=\"/upgrade\""));
        assert!(deploy.contains("deployStart("));
        assert!(deploy.contains("useState(\"10.5.4-1\")"));
        assert!(deploy.contains("ghcr.io/blinklabs-io/cardano-node"));
        assert!(deploy.contains("takeover_existing_node"));
        assert!(deploy.contains("restore_snapshot_relay"));
        assert!(deploy.contains("restore_snapshot_bp"));
        assert!(deploy.contains("mithrilInitializationAllowed = false"));
        assert!(deploy.contains("setRestoreSnapshotBp(false)"));
        assert!(deploy.contains("Mithril cold-start restore is disabled in this mac app stage"));
        assert!(deploy.contains("networkSupportsMithril"));
        assert!(deploy.contains("step === 1"));
        assert!(deploy.contains("step === 2"));
        assert!(deploy.contains("step === 3"));
        assert!(deploy.contains("step === 4"));
        assert!(deploy.contains("手动输入 BP/Relay 节点"));
        assert!(deploy.contains("点击下一步时会执行机器创建"));
        assert!(deploy.contains("Step 1 completed."));
        assert!(deploy.contains("Creating nodes..."));
        assert!(deploy.contains("Resolving runtime state"));
    }

    #[test]
    fn tc_fe_machine_add_key_flow_exists() {
        let mm = include_str!("../../src/pages/MachineManager.tsx");
        assert!(mm.contains("sshAgentAddKey("));
        assert!(mm.contains("Add Key to ssh-agent"));
        assert!(mm.contains("machineRuntimeProbe("));
        assert!(mm.contains("Runtime Probe"));
        assert!(mm.contains("Loading machines..."));
        assert!(mm.contains("Requesting machine list from local app"));
    }

    #[test]
    fn tc_fe_006_dashboard_sync_monitor_exists() {
        let dashboard = include_str!("../../src/pages/Dashboard.tsx");
        assert!(dashboard.contains("useMonitorStore()"));
        assert!(dashboard.contains("startMonitorStore(30)"));
        assert!(dashboard.contains("stopMonitorStore()"));
        assert!(dashboard.contains("集群概览（BP + Relays）"));
        assert!(dashboard.contains("节点详情"));
        assert!(dashboard.contains("近期操作日志"));
        assert!(dashboard.contains("KES remain"));
        assert!(dashboard.contains("立即 Rotate"));
        assert!(dashboard.contains("Within 1s"));
        assert!(dashboard.contains("0-50ms"));
        assert!(dashboard.contains(">1s"));
        assert!(dashboard.contains("Sync"));
        assert!(dashboard.contains("role=\"tooltip\""));
        assert!(dashboard.contains("className=\"tab-controller\""));
        assert!(dashboard.contains("snapshot.health_level"));
        assert!(!dashboard.contains("Ouro Ops · Dashboard"));
        assert!(dashboard.contains("Telemetry"));
        assert!(!dashboard.contains("Bind Existing Pool"));
    }

    #[test]
    fn tc_fe_009_macos_uses_native_window_titlebar_style() {
        let tauri_conf = include_str!("../../src-tauri/tauri.conf.json");
        assert!(tauri_conf.contains("\"titleBarStyle\": \"Overlay\""));
        assert!(tauri_conf.contains("\"hiddenTitle\": true"));
        assert!(tauri_conf.contains("\"trafficLightPosition\""));
        assert!(tauri_conf.contains("\"x\": 14"));
        assert!(tauri_conf.contains("\"y\": 14"));
    }

    #[test]
    fn tc_fe_010_layout_uses_custom_titlebar_drag_regions() {
        let layout = include_str!("../../src/components/Layout.tsx");
        let sidebar = include_str!("../../src/components/Sidebar.tsx");
        let css = include_str!("../../src/index.css");
        assert!(layout.contains("data-tauri-drag-region"));
        assert!(sidebar.contains("data-tauri-drag-region"));
        assert!(layout.contains("no-drag"));
        assert!(sidebar.contains("no-drag"));
        assert!(layout.contains("no-drag truncate text-[14px]"));
        assert!(layout.contains("no-drag inline-flex min-h-6"));
        assert!(sidebar.contains("pl-[74px]"));
        assert!(sidebar.contains("flex-1"));
        assert!(css.contains(".drag-region"));
        assert!(css.contains(".no-drag"));
        assert!(css.contains("user-select: text"));
    }

    #[test]
    fn tc_fe_026_settings_is_read_only_for_chain_fields() {
        let settings = include_str!("../../src/pages/Settings.tsx");
        assert!(settings.contains("ticker`, `margin` and `fixed cost` are not edited here."));
        assert!(settings.contains(
            "Chain-facing pool parameters are read from the bound on-chain registration."
        ));
        assert!(!settings.contains("poolUpdate("));
        assert!(!settings.contains("type=\"submit\""));
    }

    #[test]
    fn tc_fe_007_ipc_exposes_monitor_polling_commands() {
        let ipc = include_str!("../../src/lib/ipc.ts");
        assert!(ipc.contains("monitorStartPolling"));
        assert!(ipc.contains("monitorStopPolling"));
    }

    #[test]
    fn tc_fe_008_monitor_store_subscribes_to_monitor_events() {
        let store = include_str!("../../src/lib/monitorStore.ts");
        assert!(store.contains("listen<MonitorSnapshot[]>(\"monitor:snapshot\""));
        assert!(store.contains("listen<{ message?: string }>(\"monitor:error\""));
        assert!(store.contains("monitorStartPolling("));
        assert!(store.contains("monitorStopPolling("));
        assert!(store.contains("useSyncExternalStore"));
    }

    #[test]
    fn tc_dep_006_playbook_no_longer_waits_for_full_sync_completion() {
        let playbook = include_str!("../../ansible/roles/cardano-node/tasks/main.yml");
        assert!(playbook.contains("Wait for initial cardano tip query to succeed"));
        assert!(playbook.contains("Record initial cardano tip observation"));
        assert!(!playbook.contains("sync_progress.stdout == \"100.00\""));
    }

    #[test]
    fn tc_dep_007_mainnet_topology_includes_bootstrap_peers() {
        let topology =
            include_str!("../../ansible/roles/cardano-node/templates/topology-p2p.json.j2");
        assert!(topology.contains("\"bootstrapPeers\""));
        assert!(topology.contains("backbone.cardano.iog.io"));
        assert!(topology.contains("backbone.mainnet.emurgornd.com"));
        assert!(topology.contains("backbone.mainnet.cardanofoundation.org"));
        assert!(topology.contains("\"publicRoots\": ["));
    }

    #[test]
    fn tc_dep_008_relay_topology_excludes_bp_local_roots() {
        let topology =
            include_str!("../../ansible/roles/cardano-node/templates/topology-p2p.json.j2");
        assert!(topology.contains("rejectattr(\"name\", \"equalto\", inventory_hostname)"));
        assert!(!topology.contains("{% for bp in bp_nodes %}"));
        assert!(topology.contains("relay_upstreams | length"));
    }

    #[test]
    fn tc_dep_009_bp_topology_uses_relay_only() {
        let topology =
            include_str!("../../ansible/roles/cardano-node/templates/topology-p2p.json.j2");
        assert!(topology.contains("\"bootstrapPeers\": []"));
        assert!(topology.contains("{% if role == \"bp\" %}"));
        assert!(topology.contains("{% for relay in relay_nodes %}"));
        assert!(topology.contains("\"trustable\": true"));
    }

    #[test]
    fn tc_dep_010_config_changes_restart_cardano_container() {
        let playbook = include_str!("../../ansible/roles/cardano-node/tasks/main.yml");
        assert!(playbook.contains("Determine whether runtime config changed"));
        assert!(playbook.contains("cardano_runtime_config_changed"));
        assert!(
            playbook.contains("restart: \"{{ cardano_runtime_config_changed | default(false) }}\"")
        );
    }

    #[test]
    fn tc_dep_011_restore_snapshot_only_runs_for_cold_start_db() {
        let playbook = include_str!("../../ansible/roles/cardano-node/tasks/main.yml");
        assert!(playbook.contains("protocolMagicId"));
        assert!(playbook.contains("cardano_db_content_probe"));
        assert!(playbook.contains("cardano_restore_snapshot_requested"));
        assert!(playbook.contains("cardano_restore_snapshot_effective"));
        assert!(playbook.contains("restore_snapshot_relay"));
        assert!(playbook.contains("restore_snapshot_bp"));
        assert!(playbook.contains("db already initialized"));
        assert!(playbook.contains("Clean residual cold-start DB contents before Mithril restore"));
        assert!(playbook.contains("RESTORE_SNAPSHOT': ('true' if (cardano_restore_snapshot_effective | default(false) | bool) else 'false')"));
        assert!(playbook.contains("GENESIS_VERIFICATION_KEY"));
        assert!(playbook.contains("ANCILLARY_VERIFICATION_KEY"));
        assert!(playbook.contains("AGGREGATOR_ENDPOINT"));
        assert!(playbook.contains("genesis.vkey"));
        assert!(playbook.contains("ancillary.vkey"));
    }

    #[test]
    fn tc_dep_012_mithril_restore_readiness_accepts_running_container() {
        let playbook = include_str!("../../ansible/roles/cardano-node/tasks/main.yml");
        assert!(playbook.contains("Wait for cardano container to stay running"));
        assert!(playbook.contains("Capture Mithril startup logs"));
        assert!(playbook.contains("readiness: restore-in-progress"));
        assert!(playbook
            .contains("when: not (cardano_restore_snapshot_effective | default(false) | bool)"));
    }

    #[test]
    fn tc_dep_013_runtime_config_playbook_exists_and_restarts_runtime() {
        let playbook = include_str!("../../ansible/playbooks/runtime-config.yml");
        let tasks = include_str!("../../ansible/roles/cardano-node/tasks/runtime_config.yml");
        assert!(playbook.contains("tasks_from: runtime_config"));
        assert!(tasks.contains("Render topology template for runtime apply"));
        assert!(tasks.contains("docker restart cardano-node"));
        assert!(tasks.contains("use deploy for initial provisioning"));
    }

    #[test]
    fn tc_dep_014_mainnet_config_template_is_role_aware() {
        let template = include_str!(
            "../../ansible/roles/cardano-node/templates/config-mainnet-10.5.4-1.json.j2"
        );
        assert!(template.contains("role == \"relay\""));
        assert!(template.contains("TargetNumberOfRootPeers"));
        assert!(template.contains("relay_nodes | default([]) | length"));
        assert!(template.contains("PeerSharing"));
    }

    #[test]
    fn tc_dep_015_runtime_restart_playbook_exists_and_avoids_deploy_flow() {
        let playbook = include_str!("../../ansible/playbooks/runtime-restart.yml");
        let tasks = include_str!("../../ansible/roles/cardano-node/tasks/runtime_restart.yml");
        assert!(playbook.contains("tasks_from: runtime_restart"));
        assert!(tasks.contains("docker restart cardano-node"));
        assert!(tasks.contains("use deploy for initial provisioning"));
        assert!(!tasks.contains("RESTORE_SNAPSHOT"));
    }

    #[test]
    fn tc_dep_016_kes_push_playbook_exists_and_restarts_bp() {
        let playbook = include_str!("../../ansible/playbooks/kes-push.yml");
        let tasks = include_str!("../../ansible/roles/cardano-node/tasks/kes_push.yml");
        assert!(playbook.contains("tasks_from: kes_push"));
        assert!(tasks.contains("kes_cert_path"));
        assert!(tasks.contains("/opt/cardano/keys/node.cert"));
        assert!(tasks.contains("docker restart cardano-node"));
        assert!(tasks.contains("use deploy for initial provisioning"));
    }

    #[test]
    fn tc_upg_005_upgrade_commands_registered() {
        let commands = include_str!("commands/mod.rs");
        let lib = include_str!("lib.rs");
        assert!(commands.contains("pub mod upgrade;"));
        assert!(lib.contains("commands::upgrade::upgrade_start"));
        assert!(lib.contains("commands::upgrade::upgrade_status"));
        assert!(lib.contains("commands::upgrade::upgrade_confirm_next"));
        assert!(lib.contains("commands::upgrade::upgrade_rollback"));
    }

    #[test]
    fn tc_dep_017_upgrade_and_rollback_playbooks_exist() {
        let upgrade = include_str!("../../ansible/playbooks/upgrade.yml");
        let upgrade_tasks = include_str!("../../ansible/roles/cardano-node/tasks/upgrade.yml");
        let rollback = include_str!("../../ansible/playbooks/rollback.yml");
        let rollback_tasks = include_str!("../../ansible/roles/cardano-node/tasks/rollback.yml");
        assert!(upgrade.contains("serial: 1"));
        assert!(upgrade.contains("order: sorted"));
        assert!(upgrade.contains("tasks_from: upgrade"));
        assert!(upgrade_tasks.contains("upgrade_phase"));
        assert!(upgrade_tasks.contains("cardano_upgrade_image_ref"));
        assert!(upgrade_tasks.contains("backup_archive"));
        assert!(rollback.contains("tasks_from: rollback"));
        assert!(rollback_tasks.contains("previous_version"));
        assert!(rollback_tasks.contains("backup_archive"));
        assert!(rollback_tasks.contains("tar -xzf"));
    }

    #[test]
    fn tc_fe_009_ipc_exposes_kes_commands() {
        let ipc = include_str!("../../src/lib/ipc.ts");
        assert!(ipc.contains("kesStatusAll"));
        assert!(ipc.contains("kesGenerate"));
        assert!(ipc.contains("kesImportCert"));
        assert!(ipc.contains("kesPushStart"));
        assert!(ipc.contains("kesRotationStatus"));
    }

    #[test]
    fn tc_fe_010_types_expose_kes_models() {
        let types = include_str!("../../src/lib/types.ts");
        assert!(types.contains("export interface KesStatus"));
        assert!(types.contains("export interface KesSignRequest"));
    }

    #[test]
    fn tc_fe_011_machine_manager_exposes_runtime_actions() {
        let mm = include_str!("../../src/pages/MachineManager.tsx");
        let ipc = include_str!("../../src/lib/ipc.ts");
        assert!(mm.contains("Apply Runtime Config"));
        assert!(mm.contains("Restart Runtime"));
        assert!(mm.contains("runtimeConfigTasks"));
        assert!(mm.contains("runtimeRestartTasks"));
        assert!(mm.contains("does not run deploy or Mithril flows"));
        assert!(ipc.contains("runtimeApplyConfig"));
        assert!(ipc.contains("runtimeConfigStatus"));
        assert!(ipc.contains("runtimeRestart"));
        assert!(ipc.contains("runtimeRestartStatus"));
    }

    #[test]
    fn tc_fe_012_ipc_exposes_kes_rotation_commands() {
        let ipc = include_str!("../../src/lib/ipc.ts");
        assert!(ipc.contains("kesPushStart"));
        assert!(ipc.contains("kesRotationStatus"));
    }

    #[test]
    fn tc_fe_014_kes_manager_page_exists() {
        let page = include_str!("../../src/pages/KesManager.tsx");
        assert!(page.contains("KES Rotate"));
        assert!(page.contains("kesStatusAll()"));
        assert!(page.contains("kesGenerate("));
        assert!(page.contains("kesImportCert("));
        assert!(page.contains("kesPushStart("));
        assert!(page.contains("kesRotationStatus("));
        assert!(page.contains("Generate KES"));
        assert!(page.contains("Import Cert"));
        assert!(page.contains("Push to BP"));
        assert!(page.contains("Type pool ticker"));
        assert!(page.contains("Confirm KES Push"));
        assert!(page.contains("Step 4 · 校验完成"));
    }

    #[test]
    fn tc_fe_015_upgrade_wizard_page_exists() {
        let page = include_str!("../../src/pages/UpgradeWizard.tsx");
        assert!(page.contains("Upgrade Wizard"));
        assert!(page.contains("upgradeStart("));
        assert!(page.contains("upgradeStatus("));
        assert!(page.contains("upgradeConfirmNext("));
        assert!(page.contains("upgradeRollback("));
        assert!(page.contains("listen<UpgradeGateEvent>(\"upgrade:gate\""));
        assert!(page.contains("TaskLogStream"));
        assert!(page.contains("Confirm Next Step"));
        assert!(page.contains("Rollback"));
        assert!(page.contains("unlock BP upgrade"));
    }

    #[test]
    fn tc_fe_013_ipc_exposes_upgrade_commands() {
        let ipc = include_str!("../../src/lib/ipc.ts");
        assert!(ipc.contains("upgradeStart"));
        assert!(ipc.contains("upgradeStatus"));
        assert!(ipc.contains("upgradeConfirmNext"));
        assert!(ipc.contains("upgradeRollback"));
    }

    #[test]
    fn tc_evt_004_upgrade_gate_contract_exposed() {
        let types = include_str!("../../src/lib/types.ts");
        let upgrade = include_str!("commands/upgrade.rs");
        assert!(types.contains("export interface UpgradeGateEvent"));
        assert!(upgrade.contains("\"upgrade:gate\""));
        assert!(upgrade.contains("completed_machine"));
        assert!(upgrade.contains("next_machine"));
    }

    #[test]
    fn tc_fe_016_ipc_exposes_recent_tasks() {
        let ipc = include_str!("../../src/lib/ipc.ts");
        let types = include_str!("../../src/lib/types.ts");
        assert!(ipc.contains("taskRecentList"));
        assert!(types.contains("export interface RecentTaskSummary"));
    }

    #[test]
    fn tc_fe_017_frontend_error_helpers_exist() {
        let errors = include_str!("../../src/lib/errors.ts");
        assert!(errors.contains("export function toUserError"));
        assert!(errors.contains("export function formatTaskError"));
        assert!(errors
            .contains("permission denied while trying to connect to the docker daemon socket"));
        assert!(errors.contains("network.socket.connect"));
    }

    #[test]
    fn tc_fe_018_pages_use_user_error_helpers() {
        let deploy = include_str!("../../src/pages/DeployWizard.tsx");
        let dashboard = include_str!("../../src/pages/Dashboard.tsx");
        let machine = include_str!("../../src/pages/MachineManager.tsx");
        assert!(deploy.contains("toUserError"));
        assert!(deploy.contains("formatTaskError"));
        assert!(dashboard.contains("formatTaskError"));
        assert!(machine.contains("toUserError"));
        assert!(machine.contains("formatTaskError"));
    }

    #[test]
    fn tc_fe_019_dashboard_truncates_recent_task_errors() {
        let dashboard = include_str!("../../src/pages/Dashboard.tsx");
        assert!(dashboard.contains("近期操作日志"));
        assert!(dashboard.contains("最近 {Math.min(recentTasks.length, 6)} 条"));
        assert!(dashboard.contains("title={detailText}"));
    }

    #[test]
    fn tc_fe_020_dashboard_guards_other_long_text_blocks() {
        let dashboard = include_str!("../../src/pages/Dashboard.tsx");
        assert!(dashboard.contains("role=\"tooltip\""));
        assert!(dashboard.contains("max-w-[min(28rem,90vw)]"));
        assert!(dashboard.contains("group-hover:opacity-100"));
        assert!(dashboard.contains("group-focus-visible:opacity-100"));
    }

    #[test]
    fn tc_fe_021_ipc_exposes_pool_onchain_query() {
        let ipc = include_str!("../../src/lib/ipc.ts");
        assert!(ipc.contains("poolOnchainStatus"));
        assert!(ipc.contains("pool_onchain_status"));
        assert!(ipc.contains("poolBindOnchain"));
        assert!(ipc.contains("poolRegistrationPrepare"));
        assert!(ipc.contains("pool_registration_prepare"));
        assert!(ipc.contains("poolRegistrationSubmit"));
        assert!(ipc.contains("pool_registration_submit"));
        assert!(ipc.contains("poolRefreshBoundOnchain"));
        assert!(ipc.contains("poolUnbindOnchain"));
        assert!(ipc.contains("pool_unbind_onchain"));
    }

    #[test]
    fn tc_fe_022_types_expose_pool_onchain_models() {
        let types = include_str!("../../src/lib/types.ts");
        assert!(types.contains("export interface Pool {"));
        assert!(types.contains("onchain_pool_id: string | null;"));
        assert!(types.contains("onchain_registered: boolean;"));
        assert!(types.contains("export interface PoolOnchainQueryPayload"));
        assert!(types.contains("export interface PoolBindOnchainPayload"));
        assert!(types.contains("export interface PoolRegistrationPreparePayload"));
        assert!(types.contains("export interface PoolRegistrationPrepareResult"));
        assert!(types.contains("export interface PoolRegistrationSubmitPayload"));
        assert!(types.contains("export interface PoolRegistrationSubmitResult"));
        assert!(types.contains("export interface PoolOnchainRelay"));
        assert!(types.contains("export interface PoolOnchainRegistration"));
        assert!(types.contains("export interface PoolOnchainStatus"));
        assert!(types.contains("missing_requirements: string[]"));
    }

    #[test]
    fn tc_fe_023_pool_status_page_wires_onchain_query() {
        let page = include_str!("../../src/pages/PoolRegistrationStatus.tsx");
        assert!(page.contains("poolOnchainStatus({"));
        assert!(page.contains("poolBindOnchain({"));
        assert!(page.contains("Query On-chain Status"));
        assert!(page.contains("Bind Pool To Workspace"));
        assert!(page.contains("registered on-chain"));
        assert!(page.contains("not registered"));
        assert!(page.contains("Registered Parameters"));
        assert!(page.contains("cold_vkey_path"));
    }

    #[test]
    fn tc_fe_024_app_wires_pool_binding_and_background_refresh() {
        let app = include_str!("../../src/App.tsx");
        assert!(app.contains("<Dashboard"));
        assert!(app.contains("<Dashboard />"));
        assert!(!app.contains("onPoolRefreshed={(nextPool) => {"));
        assert!(!app.contains("path=\"/pool-status\""));
    }

    #[test]
    fn tc_fe_025_dashboard_owns_pool_binding_ui() {
        let dashboard = include_str!("../../src/pages/Dashboard.tsx");
        assert!(!dashboard.contains("Bound On-chain Pool"));
        assert!(!dashboard.contains("Bind Existing Pool"));
        assert!(!dashboard.contains("Register New Pool"));
        assert!(!dashboard.contains("<PoolRegistrationStatus"));
        assert!(!dashboard.contains("<PoolRegistrationWizard"));
    }

    #[test]
    fn tc_fe_027_registration_wizard_wires_prepare_and_submit() {
        let page = include_str!("../../src/pages/PoolRegistrationWizard.tsx");
        assert!(page.contains("poolRegistrationPrepare({"));
        assert!(page.contains("poolRegistrationSubmit({"));
        assert!(page.contains("Prepare Registration"));
        assert!(page.contains("Submit Registration"));
        assert!(page.contains("Confirm Pool ID"));
        assert!(page.contains("offline_signing_required"));
        assert!(page.contains("confirm_pool_id"));
        assert!(page.contains("Cold / Hot Workflow"));
        assert!(page.contains("pool-registration.cert"));
        assert!(page.contains("This file is not generated on the hot node"));
        assert!(page.contains("Cold Environment Inputs"));
        assert!(page.contains("stake-pool registration-certificate"));
        assert!(page.contains("transaction sign"));
        assert!(page.contains("--cold-verification-key-file cold.vkey"));
        assert!(page.contains("networkFlag("));
        assert!(page.contains("Copy the resulting certificate to the hot node path"));
        assert!(page.contains("Copy the signed tx back to the hot node path"));
    }

    #[test]
    fn tc_task_002_task_commands_registered() {
        let commands = include_str!("commands/mod.rs");
        let lib = include_str!("lib.rs");
        assert!(commands.contains("pub mod task;"));
        assert!(lib.contains("commands::task::task_recent_list"));
    }

    #[test]
    fn tc_obs_002_observability_commands_registered_and_telemetry_page_has_triggers() {
        let commands = include_str!("commands/mod.rs");
        let lib = include_str!("lib.rs");
        let dashboard = include_str!("../../src/pages/Dashboard.tsx");
        let telemetry = include_str!("../../src/pages/TelemetryApi.tsx");
        assert!(commands.contains("pub mod observability;"));
        assert!(lib.contains("commands::observability::observability_gateway_status"));
        assert!(lib.contains("commands::observability::observability_bootstrap_start"));
        assert!(lib.contains("commands::observability::observability_rollback_start"));
        assert!(telemetry.contains("Enable API"));
        assert!(telemetry.contains("Rollback"));
        assert!(dashboard.contains("管理 API"));
        assert!(dashboard.contains("observabilityGatewayStatus("));
    }

    #[test]
    fn tc_obs_012_dashboard_polling_orchestration_supports_visibility_interval_switch() {
        let dashboard = include_str!("../../src/pages/Dashboard.tsx");
        assert!(dashboard.contains("useDashboardStore"));
        assert!(dashboard.contains("useMonitorStore"));
        let store = include_str!("../../src/lib/dashboardStore.ts");
        assert!(store.contains("refreshDashboardData"));
        assert!(store.contains("Promise.allSettled"));
        assert!(store.contains("startDashboardPolling"));
        let app = include_str!("../../src/App.tsx");
        assert!(app.contains("startDashboardPolling"));
        assert!(app.contains("stopDashboardPolling"));
        assert!(app.contains("refreshDashboardData"));
    }

    #[test]
    fn tc_dep_013_observability_bootstrap_playbook_targets_relay_and_gateway_role() {
        let playbook = include_str!("../../ansible/playbooks/observability-bootstrap.yml");
        assert!(playbook.contains("hosts: \"{{ target_hosts | default('relay') }}\""));
        assert!(playbook.contains("ops_metrics_basic_auth_password"));
        assert!(playbook.contains("name: ops-observability-gateway"));
    }

    #[test]
    fn tc_dep_018_deploy_playbook_includes_observability_gateway_defaults() {
        let playbook = include_str!("../../ansible/playbooks/deploy.yml");
        assert!(playbook.contains("Configure relay observability gateway"));
        assert!(playbook.contains("enable_ops_observability_gateway | default(true) | bool"));
        assert!(playbook.contains("generated_metrics_basic_auth_password"));
        assert!(playbook.contains("name: ops-observability-gateway"));
    }

    #[test]
    fn tc_dep_020_hardening_allows_relay_gateway_port_when_observability_enabled() {
        let hardening = include_str!("../../ansible/roles/hardening/tasks/main.yml");
        assert!(hardening.contains("Allow relay telemetry gateway port for relay hosts"));
        assert!(hardening
            .contains("ufw allow {{ ops_metrics_gateway_listen_port | default(443) }}/tcp"));
        assert!(hardening.contains("enable_ops_observability_gateway | default(true) | bool"));
    }

    #[test]
    fn tc_dep_021_observability_role_allows_gateway_port_via_ufw_if_available() {
        let role = include_str!("../../ansible/roles/ops-observability-gateway/tasks/main.yml");
        assert!(role.contains("Allow telemetry gateway port in UFW when available"));
        assert!(role.contains("if command -v ufw >/dev/null 2>&1; then"));
        assert!(role.contains("ufw allow {{ ops_metrics_gateway_listen_port }}/tcp"));
        assert!(role.contains("failed_when: false"));
    }

    #[test]
    fn tc_dep_019_observability_rollback_playbook_exists() {
        let playbook = include_str!("../../ansible/playbooks/observability-rollback.yml");
        assert!(playbook.contains("target_hosts | default('relay')"));
        assert!(playbook.contains("ouro-ops-metrics.conf"));
        assert!(playbook.contains("nginx -t"));
        assert!(playbook.contains("state: reloaded"));
    }

    #[test]
    fn tc_audit_001_key_operations_write_audit_log() {
        let pool = include_str!("commands/pool.rs");
        let deploy = include_str!("commands/deploy.rs");
        let runtime = include_str!("commands/runtime.rs");
        let kes = include_str!("commands/kes.rs");
        let upgrade = include_str!("commands/upgrade.rs");
        let monitor = include_str!("commands/monitor.rs");

        assert!(pool.contains("\"pool_init\""));
        assert!(pool.contains("\"pool_update\""));
        assert!(pool.contains("\"pool_registration_prepare\""));
        assert!(pool.contains("\"pool_registration_submit\""));
        assert!(pool.contains("\"pool_unbind_onchain\""));
        assert!(deploy.contains("\"deploy_start\""));
        assert!(deploy.contains("\"deploy_cancel\""));
        assert!(runtime.contains("\"runtime_apply_config_start\""));
        assert!(runtime.contains("\"runtime_restart_start\""));
        assert!(kes.contains("\"kes_generate\""));
        assert!(kes.contains("\"kes_import_cert\""));
        assert!(kes.contains("\"kes_push_start\""));
        assert!(upgrade.contains("\"upgrade_start\""));
        assert!(upgrade.contains("\"upgrade_confirm_next\""));
        assert!(upgrade.contains("\"upgrade_rollback\""));
        assert!(monitor.contains("\"telemetry_degraded_retry\""));
    }
}
