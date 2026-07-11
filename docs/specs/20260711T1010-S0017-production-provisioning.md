# Production Provisioning & Real-Node Lifecycle

Spec-ID: S0017
Status: active
Created Time: 2026-07-09T11:00:00+08:00
Start Time: 2026-07-11T10:10:11+08:00
Completion Time:
Previous Spec-ID: S0016
Closure Reason:

## 1. Requirement Details

### Background
从 S0016 拆分而来。S0016 解决"决策层随 prompt 分发 + 机制层(`ouro` 二进制)的取得与版本",
但**机制层在真实生产的落地**——把一台裸机武装成受约束目标机、并在真实托管方式(systemd/
容器)下正确执行生命周期操作——体量大、风险高,独立成本 spec。

盘点结论(S0016 讨论期得到,`ouro` 顶层命令 = audit/confirm/config/kes/legacy/manifest/pool/
rollback/self-update/skill/spec/status/tool;`crates/ouro/src/cli.rs:40-52` 核实)。关键结论保持:
**顶层无 init/deinit/provision/bootstrap 命令**。

1. **无真机 provisioning**:`ouro` 顶层**无 init/provision/bootstrap**。`ouro-ops audit init` 仅初始化
   本地审计库;`deploy/provision` 只铺节点非安全状态(假设目标机已 provisioned);
   `fixtures/e2e/provision.sh` 是 docker-exec **测试夹具**,非产品命令。目标机安全底座
   (`ouro-exec`/`ouro-diag` 用户、`/usr/local/sbin/ouro-tool-run` wrapper、`/etc/sudoers.d/ouro-exec`、
   pubkey-only sshd、`ouro` 二进制)**今天只烤在 `node/Dockerfile`、`bp/Dockerfile` 里,无真机命令**。
   现状 dispatch 为 **SSH-exec-only 且有测试断言 no-scp**(`ssh.rs` 的 `only_prepares_allowlisted_tool_run_shape`
   / `tool_run_argv_uses_fixed_cli_path_and_no_secret_inline` 断言不含 `scp`)——真 `ouro-ops init`
   为**净新建**,需引入 target-mutating 传输。控制侧凭据布局(`~/.ouro/credentials/`,`secrets.rs` 仅
   解析 `creds://` 前缀 + 挡穿越,**无 bootstrap/role 分轨**,需扩展)与目标侧配方(Dockerfile)可复用。

2. **裸进程假设**:所有 skill 用 `pgrep`/`pkill`/`setsid` 把节点当**裸进程**管
   (`rotate.sh:66/71`、`restart.sh:18/20`、`upgrade-one.sh:56/58`、`topology-apply.sh:31/32`、
   `takeover.sh` 用 `pgrep`),**无任何 systemctl/docker/podman 意识**,spec 也无 runtime 声明。对已
   容器化/systemd 托管的节点运行会**脑裂重启或语义错**(upgrade 尤甚:容器世界升级 = 换镜像 digest +
   重建,而非换宿主机二进制)。

3. **占位路径**(评审新增盘点):`ouro-ops kes generate` 写的是**假 JSON 元数据**(哈希由
   network/version/machine 派生,不调 cardano-cli、不产真 KES 密钥对;`kes.rs:53-90`);`kes push` 反
   序列化自定义 `OpCertMetadata`、只报 `installed_payload=["node.cert"]`,不接受真 cardano-cli
   `node.cert` 也不在 BP 安装(`kes.rs:24-50,117-129`);`pool::build_register_tx` 写**合成假 CBOR**
   (`pool.rs:26-60`,`signed:false`);仓库**无任何 tx build/sign/submit 实现**。故本 spec p4 涉及的
   "既有 kes push / register-tx" 需先落地真实机制,不能直接承载冷签产物。

### Scope
1. **`ouro-ops init`/`deinit`(Model P + 三补强)**:把裸机武装成受约束目标机 / 一键还原;持久
   最小底座,可审计、可卸载;操作自清理。
2. **节点托管模式感知**:机制采集**脱敏**事实 → LLM 判定托管模式(bare/systemd/docker/podman)+ **候选
   身份** → 机制走**托管器**执行生命周期,全程不泄露密码学资料;模式/候选模糊或未知 → fail-closed。
3. **目标机权威审计的完整性**:对 target-minted 审计**哈希链 + 反签名** / 独立远端锚点;主机密钥 pin
   (取代 accept-new,防首连 MITM)。
4. **冷签脚本流程(deploy + kes-rotation 的 air-gapped 签名)**:据运行时数据 + 用户冷环境,经**签名
   bundle 协议**分发**环境专属、数据内嵌的自包含冷签脚本**(cardano-cli,冷机无需 `ouro-ops`),操作者
   拿到气隙机核对签名后执行即出冷签材料;回装/提交经既有机制。**脚本只带公开数据、就地读冷密钥,
   cold.skey 永不移动。**

### Constraints

> 评审驱动(S0017 multi-agent review,2026-07-10)修订:把若干"红线已宣示、机制未指定"的承诺换成
> **可执行的信任边界**。凡涉安全不变量的约束,须给出机制层 enforcement,不得只靠命名/路径/提示词。

- **不推翻 S0015 安全契约**:机制不靠提示词、密钥隔离、审计闸门、exit 码纪律不变。本 spec
  引入的 **target-mutating 传输是对现状 exec-only/no-scp 的有意突破**,仅用于 `ouro-ops init` 的
  特权 bootstrap,**独立模块(`bootstrap.rs`)、与 per-operation dispatch 使用不同凭据与代码路径**;
  per-op dispatch 的 **no-scp 断言必须保留**,init 传输走独立断言(见 §2、p1-1)。

- **provisioning 特权凭据与 agent 的边界是 OS/能力级,不是路径级(P0-1 修正)**:S0016 威胁模型 #2 已
  承认"控制机上 `ouro-ops` 之外的读取无机制保护、需受限 agent runner",而该 runner 至今未实现。因此
  "bootstrap 凭据 agent 永不可得"**不能**靠"放在另一个 `~/.ouro/` 子目录"实现。可接受的 enforcement
  二选一(或组合):
  - **(A) 独立主体/设备**:bootstrap 从 agent sandbox 不可及的独立操作员账户或 root-owned broker 执行;
    凭据置于 agent 不可读命名空间;`ouro-ops init`/`deinit` **强制交互 TTY + 一次性带外人工授权**
    (agent 无法 mint),**禁止** `--dispatch`/非交互/agent 代码路径。
  - **(B) 硬件密钥**:bootstrap 用硬件密钥 + 人工 touch/PIN + 无法无人值守行使的策略。
  - 若两者都不落地,则**必须**把该边界降级为诚实的运维纪律声明(在 spec 顶部复贴 S0016 适用边界表),
    并把"受限 agent-run 面"列为本 spec 的**前置依赖**(而非 S0016 可选遗留)。
  - per-operation dispatch 仍只走受限 `ouro-exec` 路径。

- **密码学资料零泄露(红线,P0-3 修正)**:托管模式探测**不得**依赖"secret-shaped 串过滤"作为主防线
  ——`docker inspect`/systemd env/进程 argv 可含任意密码/token/云凭据,它们不长得像 bech32/CBOR/PEM。
  必须用**带类型的封闭投影**:字段机械派生,**绝不序列化 raw env/argv/unit 文本/mounts/labels/完整
  inspect JSON**;只输出布尔、枚举、数字端口、托管器不可变 ID、哈希;遇未知字段/未识别源 → 拒绝序列化 +
  fail-closed。形状过滤(复用 S0015 no-leak #3 corpus/fingerprint)只作纵深防御。

- **LLM 判定是顾问,机制说了算,且机制复验的是"目标身份"而非仅"模式"(P1 loop 修正)**:破坏性动作前
  机制枚举**有限候选图 + 稳定 ID + 证据**(systemd MainPID、cgroup/容器 ID、可执行/config/socket 身份、
  当前镜像 digest);LLM 只解释候选、不提供任何破坏性 argv;confirmation token 绑定
  machine + 候选证据哈希 + 精确动作 + 目标 digest;执行前即刻重快照全字段,任何漂移即中止(防 TOCTOU);
  模式/候选模糊、混合/分层托管、幻觉选错候选 → exit 40 停手要人。

- **cold.skey 永不移动的边界须显式限定(P1 vrf 修正)**:该不变量对**新池 deploy** 与 **kes 冷签**成立;
  现状 `takeover` 要求并哈希目标 BP 上的 `cold.skey`(`takeover.sh:38-45`),故 takeover 迁移需另立设计
  (或改 takeover 为拒绝目标驻留冷密钥、只校验公开物/opcert)。deploy 唯一允许移动的私钥是 vrf.skey,
  且必须经**指名的操作员中介加密传输协议**(非"既有安全通道"含糊语)。

- skill 唯一真源 = `ouro-skills/*/SKILL.md`;托管模式决策 + 候选选择规则写进真源。

### Non-goals
- 不做零常驻的 Model E(每次操作临时 provision+teardown)——已在 S0016 讨论中否决。
- 不覆盖 S0016 的网站/分发/版本层(那是 S0016)。**例外**:p2-4 引入的 `runtime` schema 字段会影响
  S0016 网站生成器产出的 spec,该兼容性由本 spec p2-4 承接(见 §2 兼容性)。
- 不引入后端/托管控制面。
- 不在本 spec 内实现 takeover 的冷密钥迁移(仅声明边界 + 留待后续 spec)。

## 2. Outline Design

### Provisioning 模型:Model P + 三补强
目标机采用**持久最小底座(Model P)**,不追求零常驻:

- `ouro-ops init`(一次性、特权、人把关):建 `ouro-exec`/`ouro-diag` 用户、装
  `/usr/local/sbin/ouro-tool-run` wrapper + `/etc/sudoers.d/ouro-exec`、pubkey-only sshd、装
  `ouro` 二进制(内嵌 skill,与全机同源)、置控制机公钥、**pin 主机密钥**。参考实现 =
  `fixtures/e2e/node/Dockerfile` 现成配方。用**独立于 `ouro-exec`、agent 拿不到的特权
  bootstrap 凭据**(enforcement 见 §1 Constraints A/B);per-operation dispatch 只走受限 `ouro-exec` 路径。
- **bootstrap 输入契约(P1 修正,独立于 `PoolSpec`)**:`PoolSpec` 强制稳态 SSH 用户为 `ouro-exec`
  (`domain.rs:182-190`),描述不了 init 前状态。init 另立输入:支持的认证法(初始 root pubkey / sudo
  用户 + 一次性口令 / cloud-init 注入 key)、bootstrap 主体、sudo 模式(TTY/密码策略)、bastion/跳板、
  **期望 host 指纹来源**、恢复 console。
- **平台/架构矩阵 + 产物完整性(P1 修正)**:定义受测 OS/init/CPU 矩阵——至少 Debian/Ubuntu 与
  RHEL-family 的 sudoers/sshd 路径决策、systemd 有无、x86_64/aarch64;据目标事实(`uname -m` 等)选**签名
  发布产物**、装前验签+摘要、原子安装;装完前跑 `sshd -t`/`visudo -c` + 二次登录测试,再禁用 bootstrap
  接入。不支持的主机 → 显式 fail-closed。
- **参考 fixture 修复(P3,前置)**:现状 E2E base 仍 build/copy `target/release/ouro`,而 Cargo 二进制名
  是 `ouro-ops`(`Dockerfile.base:18-30` vs `Cargo.toml:10-12`,靠 `strip … || true` 容错掩盖)。init 参考
  配方前须修复并干净构建 fixture;**二进制唯一路径定稿** `/usr/local/bin/ouro-ops`(如需 `ouro` 名则显式
  symlink),init manifest 列路径 + 哈希。
- **三补强**:①底座最小化(只留约束必需项);②可审计(`ouro-ops init` 输出安装清单,逐条可核对);
  ③可卸载(`ouro-ops deinit` 彻底清除底座,机器还原)。
- **安装账本 + deinit 状态机(P1 修正)**:init 写**版本化、root-owned 安装账本**——记录 created-vs-adopted
  属主、改前对象摘要/备份、精确逆操作;冲突默认拒绝除非显式 `--adopt`。deinit 取全局操作锁、拒绝在途
  tool 调用、检测活跃 supervisor 属主;**节点运行时默认拒绝**(除非选 `--leave-node-running` 或
  `--stop-then-remove`);先验证替代接入再逆转 sshd 加固、**最后**移除接入主体。审计处置显式:导出到控制侧
  后再删,或保留并在清单标注(TC-2 据此可判定)。
- **操作自清理 + state 命名空间(P2 修正)**:因二进制内嵌 skill(S0016 p2-1),per-operation **无需推
  脚本**。但现状 runtime/upgrade 故意跨命令保留 `/tmp/ouro-*-state`(upgrade verify 读 upgrade 写的
  pre-PID/pre-block;`upgrade-one.sh:53-65`/`verify.sh:41-45`),"每次跑完删全部"会打断验证。故区分三层:
  **per-process 解压**(spawn 后删)、**per-invocation scratch**、**workflow-scoped state**(按 audit/workflow
  ID 命名空间 + 认证内容 + 定义消费步骤,仅在终态 verify/rollback 删除 + 崩溃 TTL GC);清理错误可见/入审计。

### 托管模式感知:机制采集脱敏(封闭投影) → LLM 判定 → 机制执行(候选绑定)
```
① 机制采集:confined 只读探测 detect/runtime(L3 姿态:只读、无 key 目录访问)
     采集:systemd unit? docker/podman 容器? bare? + config/socket/port/restart-policy/image digest
     输出:【带类型的封闭投影】—— 只含布尔/枚举/数字端口/托管器不可变 ID/哈希;
           绝不序列化 raw env/argv/unit 文本/mounts/labels/完整 inspect JSON;
           未识别源/未知字段 → 拒绝序列化 + fail-closed(P0-3)

② LLM 判定:agent 只看封闭投影,推断托管模式 + 解释候选(顾问性)——全程看不到任何 key/raw 值

③ 机制执行:走托管器,分模式 + 绑定候选身份(P1 loop)
     bare      = 换二进制 + setsid(现状路径),按 pid-file/unit 精确定位(防同机多节点误杀)
     systemd   = 换二进制 + systemctl restart <MainPID 校验过的 unit>
     container = pin 新镜像 digest + 重建容器(按 cgroup/容器 ID 定位)
     动作前:机制枚举有限候选图 + 稳定 ID + 证据;confirmation token 绑定
             machine + 候选证据哈希 + 精确动作 + 目标 digest;执行前即刻重快照,漂移即中止
     模式/候选模糊、混合(systemd 管 docker)/嵌套/多节点/陈旧 PID、幻觉选错 → exit 40 停手要人
```

**混合/分层托管**:检测到多重托管信号(如 systemd unit 内含 docker run、podman 生成的 systemd unit、
同机双匹配节点)一律视为**模糊 → exit 40**,并输出机器可读 conflict 码;绝不猜、不与未知托管器打架。

### 生命周期脚本收敛:中心 supervisor adapter(P1 修正)
托管行为现散落多脚本(runtime restart/topology-apply、upgrade upgrade-one/verify、kes rotate、deploy
takeover 均直接 `pgrep`/`pkill`/`setsid`)。p2 引入**中心化带类型 supervisor adapter**(建议
`ouro-lib.sh` 提供 `ouro_node_detect/stop/start/restart/recreate/status/verify`),**逐一改写**上述全部脚本
只调 adapter;加**静态闸**:禁止 adapter 之外出现 `pgrep|pkill|setsid|systemctl|docker|podman`;对每个生
命周期 skill × 每种受支持模式做 e2e。

### 审计完整性(P2 修正)
- 目标机权威审计**哈希链 + 反签名**:事件哈希链(machine 身份 + 单调序号),每个已接受 head 锚到**独立
  保护的控制/操作员存储**并 ack;按定义的可用性策略 fail/queue 写入。指定审计签名密钥的
  enroll/rotate/revoke。控制侧可检测:修改、删除、前缀回滚、重排、重放、fork/等价欺骗、丢失 ack。
  现状 `AuditStore` 仅 SQLite append,无签名/验签,需新建。
- 主机密钥 pin + 首跳认证前置(P1 修正,单一归属 p3-3):**首个 bootstrap 命令必须在写任何东西之前认证
  主机**。`ssh-keyscan` 不是认证。要求经 provider console/cloud-init/独立信道得到期望 SHA256 指纹或
  SSH CA/证书;首个 bootstrap 用专属 per-machine known_hosts + `StrictHostKeyChecking=yes`;dispatch 侧强
  制校验,取代 accept-new。**轮换**(OS 重装/IP 重用)是独立人工确认仪式,需旧 key 证明或新带外指纹 +
  审计 + rollback;不提供裸 `--repin` 逃生门(否则退回 TOFU)。

### 冷签脚本:签名 bundle 协议(P0-2 / P1 修正)
"脚本不含私钥"保护不了脚本被指示去读的那把私钥;被入侵控制机可篡改脚本把 cold.skey 复制进回传媒介。
故:
- **可执行逻辑作为发布签名、版本 pin、不可变模板**,独立安装/验证在冷机(冷机需 minisign/cosign 或离线
  `ouro-ops` verifier——**据此修正"冷机只需 cardano-cli"的说法**为"冷机需 cardano-cli + 一个离线验签器");
  只搬运**带签名 manifest 的规范公开数据**。
- 冷机侧 gate:模板 + 数据摘要经独立信道核对 → 人工 `cardano-cli … transaction view`/opcert 审读 → 断网
  confinement → 全新输出目录 → **严格回传文件白名单**(仅 node.cert/vrf.skey/tx.signed/counter-state.json)。
- KES 真实机制前置:目标侧真 `cardano-cli node key-gen-KES`(era-neutral,见下)、`kes.skey` 原子保留在
  BP、真 `kes.vkey` 认证导出、真 opcert 格式 + KES-vkey 哈希解析校验、目标侧安全安装 `node.cert`。
- **冷 counter 权威**:定为唯一单调权威——文件格式、属主、锁、原子备份、期望前后值显示、恢复语义;在线
  bundle 含 network/genesis hash、tip slot、slotsPerKESPeriod、算出的 period、采集时间、**最大年龄**;安装
  前重查活链/节点校验 cert KES 哈希/issuer/opcert 序号/当前 period 窗口/counter 单调;陈旧 bundle 须重生成。
- **deploy 分阶段**(P0-4):在线采集 + 校验链快照(UTxO、协议参数、deposit、tip/slot、metadata 哈希)→
  在线 build unsigned tx(或完整指定离线 build-raw 算法 + 全部输入)→ 冷机 inspect + 用枚举的
  payment/stake/cold owner 签名 → 在线复检 UTxO/validity/新鲜度并提交 → 确认精确 tx hash + 链上注册后再
  start/forging。定义快照最大年龄、validity 窗口、deposit/fee 来源、找零、witness 数、多 owner、重试、陈旧
  输入重建语义。替换 `pool.rs` 合成 builder,新增第二矿池注册 fixture(非 genesis 池)。
- **VRF 传输协议**:操作员中介的加密传输(收件人密钥、完整性/真实性、文件权限 0400、原子安装、无内容
  审计元数据、安全删除、重试/重放),作为独立 plan item + 真双机测试;在其存在前不称"既有通道"。
- **cardano-cli 版本能力矩阵(P2)**:draft 原写 `cardano-cli <era> node issue-op-cert` 不准——仓库
  pin/受测流程用 **era-neutral** `cardano-cli node issue-op-cert`(`rotate.sh:51-58`),era 前缀用于
  transaction 命令。pin 支持的 cardano-cli 版本、定义每命令能力矩阵、把 CLI 版本与 ledger era 分离、据探测
  能力/版本生成命令(而非用户给的 era 串)、对每个受支持二进制 golden test。

### runtime schema 兼容性(P2 修正)
现状 `Machine` 无 runtime 字段(`domain.rs:70-76`),根 + machine schema 均 `additionalProperties:false`
(`pool-spec.schema.json:6,160`)。加 runtime 声明需:定 optional-v1 vs required-v2 语义 + spec-version
迁移、machine 级 schema 形状、**永远 fail-safe 的默认**、迁移命令、版本偏斜(控制/目标不同 schema 代)
行为、**S0016 网站生成器更新归属(本 spec 承接)**、示例、跨版本测试。

### Risk and rollback strategy
- target-mutating 传输是新特权面 + teardown 残留是新失败面:`ouro-ops init`/`deinit` 用 trap 兜底 +
  幂等 + 安装账本逆操作;下次运行先检测清理陈旧残留;每步失败注入测试。
- 探测器是新泄密面:封闭投影 + 白名单 + fail-closed,禁止把 raw `docker inspect`/`ps auxe`/`systemctl cat`
  丢给 agent;不给 agent 目标机 shell。
- 冷签脚本是新外泄面:签名 bundle + 冷机验签 + 回传白名单 + confinement。
- 回滚:增量前向;`ouro-ops deinit` 是一等还原路径;引入问题则新增修正 item 或 `git revert`。

## References
- docs/specs/completed/20260709T2334-S0016-skill-web-onboarding.md(上游:分发 + 版本 + web/prompt 安全;
  威胁模型 #2 控制机不受机制保护;受限 agent runner 未实现)
- docs/specs/completed/20260708T0710-S0015-containerized-e2e.md(机制层现状、no-leak #3、L3 只读姿态)
- docs/reliable-skills.md(机制隔离、可证伪测试)
- crates/ouro/src/{cli.rs,ssh.rs,secrets.rs,config.rs,pool.rs,kes.rs,domain.rs,audit.rs}(现状:dispatch =
  SSH-exec-only/no-scp;凭据扁平布局;register-tx/kes 占位;Machine 无 runtime;audit 无签名)
- fixtures/e2e/{Dockerfile.base,node/Dockerfile,bp/Dockerfile,provision.sh}(目标机底座参考配方;二进制名
  不一致 `ouro` vs `ouro-ops`;keyscan≠认证)
- ouro-skills/lib/ouro-lib.sh(现状 redactor 只匹配 cold|vrf|skey + creds://)
- ouro-skills/{runtime,upgrade,kes-rotation,deploy}/scripts/*.sh(裸进程假设所在;takeover 要 cold.skey)
- schemas/pool-spec.schema.json(additionalProperties:false,无 runtime)
- Cardano Secure Transaction Workflow / Block Producer Deployment(官方:在线 build → 气隙 sign → 在线
  submit;era-neutral issue-op-cert;cardano-cli v11 vs bed 10.14)

## 3. Execution Plan
> 草案:四条 track。item 粒度在激活前可继续细化。评审(2026-07-10)后新增/修订项以 (rev) 标注。
> **依赖**:p3-3 首跳认证须部分**先于** p1 首次特权写;p1 provisioning 是 p2 执行的前提;p4 安装/restart
> 依赖 p1 传输 + p2 supervisor;p4 deploy 依赖已 provision 目标。顺序非纯线性,见各项。

### p1 — 目标机 provisioning(Model P + 三补强)
- [ ] p1-1 (rev) target-mutating bootstrap 传输,**独立模块 `bootstrap.rs`**;有意突破现状 exec-only/no-scp;
  bootstrap 凭据独立于 `ouro-exec`、agent 不可得(enforcement 见 p1-7);**per-op dispatch 的 no-scp 断言保留**,
  init 传输走独立断言
- [ ] p1-2 (rev) `ouro-ops init`:对真机幂等 provisioning(用户 + wrapper + sudoers + pubkey-only sshd +
  ouro 二进制 + authorized_keys + 主机密钥 pin);参考配方 = 修复后的 `node/Dockerfile`;**二进制路径/名定稿**
  `/usr/local/bin/ouro-ops`
- [ ] p1-3 底座最小化(只留约束必需项)
- [ ] p1-4 可审计:`ouro-ops init` 输出安装清单(逐条可核对)
- [ ] p1-5 (rev) `ouro-ops deinit`/uninstall:**deinit 状态机**——全局锁、拒绝在途工作、运行中节点默认拒绝
  (`--leave-node-running`/`--stop-then-remove`)、逆序(先验证替代接入再逆转 sshd、最后移除主体)、审计处置
  显式、每步失败注入
- [ ] p1-6 (rev) 操作自清理 + **state 三层命名空间**(per-process / per-invocation / workflow-scoped by
  audit-id,仅终态删除 + 崩溃 TTL GC);清理错误可见/入审计
- [ ] p1-7 (new) **bootstrap 凭据 enforcement**:§1 Constraints A(独立主体/设备 + 强制 TTY + 带外一次性授权 +
  禁 `--dispatch`/非交互)或 B(硬件密钥 + touch/PIN);若都不做则降级为诚实纪律声明 + 把受限 agent-run 面
  列为前置依赖
- [ ] p1-8 (new) **bootstrap 输入契约 + 平台/架构矩阵**:认证法/主体/sudo/bastion/恢复 console;Debian·Ubuntu +
  RHEL-family + systemd 有无 + x86_64/aarch64;据目标事实选签名产物 + 装前验签/摘要 + 原子安装 +
  `sshd -t`/`visudo -c` + 二次登录;不支持主机 fail-closed
- [ ] p1-9 (new) **版本化 root-owned 安装账本**:created-vs-adopted 属主 + 改前摘要/备份 + 精确逆操作;冲突
  默认拒绝除非 `--adopt`
- [x] p1-10 (new) **参考 fixture 修复**:E2E base 的 `ouro`→`ouro-ops` 名一致 + 干净构建作为 p1-2 前置

### p2 — 节点托管模式感知(LLM 判定 + 机制不泄密 + 候选绑定)
- [x] p2-1 (rev) confined 只读探测工具 `detect/runtime`(L3 姿态),采集托管信号,输出**带类型封闭投影**
  (布尔/枚举/端口/不可变 ID/哈希;绝不序列化 raw env/argv/inspect/unit/mounts/labels;未识别源 fail-closed)
  (交付:新 `detect` skill + `ouro-lib.sh` 只读探测原语,读 /proc cgroup + `--format` 单字段;bare/systemd/
  docker/podman/ambiguous/none;双节点→ambiguous。**注**:目标侧 ouro-diag 只读 principal 的 dispatch posture = p1)
- [x] p2-2 (rev) 脱敏作**纵深防御**(复用 S0015 no-leak #3 corpus/fingerprint);canary 断言 + 未知字段拒绝
  (交付:封闭投影 by-construction 不 emit raw;探测器不碰 key 目录;canary 测试含 S0015 corpus token)
- [x] p2-3 LLM 基于封闭投影判定托管模式 + **解释候选**(顾问性;写进相关 SKILL.md 决策树)
  (交付:`detect/SKILL.md` 决策树教 agent 读 mode/evidence + ambiguous→停手;生命周期侧 mode 分支集成 = p2-5)
- [~] p2-4 (rev) spec `runtime` 字段 + **schema 迁移兼容**(optional-v1/required-v2、fail-safe 默认、迁移命令、
  版本偏斜行为、**S0016 生成器更新归属**、示例、跨版本测试)+ `ouro-ops init` 探测记录验证声明
  (**已交付**:optional-v1 `runtime{mode,unit,container,image}` schema + `Machine.runtime` domain + 一致性校验
  (systemd 需 unit、docker/podman 需 container|image)+ 示例 + schema/domain 测试;absent=fail-safe 由检测治理。
  **待**:required-v2 决策、init 记录+验证声明(依赖 p1 init)、检测↔声明交叉核对 exit-40(=p2-5)、生成器可选 emit)
- [~] p2-5 (rev) 生命周期/upgrade 走托管器,分模式 + **绑定候选身份**(候选图 + 稳定 ID + 证据;token 绑定
  候选证据哈希 + 动作 + digest;执行前重快照防 TOCTOU);动作前机制复验 + 人工确认显示 ground-truth
  (**已交付 p2-5a**:adapter 分模式 restart 分发(bare/systemd/container);effective-mode = 检测治理 + 声明
  交叉核对;**目标(unit/container)由检测解析,绝非 LLM 传参**;bare 真节点 e2e 透明无回归;container 升级
  =换镜像未建模→fail-closed。**已交付 p2-5b 原语**:确认令牌绑定目标指纹——`ConfirmationToken.evidence`
  (可选)+ `confirm create --runtime-evidence`;consume 要求 live 指纹与批准指纹相等(不符/缺失即拒,不烧令牌);
  探测器输出 `data.evidence_hash`(mode|unit|container|digest 的稳定哈希,单一定义);带证据令牌单测 + 探测器指纹
  稳定单测。**待 p2-5b enforcement**:run_tool_dispatch 对破坏性工具强制"detect→consume(live 指纹)→dispatch"门
  + 3 个 e2e harness(restart/upgrade/kes)接入确认流。**待 p2-9**:systemd/container 真环境 e2e。)
- [x] p2-6 (rev) fail-closed:模式/候选模糊、混合/嵌套/多节点、幻觉选错、与声明不符 → exit 40 + conflict 码
  (交付:`ouro_node_guard_mode` 在**脚本顶层**(非子 shell,修掉一个 fail-closed 绕过 bug)对
  none/ambiguous/mismatch 发 exit 40;多节点→ambiguous;"幻觉选错候选"被结构性消除——目标由机制检测解析而非
  LLM 传参;探测器 conflict 码 + guard 结构化错误码。单测覆盖顶层终止 + 各分支)
- [x] p2-7 no-leak 可证伪测试:向探测器植入 env/config/inspect 里的 key-shaped + 通用 canary,断言 agent 侧
  输出零泄露(交付:`test_detect_runtime.py` 向 cgroup/cmdline/`docker inspect` 植入 5 类 canary,全模式零泄露;
  supervisor gate 注入 `pkill` 探针即失败)
- [x] p2-8 (new) **中心 supervisor adapter**:`ouro_node_*` 统管 detect/stop/start/restart/recreate/status/verify;
  逐一改写 runtime/upgrade/kes-rotation/deploy 全部生命周期脚本只调 adapter;**静态闸**禁 adapter 外
  `pgrep|pkill|setsid|systemctl|docker|podman`
  (交付:bare 模式 adapter + 静态闸 + bare 真节点 e2e;systemd/container 分模式 = p2-5 层叠,多模式 e2e 待 p2-9 fixtures)
- [ ] p2-9 (new) **托管模式测试 fixtures**:systemd-in-docker / 嵌套 docker / podman / 混合托管目标 +
  `make e2e-t2-runtime-modes`(补 TC-5/TC-7 基座)

### p3 — 审计完整性 & 传输安全
- [ ] p3-1 (rev) 目标机权威审计**哈希链 + 反签名 + 独立远端锚点 + ack**(机器身份 + 单调序号;检测修改/删除/
  前缀回滚/重排/重放/fork/丢失 ack;签名密钥 enroll/rotate/revoke;可用性 fail/queue 策略)
- [ ] p3-2 (rev) 主机密钥 pin:dispatch 侧强制校验取代 accept-new;**轮换仪式**(旧 key 证明或新带外指纹 +
  审计 + rollback;无裸 `--repin`)
- [ ] p3-3 (new) **首跳认证前置**(单一归属,部分先于 p1-2):期望 SHA256 指纹或 SSH CA 经独立信道;首个
  bootstrap 用 per-machine known_hosts + `StrictHostKeyChecking=yes`;覆盖恶意 enroll / DNS·IP 重用

### p4 — 冷签脚本流程(deploy + kes-rotation 的 air-gapped 签名)
> 现状:kes-rotation 的"离线冷签"是一句提示无脚本;deploy **完全没建模**冷签;且 `kes generate/push`、
> `pool register-tx` 均为占位(§1 Background 3)。目标:两者生成**环境专属、数据内嵌**的冷签脚本,经**签名
> bundle 协议**分发,操作者气隙机核对后执行即出材料。冷机需 cardano-cli + 离线验签器,无需 `ouro-ops`。
- [ ] p4-1 (rev) `ouro-ops kes cold-sign-script`:据运行时 KES vkey + kes-period + 冷环境参数生成**自包含 bash
  脚本**——内嵌**公开** vkey/period,顶部环境配置变量;跑 **era-neutral** `cardano-cli node issue-op-cert`
  就地读 cold.skey 出 `node.cert`。**脚本不含任何私钥**;附脚本生成时间戳 + period 最大年龄 + 建议执行时限
- [ ] p4-2 (rev) `ouro-ops deploy cold-sign-script`:**分阶段**(在线采集校验链快照 → 在线 build unsigned →
  冷机 inspect+签名 → 在线复检提交);产物 `node.cert`/`vrf.skey`/签名 tx 回 BP/提交;**cold.skey + counter
  留冷机**;定义快照最大年龄/validity 窗口/deposit·fee/找零/witness/多 owner/陈旧输入重建
- [ ] p4-3 (rev) 决策树更新:deploy + kes-rotation `SKILL.md` 加"**问冷环境 → 生成 bundle → 冷机核对签名 →
  执行 → 回装/提交**";kes 回装走 p4-6 真 `kes push`(counter 防重放 + confirm 门);deploy 装 vrf.skey/node.cert
  (经 p4-9 VRF 协议)+ 提交注册 tx 后再 `deploy/start`
- [ ] p4-4 安全不变量:冷签脚本只带公开数据、就地读冷密钥;**cold.skey 永不移动**(限定新池 deploy + kes);
  deploy 仅 **vrf.skey 一把私钥**经 p4-9 协议从冷机搬到 BP;agent/`ouro-ops` 永不请求/打印冷或 KES 私钥
- [ ] p4-5 (rev) **cardano-cli 版本能力矩阵**:pin 支持版本 + 每命令能力表 + CLI 版本与 ledger era 分离 + 据探测
  生成命令 + 每二进制 golden test;脚本离线可跑;对 bed(cold 同机)验证整条往返
- [ ] p4-6 (new) **真 KES generate/export/push 前置**(替换占位):目标侧真 `key-gen-KES` + `kes.skey` 原子保留 +
  真 `kes.vkey` 认证导出 + 真 opcert 格式/哈希解析校验 + 目标侧安装 `node.cert` + 托管感知 restart + rollback
- [ ] p4-7 (new) **冷 counter 权威 + 恢复语义**:文件格式/属主/锁/原子备份/期望前后值/断电后恢复;在线 bundle
  含 genesis hash/tip slot/slotsPerKESPeriod/period/采集时间/最大年龄;安装前重查链校验单调
- [ ] p4-8 (new) **签名 bundle 协议**:发布签名+版本 pin 的不可变模板独立装冷机 + 签名 manifest 公开数据 +
  独立信道摘要核对 + 人工 view/审读 + 断网 confinement + 全新输出目录 + 回传文件白名单
- [ ] p4-9 (new) **VRF 传输协议 + takeover 边界**:操作员中介加密传输(收件人密钥/完整性/权限/原子安装/无内容
  审计/安全删除/重放)+ 真双机测试;显式声明 takeover 冷密钥迁移不在本 spec(或改 takeover 拒绝目标驻留冷密钥)

## 4. Test and Acceptance Criteria
> (rev) = 评审后强化;(new) = 评审新增。可证伪性:每条须有清晰 pass/fail observable + 对应测试基座。

- TC-1 provisioning Model P:`ouro-ops init` 幂等(重复运行 changed=false)且输出可核对的安装清单。
- TC-2 (rev) 可卸载 + 还原:从**非默认预存态**起(预存 sshd 配置/用户/文件),`ouro-ops deinit` 后**逐字节
  验证还原** created 对象已删、adopted 对象保留;运行中节点场景分"空闲机"/"曾运行节点";每步失败注入;审计
  处置按清单可判定(导出后删 或 保留标注)。
- TC-3 (rev) 操作自清理:定 **N≥20** + 基线 manifest + 限定扫描路径 glob + 排除合法副作用(devnet db/gateway
  marker);连续 N 次 `ouro-ops tool run` 后目标机 workflow-scoped state 计数回基线;并发/崩溃 workflow 不串。
- TC-4 (rev) provisioning 凭据隔离(**拆分**):(a) bootstrap key 路径/内容不出现在 `ouro-ops` JSON/audit/
  `confirm preview`/corpus fingerprint;(b) **敌意 agent** 尝试读取/列出 bootstrap 凭据 + 调用 `ouro-ops
  init/deinit` + 裸 `ssh` + 复用硬件密钥——全部在 OS/能力边界失败;(c) `ouro-exec` 对 init 传输命令 sudo 拒绝。
- TC-5 (rev) 托管模式感知:节点跑在 systemd/docker/podman 下(p2-9 基座),`detect/runtime` 正确判定模式 +
  候选身份;生命周期操作走托管器(无 pkill+setsid 脑裂);断言 ground-truth(unit 名/container id/image digest);
  模式或候选模糊 → exit 40。
- TC-6 (rev) 探测不泄密(可证伪):向 env/config/inspect/unit 植入 key-shaped **与通用 canary**(短密码/UUID/
  AWS token/URL/非 ASCII),断言 agent 侧输出/日志零泄露;未识别源 fail-closed。
- TC-7 (rev) upgrade 分模式:container 模式(p2-9 基座)下 upgrade = 新镜像 digest 重建(非宿主机换二进制),
  verify 证明新版本 + 节点恢复出块;systemd 模式走 restart unit。
- TC-8 (rev) 审计完整性:构造篡改/删除/前缀回滚/重排/重放/fork,控制侧哈希链+反签名+锚点校验能识破;丢失
  ack 按可用性策略处理;说明无独立锚点时哪些仍不可能。
- TC-9 (rev) 主机密钥 pin:覆盖(a)首连 MITM(替换 host key)被拒;(b)**enroll 期 MITM**(无期望指纹时拒绝
  写入);(c)DNS/IP 重用;(d)合法轮换仪式后 dispatch 成功;accept-new 不再生效。
- TC-10 (rev) 冷签脚本不含私钥 + **执行时防注入**:生成脚本 fingerprint 扫描零私钥;**篡改脚本**尝试多读
  cold.skey 之外文件 / 多写回传输出时,冷机侧 gate(签名核对/白名单/confinement)拒绝。
- TC-11 (rev) kes 冷签往返(对 bed,经 p4-6 真机制):`kes generate`(真 key-gen)→ `cold-sign-script` → 冷机
  执行出真 `node.cert` → `kes push` 装成功、counter 前进、节点恢复出块;边界:陈旧 period / 双重执行 /
  增 counter 后断电 / 跳号 / 重放 / 错网络·genesis → 报错。
- TC-12 (rev) deploy 冷签往返(对**第二矿池** fixture,非 genesis):在线采集快照 → build unsigned → 冷机签 →
  在线复检提交 → **断言精确 tx hash + 链上注册** → `deploy/start` 出块;边界:陈旧/已花 UTxO、过期 validity、
  缺 witness、多 owner → 提交前被拒 + 可安全重生成。
- TC-13 (rev) cold.skey 不移动(**限定机制面**):kes/新池 deploy 流程的审计 + SSH + `ouro` 传输面确认零
  cold.skey 指纹;deploy 仅 vrf.skey 经 p4-9 协议搬运;takeover 冷密钥迁移显式排除(或 takeover 拒绝目标驻留)。
- TC-14 (new) supervisor 收敛:静态闸断言 adapter 之外无 `pgrep|pkill|setsid|systemctl|docker|podman`;每个
  生命周期 skill × 每种受支持模式 e2e 通过。
- TC-15 (new) bootstrap 平台矩阵:对每个受支持矩阵单元(Debian/Ubuntu·RHEL × systemd 有无 × arch)临时 VM
  init 成功 + 装前验签 + 二次登录;不支持主机显式 fail-closed;装错架构二进制被拒。
- TC-16 (new) runtime schema 兼容:旧 spec / 迁移后 spec / 缺声明(fail-safe 默认)/ 非法托管 ID / 控制·目标
  schema 偏斜——各按定义行为;S0016 生成器产出通过校验。
- TC-17 (new) LLM 候选绑定(可证伪):LLM 选不存在/错误候选、preview 与 act 之间进程变化(TOCTOU)——两者
  都须零写入。
- Pass/fail:上述 TC 全部 pass;任一安全约束(**密码学资料零泄露** / **cold.skey 永不移动(限定域)** /
  **bootstrap 凭据 OS 级隔离** / 模式·候选模糊 fail-closed / 不推翻 S0015 契约)被违反即 fail。

## 5. Execution Log (append-only)
- 2026-07-11T13:20+08:00 p2-5b 原语交付(确认令牌绑定目标指纹):`confirm.rs` 的 `ConfirmationToken` 加可选
  `evidence` 字段;`create(..., evidence)` + `consume(..., evidence)`——带证据令牌只对**批准指纹**生效,live 指纹
  不符=拒(且不烧令牌)、缺失=拒;无证据令牌保持 rollback/kes-push 旧行为(向后兼容)。`confirm create
  --runtime-evidence <fp>` 暴露;`consume_confirmation` 加 evidence 形参(rollback/kes-push 传 None)。探测器
  `detect/runtime` 输出 `data.evidence_hash = fp_ + sha256(mode|unit|container_id|image_digest)[:32]`——人核对
  ground-truth 后用它 issue 令牌、机制 re-detect 后据它放行(TOCTOU)。测试:confirm evidence 单测(不符/缺失/
  单次)、探测器指纹存在+稳定+不泄露。regen bundle-manifest。**待**:enforcement 门(run_tool_dispatch)+ 3
  harness 接入(见 p2-5 备注)。**观察**(非本 spec):`cargo test` 并行下 `embedded_fallback_*` 与
  `committed_manifest_*` 因全局 `OURO_SKILLS_DIR` env 存在既有竞态偶发失败,单线程稳定 35 pass——建议后续给那两个
  测试做隔离(与 S0017 无关)。
- 2026-07-11T12:45+08:00 p2-5a + p2-6 交付(托管模式生命周期分发 + fail-closed):adapter 加
  `ouro_node_detect_mode`(检测 mode,单一真源,与探测器一致由测试保证)、`ouro_node_effective_mode`(检测
  治理 + 声明交叉核对,**纯函数**)、`ouro_node_guard_mode`(顶层 fail-closed)、`ouro_node_restart_mode`
  (分模式:systemd=`systemctl restart <检测出的 unit>`、docker/podman=`<rt> restart <检测出的 cid>`、
  bare=原路径)。**关键安全点**:破坏性动作的目标(unit/container id)一律由**只读检测解析**,绝不接受 LLM
  传参(评审 P1)。4 个生命周期脚本(restart/topology-apply/rotate/upgrade-one)改为 resolve→guard→dispatch;
  upgrade 的 container 路径=换镜像 digest 重建,未建模→**fail-closed**(拒绝对容器做宿主二进制交换的假成功)。
  **修掉一个自身设计 bug**:原打算在 `MODE="$(require_mode …)"` 里 emit+exit,但 exit 只杀子 shell、错误 JSON
  被吞进 MODE、脚本继续跑=**fail-closed 绕过**;改为纯 resolve + 顶层 guard。新增
  `test_supervisor_mode_dispatch.py`:断言 guard **真终止脚本**(mismatch/ambiguous/none,后续行不执行、无 restart
  调用)、各模式分发到正确二进制、detect_mode 与探测器一致。真 bare 节点 `make e2e-t2-runtime` 透明无回归
  (PID 45→228 真重启、topology 228→388、forging 通过)。容器内 cgroup=`0::/`(namespace 隐藏)→ 正确判为 bare。
  regen bundle-manifest。**待**:p2-5b 人工确认令牌绑定证据 + 动手前重核(TOCTOU);p2-9 systemd/container 真 e2e。
- 2026-07-11T12:05+08:00 p2-4 部分交付(spec `runtime` 声明字段,optional-v1 + fail-safe):给
  `pool-spec.schema.json` 的 machine 加**可选** `runtime{mode:bare|systemd|docker|podman, unit?, container?,
  image?}`(additionalProperties:false);`domain.rs` 加 `Machine.runtime: Option<RuntimeDecl>`(`#[serde(default)]`
  向后兼容)+ `RuntimeMode` 枚举 + 一致性校验(systemd 须 unit;docker/podman 须 container|image;absent=未声明,
  由检测治理,不假设)。示例 complete.yaml 加 systemd + docker 声明;新增 invalid-runtime-mode fixture;
  schema 测试(minimal 无 runtime 仍 valid、complete 有 runtime valid、坏 mode invalid)+ domain 单测。regen
  bundle-manifest(schema 内嵌)。**待**:required-v2、init 记录+验证声明(p1)、检测↔声明 exit-40 交叉核对(p2-5)、
  S0016 生成器可选 emit。标 [~]。
- 2026-07-11T11:40+08:00 p2-1/p2-2/p2-3/p2-7 completed(托管模式探测:封闭投影 + 不泄密):新增只读诊断
  skill `detect`(`detect/runtime` 探测器 + `detect/SKILL.md` 决策树)。在 `ouro-lib.sh` 加只读探测原语
  (`ouro_supervisor_container_id/runtime`、`ouro_supervisor_systemd_unit`、`ouro_node_port/count`、
  `ouro_supervisor_image_digest`)——全部读 /proc cgroup(可 `OURO_PROC_ROOT` 注入),docker/podman 只用
  `--format` 抽单字段。探测器输出**带类型封闭投影**:node_running/node_count、mode
  (bare|systemd|docker|podman|ambiguous|none)、signals、evidence(unit/container_id/image_digest)、port、
  conflict;**绝不 emit raw** env/argv/mounts/inspect JSON。fail-closed:双节点 / 多信号 / running-但不可分类
  → ambiguous(p2-6 探测侧;destructive-action 侧 exit-40 强制 = p2-5)。探测器只读、不碰 key 目录、不带 L2
  审计写门(L3 posture;目标侧 ouro-diag principal 的 dispatch = p1)。新增 `test_detect_runtime.py`(TC-6):
  向 cgroup/cmdline/`docker inspect` 植入 5 类 canary(含 S0015 corpus token、AWS key、bech32、PEM),全 6 模式
  断言零泄露;精化 supervisor gate 改为剥离注释+引号后匹配命令调用(允许 `docker` 作枚举/JSON 键,仍捕获真
  `docker rm`/`setsid` 调用)。regen bundle-manifest(新 skill 内嵌)。**注**:目标侧 ouro-diag dispatch
  posture、生命周期 mode 分支(p2-5)、多模式容器 fixtures(p2-9)另行承接。
- 2026-07-11T11:05+08:00 p2-8 completed(中心 supervisor adapter + 静态闸):在 `ouro-lib.sh` 新增
  supervisor adapter——通用进程原语 `ouro_proc_running/pid/stop`、后台守护 `ouro_daemon_spawn`、
  cardano-node 封装 `ouro_node_running/pid/stop/start/restart`(bare 模式,argv 由 `OURO_DEVNET_DIR` 单一
  推导)。把 `pgrep/pkill/setsid` 从 6 类脚本(runtime restart/topology-apply/verify、upgrade
  upgrade-one/verify、kes-rotation rotate、deploy takeover/takeover-verify)以及遥测网关 install-gateway
  全部收敛到 adapter,消除四处重复的 `setsid cardano-node run …` 与 `pkill;sleep 2`。新增静态闸
  `tests/test_supervisor_gate.py`(TC-14):零容忍禁止 adapter 之外出现
  `pgrep|pkill|setsid|systemctl|docker|podman`(含注释),并断言 adapter API 齐全 + 四个节点生命周期脚本确
  经 `ouro_node_*`。因内嵌脚本变更,按 tamper-evident 机制重生成 `packaging/bundle-manifest.json`。
  **注**:systemd/container 分模式执行 = p2-5(层叠到 `ouro_node_*` 不动调用点);多模式 e2e 待 p2-9 fixtures。
- 2026-07-11T10:30+08:00 p1-10 completed(参考 fixture 修复,P3 二进制路径定稿 + 干净构建前置):
  统一二进制规范名/路径为 `/usr/local/bin/ouro-ops`(Cargo bin = `ouro-ops`,`ouro` 仅是 lib)。改 5 处
  名字不一致:`Dockerfile.base`(strip/COPY `ouro`→`ouro-ops`,去掉掩盖失败的 `|| true`)、`bp/Dockerfile`
  (COPY 路径)、`e2e-t2.sh`(`OURO_BIN`)、`tests/_ctx.py`(`target/debug/ouro`→`ouro-ops`,此前测试碰巧
  用旧残留二进制)。删除陈旧 `target/{debug,release}/ouro` 残留。**干净构建暴露并修掉更深的遗留断裂**:
  S0016 编译期内嵌(`build.rs` walk `ouro-skills/`+`schemas/` 生成 `EMBEDDED`)后,`Dockerfile.base` 的
  builder 阶段从未 COPY `build.rs`/`ouro-skills`/`schemas` —— 之前靠 `|| true` + 缓存镜像掩盖,`make
  e2e-build-base` 干净构建实际会 `E0425 EMBEDDED not found` 失败。补齐三处 COPY 后构建通过。
- 2026-07-11T10:10+08:00 spec 激活:draft → active,Start Time 记录,移入 `docs/specs/`
  (`20260711T1010-S0017-production-provisioning.md`)。Previous Spec-ID=S0016。按依赖顺序执行:
  p1-10(fixture 修复,p1-2 前置)→ p1-1(bootstrap 传输模块)→ p2-8(supervisor adapter)→ … ;
  需真实基础设施的项(真 cardano-cli/devnet、systemd·docker fixtures、气隙机)在无基座环境下如实标注阻塞。
- 2026-07-09 draft 创建:从 S0016 拆出。承接两处盘点结论(无真机 provisioning、裸进程假设),
  确立 Model P + 三补强、托管模式感知(机制采集脱敏 → LLM 判定 → 机制走托管器)、审计反签名 +
  主机密钥 pin。修正 F2(操作自清理改为"只写/清理 /tmp state,不推脚本"——脚本随内嵌二进制)、
  F3(主机密钥 pin 单一归属本 spec p3-2)、F4(新增审计反签名 TC-8)。
- 2026-07-10 新增 **p4 冷签脚本流程 track**(用户定案,归入 S0017 而非 S0016 网站):deploy 也要建模冷签
  (现状完全没有);两者都生成**环境专属、数据内嵌**的 cardano-cli 冷签脚本,操作者气隙机跑一下出材料
  (冷机无需 `ouro-ops`)。核心安全性质:脚本只带公开数据、就地读冷密钥,**cold.skey 永不移动**;deploy
  仅 vrf.skey 一把私钥从冷机搬到 BP。新增 TC-10..13(含对 bed 的往返验证)。**待激活后实现**——需
  cardano-cli/era 正确性 + 对真 devnet 验证,不在 S0016 迭代内草率做。
- 2026-07-10 **multi-agent review(claude+codex+cursor)**:三方 REQUEST_CHANGES,去重 P0 4·P1 8·P2 5·P3 3,
  12/20 交叉确认,P0/P1 经对照真实代码逐条复核成立(零误报)。评审产物见
  `code_review/S0017-production-provisioning/{claude,codex,cursor,summary}.md`。
- 2026-07-10 **按评审修订 draft(agreement≥2 的 17 项;仍为 draft,未激活)**:
  - P0-1 bootstrap 凭据隔离改为 OS/能力级 enforcement(§1 Constraints + p1-7);诚实复贴 S0016 边界。
  - P0-2 冷签脚本改为签名 bundle 协议 + 冷机验签 + 回传白名单(§2 冷签 + p4-8);修正"冷机只需 cardano-cli"。
  - P0-3 托管器输出改为带类型封闭投影(§1 Constraints + §2 探测 + p2-1);形状过滤降为纵深防御。
  - P0-4 deploy 注册 tx 改为在线 build/冷签/在线 submit 分阶段(§2 冷签 + p4-2/p4-6);替换 `pool.rs` 占位。
  - P1:supervisor adapter 收敛 + 静态闸(p2-8/TC-14);首次特权接入 + 平台矩阵(p1-8/TC-15);deinit 状态机 +
    安装账本(p1-5/p1-9/TC-2);host-key 首跳认证前置 + 轮换仪式(p3-3/TC-9);LLM 候选身份绑定 + TOCTOU
    (§2 loop + p2-5/TC-17);vrf 传输协议 + takeover 边界(p4-9/TC-13);KES period/counter 权威(p4-7/TC-11);
    真 KES 机制前置(p4-6)。
  - P2:runtime schema 迁移兼容(p2-4/TC-16);cardano-cli 版本矩阵 + era-neutral(p4-5);state 三层命名空间
    (p1-6/TC-3);审计哈希链 + 独立锚点(p3-1/TC-8)。
  - P3:二进制路径/名定稿 `/usr/local/bin/ouro-ops` + fixture 修复(§2 + p1-10)。
  - 未纳入(仅 1/3):Background 命令盘点补 manifest/self-update/skill(已顺带在 Background 补全);"凭据布局
    已就绪"措辞(已顺带在 Background 改为"需扩展")。

## 6. Validation Evidence (append-only)
- p1-10 | stack: rust | command: cargo build --release --locked | result: pass | note: 产出
  target/release/ouro-ops(无 ouro);strip 无需 `|| true` 即成功。
- p1-10 | stack: rust | command: cargo test -q | result: pass | note: 33 passed / 0 failed,名字修正无回归。
- p1-10 | stack: python | command: python3 tests/test_tool_output_schema.py && python3 tests/test_deploy_scripts.py
  | result: pass | note: 删除陈旧 target/debug/ouro 后,经 _ctx.py 走新 ouro-ops 路径仍全 pass(证明此前
  测试确在用旧残留二进制)。
- p1-10 | stack: docker | command: docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . |
  result: pass | note: 干净构建端到端成功(此前 E0425 EMBEDDED 失败);最后一步 `/usr/local/bin/ouro-ops
  version` 通过。
- p1-10 | stack: docker | command: docker run --rm ouro-e2e-base:local /usr/local/bin/ouro-ops version && … skill list
  | result: pass | note: 镜像内仅规范路径 ouro-ops(无短名 ouro);version binary=ouro-ops v0.1.0;内嵌 6
  skills(embedded_digest 非空)证明 build.rs 生效。
- TC-14 | stack: python | command: python3 tests/test_supervisor_gate.py | result: pass | note: 静态闸通过;
  注入 `pkill -f x` 探针即断言失败(可证伪);adapter API 齐全 + 4 节点生命周期脚本经 ouro_node_*。
- p2-8 | stack: bash | command: grep -rE '\b(pgrep|pkill|setsid|systemctl|docker|podman)\b' ouro-skills --include='*.sh' | grep -v lib/ouro-lib.sh
  | result: pass(0 命中) | note: 全部原语仅存于 adapter;bash -n 十个脚本全通过。
- p2-8 | stack: rust | command: cargo test -q(regen bundle-manifest 后) | result: pass | note: 33 passed;
  committed_manifest_matches_embedded 在重生成 manifest 后一致。
- p2-8 | stack: e2e | command: make e2e-t2-runtime | result: pass | note: 真 forging 节点验证 adapter 路径:
  restart PID 45→176(真重启)、verify forging、topology-apply 176→314 重启 + 幂等 changed=false + 再验证
  forging;bed 自动 teardown 无残留容器。(bare 模式;systemd/container 模式待 p2-9。)
- TC-6 | stack: python | command: python3 tests/test_detect_runtime.py | result: pass | note: 全 6 模式
  (docker/podman/systemd/bare/ambiguous-双节点/none)判定正确;cgroup/cmdline/inspect 植入 5 类 canary
  (S0015 corpus token/AWS key/password/bech32/PEM)零泄露;RAW `docker inspect` 含 canary 但探测器只调 --format 证明不走 raw。
- p2-1 | stack: shell | command: ouro-ops tool run detect/runtime --machine devhost | result: pass | note:
  内嵌探测器经真 tool-run 路径提取执行,dev host 无节点 → mode=none 封闭投影,合 tool-output schema。
- p2-2/p2-7 | stack: python | command: python3 tests/test_supervisor_gate.py(精化后) | result: pass | note:
  剥离注释+引号匹配命令调用;detect 探测器的枚举/JSON-键 `docker` 放行,注入 `docker rm`/`setsid` 仍被捕获(可证伪)。
- p2-1..7 | stack: rust+python | command: cargo test -q; 全 python 套件 | result: pass | note: cargo 33 pass
  (regen bundle-manifest 后);python 15 pass(含新增 test_detect_runtime)。
- TC-16(部分) | stack: python | command: python3 tests/test_pool_spec_schema.py | result: pass | note: 旧 spec
  (minimal 无 runtime)仍 valid=向后兼容;complete(systemd+docker 声明)valid;坏 mode(kubernetes)invalid。
- p2-4 | stack: rust | command: cargo test -q runtime_declaration | result: pass | note: absent=valid(fail-safe)、
  systemd+unit valid、systemd 缺 unit / docker 缺 container&image 被拒、bare 无需目标字段;cargo 全量 34 pass。
- p2-5/p2-6 | stack: python | command: python3 tests/test_supervisor_mode_dispatch.py | result: pass | note:
  guard 顶层**真终止脚本**(mismatch/ambiguous/none:后续行不跑、零 restart 调用);systemd→systemctl restart
  <unit>、docker→docker restart <cid12>、bare→不碰 systemctl/docker;detect_mode 与探测器 4 模式全一致。
- p2-5a | stack: e2e | command: make e2e-t2-runtime | result: pass | note: 真 forging 节点,mode-aware 分发对
  bare 透明:restart PID 45→228 真重启、topology-apply 228→388、幂等、forging 通过;容器内 cgroup=0::/ 正确判 bare。
- p2-5/p2-6 | stack: rust+python | command: cargo test -q; 全 python | result: pass | note: cargo 34、python 16
  (含 test_supervisor_mode_dispatch);supervisor gate 仍绿(新 systemctl/docker/podman 仅在 adapter)。
- p2-5b | stack: rust | command: cargo test -q evidence_bound | result: pass | note: 带证据令牌:live 指纹不符/
  缺失被拒且不烧令牌、匹配则单次生效;向后兼容(无证据令牌 rollback/kes-push 不变)。
- p2-5b | stack: python | command: python3 tests/test_detect_runtime.py | result: pass | note: `data.evidence_hash`
  fp_ 前缀 35 长、同目标稳定、canary 零泄露;探测全模式 + no-leak 不回归。
- p2-5b | stack: rust+python | command: cargo test -q -- --test-threads=1; 全 python | result: pass | note:
  cargo 35 pass(regen manifest 后;单线程规避既有 env 竞态)、python 16 pass。

## 7. Change Requests (append-only)
- 2026-07-10 评审驱动的 draft 强化(见执行日志);属 draft 阶段自由编辑,不改变 S0017 的范围边界
  (仍 = provisioning + 托管感知 + 审计完整性 + 冷签流程),仅把"红线已宣示、机制未指定"补成可执行设计 + 可
  证伪 TC。范围唯一微调:显式声明 **takeover 冷密钥迁移不在本 spec**(P1 vrf 修正,留待后续 spec)。
