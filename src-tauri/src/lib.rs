#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod db;
mod error;
mod keychain;
mod sidecar;

use std::sync::Arc;
use std::sync::Mutex;
use tauri::Manager;

pub use db::DbState;
use crate::commands::monitor::MonitorPollingState;

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
            commands::kes::kes_status_all,
            commands::kes::kes_generate,
            commands::kes::kes_import_cert,
            commands::runtime::runtime_apply_config,
            commands::runtime::runtime_config_status,
            commands::runtime::runtime_restart,
            commands::runtime::runtime_restart_status,
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
        assert!(sidebar.contains("to=\"/machines\""));
        assert!(sidebar.contains("to=\"/deploy\""));
        assert!(sidebar.contains("to=\"/settings\""));
        assert!(sidebar.contains("Dashboard"));
        assert!(sidebar.contains("Machines"));
        assert!(sidebar.contains("Deploy"));
        assert!(sidebar.contains("Settings"));
    }

    #[test]
    fn tc_fe_003_task_log_stream_filters_by_task_id() {
        let file = include_str!("../../src/components/TaskLogStream.tsx");
        assert!(file.contains("event.payload.task_id !== taskId"));
    }

    #[test]
    fn tc_fe_004_deploy_wizard_step_submit() {
        let app = include_str!("../../src/App.tsx");
        let deploy = include_str!("../../src/pages/DeployWizard.tsx");
        assert!(app.contains("path=\"/deploy\""));
        assert!(deploy.contains("deployStart("));
        assert!(deploy.contains("useState(\"10.5.4-1\")"));
        assert!(deploy.contains("ghcr.io/blinklabs-io/cardano-node"));
        assert!(deploy.contains("takeover_existing_node"));
        assert!(deploy.contains("restore_snapshot_relay (Mithril cold-start restore)"));
        assert!(deploy.contains("restore_snapshot_bp (Mithril cold-start restore)"));
        assert!(deploy.contains("setRestoreSnapshotRelay(true)"));
        assert!(deploy.contains("setRestoreSnapshotBp(false)"));
        assert!(deploy.contains("Default is enabled for relay cold-starts"));
        assert!(deploy.contains("networkSupportsMithril"));
        assert!(deploy.contains("step === 1"));
        assert!(deploy.contains("step === 2"));
        assert!(deploy.contains("step === 3"));
        assert!(deploy.contains("Loading machines..."));
        assert!(deploy.contains("Requesting machine list from local app"));
        assert!(deploy.contains("Still waiting for machine_list response"));
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
        assert!(dashboard.contains("Blocks/min"));
        assert!(dashboard.contains("Sync Progress"));
        assert!(dashboard.contains("Snapshot Restore"));
        assert!(dashboard.contains("Sync Stage"));
        assert!(dashboard.contains("snapshot.health_level"));
        assert!(dashboard.contains("snapshot restoring"));
        assert!(dashboard.contains("restore timeout"));
        assert!(dashboard.contains("fallback syncing"));
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
        let topology = include_str!("../../ansible/roles/cardano-node/templates/topology-p2p.json.j2");
        assert!(topology.contains("\"bootstrapPeers\""));
        assert!(topology.contains("backbone.cardano.iog.io"));
        assert!(topology.contains("backbone.mainnet.emurgornd.com"));
        assert!(topology.contains("backbone.mainnet.cardanofoundation.org"));
        assert!(topology.contains("\"publicRoots\": ["));
    }

    #[test]
    fn tc_dep_008_relay_topology_excludes_bp_local_roots() {
        let topology = include_str!("../../ansible/roles/cardano-node/templates/topology-p2p.json.j2");
        assert!(topology.contains("rejectattr(\"name\", \"equalto\", inventory_hostname)"));
        assert!(!topology.contains("{% for bp in bp_nodes %}"));
        assert!(topology.contains("relay_upstreams | length"));
    }

    #[test]
    fn tc_dep_009_bp_topology_uses_relay_only() {
        let topology = include_str!("../../ansible/roles/cardano-node/templates/topology-p2p.json.j2");
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
        assert!(playbook.contains("restart: \"{{ cardano_runtime_config_changed | default(false) }}\""));
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
        assert!(playbook.contains("when: not (cardano_restore_snapshot_effective | default(false) | bool)"));
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
        let template = include_str!("../../ansible/roles/cardano-node/templates/config-mainnet-10.5.4-1.json.j2");
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
    fn tc_fe_009_ipc_exposes_kes_commands() {
        let ipc = include_str!("../../src/lib/ipc.ts");
        assert!(ipc.contains("kesStatusAll"));
        assert!(ipc.contains("kesGenerate"));
        assert!(ipc.contains("kesImportCert"));
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
}
