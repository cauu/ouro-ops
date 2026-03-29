# GitHub Release + Auto-Update

Spec-ID: S0013
Status: draft
Created Time: 2026-03-29T20:30:00+08:00
Start Time:
Completion Time:
Previous Spec-ID: S0012

## 1. Requirement Details

### Background

ouro-ops 目前只能本地 `tauri build` 生成安装包，没有 CI/CD、没有 GitHub Release、没有应用内自动更新。SPO 用户需要手动获取新版本。

### Scope

1. **GitHub Actions CI** — push to main 时自动构建 macOS `.dmg` 安装包
2. **GitHub Release** — tag push 时创建 Release 并上传构建产物
3. **应用内自动更新** — 集成 `tauri-plugin-updater`，启动时检查 GitHub Release 新版本，提示用户下载安装

### 技术方案

**Tauri 2 Updater 机制：**
- 后端：`tauri-plugin-updater`（Rust crate）
- 前端：`@tauri-apps/plugin-updater`（JS API）
- 更新源：GitHub Release（Tauri 内置支持，通过 `endpoints` 配置指向 `https://github.com/{owner}/{repo}/releases/latest/download/latest.json`）
- 更新文件：Tauri build 自动生成 `latest.json` + `.tar.gz` 签名包（macOS）

**GitHub Actions 构建矩阵：**
- 首版只构建 macOS (aarch64-apple-darwin)，后续可扩展
- 使用 `tauri-apps/tauri-action` 官方 Action
- Tag push (`v*`) 触发 Release 创建

**签名：**
- Tauri updater 需要签名密钥对（`TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`）
- 密钥通过 `tauri signer generate` 生成，存入 GitHub Secrets
- 公钥写入 `tauri.conf.json` 的 `plugins.updater.pubkey`

### Constraints

- 首版只支持 macOS ARM（Apple Silicon），不构建 x86 或 Windows/Linux
- 不做 macOS 代码签名 / 公证（Developer ID）—— 用户需允许未签名应用
- 不做增量更新（Tauri updater 默认全量替换）
- Sidecar（Python runner）需要包含在 bundle 内

### Non-goals

- Windows/Linux 构建（后续独立 spec）
- macOS Developer ID 签名和 Notarization
- 自建更新服务器（直接用 GitHub Release）
- 自动版本号递增（手动管理 `tauri.conf.json` 和 `Cargo.toml` 版本）

## 2. Outline Design

### 文件变更

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `src-tauri/Cargo.toml` | 修改 | 添加 `tauri-plugin-updater` 依赖 |
| `src-tauri/tauri.conf.json` | 修改 | 添加 `plugins.updater` 配置（endpoints, pubkey） |
| `src-tauri/src/lib.rs` | 修改 | 注册 updater plugin |
| `src-tauri/capabilities/default.json` | 修改 | 添加 updater 权限 |
| `package.json` | 修改 | 添加 `@tauri-apps/plugin-updater` 依赖 |
| `src/lib/updater.ts` | 新增 | 前端更新检查逻辑 |
| `src/App.tsx` | 修改 | 启动时触发更新检查 |
| `.github/workflows/release.yml` | 新增 | GitHub Actions Release 工作流 |

### Updater 配置

```json
// tauri.conf.json
{
  "plugins": {
    "updater": {
      "endpoints": [
        "https://github.com/OWNER/REPO/releases/latest/download/latest.json"
      ],
      "pubkey": "dW50cnVzdGVk..."
    }
  }
}
```

### 前端更新逻辑

```typescript
// 启动时静默检查更新
// 有新版本 → 显示非阻塞提示（版本号 + 更新说明）
// 用户确认 → 下载并安装，重启应用
// 检查失败 → 静默忽略
```

### GitHub Actions Release 工作流

```yaml
# .github/workflows/release.yml
# 触发：push tag v*
# 步骤：checkout → setup Node → setup Rust → tauri build → create Release
# 使用 tauri-apps/tauri-action@v0 官方 Action
# Secrets: TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

### Risk and Rollback Strategy

- **Updater 检查失败**：静默忽略，不影响正常使用
- **签名密钥泄露**：重新生成密钥对，发布新版本
- **回滚**：GitHub Release 可删除；updater 只提示不强制

## References

- Tauri 2 Updater 文档：https://v2.tauri.app/plugin/updater/
- tauri-apps/tauri-action：https://github.com/tauri-apps/tauri-action
- `src-tauri/tauri.conf.json` — 当前 bundle 配置
- `src-tauri/Cargo.toml` — 当前依赖

## 3. Execution Plan

**Phase 1: Tauri Updater 集成**
- [ ] p1-1 生成 Tauri 签名密钥对（tauri signer generate），记录公钥
- [ ] p1-2 添加 tauri-plugin-updater 依赖（Cargo.toml + package.json）
- [ ] p1-3 配置 tauri.conf.json updater（endpoints, pubkey）+ capabilities 权限
- [ ] p1-4 注册 updater plugin 到 lib.rs
- [ ] p1-5 新增前端更新检查逻辑（updater.ts）+ App.tsx 集成
- [ ] p1-6 cargo build + pnpm build 验证

**Phase 2: GitHub Actions Release 工作流**
- [ ] p2-1 创建 .github/workflows/release.yml（macOS ARM 构建 + Release 发布）
- [ ] p2-2 文档：说明 GitHub Secrets 配置要求（TAURI_SIGNING_PRIVATE_KEY 等）
- [ ] p2-3 首次 tag push 验证工作流

## 4. Test and Acceptance Criteria

- TC-1 `cargo build` 编译通过（含 updater plugin）
- TC-2 `pnpm build` 编译通过
- TC-3 `tauri build` 本地构建生成 .dmg + latest.json
- TC-4 前端启动时更新检查不阻塞应用加载
- TC-5 GitHub Actions workflow 语法有效（可通过 `actionlint` 或首次触发验证）
- TC-6 tag push 后 GitHub Release 自动创建并包含 .dmg + latest.json

## 5. Execution Log (append-only)

## 6. Validation Evidence (append-only)

## 7. Change Requests (append-only)
