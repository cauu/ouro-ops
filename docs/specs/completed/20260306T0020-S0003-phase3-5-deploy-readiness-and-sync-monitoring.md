# Cardano Stake Pool 控制平面 Phase 3.5 部署就绪与同步监控规范

Spec-ID：`S0003`
状态：`completed`
创建时间：`2026-03-06`
开始时间：`2026-03-06T0020`
完成时间：`2026-03-07T0000`
前一个 Spec-ID：`S0002`
结项原因：`delivered`

## 1. 需求详情

- 背景
  - 当前部署链路会在容器启动成功后持续等待 `syncProgress == 100.00`，这会把“部署成功”和“完全同步完成”绑定在一起。
  - 对于未迁移数据库的新节点，这会导致部署任务长时间阻塞，即使节点已经成功启动并开始同步。
  - 同步过程目前缺少“同步速度”维度的可视化与监控，无法判断节点是否正常追块、是否卡住、何时能完成同步。
- 范围
  - 调整部署完成判定：节点成功启动、socket 可用、`query tip` 可返回并进入同步状态时，即视为部署成功。
  - 将“完全同步到 100%”从部署成功条件中移出，改由监控链路持续观察。
  - 增加同步速度监控，包括同步进度、区块高度增长速度、趋势与基础状态展示。
  - 为后续 Phase 4 的完整监控体系打基础，但本 spec 只聚焦部署完成判定与同步速度监控。
- 约束
  - 不回退已完成的 Phase 1 至 Phase 3 基础能力。
  - 继续沿用 local-first 架构：Tauri + Rust + SQLite + Python sidecar + Ansible。
  - SQLite 不引入外键与级联。
  - 同一时间只能有一个活动 spec。
- 非目标
  - 不在本 spec 中实现完整的 KES 生命周期管理。
  - 不在本 spec 中实现完整的升级/回退工作流。
  - 不改写历史完成归档或旧参考文档。

## 2. 概要设计

- 架构 / 受影响模块
  - `ansible/roles/cardano-node`：调整部署完成判定，不再等待 `syncProgress == 100.00`。
  - `src-tauri/src/commands/deploy.rs` 与任务状态更新链路：将“已进入同步”视为成功完成的可接受条件。
  - `src-tauri/src/commands/monitor.rs` 或等效监控入口：补充同步速度与同步状态采集。
  - 前端 Dashboard 或部署相关页面：展示同步进度与同步速度。
- 数据模型与接口
  - 可复用 `machine_health` 记录同步进度、区块高度、采样时间与推导出的同步速度。
  - 部署成功判定至少要求：容器启动成功、`/ipc/node.socket` 可用、`cardano-cli query tip` 返回有效 JSON。
  - 同步速度建议按连续两次采样的区块高度差 / 时间差计算，并附带 `syncProgress` 变化。
- 风险与回退策略
  - 若部署成功条件放宽，必须同步增加“节点进入同步态”的检查，避免把异常启动误判为成功。
  - 若同步速度监控实现不稳定，允许先记录原始采样值，再逐步引入速率计算。

## 3. 执行计划

- [x] `p35-1` 调整 `cardano-node` 部署完成判定，使“容器启动 + socket 可用 + query tip 成功”即可完成部署
- [x] `p35-2` 在部署链路中记录节点已进入同步状态的初始观测值
- [x] `p35-3` 扩展监控采集，记录 `syncProgress`、区块高度与采样时间
- [x] `p35-4` 增加同步速度计算逻辑，输出基础速率指标与异常判断依据
- [x] `p35-5` 在前端展示同步速度、同步进度与基础同步状态
- [x] `p35-6` 为部署完成判定和同步监控补充自动化或脚本化验证
- [x] `p35-7` 修复 SSH 监控与运行时探测在非 root 用户下访问 Docker daemon socket 的权限问题
- [x] `p35-8` 修复 mainnet topology 生成逻辑，给 relay/BP 注入 bootstrap peers，避免节点孤岛启动后停在 block 0
- [x] `p35-9` 修复 relay topology 误将 BP 作为 local root peer 的问题；单 relay 场景下 relay 的 `localRoots` 应为空
- [x] `p35-10` 修复 BP topology 不应包含 bootstrap peers 的问题，避免 BP 绕过 relay 直接从公网同步
- [x] `p35-11` 修复 BP bootstrap mode 下 relay peer 必须为 `trustable` 且部署重新下发 `config/topology` 后必须显式重启容器使新配置生效

## 4. 测试与验收标准

- `TC-P35-001` 未迁移数据库时，节点从零同步，部署任务在节点成功启动并可返回 `query tip` 后可完成，不再阻塞到 `100.00`。
- `TC-P35-002` 已迁移数据库时，部署任务在节点恢复运行并进入同步态后可完成。
- `TC-P35-003` 监控链路可记录并展示 `syncProgress`、区块高度与采样时间。
- `TC-P35-004` 系统可计算并展示同步速度，至少包含一个稳定可读的速率指标。
- `TC-P35-005` 若节点已启动但同步无进展，监控侧能暴露“速度异常或停滞”的基础判断信息。
- `TC-P35-006` 当 SSH 用户依赖无密码 sudo 访问 Docker 时，运行时探测与同步监控仍可成功执行 Docker 命令。
- `TC-P35-007` mainnet 部署生成的 topology 必须包含有效 bootstrap peers，relay 不得以空上游拓扑启动。
- `TC-P35-008` relay topology 不得将 BP 写入 `localRoots`；单 relay 场景下 `localRoots` 应为空数组，仅通过 bootstrap peers 接入主网。
- `TC-P35-009` BP topology 不得包含 bootstrap peers；BP 只能通过 relay 的 `localRoots` 获取链数据。
- `TC-P35-010` BP topology 中 relay `localRoots` 必须标记为 `trustable: true`，使 BP 在 bootstrap mode 下可通过 relay 推进同步。
- `TC-P35-011` 当部署重新渲染 `config.json`、genesis 或 `topology.json` 时，`cardano-node` 容器必须自动重启，避免新配置未生效。

## 5. 执行日志（仅追加）

- `2026-03-06` 新增本 spec，用于插入在 Phase 3 与 Phase 4 之间的过渡工作。
- `2026-03-06` 需求来源：实际部署过程中发现“等待同步到 100%”使部署任务阻塞过久，且缺少同步速度监控。
- `2026-03-06` 当前工作区存在其他未提交改动，因此本次 spec 文档变更不自动提交。
- `2026-03-06` `p35-1` 完成：Ansible 部署完成条件从“等待 `syncProgress == 100.00`”改为“socket 可用且 `cardano-cli query tip` 成功返回”。
- `2026-03-06` `p35-2` 完成：部署链路在首个 `query tip` 成功后记录 `block`、`syncProgress`、`era`、`hash` 作为初始同步观测值。
- `2026-03-06` `p35-3` 完成：新增 Tauri `monitor_snapshot` 命令，按机器采集 `query tip`，落库到 `machine_health`。
- `2026-03-06` `p35-4` 完成：基于连续采样的区块高度差与时间差计算 `blocks_per_minute`，并增加 5 分钟无进展的停滞判断。
- `2026-03-06` `p35-5` 完成：Dashboard 增加同步监控区域，展示同步进度、块高、速度、状态与异常说明。
- `2026-03-06` `p35-6` 完成：补充 Rust 单测、前端静态断言、Ansible 语法检查与前端构建验证。
- `2026-03-06` `p35-7` 开始：修复 `machine_runtime_probe` 与 `monitor_snapshot` 通过 SSH 执行 Docker 命令时未回退到 `sudo -n` 的权限问题。
- `2026-03-06` `p35-7` 完成：SSH 侧 Docker 命令统一增加 `sudo -n` 回退，覆盖运行时探测与同步监控对 Docker daemon socket 的访问。
- `2026-03-07` `p35-8` 开始：修复 `topology-p2p.json.j2` 在 mainnet relay 场景未注入 bootstrap peers，导致 relay/BP 形成孤岛、节点停在 `block 0 / syncProgress 0.00`。
- `2026-03-07` `p35-8` 完成：mainnet topology 模板增加 backbone bootstrap peers，relay/BP 都生成非空 `bootstrapPeers`，并将 `publicRoots` 收敛为显式空 accessPoints 结构。
- `2026-03-07` `p35-9` 开始：根据 relay 实际日志修复 topology 仍将 BP 写入 relay `localRoots` 的问题；该错误会让 relay 只与 BP 建立 hot connection，无法真正接入主网。
- `2026-03-07` `p35-9` 完成：relay topology 改为仅连接其他 relay；单 relay 场景下 `localRoots` 为空数组，不再把 BP 作为 relay 的 local root peer。
- `2026-03-07` `p35-10` 开始：根据运行态观察修复 BP topology 仍携带 bootstrap peers 的问题；该错误会让 BP 绕过 relay 直接从公网同步，破坏 `1 relay + 1 bp` 的预期链路。
- `2026-03-07` `p35-10` 完成：BP topology 的 `bootstrapPeers` 改为空数组，BP 仅通过 relay 的 `localRoots` 获取链数据。
- `2026-03-07` `p35-11` 开始：根据 BP 手动重启后的报错修复 bootstrap mode 下“缺少 trustable peers”问题，并补上 deploy 重新下发配置后未显式重启容器导致新 topology 未生效的问题。
- `2026-03-07` `p35-11` 完成：BP topology 中 relay `localRoots` 改为 `trustable: true`；部署链路为 `config/genesis/topology` 的变更计算 `cardano_runtime_config_changed`，并在启动 `cardano-node` 时显式 `restart` 使配置变更即时生效。

## 6. 验证证据（仅追加）

- `2026-03-06` 暂无实现级验证证据。本 spec 创建完成后，后续将按 `TC-P35-*` 逐项追加。
- `2026-03-06` `TC-P35-001 | stack: rust | command: cargo test -q | result: pass | note: tc_dep_006 断言 playbook 已去除对 syncProgress == 100.00 的阻塞等待，仅保留首个 query tip 成功条件`
- `2026-03-06` `TC-P35-002 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: 部署链路语法有效，首个 tip 观测记录步骤已纳入 playbook`
- `2026-03-06` `TC-P35-003 | stack: rust | command: cargo test -q | result: pass | note: tc_mon_001 覆盖 tip 解析；monitor_snapshot 将 syncProgress、block_height 与采样时间写入 machine_health`
- `2026-03-06` `TC-P35-004 | stack: rust | command: cargo test -q | result: pass | note: tc_mon_002 覆盖 blocks_per_minute 计算；Dashboard 展示 Blocks/min`
- `2026-03-06` `TC-P35-005 | stack: rust | command: cargo test -q | result: pass | note: tc_mon_003 覆盖 5 分钟无块高增长的 stalled 判断；Dashboard 展示 stalled/unreachable 状态`
- `2026-03-06` `TC-P35-003 | stack: node | command: pnpm build | result: pass | note: Dashboard 同步监控 UI 编译通过`
- `2026-03-06` `TC-P35-006 | stack: rust | command: cargo test -q | result: pass | note: tc_mch_012/tc_mch_013/tc_mon_004 断言 SSH 侧 Docker 命令会包装为 sudo -n 回退，非 Docker 命令保持不变`
- `2026-03-06` `TC-P35-006 | stack: node | command: pnpm build | result: pass | note: 修复后前端监控入口保持可构建`
- `2026-03-06` `TC-P35-006 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: 本次修复未破坏部署 playbook 语法`
- `2026-03-07` `TC-P35-007 | stack: rust | command: cargo test -q | result: pass | note: tc_dep_007 断言 mainnet topology 模板包含 bootstrapPeers 和 backbone peers`
- `2026-03-07` `TC-P35-007 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: topology 模板调整后 playbook 语法仍然有效`
- `2026-03-07` `TC-P35-007 | stack: node | command: pnpm build | result: pass | note: 本次 topology 修复未影响前端构建`
- `2026-03-07` `TC-P35-008 | stack: rust | command: cargo test -q | result: pass | note: tc_dep_008 断言 relay topology 使用 relay_upstreams 且不再遍历 bp_nodes 生成 localRoots`
- `2026-03-07` `TC-P35-008 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: relay localRoots 改为空数组/其他 relay 后 playbook 语法仍有效`
- `2026-03-07` `TC-P35-008 | stack: node | command: pnpm build | result: pass | note: 本次 relay topology 修复未影响前端构建`
- `2026-03-07` `TC-P35-009 | stack: rust | command: cargo test -q | result: pass | note: tc_dep_009 断言 BP topology 的 bootstrapPeers 为 [] 且仍通过 relay_nodes 生成 localRoots`
- `2026-03-07` `TC-P35-009 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: BP topology 去除 bootstrap peers 后 playbook 语法仍有效`
- `2026-03-07` `TC-P35-009 | stack: node | command: pnpm build | result: pass | note: 本次 BP topology 修复未影响前端构建`
- `2026-03-07` `TC-P35-010 | stack: rust | command: cargo test -q | result: pass | note: tc_dep_009 断言 BP topology 仍仅通过 relay_nodes 生成 localRoots，且 relay peer 被标记为 trustable`
- `2026-03-07` `TC-P35-010 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: BP relay peer 标记为 trustable 后 playbook 语法仍有效`
- `2026-03-07` `TC-P35-011 | stack: rust | command: cargo test -q | result: pass | note: tc_dep_010 断言 playbook 会在 config/genesis/topology 变化时设置 cardano_runtime_config_changed 并显式 restart 容器`
- `2026-03-07` `TC-P35-011 | stack: ansible | command: ANSIBLE_LOCAL_TEMP=/tmp/ansible-local ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: 显式 restart cardano-node 容器后 playbook 语法仍有效`
- `2026-03-07` `TC-P35-011 | stack: node | command: pnpm build | result: pass | note: 本次 deploy restart 逻辑修复未影响前端构建`

## 7. 变更记录（仅追加）

- `2026-03-06` 本 spec 插入在已完成的 Phase 1 至 Phase 3 归档与下一阶段 Phase 4 草稿之间，作为新的唯一活动 spec。
- `2026-03-07` 用户确认 Phase 3.5 达成目标：deploy 完成判定与同步监控已稳定，`1 relay + 1 bp` 拓扑可正常建立并同步；当前 spec 结项为 `completed`，新的活动 spec 切换到 Mithril 初始化主题。
