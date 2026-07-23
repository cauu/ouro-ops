# Compose Fleet Deploy

Spec-ID: S0027
状态: active
创建时间: 2026-07-23T11:19:16+08:00
开始时间: 2026-07-23T14:44:25+08:00
完成时间:
前一个 Spec-ID: S0026
结项原因:

## 1. Requirement Details

### Background

Ouro 的 Deploy 应表示“把一组全新的 BP/Relay 主机部署为可运行的 Cardano Fleet”，
包括 Docker/Compose、时钟、防火墙、目录、拓扑、容器启动和统一验收。

当前名为 Deploy 的实现实际承担矿池注册冷签脚本和链上提交，并在 operation/parity
层保留旧 `register-build/provision/sync/start/takeover` surface，语义与本需求冲突。
S0027 不是对旧 Deploy 的迁移、兼容或增量扩展，而是一次替换：删除旧 Deploy 的
CLI、operation、Skill、网站入口、文档和专属测试，再按本 spec 从头实现 Fleet
Deploy。旧入口不保留 alias、deprecation window 或兼容转发；矿池注册如需重新提供，
必须由未来独立需求和独立命名定义。

Ouro 当前 signed deploy recommendation 指向 Blink Labs
`ghcr.io/blinklabs-io/cardano-node:11.0.1-1` 的精确 OCI tuple。该镜像包含：

- 版本化 Cardano network config 和 genesis files；
- `cardano-cli`；
- `mithril-client`、Mithril verification keys 和空 DB 自动 snapshot restore；
- cardano-node Prometheus metrics（12798）；
- `nview` 和 `txtop`。

因此 Fresh Deploy 使用镜像内置 config、genesis、Mithril 和 metrics，不在宿主机重新
下载或维护第二套配置。Ouro 只生成 topology，并选择性挂载 DB、IPC、topology 和 BP
keys 目录。

Fresh Deploy 首先把 BP 作为 non-producing Cardano node 启动：

- `CARDANO_BLOCK_PRODUCER=false`；
- BP keys 目录允许为空；
- BP 可完成 Mithril、replay、socket、tip、P2P 和 metrics readiness；
- Deploy 完成不代表已经启用 forging。

首次 KES/VRF 生成、cold-key air-gap 签发 `node.cert` 和 forging activation 属于后续
独立 BP Bootstrap 操作。KES/VRF signing keys 在 BP 上生成并留在 BP；冷环境只接收
public KES material 并返回 public `node.cert`。后续周期性 KES 更换继续使用现有 KES
Rotation。

生产环境目前存在把宿主机完整 config 目录挂到 `/opt/cardano/config` 的运行形状。
S0027 不迁移、升级或接管这些现有容器。Fresh Deploy 使用 image-owned config 与
selective mounts，避免宿主机旧 config 遮蔽新镜像。

`dev` 分支历史 Ansible 只作为 Docker、Chrony、UFW、目录和 topology 行为参考。
S0027 不引入 Ansible runtime、inventory、playbook 或 extra-vars。

### Scope

1. 删除整个旧 Deploy surface：`deploy cold-sign-script` 和所有旧
   `deploy/preflight|status|register-build|register-submit|provision|sync|start|takeover`
   operation/name，
   包括 CLI dispatch、operation/parity registry、intent/executor、Skill、网站入口、
   当前运维文档和专属测试/fixtures；不提供 alias、迁移命令或兼容路径。
2. 新建 canonical Deploy Skill，正向描述 Fleet/SSH 信息收集、用户手动建立 SSH
   trust、只读 Inspect、一次确认、一次 Apply、最终统一 Check 和失败后继续对话。
3. 复用 operation-scoped `pool-spec.yaml` 作为 Fleet 输入；Deploy 只消费 network、
   machine role、SSH 和 Relay public endpoint 等部署字段，不新增平行 inventory。
4. CLI 提供 user-only interactive SSH trust，以及 `deploy inspect`、`deploy apply`、
   `deploy check`；远端写操作只能由固定 target runner/scripts 完成。
5. `inspect` 只读检查所有主机，返回目标事实、预计变更、阻塞原因、signed deploy
   recommendation 和空 DB 的内置 Mithril 预期。
6. `apply` 幂等配置 Ubuntu、Docker/Compose、Chrony、固定目录/权限、UFW、role
   topology 和 Compose。
7. Compose 使用实时 signed catalog 的精确 repository、platform manifest digest 和
   config digest；不 bind mount 整个 `/opt/cardano/config`。
8. Relay 公开 P2P endpoint。BP 不发布 P2P host port、不增加 P2P ingress rule，只按
   topology 主动连接 Relay。BP/Relay metrics 只绑定宿主 loopback。
9. BP keys 目录以 read-write 方式挂载，但 Deploy 不生成、读取、上传、移动或启用
   KES/VRF/opcert。Fresh BP 以显式 `bootstrap` lifecycle 和 non-producing mode 完成
   本 spec；不得放宽 existing operational BP 的 forging credential gate。
10. Apply 连续启动所有 Relay，再启动 BP。两阶段之间不插入 readiness 检查、日志、
   socket/query、metrics、sleep 或 polling。
11. Apply 全部启动尝试结束后由 Skill 单独调用一次 `deploy check`，逐节点返回
    `ready|pending|failed`、`node_readiness`、`forging_readiness` 和
    `block_production` facts。
12. 使用最小 ownership marker 和 deterministic desired digest 支持安全、幂等的
   partial recovery；不引入事务、journal 或 Fleet rollback。
13. 网站直接嵌入 canonical Deploy Skill；提供自动化测试和至少 1 BP + 1 Relay 的
   真实 Ubuntu E2E，覆盖用户 SSH trust、不同 SSH 用户、Mithril pending、最终 ready、
   幂等重跑和 S0026 Compose Upgrade handoff。

### Constraints

- Skill 是用户可见 Deploy 流程、提问顺序和结果解释的唯一真实来源；网站从 canonical
  Skill 生成，不维护第二份近似流程。
- 旧 Deploy 必须被完整删除。不得保留 compatibility alias、
  hidden operation、deprecated help、网站旧 prompt 或从 Deploy 转发到注册流程；
  completed historical specs 作为历史证据保留，不因本 spec 回写。
- CLI 是 pool-spec 消费、远端事实、安全校验、fixed templates/scripts/argv 和写操作
  的唯一实现来源。Agent 不得绕过 `ouro-ops deploy` 使用 raw SSH、scp、sudo、包管理器、
  UFW、Docker 或 Compose 写命令。
- 开始任何 Agent 发起的主机接触前，Skill 先询问所有机器是否使用同一 SSH 用户名；
  相同时询问一次，不同时收集逐机器映射。不得猜测用户名。
- SSH 凭据只使用现有 `creds://` 引用；不询问、存储或输出 SSH 私钥内容和 sudo 密码。
  v1 要求 root 或 `sudo -n` 可用。
- `deploy inspect/apply/check` 永远使用 `StrictHostKeyChecking=yes`，不得自动 TOFU 或
  `accept-new`。若 Ouro known_hosts 缺少目标，Inspect 返回 `ssh_host_key_untrusted`，
  不接触该目标，并要求用户亲自执行 interactive trust command。
- Interactive trust command 是独立的 user-only 首次信任仪式：必须显示 machine、host、
  port 和服务端 fingerprint。用户提供带外 `--expected-host-key` 时必须精确匹配；未提供
  时必须明确警告“这是未经带外验证的首次信任（TOFU）”，并要求用户亲自确认完整
  fingerprint。Agent 不得运行、代答或自动确认。用户完成后通知 Agent 重跑 Inspect。
  Host key 变化不得走普通确认，必须要求新的带外 fingerprint 后才能替换。
- v1 支持 Ubuntu 22.04/24.04 LTS、linux/amd64 和 linux/arm64，且目标平台必须存在
  signed deploy recommendation；其他系统或平台返回 unsupported。
- Host package 安装只使用目标已启用的 Ubuntu signed repositories，固定 allowlist 为
  `docker.io`、`docker-compose-v2`、`chrony`、`ufw`、`ca-certificates`、`curl`。
  不得添加第三方 apt repository、执行 full upgrade 或安装 Ansible/fail2ban。
- 已存在的 Docker Engine/Compose v2 若通过 capability check 则复用。缺失时安装固定
  Ubuntu packages；package 不可用或 Compose v2 capability 不满足时 fail closed。
- Fresh Deploy 不自动接管、覆盖或迁移未知 Cardano 容器、systemd service、数据库、
  `/opt/cardano`、`/home/cardano/node-config` 或其他 operator 数据。
- 现有生产完整 `/opt/cardano/config` bind mount 只允许读取/解释；S0027 不迁移它。
- Fresh Deploy 不 bind mount 整个 `/opt/cardano/config`。镜像内 config/genesis/
  Mithril vkeys 必须保持可见，Ouro 只覆盖 topology 路径。
- Docker daemon 既有配置不得被整体覆盖；不得无条件重启已有 Docker。日志 rotation
  在 Compose service 内声明。
- 防火墙写入必须先保留当前实际 SSH 入口。CLI 保持原 SSH session，变更后用新连接
  验证；新连接失败时通过原 session 撤销本次 UFW delta。
- Relay P2P 只开放 pool spec 的 public endpoint port。BP 不发布 P2P host port，
  不设置 BP P2P ingress allowlist；BP 通过允许的 outbound traffic 主动连接 Relay。
- 12798 只绑定宿主 loopback，BP/Relay metrics 均不得直接暴露公网。
- 不修改 SSH password authentication、root login 或其他 sshd policy。
- `/opt/ouro/keys` 必须是真实、非 symlink、非 world-writable 目录。Fresh Deploy
  允许目录为空并以 read-write 挂载到 BP；Relay 不创建或挂载该目录。
- Deploy 不读取 key bytes，不生成 KES/VRF，不处理 pool cold key，不签发/安装
  `node.cert`，不把 BP 切换为 producer。明显的 cold-key/counter artifact 位于
  `/opt/ouro/keys` 时 fail closed，但不通过读取文件内容推断秘密类型。
- Compose 使用镜像 `run` 路径、正确 `CARDANO_NETWORK`、
  `CARDANO_BLOCK_PRODUCER=false` 和 `RESTORE_SNAPSHOT=true`。
- Fresh BP 必须带 `io.ouro.lifecycle=bootstrap`；Relay 使用
  `io.ouro.lifecycle=operational`。`lifecycle` 参与 desired digest、Inspect drift 和
  Check。只有未来 typed BP Bootstrap 可以把 BP 从 `bootstrap` 原子转换为
  `operational` 并同时更新 producer mode 与 desired digest。
- Bootstrap BP 的 Deploy Check 只用 node readiness 判定本 spec 的 ready，同时明确
  `forging_readiness=not_applicable` 和 `block_production=disabled`。现有
  operational BP、KES Rotation 和 runtime gate 继续要求 KES/VRF/opcert，不得因
  S0027 接受缺失 forging credentials。
- Inspect 在空 DB 场景报告内置 Mithril，检查 signed bootstrap policy 的 resource
  budget 和 aggregator 出站可达性；Apply 不等待 restore。
- Inspect 发现全部声明节点已经构成与输入一致的完整 Fleet 时返回
  `already_deployed`，向用户说明实际 runtime/image，`deploy apply` 拒绝修改；用户可
  选择 `deploy check` 查看状态，镜像升级交给 S0026。只有 marker/fleet identity
  一致、且 Fleet 中仍有未完成节点时，已完成节点才作为 safe partial 保留并继续其余
  节点。任一目标存在其他 Cardano deployment 时返回 blocked，不自动接管。
- Apply 前由 Skill 展示 change set、推荐镜像和 Mithril 预期，并获得 operator 一次
  明确同意。不增加 candidate hash、confirm-token、fleet permit、transaction、锁或
  跨机器 journal。
- 一台 Relay 当前命令失败时继续其他 Relay；至少一台 Relay Compose up 返回零时启动
  BP；所有 Relay Compose up 均失败时 BP 返回
  `skipped_no_relay_command_success`。
- Chrony 只执行一次有固定短超时的 `chronyc -n tracking` 当前状态读取，不使用
  `waitsync`、sleep 或轮询。已同步且绝对 offset ≤ 1 second 时继续；否则该机器本次
  Apply 失败且不启动容器，其他机器继续，用户稍后重跑。
- `deploy check` 是一次当前状态读取，不 sleep/长轮询。用户可稍后再次 Check。
- E2E harness 可以在测试预算内重复调用单次 Check，但产品 CLI 本身不得等待。
- 失败通过重新 `inspect/apply/check` 向前恢复；不实现 Fleet 自动回滚、不删除 DB、
  不运行 `docker compose down -v`、不停止无关服务。

### Non-goals

- 不生成、传输、安装或激活 KES/VRF/opcert，不启用 BP forging。
- 不创建 BP Bootstrap Skill；S0027 只为后续独立 BP Bootstrap 保留兼容运行形状。
- 不把旧 Deploy 注册能力迁移到新命令，也不在 S0027 重新实现、重命名或保留矿池注册
  行为；其删除是本 spec 的显式 scope，而非需要兼容的外部约束。
- 不升级、自动接管或迁移已有 Cardano 部署。
- 不安装或管理独立 Mithril client，不提供 host-side bootstrap executor。
- 不安装 Prometheus Server、Nginx、TLS、Basic Auth、node_exporter 或外部
  observability gateway。
- 不安装 fail2ban，不修改 SSH authentication。
- 不等待 snapshot restore、replay、socket、`query tip`、P2P、metrics 或同步完成。
- 不提供任意远端 shell、任意 package、任意 systemd unit、任意 Compose YAML、
  任意 mount 或任意防火墙规则入口。
- 不调用 Ansible，不生成 inventory。
- 不实现 Kubernetes、Swarm、Podman、Portainer 或跨主机 Compose。
- 不实现 Fleet 自动回滚、卸载、deinit、灾备恢复或数据库迁移。

## 2. Outline Design

### 2.1 Old Deploy Removal Contract

S0027 最终态删除旧 Deploy，不保留任何可执行或用户可见兼容面。实现职责拆分为：
p1-1 删除 legacy CLI/operation/live-code 名称，p4-2 一次性替换 Skill/docs/web；两项
不重复修改同一 surface。

| 旧 surface | S0027 动作 |
| --- | --- |
| `ouro-ops deploy cold-sign-script` | 删除 parser、dispatch、help 和实现 |
| `deploy/preflight`、`status`、`register-build`、`register-submit`、`provision`、`sync`、`start`、`takeover` | 从 live code、operation/parity registry、intent、executor、allowlist 和非 absence tests 删除或改为中性 fixture 名 |
| legacy `ouro-skills/deploy/SKILL.md` | p4-2 整体替换为 Fleet Deploy Skill |
| 网站 Deploy 注册入口/prompt | p4-2 删除并由 canonical Fleet Deploy Skill 重新生成 |
| `docs/S0020-operations.md` 当前 Deploy 注册语义 | p4-2 改为 Fleet Deploy；不为旧命令保留操作说明 |
| 旧 Deploy 专属 tests/fixtures | p1-1 删除；只允许 dedicated absence regression 保留旧名称作为拒绝输入 |

完成后，`deploy` namespace 只允许 `inspect|apply|check`。调用任一旧 command/operation
必须得到 typed unknown/unsupported command，所有 registry 中不得存在旧 operation。
除 S0027 removal contract、completed historical specs 和 dedicated absence regression
外，live code/current docs/tests 不得再出现旧名称。不得通过 alias、feature flag、
hidden help、兼容 parser 或转发到其他 Skill 继续暴露旧行为。
已完成 spec 中的历史文本不属于可执行 surface，保留不改。

### 2.2 Signed Image And Bootstrap Contract

Signed deploy recommendation 决定精确 Blink Labs OCI tuple；Skill 和 pool spec 不允许
提供 repository/tag/digest 覆盖。Image Contract item 必须验证当前精确推荐：

- image 包含 `cardano-node`、`cardano-cli`、`mithril-client`、`nview` 和 `txtop`；
- image 包含每个支持 network 的 config、genesis 和 Mithril vkeys；
- signed contract 将 network/genesis identity、config paths、metrics binding、
  entrypoint/run semantics 和 network-specific resource budget 与精确 image config
  digest 绑定；
- `/data/db/protocolMagicId` 缺失且 `RESTORE_SNAPSHOT=true` 时运行内置 Mithril，
  已有 DB 时跳过；
- `CARDANO_SOCKET_PATH=/ipc/node.socket`；
- selective topology override 生效且不遮蔽 image config；
- non-root/capability/security 选择若不能由精确镜像实证支持，Compose 使用经过验证的
  image default，而不猜测强化。

Fresh Deploy 只设置：

```text
CARDANO_NETWORK=<network>
CARDANO_TOPOLOGY=/ouro/topology.json
CARDANO_DATABASE_PATH=/data/db
CARDANO_SOCKET_PATH=/ipc/node.socket
CARDANO_BLOCK_PRODUCER=false
RESTORE_SNAPSHOT=true
```

Fresh BP 还必须绑定：

```text
io.ouro.lifecycle=bootstrap
CARDANO_BLOCK_PRODUCER=false
```

Relay 绑定 `io.ouro.lifecycle=operational`。`role=bp` 不再单独决定当前是否应具备
forging credentials；`role + lifecycle` 决定 gate：

| Role/lifecycle | Node readiness | Forging readiness |
| --- | --- | --- |
| Relay / operational | 必须通过 | `not_applicable` |
| BP / bootstrap | 必须通过 | `not_applicable`，block production disabled |
| BP / operational | 必须通过 | 必须具备并验证 KES/VRF/opcert |

该区分只能作用于明确带 S0027-owned bootstrap label 的 Fresh BP。缺少 lifecycle 的
既有 BP 按 operational 安全默认处理，不能借 S0027 绕过现有 readiness、KES Rotation
或 runtime credential gate。

Inspect 在 image 尚未存在于 target 时信任 signed contract facts，不通过 pull 修改
target。Apply pull 后验证 platform manifest 和 image config digest；Check 再验证
实际 container/image/runtime facts。

### 2.3 Responsibility Boundary

| 能力 | Deploy Skill | CLI |
| --- | --- | --- |
| 旧 Deploy 删除 | 不提供注册型流程或兼容提示 | 删除旧 command/operation，实现 surface 唯一性 |
| 兼容检查 | 首次动作运行 contract check | 验证 CLI/contract version |
| Fleet 收集 | 询问机器、角色、地址、同/不同 SSH 账号和凭据引用 | 校验 operation-scoped pool spec |
| SSH trust | missing 时要求用户亲自执行，等待用户完成 | interactive trust 只写控制机 known_hosts |
| 镜像/Mithril | 展示 signed recommendation 和空 DB restore 预期 | 验证 signed OCI/bootstrap contract |
| 只读检查 | 调用 Inspect，解释 blocker/change set | SSH 探测 host/runtime/ports/ownership |
| 执行批准 | 展示变更并等待一次明确同意 | 不实现第二套对话审批状态 |
| 主机配置 | 只调用 Apply | 固定 scripts 配置 packages、Chrony、目录和 UFW |
| Compose | 解释 Relay 与 non-producing BP 形状 | 生成、验证、原子安装并启动固定 Compose |
| 执行顺序 | 告知 Relay → BP、无中间等待 | Relay command 完成后直接继续 BP |
| 统一验收 | Apply 后调用一次 Check | 返回 node/forging readiness 与 block-production fact |
| 后续 BP Bootstrap | 只提示是独立后续能力 | S0027 不实现 |
| 网站 | canonical Skill 是生成输入 | 不维护网站流程文案 |

### 2.4 Public CLI Surface

用户亲自运行：

```text
ouro-ops ssh trust --spec <pool-spec.yaml> --node <machine-id> \
  [--expected-host-key <SHA256:base64>]
```

Agent 运行：

```text
ouro-ops deploy inspect --spec <pool-spec.yaml>
ouro-ops deploy apply   --spec <pool-spec.yaml>
ouro-ops deploy check   --spec <pool-spec.yaml>
```

- `ssh trust`：interactive、user-only；显示并确认 host key，验证声明的 SSH account/
  credential，只写控制机 Ouro known_hosts；省略 expected fingerprint 时明确标记为
  user-accepted TOFU。
- `inspect`：只读 target；预检、分类、change set、signed recommendation、Mithril 预期。
- `apply`：写 target；Skill 获得 operator 同意后一次调用，连续执行 Fleet。
- `check`：只读 target；Apply 后统一读取当前事实，不等待 readiness。

四个命令共享同一个 pool spec。CLI 不回写 pool spec，也不新增 Fleet inventory、
permit、transaction 或 target state journal。

旧的 `deploy cold-sign-script` 必须解析失败，旧
`deploy/preflight|status|register-build|register-submit|provision|sync|start|takeover`
不得存在于 live-code operation/parity registry、intent/executor dispatch 或帮助文档。
S0027 不提供迁移指引或兼容转发。

### 2.5 Operation-Scoped Pool Spec

Deploy 只消费：

- `spec_version`；
- `pool.network`、`pool.network_magic` 和 genesis identity；
- `topology_mode: p2p`；
- `machines[].id`；
- `machines[].role: bp|relay`；
- `machines[].ssh.host|port|user|key_ref`；
- Relay 的 `public_endpoint.host|port`。

规则：

- 恰好一个 BP，至少一个 Relay；
- BP topology 使用全部 Relay public endpoints；
- Relay topology 不把 BP 当公共上游，使用 signed image/network contract 固定的
  bootstrap peers；用户和 Agent 不提供任意 upstream；
- Deploy 不消费 pool economics、`node_version`、registration、`sync.mode`、
  BP credentials 或 external observability fields；
- signed recommendation 由 CLI 实时选择，不从 spec 的 `node_version` 推导；
- SSH user 是 operator fact，Skill 在写 spec 前完成同/不同账号询问；
- 不保存密码、私钥内容、sudo 密码、任意 command/Compose/Ansible/mount/package/
  firewall fragment。

### 2.6 Fixed Deployment Policy

| Policy | Fixed v1 behavior |
| --- | --- |
| OS | Ubuntu 22.04/24.04 LTS |
| Platform | linux/amd64 或 linux/arm64，且 signed recommendation 覆盖 |
| Privilege | root 或 `sudo -n` |
| Packages | Ubuntu repositories 的 docker.io/docker-compose-v2/chrony/ufw/ca-certificates/curl |
| Docker | 复用通过 capability check 的现有 Engine；否则安装 docker.io |
| Compose | 必须为 `docker compose` v2，支持 `config` 和 `up -d` |
| Time | chrony synchronized 且绝对 offset ≤ 1 second |
| Resources | signed bootstrap contract 的 network-specific minimum RAM/free disk |
| Base path | `/opt/ouro` |
| Compose project | `ouro-<machine-id>` |
| Service | `cardano-node` |
| Compose file | `/opt/ouro/compose.yaml` |
| Container | Compose 生成，不自行模拟 Compose labels |
| Log rotation | json-file, `max-size=50m`, `max-file=3` |
| Relay P2P | `<public_endpoint.port>:3001/tcp` |
| BP P2P | 无 host port mapping、无 ingress UFW rule |
| Metrics | `127.0.0.1:12798:12798/tcp` |

Resource minimum 是 signed image/bootstrap contract 的一部分，不是 Agent 或 pool spec
可覆盖的参数。无法证明资源满足时返回 blocked，不猜测继续。

### 2.7 Inspect And Ownership

`deploy inspect` 至少检查：

- Ouro known_hosts、SSH、逐机 user/credential 和 `sudo -n`；
- Ubuntu release、architecture、RAM/free disk；
- Docker Engine、Compose v2 capability 和 daemon 状态；
- Chrony/NTP 状态和 offset；
- firewall backend、UFW 状态、当前 SSH listener；
- Relay public P2P、BP 不应公开的 P2P、12798 等端口占用/暴露；
- `/opt/ouro`、历史 `/opt/cardano`、`/home/cardano/node-config`、DB 和已有 runtime；
- BP keys dir 的 path type、symlink 和 directory mode；不要求 key files 存在；
- signed deploy recommendation、platform 和 bootstrap contract；
- 目标 DB 的 `protocolMagicId`/empty/residual 状态；
- 空 DB 的 resource budget 和 Mithril aggregator 出站可达性；
- ownership marker、Compose labels 和 desired digest。

分类只使用：

| 分类 | 含义 |
| --- | --- |
| `applicable` | clean Fleet，或 fleet identity 完全一致且整个 Fleet 尚未完成的安全 partial；已完成节点保持不变 |
| `already_deployed` | 全部声明节点已构成完整且 identity 一致的 Fleet；仅提示用户并允许 Check，Apply 拒绝 |
| `blocked` | untrusted/unreachable/unsupported、其他既有 deployment、未知 data 或安全问题 |

空的 `/opt/ouro` 可以作为 clean host 继续；非空且没有有效 marker 的 `/opt/ouro` 始终
blocked。Fresh Deploy 的第一次持久化动作保存不可变 identity marker：

```json
{
  "schema_version": 1,
  "fleet_identity_digest": "sha256:<canonical-fleet-identity>",
  "machine_id": "relay1",
  "role": "relay",
  "network": "mainnet",
  "genesis_identity": "sha256:<canonical-genesis-hashes>",
  "repository": "ghcr.io/blinklabs-io/cardano-node",
  "platform": "linux/amd64",
  "platform_manifest_digest": "sha256:<digest>",
  "image_config_digest": "sha256:<digest>"
}
```

Marker 不含 SSH、credentials、step/result/timeline、secrets 或 mutable desired state。
`fleet_identity_digest` 绑定 network/genesis、声明的 machine id/role 和 Relay public
endpoints，用于区分同一 Fleet 的 partial rerun 与无关既有部署。
它固定首次部署选择的 network/genesis/image identity。Partial rerun 只能继续使用 marker
中的精确 tuple，且该 tuple 仍须通过当前 signed allow/deny policy；当前 recommendation
变化不得改写 marker。完整 deployment 始终返回 `already_deployed`，任何镜像变更都
交给 S0026。
`desired_digest` 是 canonical desired machine document 的 SHA-256，至少绑定：

- deployment policy version；
- machine id、role、lifecycle、network/genesis identity；
- platform manifest/config digest；
- Compose project/service、env、mounts、ports、logging/security；
- topology content digest。

Desired digest 写入 Compose label 和 Check output，可以由未来 typed BP Bootstrap 更新；
identity marker 保持不变。非空且未知的 `/opt/ouro` 始终 fail closed。

### 2.8 Host Convergence

Apply 对 applicable 主机执行固定、幂等步骤：

1. 重新校验 spec、signed contract、SSH/sudo、ownership 和 blocker。
2. 安装/启用固定 Ubuntu packages；已有 capability-conformant 版本不重复修改。
3. 使用 runner 强制的 ≤5 秒命令超时执行一次 `chronyc -n tracking`；未同步或绝对
   offset > 1 second 时该主机失败且不启动容器，不执行 waitsync/sleep/poll。
4. 对 clean host 创建空 `/opt/ouro` 并立即原子写入 identity marker，再创建 DB、IPC、
   topology；BP 额外创建空 keys dir。对 partial host 先验证 marker 精确匹配。
5. 收敛 UFW：
   - 先放行实际 SSH port 并保持当前 session；
   - Relay 放行 public endpoint P2P；
   - BP 不放行 P2P ingress；
   - 12798 不对公网开放；
   - reload 后从控制机建立新 SSH connection；
   - 新连接失败时用原 session 恢复本次 UFW delta。
6. 生成 role-specific topology。
7. 生成并验证 Compose，原子安装 topology、marker 和 Compose。
8. 按精确 platform manifest digest pull，验证 image config digest。
9. 执行固定 `docker compose up -d`。

不得 full apt upgrade、覆盖 `/etc/docker/daemon.json`、无条件重启已有 Docker、安装
fail2ban、改变 sshd policy、移动旧目录或自动 takeover。

### 2.9 Compose Runtime Contract

每台机器是独立 Compose project。固定 mounts：

| Host | Container | Mode |
| --- | --- | --- |
| `/opt/ouro/db` | `/data/db` | read-write |
| `/opt/ouro/ipc` | `/ipc` | read-write |
| `/opt/ouro/topology.json` | `/ouro/topology.json` | read-only |
| `/opt/ouro/keys`（BP only） | `/opt/cardano/config/keys` | read-write |

明确禁止 `/opt/cardano/config` 整目录 bind mount。BP keys dir 初始允许为空。

Compose 固定：

- signed `repository@platform-manifest-digest` 和 expected config digest；
- Compose native labels 与
  `io.ouro.machine-id/role/lifecycle/network/desired-digest`；
- `run` 与 2.2 的 environment；
- `unless-stopped` restart policy；
- fixed json-file rotation；
- image-contract-validated user/capabilities/security options；
- 禁止 privileged、host pid、任意 device/mount/extension；
- Relay 公开 P2P，BP 无 P2P host mapping；
- BP/Relay 都只向宿主 loopback发布 metrics；
- 不用短周期 socket healthcheck 把 Mithril/replay 标成 unhealthy。

未来 BP Bootstrap 可以在同一 owned Compose project 中安装 BP credentials、将
`io.ouro.lifecycle` 从 `bootstrap` 改为 `operational`、将
`CARDANO_BLOCK_PRODUCER` 切换为 true 并更新 desired digest；该转换必须作为一个
typed desired-state 更新发生。S0027 不执行该转换。

### 2.10 Apply Ordering

```text
配置所有 applicable 主机
    ↓
执行所有 Relay docker compose up -d
    ↓
至少一个 Relay 命令返回 0 时执行 non-producing BP docker compose up -d
    ↓
返回逐主机 command success/error/skipped
```

Relay 与 BP 之间没有独立检查。CLI 不运行 inspect/log/socket/query/metrics/sleep/poll；
只使用当前 SSH/package/file/UFW/Compose 命令的直接返回值。

一台 Relay 命令失败不终止其他 Relay。所有 Relay Compose up 均失败时跳过 BP。
Compose up 返回零后即使正在 Mithril/replay、socket 尚未创建或稍后退出，Apply 仍结束；
真实运行状态由随后的统一 Check 发现。

### 2.11 Unified Check

`deploy check` 一次读取：

- Chrony、UFW、SSH/P2P/metrics rule；
- identity marker、Compose labels/rendered config/desired digest；
- actual image/platform/config digest；
- mounts、ports、env、restart/log/security shape；
- container state、restart state/count；
- topology；
- socket、`query tip`、P2P listener；
- established peer facts；BP 必须连接至少一个声明的 Relay，Relay 必须连接至少一个
  signed network bootstrap/ledger peer；
- image 12798 metrics；
- BP keys directory path safety，不读取文件 bytes；
- `node_readiness`、`forging_readiness` 和 `block_production`；
- BP lifecycle，本 spec 必须是 `bootstrap`。

逐节点状态：

| 状态 | 含义 |
| --- | --- |
| `ready` | 全部 deployment invariants 通过，container running、socket/`query tip`/metrics 可用、P2P listener 与 role-specific peer 条件通过 |
| `pending` | 静态 deployment invariants 通过且 container running，但 Mithril/replay/startup 尚未建立 socket、tip、metrics 或 peers |
| `failed` | 任一 host/identity/image/Compose/mount/port/UFW/topology/security invariant 失败，或 container 不运行、restart loop、明确 fatal runtime error |

Check 可读取 bounded logs 作为明确 fatal evidence，但不要求通过特定日志字符串证明
pending。同步 100% 不参与 Deploy pass/fail。`ready|pending|failed` 对应
`node_readiness`；Fresh BP 必须同时报告
`lifecycle=bootstrap`、`forging_readiness=not_applicable` 和
`block_production=disabled`。

Deployment invariants 至少包括 Chrony、SSH/UFW、ownership/desired digest、精确
network/genesis/image tuple、Compose labels/rendered shape、mounts、ports、logging/
security、topology、P2P/metrics 不公网暴露。只有这些静态条件全部通过时，动态 startup
事实才允许归类为 pending；静态条件错误不得伪装成 pending 或 ready。

`forging_readiness=not_applicable` 只允许用于 Relay 或 S0027-owned bootstrap BP。
operational BP 必须继续执行既有 KES/VRF/opcert gate；缺失或无效 credentials 时必须
失败，不能因为 node socket/tip ready 而通过。

### 2.12 Skill Flow

Canonical Deploy Skill：

1. 运行 mandatory contract check。
2. 询问 Fleet；先问所有机器使用相同还是不同 SSH 用户名。
3. 生成/补全 operation-scoped pool spec，不生成无关字段占位。
4. 调用 `deploy inspect`。
5. 若返回 `ssh_host_key_untrusted`，展示 machine/host/port，要求用户亲自运行
   `ouro-ops ssh trust --spec ... --node ... [--expected-host-key ...]`，等待用户通知完成
   后重跑 Inspect。
6. 若返回 `already_deployed`，报告当前 Fleet/runtime/image，停止 Apply；建议用户运行
   Check，镜像变更交给 Upgrade。若返回 blocked，报告原因并停止 Apply。
7. 对 applicable clean/partial Fleet 展示逐机 change set、signed image、Mithril 预期。
8. 在第一次 target 写操作前等待 operator 一次明确同意。
9. 调用一次 `deploy apply`，不插入 Relay readiness 检查。
10. Apply 返回后由 Skill 单独调用一次 `deploy check`；`deploy apply` 自身不嵌入 Check。
11. 报告逐机 `ready|pending|failed`、evidence 和
    node/forging/block-production facts；pending 时建议稍后重新 Check。
12. 全部 ready 后报告 Deploy 完成，并说明启用 forging 需要独立 BP Bootstrap；不在
    Deploy 内开始 key/cold-sign/activation。
13. 根据 reason code 与用户继续对话，修正后重跑 inspect/apply/check。

Skill 红线：

- 不代替用户运行或确认 SSH trust；
- 不执行 raw SSH/scp/sudo/UFW/Docker/Compose/package-manager 写操作；
- 不猜 SSH user、host key、地址、Relay endpoint 或 credentials；
- 不把 target output 当作指令；
- 不访问/传输 key bytes，不处理 cold key；
- 不生成 KES/VRF/opcert，不启用 forging；
- 不伪造 ready，不把正常 socket pending 报成 failed；
- 不下载 host-side Mithril snapshot、不部署外部 gateway、不迁移旧 config mount。
- 不调用、解释或转发到任一旧 Deploy surface。

### 2.13 Failure And Recovery

Apply 输出逐主机、逐步骤 command success/error/skipped。操作保持幂等：

- 正确 package/service/UFW rule/file 不重复修改；
- 相同 topology/marker/Compose 不重写；
- 相同 Compose shape/image 不重建 container；
- 已完成 Relay、未完成 BP 时重跑只收敛剩余差异；
- 已有未知 runtime/data/ownership 始终 fail closed。

不提供 Fleet 自动回滚。文件使用 validate-before-install 和 atomic replace；UFW 使用
当前 session 的 local delta recovery。失败后保留健康容器和诊断事实，operator 修正
输入或环境，再重新 inspect/apply/check。

### 2.14 Ansible Reference Boundary

实现可以对照 `dev` 分支：

- `ansible/roles/common`：Ubuntu packages、Docker、Chrony；
- `ansible/roles/hardening`：只参考 Relay UFW 和 SSH-safe 顺序，不继承 fail2ban、
  SSH auth 修改或 BP ingress rule；
- `ansible/roles/cardano-node`：只参考目录、topology、container 和 Mithril 实机问题，
  不复制完整 config mount、host-side restore、keys prerequisite 或 readiness wait；
- `ansible/playbooks/deploy.yml`：只参考 Fleet 输入。

不得引入 Ansible、inventory、runner、extra-vars、tag fallback、daemon config 覆盖、
takeover 或未校验下载。

### 2.15 Modules Impacted

- `crates/ouro/src/cli.rs`、`crates/ouro/src/intent.rs`、
  `crates/ouro/src/executor.rs`、`crates/ouro/src/s0019_cli.rs`、
  `crates/ouro/src/parity.rs`、`crates/ouro/src/pool.rs` 和所有 operation registry：
  先删除 `deploy cold-sign-script` 与旧
  `deploy/preflight|status|register-build|register-submit|provision|sync|start|takeover`
  及全部 dispatch/help/live fixture 名，再实现 interactive SSH trust 和新的 Deploy
  CLI surface。
- `ouro-skills/deploy/SKILL.md`：删除注册型内容，整体替换为 Compose Fleet Deploy。
- pool-spec schema/domain：保持 operation-scoped optional fields。
- 新的 deploy inspect/apply/check、fixed target scripts、Compose/topology/ownership
  modules。
- release catalog/contract：补充 signed image/bootstrap facts 和 resource policy。
- `crates/ouro/src/supervisor.rs`、readiness、probe/typed mounts：接受 image-owned
  config、selective mounts 和显式 bootstrap lifecycle，不放宽 operational BP gate。
- `ouro-skills/observability/SKILL.md`：读取新 Compose shape，分别报告 node/forging
  readiness，不承担外部 gateway。
- `docs/S0020-operations.md`：删除旧 Deploy 注册语义，只记录 Fleet Deploy 和 BP
  Bootstrap boundary。
- `web/onboarding/generate.py`、`web/onboarding/index.html`：删除旧 Deploy 注册入口，
  从 canonical Skill 生成 Fleet prompt。
- 旧 Deploy tests/fixtures：删除或替换为 absence regression；新增
  `tests/test_skill_docs.py`、`tests/test_web_generator.py` 和 S0027 integration/E2E。

### References

- `data/releases.json`
- `docs/specs/completed/20260721T1527-S0026-upgrade-orchestration-aware-refactor.md`
- `docs/specs/completed/20260307T0000-S0004-phase3-6-mithril-bootstrap.md`
- `docs/specs/completed/20260711T1010-S0017-production-provisioning.md`
- `ouro-skills/deploy/SKILL.md`
- `ouro-skills/kes-rotation/SKILL.md`
- `ouro-skills/observability/SKILL.md`
- `docs/S0020-operations.md`
- `crates/ouro/src/cli.rs`
- `crates/ouro/src/domain.rs`
- `crates/ouro/src/executor.rs`
- `crates/ouro/src/supervisor.rs`
- `ouro-skills/lib/ouro-probe.sh`
- `web/onboarding/generate.py`
- `web/onboarding/index.html`
- `dev:ansible/playbooks/deploy.yml`
- `dev:ansible/roles/common`
- `dev:ansible/roles/hardening`
- `dev:ansible/roles/cardano-node`
- `https://github.com/blinklabs-io/docker-cardano-node/releases/tag/v11.0.1-1`
- `https://github.com/blinklabs-io/docker-cardano-node/blob/v11.0.1-1/Dockerfile`
- `https://github.com/blinklabs-io/docker-cardano-node/blob/v11.0.1-1/bin/entrypoint`
- `https://github.com/blinklabs-io/docker-cardano-node/blob/v11.0.1-1/bin/run-node`
- `https://packages.ubuntu.com/jammy-updates/docker-compose-v2`

## 3. Execution Plan

- [x] p1-1 [Old Deploy Removal] 删除 `deploy cold-sign-script` 和旧
  `deploy/preflight|status|register-build|register-submit|provision|sync|start|takeover`
  的 CLI/operation/parity/intent/executor/live-code 名称和专属 tests/fixtures；加入
  namespace absence regression，不保留 alias 或兼容转发。Skill/docs/web 只由 p4-2
  一次性替换。
- [ ] p1-2 [Image/Lifecycle Contract] 验证 signed recommended Blink Labs image 的
  config/genesis/
  Mithril/metrics/run/non-producing/selective-mount contract，扩展 signed bootstrap facts
  与 resource policy；加入 bootstrap/operational lifecycle、node/forging readiness，
  并完成 probe/Observability/S0026 Compose Upgrade 和 operational BP strict-gate 回归。
- [ ] p2-1 [SSH Trust] 实现 user-only interactive `ssh trust` 和严格 known-host dispatch；
  Agent 缺少/变化 host key 时只能返回 actionable blocker 并等待用户。
- [ ] p2-2 [Inspect] 复用 operation-scoped pool spec，实现多 SSH 账号、host/runtime/
  resource/port/ownership、signed recommendation、Mithril prerequisites 和 deterministic
  change set 的只读检查。
- [ ] p3-1 [Host Executor] 使用 CLI 控制的 fixed target scripts 幂等配置 Ubuntu
  packages、Docker/Compose、Chrony、目录和 SSH-safe UFW；Relay-only P2P ingress，
  不引入 Ansible/fail2ban/任意 shell input。
- [ ] p3-2 [Compose Apply] 实现 role/lifecycle topology、identity marker、
  desired digest、固定 Compose 和连续 Relay → bootstrap BP Apply；BP empty read-write
  keys mount，无中间 readiness 检查，支持 partial/idempotent rerun。
- [ ] p4-1 [Unified Check] 实现一次 Fleet Check，覆盖 host/UFW/Compose/image/mount/
  container/socket/tip/P2P/built-in metrics/keys-dir safety，并返回逐节点
  `ready|pending|failed`、node/forging readiness 与 block-production fact。
- [ ] p4-2 [Skill/Docs/Web] 重写 canonical Deploy Skill、operations docs 和网站 Fleet
  入口，保证 SSH user/trust、一次确认、单次 Apply、最终 Check 和 BP Bootstrap boundary
  来自 canonical Skill，且旧注册型 Deploy 不再出现。
- [ ] p5-1 [Integration/E2E] 完成 Rust/Python 回归、fixed executor failure injection、
  idempotency fixtures 和真实 Ubuntu 1 BP + 至少 1 Relay E2E；观测 Mithril pending，
  最终全部 ready，并验证 metrics 私有、不同 SSH 用户、lifecycle strict gate 和 S0026
  Upgrade handoff。

### Item → TC Mapping

| Item | Acceptance |
| --- | --- |
| p1-1 | TC-1 |
| p1-2 | TC-2, TC-3 |
| p2-1 | TC-4, TC-5 |
| p2-2 | TC-6, TC-7 |
| p3-1 | TC-8, TC-9 |
| p3-2 | TC-10, TC-11 |
| p4-1 | TC-12, TC-13 |
| p4-2 | TC-14 |
| p5-1 | TC-15, TC-16, TC-17, TC-18 |

## 4. Test And Acceptance Criteria

- TC-1：`deploy cold-sign-script` 的 parser/dispatch/help/implementation，以及旧
  `deploy/preflight|status|register-build|register-submit|provision|sync|start|takeover`
  的 operation/parity registry、intent/executor/live-code fixture 全部不存在；除
  dedicated absence regression 外，任一旧 command/operation 返回 unknown/unsupported，
  且不存在 alias、hidden operation、feature flag 或兼容转发。
- TC-2：精确 signed recommended image 被实证包含正确 config/genesis/Mithril vkeys、
  `mithril-client`、`cardano-cli`、12798 metrics 和 `run` semantics；空 DB 自动 restore、
  已有 `protocolMagicId` 跳过 restore，Ouro 不执行 host-side restore。
- TC-3：signed image/bootstrap contract 绑定 network/genesis/config/metrics/platform/
  config digest 和 nonzero resource policy；Fresh Deploy 不 bind mount
  `/opt/cardano/config`，image config/vkeys 可见，selective topology/data/ipc/keys mounts
  下 probe、Observability 和 S0026 Compose detection/manual Upgrade 回归通过。Fresh
  BP 明确为 bootstrap，operational BP 缺失 KES/VRF/opcert 仍严格失败。
- TC-4：Ouro known_hosts 缺少目标时 Inspect 不接触 target，返回
  `ssh_host_key_untrusted`。Interactive trust 显示 machine/host/port/fingerprint；
  supplied expected fingerprint 必须精确匹配，省略时必须明确警告 user-accepted TOFU，
  只有用户确认完整 fingerprint 才写 control known_hosts；changed fingerprint 无新的
  带外 expected value 时 fail closed。
- TC-5：Agent-facing Skill/tests 禁止 Agent 执行或确认 SSH trust；用户完成后，相同和
  逐机不同 SSH 用户、不同 port/credential 都按 pool spec 正确 dispatch，始终使用
  `StrictHostKeyChecking=yes`。
- TC-6：Inspect 不修改 target，输出 SSH/OS/platform/resource/Docker/Compose/time/UFW/
  ports/runtime/keys-dir/ownership、signed recommendation、Mithril prerequisites 和
  deterministic change set；clean/same-fleet partial 为 applicable，完整同一 Fleet 为
  already_deployed，其他既有 deployment/data 为 blocked，reason code 正确。Apply 对
  already_deployed 拒绝且不修改任何节点。
- TC-7：Deploy 不要求 economics/node_version/sync/BP key 占位，不接受 password、
  key bytes、任意 command/Compose/Ansible/mount/package/firewall 字段；unknown runtime/
  data、unsupported OS/platform 和 insufficient signed resource budget 均 blocked。
- TC-8：从只有 trusted SSH、root 或 `sudo -n` 的 Ubuntu 22.04/24.04 clean host 可从
  Ubuntu repositories 安装/启用 Docker、Compose v2、Chrony、UFW 和固定目录；不要求
  target 已有 Ouro/Python/jq，不添加第三方 apt repo，不覆盖 daemon/sshd config。
  Chrony 只执行一次 ≤5 秒 `tracking` 读取，不使用 waitsync/sleep/poll；未达阈值时该机
  失败、其他机器继续。
- TC-9：UFW 先保留实际 SSH port；Relay 只开放 public P2P，BP 不开放 P2P ingress，
  12798 不对公网。reload 后新 SSH 成功；注入失败时原 session 恢复本次 UFW delta，
  其他既有规则不变。
- TC-10：Compose 使用精确 platform manifest/config digest、固定 project/service/
  labels/run/env/restart/log/mounts/ports/security；BP keys mount 为 read-write 且允许
  empty，BP 固定 `lifecycle=bootstrap` 和 `CARDANO_BLOCK_PRODUCER=false`，Relay 为
  operational 且无 keys mount，BP 无 P2P host mapping，未知 `/opt/ouro` fail closed。
  Identity marker 固定 fleet/network/genesis/image tuple；current recommendation 变化
  不得让 Deploy 改写已有或 partial deployment 的 image。
- TC-11：Apply 一次调用按 Relay → bootstrap BP 连续执行；两者之间无 inspect/
  log/socket/query/metrics/sleep/poll。一个 Relay command 失败时其他 Relay 继续；至少
  一个 Relay Compose up 返回零则 BP 继续，全部失败则 BP typed skip。
- TC-12：Check 一次读取并只返回逐节点 ready/pending/failed；running container 在
  Mithril/replay/startup 中 socket 不可用为 pending，明确 container/restart/runtime
  failure 为 failed，不等待同步 100%，稍后重跑可自然变为 ready。任一静态 host/
  identity/image/Compose/mount/port/UFW/topology/security invariant 错误必须为 failed，
  不得仅凭 socket/tip 返回 ready。
- TC-13：BP/Relay node ready 都需要 socket 和 `query tip`；12798 在 container/host
  loopback 可读且公网不可达；Relay 有 signed bootstrap/ledger peer，BP 至少连接一个
  声明的 Relay；bootstrap BP 同时报告
  `forging_readiness=not_applicable` 和 `block_production=disabled`。operational BP
  缺失或无效 forging credentials 时失败；Deploy 不读取 key bytes、不生成
  KES/VRF/opcert、不启用 forging。
- TC-14：Deploy Skill 正向描述 contract check、同/不同 SSH 用户、user-only trust、
  pool spec、Inspect、signed image/Mithril 摘要、一次同意、单次 Apply、最终 Check 和
  独立 BP Bootstrap boundary；网站内容由 canonical Skill 生成且不包含 Pool
  Registration、cold signing、BP key generation、forging activation 或其他 legacy
  Deploy flow；p1-1 不承担 Skill/docs/web 重写。
- TC-15：完整 Fleet 的第二次 Inspect 返回 already_deployed，Apply 拒绝且不改变
  started_at、文件或 UFW；在 package/dir/topology/image-pull/Relay-up/BP-up 前后注入
  失败后，fleet identity marker + desired digest 允许相同 tuple 的 partial Fleet
  只收敛未完成节点，未知数据或不同 tuple 仍拒绝。
- TC-16：security regression 覆盖 target output injection、secret fingerprint、symlink/
  world-writable keys dir、obvious cold-key artifact、arbitrary package/mount/Compose/UFW、
  tag fallback、metrics/BP P2P public exposure 和 Agent 代确认 SSH trust，全部 fail
  closed。
- TC-17：lifecycle regression 覆盖 bootstrap BP 缺 key 可 node-ready、显式 future
  bootstrap → operational desired-state 转换边界、缺 lifecycle 的既有 BP 按
  operational fail closed，以及 KES Rotation/runtime readiness 没有被全局放宽。
- TC-18：真实 Ubuntu E2E 至少覆盖 1 bootstrap non-producing BP + 1 Relay、不同 SSH 用户、
  用户 interactive trust、clean Inspect、Apply、一次真实 Mithril/replay pending、
  bounded harness 重复单次 Check 后所有节点最终 ready、metrics 私有、幂等重跑和
  S0026 Compose Upgrade handoff；完整 Fleet 再次 Deploy 被识别为 already_deployed，
  最终 pending 不能作为 E2E pass。

Pass/fail：

- TC-1 至 TC-18 全部通过。
- 任一旧 Deploy command、operation、Skill、网站入口、当前文档或
  可执行兼容路径仍存在，均为 fail。
- 任一 Agent 可绕过 CLI 做远端写、代确认 SSH trust、Fresh Deploy 覆盖 image config、
  CLI 重做 Mithril、BP P2P/metrics 公网暴露、Deploy 处理 BP secrets/forging 或 Pool
  Registration、未知 deployment/data 被覆盖、key bytes 泄露、tag fallback、UFW 导致
  SSH 锁死、Chrony/readiness 长轮询、静态 deployment invariant 失败却返回 ready/
  pending、Deploy 重跑隐式换镜像、E2E 以 pending 结束、网站与 canonical Skill 分叉，
  或 operational BP 缺少 forging credentials 仍通过，均为 fail。
- 每个 item 对应 TC 通过并追加 evidence 后，才可标记完成并按
  `immutable-spec-delivery` 单独 commit。

建议验收命令在实现阶段按实际文件固化，至少包括：

```text
cargo fmt --all -- --check
cargo test -q
python3 tests/test_skill_docs.py
python3 -m pytest -q tests/test_web_generator.py
python3 tests/test_s0027_deploy.py
<S0027 Ubuntu E2E harness>
```

## 5. Execution Log (append-only)

- 2026-07-23T14:44:25+08:00 S0027 activated；确认没有其他 active markdown spec，
  开始按 item-level evidence/commit 顺序执行。
- 2026-07-23T14:44:25+08:00 p1-1 started：删除旧 Deploy CLI/operation/live-code
  namespace，并建立 dedicated absence regression。
- 2026-07-23T11:19:16+08:00 S0027 以 draft 创建，operator 将 Deploy 定义为 BP/Relay
  主机、Compose、网络、时钟、防火墙、端口和 observability 的首次部署。
- 2026-07-23T11:19:16+08:00 draft 明确 Ansible 只作为 `dev` 分支历史行为参考，
  Apply 保留 Relay → BP 顺序但没有中间 readiness 检查，最后统一 Check。
- 2026-07-23T12:38:58+08:00 operator 接受反方评审后的范围收敛：复用 operation-scoped
  pool spec；删除 fail2ban、外部 observability gateway 和 Ouro 自建 Mithril；复用
  Blink Labs exact image 的 config/Mithril/metrics。
- 2026-07-23T12:38:58+08:00 operator 确认现有生产完整 config bind 内已包含 Mithril
  vkeys。Draft 据此明确现有生产不迁移，Fresh Deploy 改用 image-owned config 与
  selective topology/keys/data/ipc mounts。
- 2026-07-23T13:59:31+08:00 operator 接受 executable review 修订：BP keys 改为
  read-write；缺失 host key 交由用户 interactive trust；Fresh Deploy 启动 empty-key
  non-producing BP；KES/VRF 在 BP 生成，cold environment 只返回 public node.cert，
  后续由独立 BP Bootstrap 激活；BP 不开放 P2P ingress；E2E 必须最终 ready；Deploy
  完全排除 Pool Registration。
- 2026-07-23T14:13:04+08:00 operator 明确拒绝为旧 Deploy 保留
  迁移或兼容。Draft 改为先完整删除旧 command/operation/Skill/web/docs/tests，再从头
  实现 Fleet Deploy；不提供 alias、deprecation window 或兼容转发。
- 2026-07-23T14:13:04+08:00 draft 增加显式 lifecycle：Fresh BP 为 bootstrap，
  Relay 为 operational；Deploy 分离 node/forging readiness，且保持既有 operational
  BP、KES Rotation 和 runtime forging credential gate 严格不变。
- 2026-07-23T14:29:25+08:00 operator 确认最终 Check 应包含全部 deployment
  invariants，并要求旧 Deploy 删除覆盖完整 namespace。Draft 增加 preflight/status
  清理、role-specific peers、静态 invariant gate 和 p1-1/p4-2 单一职责。
- 2026-07-23T14:29:25+08:00 operator 明确 Deploy 应通过 Inspect 识别既有部署。
  Draft 将完整同一 Fleet 定义为 already_deployed、Apply 拒绝且 Upgrade 交给 S0026；
  同一 fleet identity 的未完成 Fleet 仍允许 partial recovery，初始 image tuple 固定。
- 2026-07-23T14:29:25+08:00 Chrony 改为 ≤5 秒 one-shot tracking，不使用 waitsync 或
  polling；SSH 首次信任明确区分带外 fingerprint 验证与 user-accepted TOFU，Agent 均
  不得代确认。
- 2026-07-23T14:54:46+08:00 p1-1 completed：旧 Deploy CLI、operation registry、
  executor、transaction cold-sign/submit 实现及专属 fixtures/tests 已删除；只在
  dedicated absence regression 中保留旧名称作为拒绝输入。

## 6. Validation Evidence (append-only)

- （待执行）
- TC-1 | stack: Rust + Python | command: `cargo test -q`; `python3 -m pytest -q
  tests/test_s0027_deploy_absence.py tests/test_s0019_pipeline.py
  tests/test_external_skill_boundary.py`; `python3 tests/test_coldsign_invariants.py`;
  `python3 tests/test_cardano_cli_matrix.py`; `python3 tests/test_tool_output_schema.py`;
  `rg` legacy namespace audit | result: pass | note: 174 Rust tests and targeted Python gates
  passed；旧 namespace 在 live code/tests 中仅存在于 dedicated absence regression，
  Skill/docs/web replacement 按 item boundary 留给 p4-2。

## 7. Change Requests (append-only)
