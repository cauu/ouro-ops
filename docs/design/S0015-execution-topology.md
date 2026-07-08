# S0015 执行拓扑与节点模型 — 设计锁（p1-0）

> 本文件是 S0015 `p1-0` 的交付物：把 spec §2.1 的执行拓扑、调用入口、审计权威与节点部署模型固化为**可评审、
> 供实现者遵循的决策记录**。任何 p1/p2/p3 实现必须与本记录一致；如需变更，走 spec §7 Change Request。
> 对应 TC：E2E-T0（设计锁评审）。规格来源：`docs/specs/20260708T0710-S0015-containerized-e2e.md` §2.1/§2.3。

## D1 — 执行拓扑：Model B（远端派发）
- **决策**：`ouro tool run --machine <m>`（在 `control`）= **远端派发**。control 的 L1 SSH runner
  （`crates/ouro/src/ssh.rs::execute`，p1-3 实现）以 `ouro-exec` 身份 SSH 到 `<m>`，执行
  `sudo /usr/local/bin/ouro tool run <skill>/<script> --spec <path>`（**不带 `--machine`**）。
- 目标机上的 `ouro tool run`（无 `--machine`）= **本地执行**：解析并运行 `ouro-skills/<skill>/scripts/<script>.sh`，
  在目标机产生真实系统副作用。语义与 S0014 现状（本地执行）一致，仅新增 `--machine` 的远端派发分支。
- **拒绝的替代**：让 agent 在 shell 里裸跑 `ssh <m> sudo …`（绕过 `ouro`，与「写只经 tool run」冲突）；
  在 control 上执行 L2 后再 `scp` 产物（副作用不在目标机、不可信）。

## D2 — 审计权威与 token 边界
- **audit 权威库在目标机**（写发生处）。`audit_id` 与签名 invocation token 由**目标机的 `ouro tool run`**
  生成并经 HMAC `verify-context` 校验（复用 S0014 p5 机制）。
- **不**经 SSH 命令行传 `--audit-id`；**不**靠 sudoers 匹配 argv 约束 `audit_id`/token。合法性一律由目标机
  `ouro tool run` 自校验。control 经 SSH 回传 stdout(JSON) + `audit_id` 供展示/关联。
- **跨机断言**（p3-2）：不变式检查器 **fan-out 聚合各目标机审计 DB**；control 上 `ouro audit log` 本身为空。

## D3 — sudoers 边界（p1-5 遵循）
- `ouro-exec` 的 `sudoers.d`：`NOPASSWD` 仅允许**固定绝对路径** `/usr/local/bin/ouro`，`env_reset` +
  `secure_path` + 显式最小 env + **无 shell**。不用 argv 通配去限制子命令/参数（脆且错）——危险动作的确认门、
  审计门由 `ouro` 自身负责。
- `ouro-diag`：普通用户、无 sudo、对 `/opt/cardano/keys/*`（`0400`、属主节点用户）无读权限；如需可加 sshd
  `ForceCommand` 限制为只读诊断集。

## D4 — 节点部署模型 + 简化记录（reconcile S0014 §2.2#1）
- 目标容器内以**受管进程/兄弟容器**跑 `cardano-node`；**不**做 docker-in-docker 嵌套隔离。
- 简化取舍：

  | S0014 原始向量 | S0015 处置 |
  | --- | --- |
  | `ouro-diag` 不得 `docker exec` 进节点容器读密钥 | **移出**（Non-goal），留真实基础设施验 |
  | `ouro-diag` `cat/find/tar` 读 `/opt/cardano/keys` **文件**被拒 | **保留**，E2E-2 真跑（文件权限） |
  | 密钥 `0400`、属主节点用户、无 sudo | **保留**，E2E-2 真跑 |
  | **日志泄漏** | 由 E2E-9 fingerprint/canary 扫描覆盖（非文件读取语义） |

## D5 — 安装拓扑（p1-2 遵循）
- `control` 与**每台目标机**都安装**同一 digest-pin 版本**的 `ouro` + `ouro-skills` + `schemas`；版本 skew 即 fail。
- 安装机制（p1-2 定：build-in-image / 挂载 / release artifact 三选一）；`OURO_HOME` 于目标机，审计库随之在目标机。

## 自测床布局（p1-1 遵循）
```
control                      # ouro（同版本）、ssh client、~/.ssh 私钥、~/.ouro/credentials
  └─ ssh ─▶ bp1 / relay1 / relay2   # sshd; ouro-diag(只读)/ouro-exec(sudoers); ouro 同版本;
                                     #   /opt/cardano/keys 0400; (p2 起)受管 cardano-node
private devnet genesis（p2-1 引入）
```
p1（p1-1..p1-8）不需要 cardano-node，用轻量基础镜像 + sshd 即可；真节点自 p2-1 起引入。
