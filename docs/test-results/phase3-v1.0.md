# Phase 3 验证记录（v1.0）

日期：2026-03-05

## 1. 自动化结果

- `cd src-tauri && cargo test -q`
  - 结果：47 passed, 0 failed
- `pnpm build`
  - 结果：构建成功

## 2. 用例映射（自动化）

| 用例 | 自动化覆盖 |
| :--- | :--- |
| TC-DEP-001 | `commands::deploy::tests::tc_dep_001_payload_validation` |
| TC-DEP-002 | `commands::deploy::tests::tc_dep_002_insert_task_and_task_machine_rows` |
| TC-DEP-003 | `commands::deploy::tests::tc_dep_003_deploy_status_reads_db` |
| TC-DEP-004 | `commands::deploy::tests::tc_dep_004_cancel_running_task` |
| TC-DEP-005 | `commands::deploy::tests::tc_dep_005_preflight_disk_insufficient` |
| TC-DEP-006 | `commands::deploy::tests::tc_dep_006_mark_failed_and_success_transitions` |
| TC-DEP-007 | `commands::deploy::tests::tc_dep_007_minimum_topology_requires_relay_and_bp` |
| P3-默认值回填 | `commands::deploy::tests::tc_dep_010_normalize_payload_defaults` |
| P3-覆盖优先 | `commands::deploy::tests::tc_dep_011_normalize_payload_keeps_overrides` |
| P3-digest透传 | `commands::deploy::tests::tc_dep_008_extra_vars_contains_safe_validation_mode` |
| TC-INV-001 | `commands::deploy::tests::tc_inv_001_inventory_contains_groups_and_hostvars` |
| TC-EVT-001/002/003 | `sidecar::runner::tests::tc_evt_payloads_and_error_mapping` |
| TC-FE-003 | `frontend_tests::tc_fe_003_task_log_stream_filters_by_task_id` |
| TC-FE-004 | `frontend_tests::tc_fe_004_deploy_wizard_step_submit`（含 `latest` + blinklabs 默认断言） |
| P3-S1 (可用性补充) | `frontend_tests::tc_fe_machine_add_key_flow_exists`、`keychain::tests::normalize_key_path_*` |
| P3-S3 runtime probe | `commands::machine::tests::tc_mch_010_runtime_probe_parses_inspect_fixtures`、`tc_mch_011_runtime_probe_returns_absent_when_container_missing` |

## 3. blinklabs 对齐检查（代码级）

- 默认镜像已切换：`ghcr.io/blinklabs-io/cardano-node:latest`
  - 后端默认：`src-tauri/src/commands/deploy.rs`
  - 前端默认：`src/pages/DeployWizard.tsx`
  - DB 默认：`src-tauri/migrations/001_init.sql`
- Ansible 启动语义已对齐 `run` 模式：
  - `CARDANO_DATABASE_PATH=/data/db`
  - `CARDANO_SOCKET_PATH=/ipc/node.socket`
  - config/topology 使用 `/opt/cardano/config/{{ network }}/...`
- digest 优先规则已生效：
  - 有 `image_digest` 时使用 `registry@digest`
  - 否则使用 `registry:tag`
- 旧路径兼容迁移已实现：
  - 检测 `/opt/cardano/data`
  - 在 `/opt/cardano/db` 为空时一次性迁移
  - 写入 `/opt/cardano/logs/deploy-migration.log`
- `safe_validation_mode` 保持只读：
  - `ansible/playbooks/deploy.yml` 在 safe 模式只执行 `safe-validate` role
- takeover 切换托管已实现：
  - 新增 `machine_runtime_probe(machine_id)` 命令（识别镜像/挂载/compose 标签）
  - 新增 `takeover` role：迁移 `/home/cardano/*` 到 `/opt/cardano/*`（幂等）
  - `cardano-node` 角色接入切换回滚（rename legacy + 启动失败自动回滚）
  - `DeployPayload` 增加 `takeover_existing_node`、`restore_snapshot`（默认 false）

## 4. 待补充验证（联调）

- TC-INV-002（真实 ansible-runner + 目标主机执行路径）仍需在联调环境做端到端验证。
- 需在真实机器补充 takeover 联调：运行中 compose 探测、迁移、切换、故障回滚演练。
