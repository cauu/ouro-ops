# Production Provisioning & Real-Node Lifecycle

Spec-ID: S0017
Status: draft
Created Time: 2026-07-09T11:00:00+08:00
Start Time:
Completion Time:
Previous Spec-ID: S0016
Closure Reason:

## 1. Requirement Details

### Background
从 S0016 拆分而来。S0016 解决"决策层随 prompt 分发 + 机制层(`ouro` 二进制)的取得与版本",
但**机制层在真实生产的落地**——把一台裸机武装成受约束目标机、并在真实托管方式(systemd/
容器)下正确执行生命周期操作——体量大、风险高,独立成本 spec。

盘点结论(S0016 讨论期得到,`ouro` 顶层命令 = audit/confirm/config/kes/legacy/pool/rollback/
spec/status/tool):

1. **无真机 provisioning**:`ouro` 顶层**无 init/provision/bootstrap**。`ouro-ops audit init` 仅初始化
   本地审计库;`deploy/provision` 只铺节点非安全状态(假设目标机已 provisioned);
   `fixtures/e2e/provision.sh` 是 docker-exec **测试夹具**,非产品命令。目标机安全底座
   (`ouro-exec`/`ouro-diag` 用户、`/usr/local/sbin/ouro-tool-run` wrapper、`/etc/sudoers.d/ouro-exec`、
   pubkey-only sshd、`ouro` 二进制)**今天只烤在 `node/Dockerfile`、`bp/Dockerfile` 里,无真机命令**。
   现状 dispatch 为 **SSH-exec-only 且有测试断言 no-scp**(`ssh.rs:129/164`)——真 `ouro-ops init`
   为**净新建**,需引入 target-mutating 传输。控制侧凭据布局(`~/.ouro/credentials/`,`secrets.rs`)
   与目标侧配方(Dockerfile)已就绪可复用。

2. **裸进程假设**:所有 skill 用 `pgrep`/`pkill`/`setsid` 把节点当**裸进程**管
   (`rotate.sh:66/71`、`restart.sh:18/20`、`upgrade-one.sh:56/58`、`topology-apply.sh:31/32`),
   **无任何 systemctl/docker/podman 意识**,spec 也无 runtime 声明。对已容器化/systemd 托管的
   节点运行会**脑裂重启或语义错**(upgrade 尤甚:容器世界升级 = 换镜像 digest + 重建,而非换
   宿主机二进制)。

### Scope
1. **`ouro-ops init`/`deinit`(Model P + 三补强)**:把裸机武装成受约束目标机 / 一键还原;持久
   最小底座,可审计、可卸载;操作自清理。
2. **节点托管模式感知**:机制采集**脱敏**事实 → LLM 判定托管模式(bare/systemd/docker/podman)
   → 机制走**托管器**执行生命周期,全程不泄露密码学资料;模式模糊/未知 → fail-closed。
3. **目标机权威审计的完整性**:对 target-minted 审计**反签名** / 追加式远端留痕;主机密钥 pin
   (取代 accept-new,防首连 MITM)。
4. **冷签脚本流程(deploy + kes-rotation 的 air-gapped 签名)**:据运行时数据 + 用户冷环境,生成
   **环境专属、数据内嵌的自包含冷签脚本**(cardano-cli,冷机无需 `ouro-ops`),操作者拿到气隙机跑一下
   即出冷签材料;回装/提交经既有机制。**脚本只带公开数据、就地读冷密钥,cold.skey 永不移动。**

### Constraints
- **不推翻 S0015 安全契约**:机制不靠提示词、密钥隔离、审计闸门、exit 码纪律不变。本 spec
  引入的 **target-mutating 传输是对现状 exit-only/no-scp 的有意突破**,仅用于 `ouro-ops init` 的
  特权 bootstrap,**与 per-operation dispatch 使用不同凭据**(见 §2)。
- **provisioning 特权凭据与 agent 隔离**:`ouro-ops init` 用的特权 bootstrap 凭据 **agent 永不可得**;
  per-operation dispatch 只走受限 `ouro-exec` 路径。
- **密码学资料零泄露(红线)**:托管模式探测绝不把 key 内容/secret-shaped 串带入 agent 上下文/
  输出/日志;复用 S0015 no-leak(#3)corpus/fingerprint 机制。
- **LLM 判定是顾问,机制说了算**:破坏性动作前机制**复验**托管模式、人工确认显示 ground-truth;
  幻觉/模糊不得驱动破坏性操作。
- skill 唯一真源 = `ouro-skills/*/SKILL.md`;托管模式决策写进真源。

### Non-goals
- 不做零常驻的 Model E(每次操作临时 provision+teardown)——已在 S0016 讨论中否决(避免每次
  人工 bootstrap 与易错 teardown)。
- 不覆盖 S0016 的网站/分发/版本层(那是 S0016)。
- 不引入后端/托管控制面。

## 2. Outline Design

### Provisioning 模型:Model P + 三补强
目标机采用**持久最小底座(Model P)**,不追求零常驻:

- `ouro-ops init`(一次性、特权、人把关):建 `ouro-exec`/`ouro-diag` 用户、装
  `/usr/local/sbin/ouro-tool-run` wrapper + `/etc/sudoers.d/ouro-exec`、pubkey-only sshd、装
  `ouro` 二进制(内嵌 skill,与全机同源)、置控制机公钥、**pin 主机密钥**。参考实现 =
  `fixtures/e2e/node/Dockerfile` 现成配方。用**独立于 `ouro-exec`、agent 拿不到的特权
  bootstrap 凭据**;per-operation dispatch 只走受限 `ouro-exec` 路径。
- **三补强**:①底座最小化(只留约束必需项);②可审计(`ouro-ops init` 输出安装清单,逐条可核对);
  ③可卸载(`ouro-ops deinit` 彻底清除底座,机器还原)。
- **操作自清理**:因二进制内嵌 skill(S0016 p2-1),per-operation **无需推脚本**;每次
  `ouro-ops tool run` 只写 `/tmp` 临时 state,跑完清理,不累积;唯一有意留存的持久记录是审计。
  (注:此为 F2 修正——脚本随内嵌二进制,不再"每次推脚本"。)

### 托管模式感知:机制采集脱敏 → LLM 判定 → 机制执行
```
① 机制采集:confined 只读探测 detect/runtime(L3 姿态:只读、无 key 目录访问)
     采集:systemd unit? docker/podman 容器? bare? + config/socket/port/restart-policy/image digest
     输出:结构化【脱敏】事实(key 只报路径/存在/权限,secret-shaped 串过滤)

② LLM 判定:agent 只看脱敏事实,推断托管模式(顾问性)——全程看不到任何 key

③ 机制执行:走托管器,分模式
     bare      = 换二进制 + setsid(现状路径)
     systemd   = 换二进制 + systemctl restart <unit>
     container = pin 新镜像 digest + 重建容器
     动作前:机制【复验模式】+ 人工确认显示 ground-truth
     模式模糊/未知/与 spec 声明不符 → exit 40 停手要人
```

### 审计完整性
- 目标机权威审计**反签名**(控制侧可识破被篡改的 target-minted 审计)/ 追加式远端留痕。
- 主机密钥 pin:`ouro-ops init` enroll 时从可信位置捕获目标真机 host key,dispatch 侧**强制校验**,
  取代 accept-new。

### Risk and rollback strategy
- target-mutating 传输是新特权面 + teardown 残留是新失败面:`ouro-ops init`/`deinit` 用 trap 兜底 +
  幂等;下次运行先检测清理陈旧残留。
- 探测器是新泄密面:白名单输出 + 脱敏,禁止把 raw `docker inspect`/`ps auxe`/`systemctl cat`
  丢给 agent;不给 agent 目标机 shell。
- 回滚:增量前向;`ouro-ops deinit` 是一等还原路径;引入问题则新增修正 item 或 `git revert`。

## References
- docs/specs/draft/S0016-skill-web-onboarding.md(上游:分发 + 版本 + web/prompt 安全)
- docs/specs/completed/20260708T0710-S0015-containerized-e2e.md(机制层现状、no-leak #3、L3 只读姿态)
- docs/reliable-skills.md(机制隔离、可证伪测试)
- crates/ouro/src/{cli.rs,ssh.rs,secrets.rs}(现状 dispatch = SSH-exec-only/no-scp;凭据布局)
- fixtures/e2e/{node,bp}/Dockerfile(目标机安全底座的参考配方)
- ouro-skills/{runtime,upgrade,kes-rotation}/scripts/*.sh(裸进程假设所在)

## 3. Execution Plan
> 草案:四条 track。item 粒度在激活前可继续细化。

### p1 — 目标机 provisioning(Model P + 三补强)
- [ ] p1-1 target-mutating bootstrap 传输(经特权 SSH 推二进制 + 写文件;有意突破现状
  exec-only/no-scp;bootstrap 凭据独立于 `ouro-exec`、agent 不可得)
- [ ] p1-2 `ouro-ops init`:对真机幂等 provisioning(用户 + wrapper + sudoers + pubkey-only sshd +
  ouro 二进制 + authorized_keys + 主机密钥 pin);参考配方 = `node/Dockerfile`
- [ ] p1-3 底座最小化(只留约束必需项)
- [ ] p1-4 可审计:`ouro-ops init` 输出安装清单(逐条可核对)
- [ ] p1-5 `ouro-ops deinit`/uninstall:彻底清除底座,机器还原
- [ ] p1-6 操作自清理:每次 tool run 只写 `/tmp` 临时 state 并清理,N 次操作后目标机无累积残渣

### p2 — 节点托管模式感知(LLM 判定 + 机制不泄密)
- [ ] p2-1 confined 只读探测工具 `detect/runtime`(L3 诊断姿态:只读、无 key 目录访问),采集
  托管信号(systemd unit / docker|podman container / bare;config/socket/port/restart-policy/
  image digest),输出结构化【脱敏】事实
- [ ] p2-2 脱敏/fingerprint 过滤(复用 S0015 no-leak #3 corpus/fingerprint):key 只报路径+存在+
  权限,绝不报内容;env/inspect/命令行输出过滤 secret-shaped 串(bech32/CBOR hex/PEM)
- [ ] p2-3 LLM 基于脱敏事实判定托管模式(顾问性;写进相关 SKILL.md 决策树)
- [ ] p2-4 spec `runtime` 字段(声明:mode=bare|systemd|docker|podman + unit/container/image)+
  `ouro-ops init` 探测记录(验证声明,非假设)
- [ ] p2-5 生命周期/upgrade 走托管器,分模式(bare=换二进制;systemd=换二进制+restart unit;
  container=pin 新镜像 digest+重建);动作前机制【复验模式】+ 人工确认显示 ground-truth
- [ ] p2-6 fail-closed:模式模糊/未知/与声明不符 → exit 40 停手要人(不猜、不与未知托管器打架)
- [ ] p2-7 no-leak 可证伪测试:向探测器植入 env/config 里的 key-shaped 物,断言 agent 侧输出零泄露

### p3 — 审计完整性 & 传输安全
- [ ] p3-1 目标机权威审计反签名 / 追加式远端留痕(控制侧可识破被篡改的 target-minted 审计)
- [ ] p3-2 主机密钥 pin:init enroll 捕获 host key + dispatch 侧强制校验(取代 accept-new)

### p4 — 冷签脚本流程(deploy + kes-rotation 的 air-gapped 签名)
> 现状:kes-rotation 有"generate vkey → 离线冷签 → push"的显式流程,但"离线冷签"是一句提示,无脚本;
> deploy **完全没建模**冷签(假设 opcert/VRF 已 provision)。目标:两者都生成**环境专属、数据内嵌**的
> 冷签脚本,操作者气隙机执行即出材料。冷机只需 `cardano-cli`,无需 `ouro-ops`。
- [ ] p4-1 `ouro-ops kes cold-sign-script`:据运行时 KES vkey + kes-period + 冷环境参数(cold.skey/
  counter 路径、cardano-cli era)生成**自包含 bash 脚本**——内嵌**公开** vkey/period,顶部环境配置变量;
  跑 `cardano-cli <era> node issue-op-cert` 就地读 cold.skey 出 `node.cert`。**脚本不含任何私钥**。
- [ ] p4-2 `ouro-ops deploy cold-sign-script`:初始池冷签脚本——(新池)生成 cold/VRF 密钥对 + counter=0
  初始 opcert + 用 spec 经济参数构造注册/委托证书并冷签出待提交 tx。产物 `node.cert`/`vrf.skey`/签名 tx
  回 BP/提交;**cold.skey + counter 留冷机**。
- [ ] p4-3 决策树更新:deploy + kes-rotation `SKILL.md` 加"**问冷环境 → 生成脚本 → 操作者气隙机执行 →
  回装/提交**"环节;kes 回装走既有 `ouro-ops kes push`(counter 防重放 + confirm 门);deploy 装
  vrf.skey/node.cert + 提交注册 tx 后再 `deploy/start`。
- [ ] p4-4 安全不变量:冷签脚本只带公开数据、就地读冷密钥;**cold.skey 永不移动**;deploy 仅
  **vrf.skey 一把私钥**从冷机搬到 BP(经既有安全通道);agent/`ouro-ops` 永不请求/打印冷或 KES 私钥
  (承接 S0015 红线)。
- [ ] p4-5 cardano-cli 正确性 + era 适配(conway/babbage…);脚本离线可跑(冷机仅 cardano-cli);对
  **bed**(cold 密钥同机可复验签名链路)验证整条往返。

## 4. Test and Acceptance Criteria
- TC-1 provisioning Model P:`ouro-ops init` 幂等(重复运行 changed=false)且输出可核对的安装清单。
- TC-2 可卸载:`ouro-ops deinit` 后目标机无 `ouro-exec`/wrapper/sudoers/ouro 二进制残留(还原验证)。
- TC-3 操作自清理:连续 N 次 `ouro-ops tool run` 后,目标机除审计外无累积脚本/state 残渣。
- TC-4 provisioning 凭据隔离:特权 bootstrap 凭据不出现在 agent 上下文/输出/审计;per-op 只走
  受限 `ouro-exec` 路径(越权尝试被 sudoers 拒绝)。
- TC-5 托管模式感知:节点跑在 systemd/docker 下时,`detect/runtime` 正确判定模式,生命周期
  操作走托管器(无 pkill+setsid 脑裂);模式模糊/未知 → exit 40 停手。
- TC-6 探测不泄密(可证伪):向探测目标 env/config 植入 key-shaped 物,断言 agent 侧输出/日志
  零泄露(复用 S0015 no-leak 断言)。
- TC-7 upgrade 分模式:container 模式下 upgrade = 新镜像 digest 重建(非宿主机换二进制),verify
  证明新版本 + 节点恢复出块/服务。
- TC-8 审计反签名:伪造/篡改 target-minted 审计,控制侧反签名校验能识破。
- TC-9 主机密钥 pin:首连 MITM(替换 host key)被拒绝;accept-new 不再生效。
- TC-10 冷签脚本不含私钥(可证伪):生成的 kes/deploy 冷签脚本经 fingerprint 扫描**零私钥**;内嵌 KES
  vkey 为公开物;cold.skey 仅以变量/路径引用(冷机就地读)。
- TC-11 kes 冷签往返(对 bed):`kes generate` → `kes cold-sign-script` → 冷机(bed cold 同机)执行出
  `node.cert` → `kes push` 装成功、counter 前进、节点恢复出块;缺 cold.skey / 错 era 脚本报错。
- TC-12 deploy 冷签往返(对 bed):生成脚本 → 冷机执行出 vrf.skey/node.cert/注册 tx → 装/提交 →
  `deploy/start` 出块。
- TC-13 cold.skey 不移动:整条 kes/deploy 流程的审计 + 传输面确认 **cold.skey 从不离开冷机**;deploy
  仅 vrf.skey 搬运且经安全通道。
- Pass/fail:上述 TC 全部 pass;任一安全约束(**密码学资料零泄露** / **cold.skey 永不移动** /
  provisioning 凭据隔离 / 模式模糊 fail-closed / 不推翻 S0015 契约)被违反即 fail。

## 5. Execution Log (append-only)
- 2026-07-09 draft 创建:从 S0016 拆出。承接两处盘点结论(无真机 provisioning、裸进程假设),
  确立 Model P + 三补强、托管模式感知(机制采集脱敏 → LLM 判定 → 机制走托管器)、审计反签名 +
  主机密钥 pin。修正 F2(操作自清理改为"只写/清理 /tmp state,不推脚本"——脚本随内嵌二进制)、
  F3(主机密钥 pin 单一归属本 spec p3-2)、F4(新增审计反签名 TC-8)。
- 2026-07-10 新增 **p4 冷签脚本流程 track**(用户定案,归入 S0017 而非 S0016 网站):deploy 也要建模冷签
  (现状完全没有);两者都生成**环境专属、数据内嵌**的 cardano-cli 冷签脚本,操作者气隙机跑一下出材料
  (冷机无需 `ouro-ops`)。核心安全性质:脚本只带公开数据、就地读冷密钥,**cold.skey 永不移动**;deploy
  仅 vrf.skey 一把私钥从冷机搬到 BP。新增 TC-10..13(含对 bed 的往返验证)。**待激活后实现**——需
  cardano-cli/era 正确性 + 对真 devnet 验证,不在 S0016 迭代内草率做。

## 6. Validation Evidence (append-only)
- (待执行)

## 7. Change Requests (append-only)
- (无)
