# Cardano Stake Pool 控制平面 Phase 3.6 Mithril 初始化与冷启动加速规范

状态：`active`  
日期：`2026-03-07`

## 1. 需求详情

- 背景
  - 当前 `1 relay + 1 bp` 最小拓扑已经打通，relay 与 bp 均可正常建立连接并开始同步区块。
  - 在数据库为空的冷启动场景下，从 `block 0` 开始全量同步主网耗时很长，影响部署体验与验证效率。
  - blinklabs `docker-cardano-node` 镜像已内置 Mithril snapshot restore 能力，但 app 尚未把该能力收敛成明确、可观测、可验收的产品行为。
- 范围
  - 明确 Mithril 在冷启动部署中的默认启用策略。
  - 在 deploy 链路中暴露并落库 Mithril 初始化相关状态。
  - 为 relay / bp 区分 Mithril 策略与验收路径。
  - 为 Mithril 失败、回退到普通同步、已有数据库跳过恢复等路径补齐可观测性与错误提示。
- 约束
  - 继续沿用 Tauri + Rust + SQLite + Python sidecar + Ansible 的既有架构。
  - SQLite 不引入外键与级联。
  - 不破坏当前已验证通过的 `1 relay + 1 bp` 最小拓扑。
  - 需求变更与 bugfix 继续追加到本 spec，直到用户明确确认结项。
- 非目标
  - 不在本 spec 中实现完整的 KES 生命周期校验与轮换。
  - 不在本 spec 中实现完整的升级/回退工作流。
  - 不重写已完成的 Phase 3.5 或 Phase 4 草稿。

## 2. 概要设计

- 架构 / 受影响模块
  - `src-tauri/src/commands/deploy.rs`：补充 Mithril 相关 payload 默认值、归一化与任务状态语义。
  - `ansible/roles/cardano-node`：在冷启动场景下注入或关闭 `RESTORE_SNAPSHOT`，并增加初始化阶段的状态采集。
  - `src-tauri/src/commands/monitor.rs` 与 Dashboard：展示节点处于 snapshot restore、普通同步、失败回退等状态。
  - `machine_health` 或等效监控快照：记录 Mithril 初始化的阶段、最近观测时间与错误摘要。
- 数据模型与接口
  - deploy 参数需要明确区分“数据库为空时是否启用 Mithril”与“已有数据库时是否跳过恢复”。
  - 监控快照需至少能区分：`snapshot_restoring`、`syncing`、`stalled`、`unreachable`。
  - 任务日志应能输出 Mithril 恢复开始、成功、失败与回退到普通同步的关键事件。
- 风险与回退策略
  - Mithril restore 不应覆盖已有数据库；若检测到已有有效数据，应显式跳过。
  - 恢复失败时必须给出明确错误或状态，而不是仅表现为“同步仍然很慢”。
  - 若镜像内置行为与 app 策略不一致，应以 app 明确控制和可观测性优先。

## 3. 执行计划

- [x] `p36-0` 初始化 Mithril active spec，切换文档入口，并将 Phase 3.5 结转为 completed
- [x] `p36-1` 明确 Mithril 默认策略，并在 deploy payload / UI / Ansible 之间统一 relay 与 bp 的默认值
- [x] `p36-2` 在 deploy 链路中区分“空数据库触发 restore”和“已有数据库跳过 restore”
- [x] `p36-3` 增加 Mithril 初始化状态采集与任务日志输出
- [x] `p36-4` 在监控与 Dashboard 中展示 snapshot restore / 回退同步状态
- [x] `p36-5` 为 Mithril 失败、超时与回退普通同步补齐错误处理
- [ ] `p36-6` 补充自动化验证与联调验收记录
- [x] `p36-7` 补充 machine_health migration 自动化覆盖，并整理真实环境 Mithril 联调步骤

## 4. 测试与验收标准

- `TC-P36-000` 新 spec 已成为唯一 active spec，README 入口已切换，Phase 3.5 已按用户确认结转为 completed。
- `TC-P36-001` 空数据库冷启动时，按默认策略正确启用或关闭 Mithril，且 relay / bp 行为符合设计。
- `TC-P36-002` 已有数据库时，deploy 不应误触发 Mithril restore。
- `TC-P36-003` 任务日志与监控链路可区分 `snapshot_restoring`、`syncing`、`restore_failed` 等状态。
- `TC-P36-004` Mithril restore 失败时，用户能看到明确错误或状态，而不是仅看到长期 `0%`。
- `TC-P36-005` Dashboard 可展示 Mithril 初始化阶段及恢复后的同步进度。
- `TC-P36-006` 自动化测试与联调记录覆盖默认策略、跳过恢复、失败处理和 UI 展示。

## 5. 执行日志（仅追加）

- `2026-03-07` 基于用户确认的下一步需求创建本 spec，用于把镜像内置 Mithril 能力收敛成可配置、可观测、可验收的产品能力。
- `2026-03-07` `p36-0` 完成：新建 Mithril active spec，切换 `docs/README.md` 入口，并将 Phase 3.5 spec 标记为 `completed`。
- `2026-03-07` `p36-1` 完成：`restore_snapshot` 调整为可缺省并由后端按网络兜底默认；DeployWizard 新增 Mithril 冷启动恢复开关，默认在 `mainnet/preprod` 启用、在 `preview` 关闭。
- `2026-03-07` `p36-2` 完成：Ansible 通过 `/opt/cardano/db/protocolMagicId` 判断数据库是否已初始化，仅在“支持 Mithril 的网络 + payload 允许 + DB 未初始化”时才有效开启 `RESTORE_SNAPSHOT`，并输出决策原因。
- `2026-03-07` `p36-3` 完成：monitor 增加对 `RESTORE_SNAPSHOT` 环境变量、`protocolMagicId` 和最近日志的采集，推断 `snapshot_restoring` / `restore_failed` 等阶段，并将 `sync_stage`、`sync_note` 落库到 `machine_health`。
- `2026-03-07` `p36-4` 完成：Dashboard 新增 `Snapshot Restore` 和 `Sync Stage` 展示，支持把 `snapshot_restoring` / `restore_failed` / `syncing` 等阶段直接呈现给用户。
- `2026-03-07` `p36-5` 完成：monitor 新增 `restore_timeout` 和 `fallback_syncing`，分别用于表达 Mithril 长时间无进展和 restore 失败后退回普通同步；Dashboard 对这两类状态给出明确提示。
- `2026-03-07` `p36-7` 完成：补充 `machine_health` migration 002 的自动化断言，确认 `sync_stage`/`sync_note` 字段被正确创建；同时在 spec 中写入真实环境 Mithril 联调步骤，保持 `p36-6` 继续等待用户执行后的真实结果。

## 6. 验证证据（仅追加）

- `2026-03-07` `TC-P36-000 | stack: other | command: manual inspection of docs/specs and docs/README.md | result: pass | note: README 当前入口已切换到 2026-03-07-phase3-6-mithril-bootstrap-active.md，Phase 3.5 已标记为 completed`
- `2026-03-07` `TC-P36-001 | stack: rust | command: cargo test -q | result: pass | note: deploy payload 归一化会在 mainnet/preprod 默认启用 restore_snapshot，在 preview 默认关闭，并保留显式覆盖值`
- `2026-03-07` `TC-P36-001 | stack: node | command: pnpm build | result: pass | note: DeployWizard 新增 Mithril 冷启动恢复开关，默认值随网络变化且前端构建通过`
- `2026-03-07` `TC-P36-002 | stack: rust | command: cargo test -q | result: pass | note: tc_dep_011 断言 playbook 基于 protocolMagicId 计算 cardano_restore_snapshot_effective，避免已有数据库误触发 restore`
- `2026-03-07` `TC-P36-002 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: 新增 Mithril 决策分支后 deploy playbook 语法仍有效`
- `2026-03-07` `TC-P36-003 | stack: rust | command: cargo test -q | result: pass | note: tc_mon_005/tc_mon_006/tc_mon_007 覆盖 snapshot_restoring、restore_failed 与 RESTORE_SNAPSHOT env 解析；machine_health 新增 sync_stage/sync_note 落库`
- `2026-03-07` `TC-P36-005 | stack: rust | command: cargo test -q | result: pass | note: tc_fe_006 断言 Dashboard 展示 Snapshot Restore 与 Sync Stage`
- `2026-03-07` `TC-P36-005 | stack: node | command: pnpm build | result: pass | note: Dashboard 可构建并消费新的 sync_stage / restore_snapshot_requested 字段`
- `2026-03-07` `TC-P36-004 | stack: rust | command: cargo test -q | result: pass | note: tc_mon_008/tc_mon_009 覆盖 restore_timeout 与 fallback_syncing 的推断逻辑`
- `2026-03-07` `TC-P36-004 | stack: node | command: pnpm build | result: pass | note: Dashboard 能展示 restore timeout 与 fallback syncing 的明确提示`
- `2026-03-07` `TC-P36-006 | stack: rust | command: cargo test -q | result: pass | note: tc_db_003 断言 migration 002 已为 machine_health 增加 sync_stage/sync_note；本地自动化覆盖默认策略、跳过恢复、状态推断与 UI 展示`
- `2026-03-07` `TC-P36-006 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: deploy playbook 语法在引入 Mithril 默认策略与空库门控后保持有效`
- `2026-03-07` `TC-P36-006 | stack: node | command: pnpm build | result: pass | note: 前端在新增 Mithril 开关、Sync Stage、错误提示后仍可构建`
- `2026-03-07` `TC-P36-006 | stack: other | command: manual test plan authored in this spec | result: pass | note: 已补充真实环境联调步骤；待用户在实际机器执行并追加结果`

## 6.1 联调验收步骤（待追加真实结果）

1. 空数据库冷启动，`mainnet` 或 `preprod`，保持 `restore_snapshot=true`
   - 预期：
   - deploy 完成后 Dashboard 显示 `Snapshot Restore=requested`
   - 初始阶段显示 `snapshot restoring`
   - 数据库初始化后切换为 `syncing`
2. 已有数据库的机器再次 deploy
   - 预期：
   - Dashboard 显示 `Snapshot Restore=requested` 或 payload 请求值
   - 但 `sync_stage` 不应进入 `snapshot restoring`
   - 节点直接进入普通 `syncing` 或 `synced`
3. 人为制造 Mithril 失败或错误日志
   - 预期：
   - Dashboard 显示 `restore failed`
   - 若随后节点开始普通同步，则转为 `fallback syncing`
4. 空数据库且长时间无进展
   - 预期：
   - 超过 15 分钟后出现 `restore timeout`
   - note 中给出检查日志或允许普通同步的提示

## 7. 变更记录（仅追加）

- `2026-03-07` 新建本 spec 作为 Phase 3.5 之后的唯一活动 spec；`docs/specs/2026-03-06-phase3-5-deploy-readiness-and-sync-monitoring.md` 结转为 `completed`。
