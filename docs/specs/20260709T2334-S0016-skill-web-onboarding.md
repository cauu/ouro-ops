# Skill Distribution via Static Web Prompt Generator

Spec-ID: S0016
Status: active
Created Time: 2026-07-09T10:00:00+08:00
Start Time: 2026-07-09T23:34:00+08:00
Completion Time:
Previous Spec-ID: S0015
Closure Reason:

## 1. Requirement Details

### Background
`ouro-ops` 的能力以 skill 形式交付(6 个 skill:deploy / kes-rotation / upgrade /
observability / runtime / troubleshooting)。当前让用户的 agent 使用这些 skill,隐含要求
用户先"安装 skill"到 agent。但节点部署与升级是**低频运维**场景,为它常驻安装一套 skill
摩擦过大。

关键洞察(承接 S0015 的"安全靠机制不靠提示词"结论):一个 skill 由两层构成——

| 层 | 谁提供 | 说明 |
|---|---|---|
| 决策层 | `SKILL.md` 决策树 / 停止条件 / 红线 | **可信二进制**(`ouro-ops skill show <op>`,已验签内嵌)——**不经不可信 prompt**(防投毒,评审 R2 N3) |
| 数据 + 操作 + 指针 | 网站生成的 prompt | pool-spec + 跑哪个操作 + "去查 `ouro-ops skill show`" —— 可从网站来 |
| 机制层 | `ouro` 二进制、审计闸门、目标机 wrapper、SSH 凭据 | prompt 里没有 SSH/密钥/wrapper |

"安装摩擦"重的从来不是 `SKILL.md`(几段文字),而是机制层落地——而机制层恰恰是 prompt
替代不了的。本 spec 用一个**纯静态网站**消除"决策层**安装到 agent**"的摩擦:填表单 → 生成
`pool-spec` + 一段**薄操作 prompt**(数据 + 操作 + 指针)→ 粘给 agent;agent 通过
`ouro-ops skill show <op>` **从已验签的本地二进制**拿权威决策树再执行。**决策层与机制层同源于可信
二进制**(都不经不可信 prompt,评审 R2 N3),`ouro` 通过**自更新单二进制**分发。这样既免"往
agent 装 skill",又不给决策层留投毒面。

> **范围边界(从原草案拆分)**:目标机 provisioning(`ouro-ops init`/`deinit`)与真节点生命周期
> (托管模式感知、审计反签名)体量大、风险高、可独立交付,已拆至 **S0017**。本 spec 只负责
> **决策层随 prompt 分发 + 机制层(`ouro` 二进制)的取得与版本**,并把目标机 provisioning
> 作为**由 S0017 交付的前置依赖**引用。

### 交互模型与信任锚(本 spec 的地基)
维持"网站发 prompt、粘给 agent、可信本地 `ouro` 执行"的现状交互。安全成立的**唯一不变量**:

> **prompt 只提供"数据 + 操作 + 指针",绝不提供"代码",也不内联"决策树"。所有**代码**
> (`ouro` 本体、skill 脚本)**与决策层**(`SKILL.md` 决策树)只经身份固定、完整性可校验的
> 可信渠道(已验签二进制)获得——其身份不由 prompt 决定。**

- ✅ 数据从网站来(pool-spec、跑哪个操作、参数):对 **`ouro-ops tool run` 路径**,被信任的本地
  `ouro` + 目标机制会框住它(S0015 已强验证)。
- ❌ 代码绝不能"prompt 里 `curl <prompt给的URL> | sh` 现拉":那等于让恶意 prompt 替换掉机制
  本身,而这段代码跑在握有 fleet 凭据的控制机上,一次投毒即整池沦陷。
- ❌ **决策树也不经不可信 prompt**:agent 用 `ouro-ops skill show <op>` 从**已验签二进制**取决策层,
  仿冒站改不动它(评审 R2 N3)。否则"带官方 manifest digest + 篡改内联决策树"可蒙混过关。
- ⚠️ **但这条不变量只覆盖 `ouro` 路径 + 决策/代码来源**。见 §1「本方案保护什么 / 不保护什么」
  ——控制机整体仍不受机制保护。

信任锚 = 用户**首次主动**从官网确立的 `ouro` 包身份(见 §2 分发 + bootstrap 供应链红线);
此后每次操作不再重新做信任决定。

### 本方案保护什么 / 不保护什么(评审 R1 修订,勿过度承诺)
> 这张表是价值主张的诚实边界。中心不变量保护的是**机制/fleet 路径**,不是控制机整体。

| 维度 | 保护来源 | 结论 |
|---|---|---|
| `ouro-ops tool run` 路径(写权限、密钥、目标机执行) | 机制(wrapper/sudoers/审计/密钥隔离,S0015) | ✅ 机制强制,prompt 无法逾越 |
| `ouro` 本体与 skill 代码来源 | 固定身份 + 完整性校验(非 prompt 决定) | ✅ 见分发红线 |
| **控制机本身**(agent 在 `ouro` 之外的行为:读 `~/.ssh`、`curl\|sh`、外传) | **无机制保护** | ⚠️ = 信任 agent 运行环境;**弱于手动 SSH**(手动不引入一个握凭据主机的云端解释器) |
| **决策层完整性**(agent 遵循的 SKILL 决策树) | 已验签二进制(`ouro-ops skill show`),不经 prompt | ✅ 仿冒站改不动(评审 R2 N3) |
| **拓扑机密性** | 网站不上传;但粘 prompt = 交给 agent 提供商 | ⚠️ 见威胁模型 #4 |

**适用边界**:把不可信 prompt 粘进通用云 agent 是**便利模式,其控制机侧安全弱于手动 SSH**。
要达到"安全路径",需受限 agent-run 硬命令面(仅 `ouro` 子命令、无 shell/网络/凭据旁路)+
对抗性绕过测试(可选加固轨,见 p4-4);否则须向用户明示这是便利模式。

### Scope
1. **表单 → spec/prompt 生成器**:纯静态、纯客户端网站。表单收集机器列表 / 角色(bp|relay)/
   网络 / network_magic / quorum(min_online_relays)/ node 版本 + `pool-spec` schema 要求的其余
   必填字段(ticker/metadata_url/genesis_hashes/topology_mode/sync/relay public_endpoint/creds ref
   —— 逐字段映射,标注安全默认),浏览器本地生成:
   - `pool-spec.yaml`(与 `ouro-ops spec validate` / `schemas/pool-spec.schema.json` 兼容);
   - 一段**薄操作 prompt**:**数据(spec 引用)+ 操作 + 指针 `ouro-ops skill show <op>`**(**不内联决策树**,
     评审 R2 N3)+ **bundle manifest digest**(见 §2)+ `min_ouro_version`(**互操作建议,只能抬高、
     不能降低机制要求**)。决策树由 agent 从**已验签二进制**取。
   - **复制前强制披露**:UI 明示"复制即把拓扑发送给你的 agent 提供商"(威胁模型 #4)。
2. **自更新单二进制分发**:`ouro` 作为**单个静态二进制**分发,内嵌 skill 机制资产;支持
   `ouro-ops self-update`——装一次,之后自己从**签名过的 stable channel** 拉取、**严格验签 + 单调
   防回滚**后在原地替换。首次 bootstrap 只允许**官网列出的单一安装向量**(`brew` 或 `npx`),
   并展示可跨 ≥2 独立渠道核对的签名指纹。目标:装一次 + 永远够新 + **安装 CLI 零运行时依赖** +
   全机同源。(注:**执行** skill 仍依赖目标机的 `bash`/`cardano-cli`/`python3` 等,见 Constraints。)
3. **版本兼容与审计(防 prompt 压低)**:`ouro` 执行前计算
   `required = max(prompt 的 min_ouro_version, 内嵌 skill 地板, 签名 security/revocation 地板)`;
   仅当 `local < required` 才强制自更新到**稳定签名版**;**prompt 值只能抬高、绝不能降低机制
   要求**。审计记录实际执行的确切版本;拒绝低于 required / 已知漏洞版本(单调防回滚 + 签名 denylist)。
4. **bootstrap 命令生成 + 引导**:网站据表单生成**装本机自更新二进制**的单一官方命令 + 签名指纹
   核对步骤;明确区分"一次性 setup"(①装本机二进制(本 spec)+ ②`ouro-ops init` 武装目标机(S0017))
   与"每次操作"。

### Constraints
- **纯静态、纯客户端(边界精确化;p1-fix7 更新)**:生成全在浏览器本地,**拓扑 / pool 数据一个字节都不
  上传**;页面**唯一**出站是加载时对 `api.github.com` 的**一次只读 GET**(查 cardano-node 最新版本,URL
  固定、不含任何用户数据),CSP `connect-src` 锁死该单一 host,页面到不了别处;GitHub 不可达时回退到
  烤入版本列表(near-offline 可用)。**但**"不上传"仅限网站侧——用户**粘 prompt 到云 agent** 仍把拓扑
  交给 agent 提供商(见威胁模型 #4)。副作用:该 GET 让 GitHub 看到"有人打开了本页"(IP/时间),不含
  任何用户/pool 数据。
- **安全仍靠机制,但边界要诚实**:网站是配置/引导前端,不是执行替代品;机制保护 `ouro` 路径,
  **不保护控制机整体**(见「保护什么/不保护什么」)。版本校验是纵深防御,不是唯一防线。
- **self-update 严格验签 + 单调防回滚(红线)**:二进制能改自己,验签一旦松即后门。必须:严格验签 +
  **本地单调防回滚状态**(绝不运行低于已装版本)——该状态须 **tamper-evident**(与 audit DB 同级、
  签名/只追加存储;原子更新;**擦除/重置可被检测并记入审计**,重置后回落到**内嵌 security 地板**而非
  "无下限")+ **签名 release 元数据带序号/时效/吊销**,**写操作要求元数据未过期**(过期即 fail-closed,
  除非显式验签的离线 bundle);叠加**透明日志(Sigstore/rekor)+ 可复现构建**。签名私钥**硬件托管**
  (泄露即验签全线归零)。(评审 R2 N1)
- **bootstrap 供应链红线**:首装是整链 TOFU 弱点。只允许官网**单一**安装向量;包名锁定 + 展示
  签名指纹 + 跨 ≥2 独立渠道(GitHub Releases / Homebrew core / 透明日志)可交叉核对;首装后
  `ouro-ops version`+`ouro-ops contract` 与官网比对;`npx` 必须带精确版本 + integrity(或首选"下载已验签
  tarball")。防 typosquat/假包/bootstrap-key 替换。
- **离线 fallback 完整性**:近乎离线的 BP 须走"手动下载 + 校验 pinned 版本",且离线包**内嵌可离线
  核验的签名材料**(minisign 公钥 / SIGSTORE bundle 打印版);`ouro-ops install --offline` 执行与
  self-update **同等**的验签 + denylist + `max(floor)` 检查;不得因无法自更新而跳过验签或卡死。
- **两个真源,分开 + 同由二进制供给**(评审 R1/R2):`SKILL.md` = 决策层真源;
  `ouro-skills/*/scripts/*.sh` + schema = 机制真源。**二者都编译进二进制**,运行时分别由
  `ouro-ops skill show`(决策)与 `ouro-ops tool run`(机制)供给;由 **release bundle manifest** 绑定
  (二进制 digest / 内嵌决策 hash / 内嵌 skills hash / schema hash / `required_ouro`)。网站**不内联
  决策树**,故无"站点决策文本"漂移面(评审 R2 N3 同时化解 N8);仍禁手抄漂移(S0015 kes 漂移教训)。
- **执行期依赖如实声明**:"零运行时依赖"仅指**安装 `ouro` CLI**;`ouro-ops tool run` 实际 shell out 到
  `bash`(`crates/ouro/src/cli.rs:257`)跑磁盘/内嵌脚本,脚本依赖 `cardano-cli`/`pgrep`/`pkill`/
  `setsid`,目标机装 `python3`/`python3-yaml`——这些是**执行期依赖**,须在文档显式列出。
- 品牌与视觉:专业 / 克制 / 极简,Apple 桌面产品观感,仅 light mode,Cardano 调色板为准;
  无障碍 WCAG 2.1 AA + reduced-motion(遵循 CLAUDE.md)。

### Non-goals
- 不做托管控制面 / 后端执行代理(违背 SPO"保留控制"与密钥隔离模型)。
- 本版不做 deep-link(`ouro run --from <url>`)——推迟到后续 spec。
- 不消除 `ouro` 二进制安装——那是机制层,只能压成一条命令/一次自更新,不能取消。
- **不推翻 S0015 的安全模型与契约**(机制不靠提示词、密钥隔离、审计闸门、exit 码纪律);
  本 spec 会为分发**扩展** skill 的打包/取得方式(内嵌进二进制、版本闸),但不改变其安全契约。
- **不承诺控制机整体安全**:云 agent 旁路(#2)不在机制保证内;本 spec 只降级披露 + 提供可选
  受限 agent-run 加固轨(p4-4),不宣称已闭合。
- **不含目标机 provisioning 与真节点生命周期**——`ouro-ops init`/`deinit`、托管模式感知、审计
  反签名全部拆至 **S0017**;本 spec 仅将其作为前置依赖引用。

## 2. Outline Design

### Architecture / modules impacted
- **新增**:静态站点(建议独立目录 / 仓库内 `web/onboarding/`);二选一写清:**预构建静态产物**
  或**带 vendored 依赖的单文件构建**(避免激活时临时决策);优先可离线打开、纯客户端可审计。
- **生成器数据源**:构建期取操作清单 + `pool-spec` schema + release bundle manifest digest 注入
  prompt 头。**不再内联决策树文本**(决策树运行时由 `ouro-ops skill show` 从二进制供给,评审 R2 N3)。
- **`ouro` 二进制**:
  - 打包为单个静态二进制,**内嵌 skill 机制资产**;**内嵌策略需定稿**(解压到 `/tmp` 验签执行
    并自清理 vs bash→Rust 重写)——p2-1 拍板,并与"执行期依赖如实声明"一致;
  - `ouro-ops self-update`:查签名 release feed → 验签 + 单调防回滚 + 元数据时效 → 原地替换;
    "最多每 X 小时查一次"缓存(低延迟、离线容忍);
  - 版本闸:`required = max(prompt_min, 内嵌地板, 签名 security 地板)`,记录实际版本入审计,
    降级/漏洞/低于 required 拒绝;
  - **manifest 自校验**:启动校验内嵌 manifest 与自身 digest 一致,不一致拒跑并提示重拷 prompt/自更新。
- **分发渠道**:首次 bootstrap 走官网单一向量(brew / npx,带指纹核对);之后由二进制自更新接管。

### 分发与版本模型(评审 R1 修订)
```
一次性(可信渠道,主动验证一次):
    官网单一安装向量(brew install ouro  或  npx @ouro/cli@<锁定版本+integrity>)
    → 展示签名指纹,用户跨 ≥2 独立渠道核对 = 确立信任锚(防 typosquat/假包)
    → 落一个自更新单二进制
    (随后 ouro-ops init 武装目标机 = S0017)

每次操作(prompt 只给数据+流程,版本值只能抬高):
    prompt 携带 min_ouro_version: X.Y(建议)+ bundle manifest digest
    ouro 执行前:
        required = max(prompt_min, 内嵌 skill 地板, 签名 security/revocation 地板)
        local < required → 自更新(验签 + 单调防回滚)到稳定签名版
        local ≥ required → 直接跑
        校验内嵌 manifest.digest == prompt.manifest_digest,不一致 → 拒跑
    绝不因 prompt 低 min 而降级或跳过安全地板
    审计记录实际执行的确切版本(可复现)
```

### 威胁模型与残留风险(知情接受)
验收基准:**机制路径的风险不得高于手动 SSH;但控制机旁路(#2)与拓扑可见性(#4)是本交互
固有抬高的部分,分项如实披露、不用单一"手动同级"基准掩盖。**

- 已由机制压到"手动同级或更好"(仅限机制路径):spec 字段注入校验(p4-1)、ground-truth 确认
  (p4-2,现状仅特定 `ouro-ops confirm` 门,通用化前标注 planned)。
- **全领域公共成本、非本方案特有**(手动亦然):供应链/工具根信任、控制机握 fleet 凭据。缓解:
  self-update 严格验签 + 单调防回滚 + 透明日志 + 可复现构建;凭据硬件背书。
- **#2 控制机旁路(固有抬高、无法补丁消除、明确接受)**:不可信 prompt 喂给有 shell、能触达
  基础设施的通用 agent,agent 可被诱导**绕开 `ouro`** 在控制机上作恶。机制只保护 `ouro` 路径。
  缓解到可接受:prompt 极简可人读、凭据硬件背书、每(写)操作人工确认显示 ground-truth、可选
  受限 agent-run 加固轨。**适用边界**:成立前提是"用户信任其 agent 运行环境";否则该类操作宜
  保持手动(网站退化为"生成 spec + 命令清单")。
- **#3 控制机 = 皇冠明珠**:吞不可信输入又握 fleet 凭据的最高价值目标。缓解:凭据硬件背书
  (Mac 被攻破也导不出私钥)。
- **#4 云 agent 提供商可见性(评审 R1 新增)**:用户按设计把含 `ssh.host`/`public_endpoint`/角色/
  网络的 prompt 粘进 Claude/Codex,拓扑即进入第三方 transcript / 训练/留存管线。手动 SSH 不必然
  如此。缓解:UI 复制前强制披露;支持最小化/本地 agent 模式;把 "零上传" 限定为网站阶段。

### Risk and rollback strategy
- prompt 注入面:机制不靠提示词(主,限 `ouro` 路径)+ 版本/校验(纵深)+ ground-truth 确认 +
  控制机旁路的诚实披露。
- 版本漂移:构建期从真源派生 + bundle manifest 绑定 + self-update 验签 + 单调防回滚。
- 回滚:静态站点与 `ouro` 变更为增量前向;引入问题则新增修正 item 或 `git revert`,不改历史。

## References
- docs/reliable-skills.md(S0015 复盘:安全靠机制不靠提示词、唯一真源、机制层隔离)
- docs/specs/completed/20260708T0710-S0015-containerized-e2e.md(skill 执行/安全机制现状)
- docs/specs/draft/S0017-production-provisioning.md(前置依赖:`ouro-ops init`/生命周期/审计反签名)
- code_review/S0016-skill-web-onboarding/summary.md(多 agent 评审 R1:P0×2 + 3/3 高置信项)
- ouro-skills/*/SKILL.md(决策层真源)+ ouro-skills/*/scripts/*.sh(机制脚本真源)
- crates/ouro/src/{cli.rs,ssh.rs,secrets.rs}(现状:顶层无 provisioning init;dispatch=SSH-exec-only;
  `cli.rs:257` shell out 到 bash 跑磁盘脚本 `cli.rs:151`)
- schemas/pool-spec.schema.json(表单字段映射标准)
- CLAUDE.md(设计上下文:品牌 / 视觉 / 无障碍基线)

## 3. Execution Plan
> 草案:四条 track。item 粒度在激活前可继续细化。

### p1 — 表单 → spec/prompt 生成器(静态网站)
- [x] p1-1 站点骨架 + 视觉基线(light-only、Cardano 调色板、Apple 极简、a11y 基线);**构建策略
  (R2 N11 落盘)= 单个自包含 HTML、零构建、可离线打开**(最契合纯客户端可审计)
- [x] p1-2 表单模型与客户端校验(完整映射 `pool-spec.schema.json` 必填字段 + 安全默认)
- [x] p1-3 `pool-spec.yaml` 本地生成 + 下载/复制,通过 `ouro-ops spec validate`
- [x] p1-4 薄操作 prompt 生成(数据/spec 引用 + 操作 + `ouro-ops skill show <op>` 指针 + **manifest digest**
  + `min_ouro_version`;**不内联决策树**,评审 R2 N3)
- [x] p1-5 构建期绑定 bundle manifest(操作清单 + schema 版本 + manifest digest);**不派生/不内联决策
  文本**(决策树运行时由 `ouro-ops skill show` 供给)
- [x] p1-6 复制前强制披露 UI:"复制即把拓扑发送给你的 agent 提供商"(#4)
- [x] p1-7 表单可用性重构(交付后改进):**操作优先**——先选操作,再按"字段×操作依赖矩阵"只显示该操作
  需要的字段(deploy→node版本+sync;upgrade→node版本+quorum;kes/runtime/observability/troubleshooting→
  仅机队);network→**派生**魔数(只读)+ 创世哈希(mainnet 真值/其余占位+校验提示);node版本→下拉;
  经济参数仅注册需要→现有 6 操作隐藏并用中性默认补全 schema;`min_ouro_version`/digest→进阶粘贴
  `ouro-ops manifest show` 自动解析;margin→%、pledge/cost→ADA 换算。生成器仍产 schema-valid spec。

### p2 — 自更新单二进制分发
- [x] p2-1 `ouro` 打包为单静态二进制 + **编译期内嵌** skill 决策/机制资产(build.rs + `include_bytes!`);
  **执行策略(R2 N2 落盘)**:磁盘缺失时(安装态二进制)每次 `tool run` 把内嵌脚本落到 per-run `0700`
  临时 `ouro-skills/` 布局 → 执行 → **自清理**;脚本仍是 bash(不做 bash→Rust 大改),故"无外部脚本拉取"
  成立(脚本来自二进制内部,非磁盘/网络),执行期依赖(bash/cardano-cli/python3)如实声明。磁盘/dev/bed
  仍走 `skills_root()`(OURO_SKILLS_DIR 可覆盖),行为不变。
- [x] p2-2 验签逻辑 + 打包(homebrew formula post_install cosign verify、install.sh 对 pinned identity 验签、
  可复现构建/透明日志设计见 RELEASE.md)。**真实签名密钥/发布 = infra**(不在 repo)
- [~] p2-3 `ouro-ops self-update --check [--against <meta>]`(报告版本/内嵌地板、严格不降级)+ 单调防回滚
  (version.rs)+ 离线/时效/denylist 设计(RELEASE.md)。**网络拉取+验签+原地替换 = release infra**,不在 repo
- [x] p2-4 首次 bootstrap(R2 N4/N9):主向量 Homebrew(`packaging/homebrew/ouro.rb`)+ `packaging/install.sh`
  对 **`packaging/SIGNING_IDENTITY`** pinned identity 自动验签(不靠手工比指纹);官方向量单一化,防 typosquat/假包/key 替换
- [x] p2-5 引导文案:`packaging/RELEASE.md`(发布/签名/自更新/离线 + in-repo vs infra 边界);网站 bootstrap 区块区分一次性 setup(①装本机 + ②S0017 init)vs 每次操作
- [x] p2-6 release **bundle manifest**(ouro_version/内嵌决策 hash/内嵌 skills hash/schema hash/
  embedded_digest/`required_ouro`)+ `ouro-ops manifest show|verify --against`;committed
  `packaging/bundle-manifest.json` + 单测 drift 守卫(改 skill/schema 未重生 manifest → CI red)。
  (完整"启动即校验签名 manifest"依赖 release 签名基础设施,见 p2-2/p2-3 infra 边界;本地 verify 门已就绪)
- [x] p2-7 `ouro-ops skill show <op>`:打印**内嵌、已验签**的决策树(供 agent 消费),经 manifest 自校验;
  agent 的决策层只从此取,不信 prompt 内联(评审 R2 N3)。落地:`build.rs` 编译期内嵌 + `skills.rs` +
  `ouro-ops skill show/list`(traversal 拒绝)。manifest 自校验见 p2-6。

### p3 — 版本兼容与审计
- [x] p3-1 skill 真源 machine-readable 版本头(`SKILL.md` front matter `skill_version` + `requires_ouro >=`)
  + 文档校验测试
- [x] p3-2 版本闸:`required = max(prompt_min, 内嵌地板, 签名 security 地板)`;**prompt 只能抬高**;
  `local < required` fail-closed(自更新网络路径=p2-3 infra)。落地 `version::gate` + `tool run --min-ouro`。
- [x] p3-3 降级/回滚保护:**tamper-evident** 单调防回滚状态(HMAC(tool-run.secret) over version;
  擦除/篡改可检测+审计 rollback_reset,重置**回落内嵌地板非零**)+ 拒绝低于 required。签名
  denylist/revocation floor + 写操作元数据新鲜度门依赖 release 签名 infra(p2-3 边界),security_floor
  当前=内嵌地板占位、绝不更低。
- [x] p3-4 审计记录实际执行的确切 `ouro` 版本(可复现):terminal detail `ouro=<ver>`(+ rollback_reset)。

### p4 — 安全加固(web/prompt/dispatch 面;在 S0015 契约上扩展)
- [x] p4-1 spec 字段注入校验(`PoolSpec::validate`:machine.id/ssh.host/endpoint.host=[a-z0-9-.:]、
  ticker=[A-Z0-9]、metadata_url=clean http(s);承接 S0015 shell 注入回归)。真实 spec 全过、注入被拒。
- [x] p4-2 人工确认显示 ground-truth(凡 `changed=true` 写操作:扩展 confirm + 展示 `ssh.rs` 将执行
  的真实 argv,不回显 prompt 自述);通用化前威胁模型标注 planned
- [x] p4-3 威胁模型 / 适用边界文档化(「保护什么/不保护什么」表 + #2 旁路 + #4 云可见性 + bootstrap
  供应链;残留风险的知情接受)
- [ ] p4-4(可选加固轨)受限 agent-run 硬命令面:仅允许 `ouro` 子命令、无 shell/网络/凭据旁路 +
  对抗性绕过测试(把云 agent 便利模式升级为"安全路径")
- [x] p1-restyle 首页视觉重设计:采用 bubble-studio 设计系统(`bubble-studio-assets/design/
  DESIGN-bubble-light.md` + extensions)重做 `web/onboarding/index.html` 的视觉层——毛玻璃导航、
  hero aura(sky/lavender/mint/lemon 光斑 + grain)、双层 ink 蓝黑阴影、Cardano 轨道点环 logo/
  装饰(克制)、操作卡片单选(替代 select)、segmented 网络/同步切换、role pill、深色终端窗口
  (incana 同色影)输出区、reveal 入场 + reduced-motion 降级;**功能与安全门不变**(CSP/单一
  fetch/`skill show` 指针/披露弹窗/校验逻辑全保留)。

- [x] p1-restyle-fix1 首页信息架构调整(用户 review):①统一 1120px 容器,消除信任卡与表单列
  宽度不一致造成的"缩进";②setup 前置为 Step 1 编辑带(安装/验签四步 + 命令卡);③操作表单整体
  移入 modal——三步流:点操作卡 → 弹窗填表(fleet + 操作字段)→ 复制 prompt(披露弹窗叠层于
  modal 之上);modal 打开重置滚动、backdrop/Esc 关闭。安全门不变。
- [x] p1-restyle-fix2 视觉主色强化 + modal 两步向导(用户 review):①hero aura 改为 sea 主导
  (sea .42 主光 + sky/mint 次光、撤 lemon);②hero 新增自绘 SVG「3D 轨道球」(sea/sky 渐变球体 +
  前后分层椭圆轨道 + 卫星点,原创致意 Cardano 轨道几何,非官方 logo);③sea 比重提升:CTA band
  incana→sea、终端/on-color 按钮同色系阴影转 sea、setup kicker 统一 sea;④modal 拆两步:
  Step 1 Configure(fleet+操作字段+校验)→ Continue(校验不过留在本步并滚到错误)→ Step 2
  Copy(披露提示+终端+Back/Copy),顶部步骤胶囊指示,切步重置滚动。安全门不变。
- [x] p1-restyle-fix3 hero 图形改为**真 Cardano logo**(用户指出自创轨道球不是 Cardano logo):
  采用 simple-icons 的 Cardano 官方几何(CC0,构建期取回、**内嵌** path,页面零出站),
  以 sea 调色板做 3D 渲染——径向渐变受光 + 左上高光层 + 同色系软投影 + 环境辉光
  (defs path + 多层 <use>,单一几何真源)。
- [x] p1-restyle-fix4 setup 收进 hero(用户 review):独立 setup 编辑带撤除,改为 hero 内紧凑
  黑底终端(Step 1 kicker + macOS/Linux tab 切换 + 一键 Copy,无需披露——命令不含用户数据);
  mac=brew tap、linux=官方 install.sh(ouro.example 占位,与 packaging 验签逻辑一致);无 JS 时
  默认显示 mac 命令(pre 内静态文本)。hero CTA 收敛为单个 "Pick an operation"。
- [x] p1-restyle-fix5 hero 密度优化(用户 review:"这一坨有点拥挤"):lede 从 5 行砍到 3 行
  (与信任卡重复的机制论述删除);setup 终端从多行窗口压成**单行安装命令 pill**(OS tab + 命令 +
  Copy),与主 CTA "Pick an operation →" 并排为**一行动作行**(step1→step2 动线);提示压成
  一行半(验签交叉核对 + S0017 init);cross-check 命令移入提示文案。
- [x] p1-restyle-fix6 hero orb 遮挡修复(用户 review:"遮住了"):logo 的 z-index 0→-1(退到
  hero 内容层之下,Linux 长命令时 CTA 不再被卫星圆点覆盖),并右移出内容热区(right -20px)。
## 4. Test and Acceptance Criteria
- TC-1 生成的 `pool-spec.yaml` 通过 `ouro-ops spec validate`(含 1 BP + N relay,N≥2;覆盖
  mainnet/preprod/preview、genesis vs Mithril、缺失/非法必填字段的 fixture)。
- TC-2 生成 prompt 的**命令正确性**(主路径,确定性):脚本化 prompt driver(复用
  `tests/e2e/agent-harness/run-scenarios.py` 模式,输入=生成的 prompt 文本)断言 deploy/upgrade 的
  **精确 `ouro-ops tool run` 命令 trace**(含 `--dispatch`/`--spec`/顺序/stop 规则);真 agent 降 nightly。
  **执行**正确性以 S0017 delivered 为前提(bed 轨 vs production 轨分开)。
- TC-3(p1-fix7)出站面最小化:CSP `default-src 'none'` + `connect-src` 锁死 `api.github.com` 单一 host;
  页面**唯一**出站是固定 URL、无用户数据的只读版本查询(真浏览器实测=1 条 github 请求、0 CSP 违规);
  无外链/XHR/ws/beacon;拓扑/pool 数据永不出站(静态门 `test_web_generator.py` 断言 fetch 唯一且 URL 固定)。
- TC-3b 拓扑披露:复制 prompt 前 UI 强制展示"发送给 agent 提供商"确认 + prompt 字段披露清单;
  最小化模式下敏感字段可被排除。
- TC-4 **真源↔二进制一致**:决策真源 `SKILL.md` + 机制真源 `scripts/*.sh` + schema,经 bundle manifest
  与二进制内嵌**逐类**校验一致(`ouro-ops skill show` 输出 == 内嵌 == 源;各类变异均 fail 验证)。网站不内联
  决策文本,故无"站点决策"漂移腿(评审 R2 N3/N8)。
- TC-5 安装 CLI 零运行时依赖 + 执行期依赖如实:干净环境**装 CLI 无包管理器/运行时依赖**;文档列出
  的执行期依赖(bash/cardano-cli/python3…)缺失时**明确报错**,不假绿。
- TC-6 self-update 严格验签:**篡改/无效签名/过期元数据的更新被拒绝**(红线),透明日志/可复现构建可验。
- TC-7 版本闸(含负例):`local < required` 自更新到稳定签名版;**恶意低-min prompt + 本地旧版 →
  仍以 max(floor) 为准不降级**;**擦除/重置防回滚状态后仍拒 vulnerable 版(且重置入审计)、重放过期/旧
  签名元数据被拒、低于 security_floor 被拒**;审计记实际版本。
- TC-8 离线 fallback:无网络时"手动下载 + 离线核验签名"完成,**篡改包/低于 security_floor 被拒**,不卡死。
- TC-9a prompt 不供代码/不供决策(机制):构造含"现拉代码 URL"**及篡改内联决策树**的恶意 prompt,验证
  `ouro` self-update/脚本路径**不采用** prompt 指定来源,且 agent 的权威决策来自 `ouro-ops skill show` 内嵌
  决策树(非 prompt);机制/决策身份均不受 prompt 控制(评审 R2 N3)。
- TC-9b agent 旁路(交互,负向声明):文档 + p4-3 明确 agent 在 `ouro` 外的作恶**不在机制保证内**;
  若启用 p4-4 加固轨,则加对抗性绕过负例(shell/网络/凭据访问被拒)。
- TC-10 安全加固:spec 字段注入被拒(承接 S0015 shell 注入回归);写操作确认弹窗显示真实 argv。
- TC-11 视觉/无障碍(可机械执行):axe-core CI(0 critical/serious)+ 关键页清单 + `prefers-reduced-motion`
  断言 + 品牌色 design-token 单测。
- TC-12 bootstrap 供应链:仿冒/typosquat 包名不被官网向量接受;首装签名指纹跨渠道核对;
  bootstrap-key 替换负例被识破。
- TC-13 manifest 自校验:内嵌 manifest 与二进制 digest 不一致 / prompt manifest digest 与本地不符 →
  `ouro` 拒跑并提示。
- Pass/fail:上述 TC 全部 pass;任一安全约束(网站阶段零上传 / 机制独立兜底 / self-update 验签 +
  单调防回滚 / prompt 不供代码且只能抬高版本 / bootstrap 单一可核对向量)被违反即 fail。

## 5. Execution Log (append-only)
- 2026-07-09 draft 创建:确立"决策层随 prompt 传递、机制层压成一条命令、纯静态客户端生成器"
  方向;交付形态定为纯 copy-paste prompt(deep-link 推迟)。
- 2026-07-09 设计收敛:分发改**自更新单二进制**;版本 `min_ouro_version` 下限;确立"prompt 只给
  数据+流程、不供代码"不变量;补威胁模型 #2/#3 并知情接受;定 self-update 红线。
- 2026-07-09 一致性检查后**拆分**:provisioning(Model P、`ouro-ops init`/`deinit`、target-mutating 传输、
  操作自清理)、托管模式感知、审计反签名 + 主机密钥 pin **移至 S0017**;S0016 收回到分发+版本+
  web/prompt 安全;修正 non-goal 与实际改动对齐。
- 2026-07-09 **多 agent 评审 R1**(claude/codex/cursor 3/3 REQUEST_CHANGES,17 findings)后修订:
  按用户指示修 P0×2 + 全部 3/3 高置信项(+ 同区事实错误):
  · **P0-1 过度承诺** → 新增「保护什么/不保护什么」表 + 适用边界:机制只保护 `ouro`/fleet 路径,
    控制机侧=信任 agent 运行环境、弱于手动 SSH;云 agent 粘 prompt 标为便利模式。
  · **P0-2 版本闸被 prompt 压低** → `required = max(prompt_min, 内嵌地板, 签名 security 地板)`,
    prompt 只能抬高;单调防回滚 + 签名元数据时效;TC-7 加负例。
  · **#3 拓扑云暴露** → Constraints "零上传"限定网站阶段;威胁模型 #4;p1-6 披露 UI;TC-3 自动化 + TC-3b。
  · **#4 bootstrap TOFU/typosquat** → Constraints bootstrap 供应链红线;p2-4;TC-12。
  · **#5 零依赖不实(已核 `cli.rs:257` bash)** → 收窄为"安装 CLI 零依赖" + 执行期依赖如实声明;TC-5 改。
  · **#6 双真源/内嵌未定** → SKILL.md=决策真源、scripts/*.sh=机制真源;bundle manifest 绑定 + 自校验
    (p2-6/p1-5);p2-1 定稿内嵌策略;TC-4 三方一致 + TC-13。
  · **#7 TC-9 无效** → 拆 TC-9a(机制)/TC-9b(agent 旁路负向声明)+ 可选 p4-4 加固轨。
  · **#8 TC-2 非确定** → 脚本化 prompt driver 主路径 + bed/production 分轨 + S0017 前置。
  未修:P2/P3 单 agent 项(#11 ground-truth 通用化、#12 字段映射已并入 p1-2、#13 版本头已并入 p3-1、
  #15/#16/#17 措辞)——作为后续 append 或实现期处理。
- 2026-07-10 **实现推进完成(待验收)**:激活后逐 item 交付并按 item 提交——
  p1-1..p1-6(静态网站,真浏览器验证 0 出站/0 console error/拓扑披露)、p2-1(编译期内嵌+解压验签自清理)、
  p2-6(bundle manifest + verify + drift 守卫)、p2-7(`ouro-ops skill show` 决策源=已验签二进制)、
  p2-2/4/5(SIGNING_IDENTITY/homebrew/install.sh 自动验签 + RELEASE.md)、p3-1(SKILL.md 版本头)、
  p3-2/3/4(version gate `max(...)` 只能抬高 + tamper-evident 单调防回滚 + 审计记版本)、
  p4-1(spec 字段注入校验)、p4-2(`confirm preview` ground-truth argv)、p4-3(威胁模型文档)。
  **有意保持未完成**:p2-3 `[~]`(--check+设计已交付,网络拉取+验签+原地替换=release infra,见
  packaging/RELEASE.md);p4-4 `[ ]`(可选加固轨,威胁模型已标为通往'安全路径'的前提,本期不做)。
  测试:cargo 33 passed;python(skill_docs/deploy_scripts/web_generator/dep_convergence)全 pass。
  **等待用户验收后再 close**(用户指示)。
- 2026-07-09 **多 agent 评审 R2**(验证 R1 闭合;claude COMMENT / codex+cursor REQUEST_CHANGES):
  两条 P0 原洞实质闭合;#3/#5/#7 机制门闭合;新翻出 4 条 P1(N1-N4)。用户选 (a):修 N1+N3、
  N2/N4 决策落盘。修订:
  · **N1 防回滚完整性** → Constraints 红线加 tamper-evident 单调状态(擦除可检测+审计、回落内嵌地板)+
    写操作元数据新鲜度门;p3-3、TC-7 同步。
  · **N3 决策层投毒** → **反转核心前提**:决策树**不再内联进 prompt**,改由 `ouro-ops skill show <op>` 从
    **已验签二进制**供给;prompt 只带数据+操作+指针。§1 洞察表/不变量/保护表、Scope#1、生成器数据源、
    两个真源、p1-4/p1-5、新增 p2-7、TC-4/TC-9a 全部对齐;同时化解 N8。
  · **核心决策落盘**:N2 内嵌=编译期 `include_dir!` + per-run 解压验签自清理(脚本仍 bash);
    N4 bootstrap 签名身份 pin 进仓库 artifact + 安装命令自动验证;N9 主向量=Homebrew(npx 次);
    N11 网站=单个自包含 HTML 零构建。写入 p1-1/p2-1/p2-4。
  · 未修:N5-N8 P2、N9-N11 余项——随实现期处理或后续 append。

- 2026-07-10 p1-restyle completed: 首页按用户指定的 bubble-studio 设计原则重设计(简洁、科技感、
  适度 Cardano 元素、现代表单交互;参考 ouro-pass.paopao.studio 的气质但不照搬)。视觉层全部重写
  (token 化调色/字体/圆角/间距、glass nav、aura+grain、双层阴影、op 卡片、segmented control、
  终端式 prompt 窗口、CTA band、reveal 动效);JS 生成逻辑仅适配控件形态(radio 取值),collect/
  yaml/prompt/bootstrap/NET/OP/RE 与披露流程逐字保留。静态安全门与真浏览器验证均通过。

- 2026-07-10 p1-restyle-fix1 completed: 按用户三点反馈重构信息架构(对齐/为 modal 流/setup 前置)。
  验证:静态门 pass;真浏览器——deploy modal 全字段 + live 11.0.1、披露弹窗正确叠层列出机器清单、
  kes-rotation modal 收敛为 fleet-only 且打开即顶部、0 console 错误。

- 2026-07-10 p1-restyle-fix2 completed: sea 主色强化 + hero 3D 轨道球 + modal 两步向导。验证:静态门
  pass;真浏览器——orb 渲染正常且不压文案、Step1→Continue→Step2 切换正确(胶囊高亮跟随)、披露弹窗
  叠层于 Step2 之上、Back 返回、0 console 错误。

- 2026-07-10 p1-restyle-fix3 completed: hero 换为官方 Cardano mark(simple-icons CC0 path 内嵌)+
  sea 3D 渲染。验证:静态门 pass(无外链资源,SVG 全内联);真浏览器渲染正常、不压 hero 文案。

- 2026-07-10 p1-restyle-fix4 completed: setup 前移进 hero 终端(双 OS tab + copy)。验证:静态门 pass;
  真浏览器——tab 切换 mac/linux 命令正确、Copy→"Copied ✓" 反馈、0 console 错误;operate 区衔接正常。

- 2026-07-10 p1-restyle-fix5 completed: hero 密度收敛为「3 行 lede + 1 行动作 + 1.5 行提示」,首屏
  可见信任卡。验证:静态门 pass;真浏览器 mac/linux pill 切换正常、Copy 正常、0 console 错误。

- 2026-07-10 p1-restyle-fix6 completed: orb 退至内容层后;Linux 最长命令下动作行完全可读。

## 6. Validation Evidence (append-only)
- p3-1 | stack: python | command: python3 tests/test_skill_docs.py | result: pass | note: 6 份 SKILL.md
  均带 YAML front matter(skill_version 整数 + requires_ouro semver);现有决策树/红线校验无回归。
- p2-7 | stack: rust | command: cargo test skills:: + ./target/debug/ouro-ops skill show/list | result: pass |
  note: build.rs 编译期内嵌 6 skill 决策树 + 机制脚本;`skill show <name>` 打印内嵌 SKILL.md(决策源=
  已验签二进制,非 prompt);`skill list` 输出 embedded_digest;`../etc`/未知 skill 被拒。4 skills 单测通过。
- p2-1 | stack: rust | command: cargo test (27 passed) + 内嵌模式 tool run 对拍 | result: pass | note:
  OURO_SKILLS_DIR 缺失→内嵌提取到 per-run 0700 `ouro-skills/` 布局并执行,输出与磁盘态**逐字节一致**
  (deploy/status 同样 node_query_failed/exit30);运行后 0 残留临时目录(自清理);unknown tool 仍被拒。
- p2-6 | stack: rust | command: ouro-ops manifest show/verify + cargo test skills:: | result: pass | note:
  按类 hash(decision/skills/schema)+ embedded_digest + required_ouro;verify 对拍 committed
  packaging/bundle-manifest.json 通过,篡改 decision_hash 被拒并指名漂移类;committed-manifest 单测守 drift。
- p3-2/3/4 | stack: rust | command: cargo test version:: + tool run --min-ouro | result: pass | note:
  required=max(prompt_min,内嵌地板,rollback,security);--min-ouro 9.9.9 fail-closed;--min-ouro 0.1.0 通过;
  审计 detail 记 ouro=0.1.0;擦除/伪造 floor→回落内嵌地板非零(3 单测);HMAC(tool-run.secret) 防篡改。
- p4-1 | stack: rust | command: cargo test (33 passed) + ouro-ops spec validate | result: pass | note:
  `relay1; rm -rf /`/`$(curl evil)`/backtick metadata_url/file://、含空格的 machine id、非 alnum ticker 均被拒;
  bed/quorum2/minimal 三份真实 spec 仍 ok(无误报)。
- p1 (p1-1..p1-6) | stack: web/python | command: tests/test_web_generator.py + 真浏览器验证 | result: pass |
  note: 单个自包含 HTML(零构建),CSP default-src/connect-src 'none' 阻断一切出站;真浏览器渲染无
  console error、发起 0 网络请求;表单实时生成 pool-spec.yaml(经 ouro-ops spec validate=ok,存 examples/
  pool-spec.generated-default.yaml)+ 薄 prompt(指针 `ouro-ops skill show`,不内联决策树);复制前弹出拓扑
  披露确认(p1-6/TC-3b,截图已验)。静态门测试守 CSP/无外链/无决策树内联/披露存在。
- p4-2/4-3 | stack: rust/doc | command: ouro-ops confirm preview + docs/S0016-threat-model.md | result: pass |
  note: `confirm preview --tool --dispatch --spec` 打印将执行的**真实** ssh+wrapper argv(shell-quoted)
  且不执行=ground-truth;威胁模型文档含保护什么/不保护什么表 + #2/#3/#4 + bootstrap 供应链 + 适用边界
  (何时应保持手动)。
- p2-2/3/4/5 | stack: rust/packaging | command: ouro-ops self-update --check + packaging/* | result: pass |
  note: self-update --check 报 current=0.1.0/floor>=0.1.0;--against 更新版→update_available,旧版→不降级;
  packaging/{SIGNING_IDENTITY,homebrew/ouro.rb,install.sh(cosign verify),RELEASE.md}。真实签名密钥/发布/
  网络 apply = infra(RELEASE.md 明列),不在 repo。33 tests 无回归。
- p1-7 | stack: web | command: tests/test_web_generator.py + 真浏览器验证 | result: pass | note:
  操作优先三步(选操作→机队→操作专属字段);deploy 视图仅显示 node版本+sync(经济参数折叠),quorum 隐藏;
  network→魔数只读派生(mainnet 764824073)+ 创世哈希(mainnet 真值/preprod/preview 占位+校验门);
  node版本下拉、margin=%、pledge/cost=ADA 换算、min_ouro/digest 进阶粘 manifest 自动解析;经济参数隐藏
  时用中性默认补全 → 生成 spec 仍 `ouro-ops spec validate`=ok;真浏览器 0 console error、0 网络请求、拓扑披露正常。
- p1-fix1 | stack: web | command: 真浏览器复审(deploy) | result: pass | note: 按用户逐操作复审反馈——
  ①deploy 经济参数(metadata/pledge/margin/cost)由折叠默认改为**必填可见**(不再藏/不再造假);
  ②删除 Advanced 的"Pin exact tooling"(工具元数据,难懂,内嵌地板已兜底);③创世哈希取官方三网络
  真值(mainnet/preprod/preview,64 hex)并**彻底隐藏**,魔数只读派生,用户不再面对网络常量。生成 spec 仍 ok。
- p1-fix2 | stack: web | command: 真浏览器复审 | result: pass | note: 删除 Advanced 整节——node 端口硬编码
  3001(同 ssh 22),极少数非标端口用户直接改生成的 YAML;表单收敛为"选操作→网络/机队→操作字段→输出",
  再无进阶疑惑项。静态门 pass、无 console error、无悬空引用。
- p1-fix3 | stack: web | command: 真浏览器复审(upgrade) | result: pass | note: min_online_relays 默认写死 1
  对**单 relay 会触发 rollout exit 10 拒绝**(升唯一 relay→在线 0<quorum1)。改为默认=max(0,relays−1)
  动态跟随 + 校验 quorum>relays−1 直接报错(说明会 exit 10、单 relay 用 0)+ hint 解释;用户手改后不覆盖。
- p1-fix4 | stack: web | command: 真浏览器复审(upgrade) | result: pass | note: 按用户定案,upgrade 两字段
  全自动、不再让用户选:node version 一律用最新(latest);min_online_relays 派生规则=relay 数==1→0、>1→1。
  upgrade 第 3 段变为"无需额外字段"。移除 quorum 输入/校验/userset。生成 spec 仍 ok,无 console error。
- p1-fix5 | stack: web | command: 真浏览器验证(upgrade, JS 断言) | result: pass | note: prompt/输出随操作收敛为
  **两种模式**——deploy 生成规范 pool-spec 并内联;**非 deploy 引用你已有的 pool-spec.yaml**,不再输出 spec
  (显示"用已有 spec"提示、隐藏 Copy/Download),prompt 收敛为最小(操作+机队+`ouro-ops skill show`指针+该操作
  的单条命令,**不内联任何 spec/经济参数**)。JS 断言:upgrade 下 nodever/sync/registration 全隐、spec-out
  隐藏+note 显示、prompt=最小模板。彻底消除"预填假默认参数"。
- p1-fix6 | stack: web | command: 真浏览器验证(upgrade, JS 断言) | result: pass | note: upgrade 显示**目标
  node 版本**(可见,默认最新)+ **release notes 深链**(随所选版本指向 cardano-node releases/tag/<v>,
  供用户确认更新内容);prompt 含"upgrade to cardano-node <v>"+"Set node_version:<v> in pool-spec.yaml then
  rollout"。诚实说明:版本号是站点构建期烤入(非页面内实时——CSP 零网络),链接=实时真源;真·页面内实时
  拉取需放开一个只读 GET(待用户拍板)。JS 断言 relnotes href/prompt 目标版本均正确。
- p1-fix7 | stack: web | command: 真浏览器实测(fetch + 网络面板) | result: pass | note: 用户拍板要真·实时——
  放开 CSP `connect-src` 到**唯一** host `api.github.com`,加载时 GET cardano-node releases/latest(URL 固定、
  无用户数据),实测拉到真最新 **11.0.1**、relnotes 深链跟随;网络面板确认**仅 1 条 github 请求、0 CSP 违规**;
  失败回退烤入列表。更新 Constraints/TC-3/威胁模型 + 静态门断言 fetch 唯一且 URL 固定不含用户数据。
- p1-fix8 | stack: web | command: 真浏览器 JS 断言 | result: pass | note: 用户发现非 deploy 改 host 不反映到
  prompt——根因:非 deploy prompt 引用机器 **id**(host 来自已有 spec,不在 prompt)。修:非 deploy **隐藏
  host 字段**(避免死输入)、host 校验只在 deploy 生效;非 deploy 由 id 驱动。所有非 deploy 命令统一修复。
  JS 实测:deploy host 改动进 spec;upgrade 隐藏 host、改 relay id→prompt Fleet 行随之更新、无误报。
- p1-fix9 | stack: web | command: 真浏览器 JS 断言 | result: pass | note: 用户选定工作流="每次重新生成完整
  spec"。**退回 p1-fix5/fix8**(复用已有 spec/藏 host):**host 回归所有操作**、每个操作都生成并显示完整
  pool-spec.yaml(含真实 host)。经济参数仍只 deploy 显示,非 deploy 在 spec 里用**带注释占位**;upgrade
  目标版本(实时 11.0.1)写入生成的 spec,prompt 改为"保存本页 spec 再跑"。JS 实测:upgrade host 可见+改
  IP 进 spec、spec 显示、占位注释在、node_version=目标版。(p1-fix5"两种模式"作废,归并为单一模式。)
- p1-fix10 | stack: web | command: 真浏览器 JS 断言 | result: pass | note: 去掉独立 pool-spec.yaml 面板;
  配置**内联进 prompt**——每个操作=一个自包含原子 prompt(复制一次扔给 AI 即可,无需存文件/额外上下文)。
  内联 spec 含真实 host、目标 node_version(实时)、非 deploy 经济参数带注释占位。JS 实测:spec 面板消失、
  prompt 含 spec_version/machines/10.0.0.9/node_version 11.0.1/占位注释/op 命令,单一 prompt 46 行。
- p1-fix11 | stack: web | command: 真浏览器 JS + ouro-ops spec validate | result: pass | note: 进一步精简——按脚本
  实读字段核对,**ticker 对 upgrade/kes 不使用**,与经济参数一样只在 deploy 显示、非 deploy 用占位。精简后
  upgrade=network+machines+node版本、kes=network+machines(其余真需要)。生成 spec 把 ticker 并入
  'placeholders — not used by <op>' 注释组;实测:kes/upgrade 字段可见性正确、内联 spec ticker:POOL 带注释、
  该 spec 通过 ouro-ops spec validate。
- p1-fix12 | stack: web | command: 真浏览器 JS 断言 | result: pass | note: Network magic 纯派生、用户不需看,
  从表单彻底移除(内部 collect 仍从常量表 net.magic 取,写入生成 spec)。fleet 段现仅 Network(+Ticker 仅
  deploy)。实测:magic 字段消失、切 preprod 后内联 spec network_magic:1 正确派生、无 console error。
- p1-rename | stack: project-wide | command: cargo test + manifest regen + 真浏览器 | result: pass | note:
  用户决定:CLI 命令 `ouro` → **`ouro-ops`**(留出 `ouro` 前缀给未来 ouro-* 家族)。项目级重命名:Cargo
  [[bin]]、cli.rs(help/version binary/self_bin)、ssh.rs prepare_tool_run、6×SKILL.md、脚本、目标机 wrapper
  (Dockerfile 内 exec ouro-ops)、网站所有 prompt、packaging(formula 改名 ouro-ops.rb+artifacts、install.sh、
  SIGNING npm=@ouro/ops)、README、docs、tests。**保留**:OURO_* 环境变量、ouro-skills/ 目录、ouro-tool-run
  wrapper 名、ouro.* 输出事件标签、已归档 S0015 spec/design 历史。**重生成 bundle-manifest.json**(SKILL.md
  内容变)。验证:cargo 33 passed、python 全 pass、二进制 target/debug/ouro-ops、spec validate ok、网站 prompt
  全用 ouro-ops。

- TC-3 | stack: python | command: python3 tests/test_web_generator.py | result: pass | note: p1-restyle 重设计后
  CSP default-src 'none' + connect-src 单 host、唯一固定 fetch、无外链资源、skill-show 指针、披露、tool-run 门全过
- TC-3/TC-3b | stack: ui | command: manual validation via Chrome (localhost:8080) | result: pass | note: p1-restyle
  0 console error/0 CSP violation;live 版本拉取 11.0.1;op 卡片切换驱动字段矩阵(upgrade 隐 ticker、
  troubleshooting 空 step3 + 终端标题跟随);披露弹窗列出机器拓扑后才可复制
- TC-11 | stack: ui | command: manual reduced-motion/a11y spot check | result: pass | note: reveal JS-gated、
  prefers-reduced-motion 全量降级、radiogroup/aria-label 保留、focus-visible 3px 焦点环

- TC-3/TC-3b | stack: python+ui | command: python3 tests/test_web_generator.py + manual Chrome | result: pass | note:
  p1-restyle-fix1 modal 化后 CSP/单 fetch/skill-show/tool-run/披露门全过;披露 dialog 叠层于操作 modal 之上生效

- TC-3/TC-3b/TC-11 | stack: python+ui | command: python3 tests/test_web_generator.py + manual Chrome | result: pass | note:
  p1-restyle-fix2 两步向导后所有静态安全门不变;Step2 才可复制且披露确认仍强制;SVG orb 为内联资源(零出站)

- TC-3/TC-11 | stack: python+ui | command: python3 tests/test_web_generator.py + manual Chrome | result: pass | note:
  p1-restyle-fix3 真 Cardano logo 为内嵌 path(CC0),零新增出站;hero 视觉核验通过

- TC-3/TC-11 | stack: python+ui | command: python3 tests/test_web_generator.py + manual Chrome | result: pass | note:
  p1-restyle-fix4 hero 终端为纯静态文本 + 剪贴板写入,零新增出站;唯一 fetch 不变

- TC-3/TC-11 | stack: python+ui | command: python3 tests/test_web_generator.py + manual Chrome | result: pass | note:
  p1-restyle-fix5 安装 pill 仍纯静态文本;安全门不变

- TC-11 | stack: ui | command: manual Chrome (Linux tab, longest command) | result: pass | note: p1-restyle-fix6
  CTA/命令 pill 不再被 orb 覆盖

## 7. Change Requests (append-only)
- 2026-07-09 范围拆分:p5/p6/审计反签名 移出 → S0017。
- 2026-07-09 评审 R1 修订:见 §5 对应条目(P0×2 + 3/3 高置信项 + 同区事实错误)。
