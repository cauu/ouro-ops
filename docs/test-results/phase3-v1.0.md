# Phase 3 验证记录（Baseline）

日期：2026-03-03

## 1. 自动化结果

- `cd src-tauri && cargo test -q`
  - 结果：39 passed, 0 failed
- `pnpm build`
  - 结果：构建成功
- `make phase3-verify`（去除关键路径 `ignore_errors` 后复验）
  - 结果：通过（Rust tests + Frontend build）

## 2. 用例映射（当前已覆盖）

| 用例 | 自动化覆盖 |
| :--- | :--- |
| TC-DEP-001 | `commands::deploy::tests::tc_dep_001_payload_validation` |
| TC-DEP-002 | `commands::deploy::tests::tc_dep_002_insert_task_and_task_machine_rows` |
| TC-DEP-003 | `commands::deploy::tests::tc_dep_003_deploy_status_reads_db` |
| TC-DEP-004 | `commands::deploy::tests::tc_dep_004_cancel_running_task` |
| TC-DEP-005 | `commands::deploy::tests::tc_dep_005_preflight_disk_insufficient` |
| TC-DEP-006 | `commands::deploy::tests::tc_dep_006_mark_failed_and_success_transitions` |
| TC-INV-001 | `commands::deploy::tests::tc_inv_001_inventory_contains_groups_and_hostvars` |
| TC-EVT-001/002/003 | `sidecar::runner::tests::tc_evt_payloads_and_error_mapping` |
| TC-FE-003 | `frontend_tests::tc_fe_003_task_log_stream_filters_by_task_id` |
| TC-FE-004 | `frontend_tests::tc_fe_004_deploy_wizard_step_submit` |

## 3. 待补充验证

- TC-INV-002（真实 ansible-runner + 目标主机执行路径）仍需在联调环境做端到端验证。
- 部署取消在真实长任务下的中断与恢复，建议补一条集成脚本。
- 已去除 `ansible` 关键执行路径中的 `ignore_errors`，部署改为 fail-fast。
