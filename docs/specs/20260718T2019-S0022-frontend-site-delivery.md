# Frontend Site Delivery Pipeline

Spec-ID: S0022
状态: active
创建时间: 2026-07-18T20:19:28+08:00
开始时间: 2026-07-18T20:19:28+08:00
完成时间:
前一个 Spec-ID: S0021
结项原因:

## 1. Requirement Details

### Background

`web/onboarding/index.html` is the public prompt generator and already contains the accepted skill
prompts, but the repository has no website publication workflow. Use the proven `ouro-pass`
Cloudflare Workers pattern so pull requests build and preview the page and pushes to `main` publish
production.

### Scope

- Add a deterministic build step for the static onboarding page.
- Add same-repository pull-request preview deployment and preview URL comments.
- Add production deployment on pushes to `main`.
- Document required Cloudflare/GitHub one-time configuration and local verification.

### Constraints

- The page remains a static, client-only artifact; no frontend framework or runtime dependency is
  introduced.
- Fork pull requests never receive Cloudflare secrets and therefore only run the build check.
- The workflow is path-filtered to the site source, build script/config, and its own workflow.
- Existing user-owned `pool-spec.yaml` is out of scope and must not be modified or committed.

### Non-goals

- Creating Cloudflare credentials, GitHub secrets, custom domains, or production environment
  protection rules.
- Publishing CLI binaries, installers, Homebrew formulae, npm packages, or release metadata.
- Changing the onboarding page content or visual design.

## 2. Outline Design

- `web/onboarding/build.sh` stages the release artifact in `web/onboarding/dist/` and verifies that
  the output contains exactly the expected static entry point.
- `web/onboarding/wrangler.jsonc` declares an assets-only `ouro-ops-site` Worker rooted at `dist/`.
- `.github/workflows/site.yml` follows the `ouro-pass` split of build, same-repo PR preview, and
  `main` production deployment. Build output crosses jobs only through a short-lived Actions
  artifact.
- `web/onboarding/README.md` records local build commands and one-time Cloudflare/GitHub setup.
- Rollback is a normal revert on `main`, which causes the same workflow to redeploy the previous
  static page.

## 3. Execution Plan

- [x] p1-1 implement and verify the frontend build, preview, and production publication pipeline

## 4. Test And Acceptance Criteria

- TC-1 `web/onboarding/build.sh` succeeds from any working directory and produces only
  `web/onboarding/dist/index.html`, byte-identical to the tracked source page.
- TC-2 the workflow parses as valid YAML and has path-filtered PR/push triggers, a secret-free build
  job, same-repository-only preview, and `main`-only production deployment.
- TC-3 Wrangler configuration names `ouro-ops-site` and serves only the staged `dist/` directory.
- TC-4 repository hygiene checks pass and `pool-spec.yaml` remains untouched/untracked.
- Pass/fail: all TC checks pass locally; live Cloudflare deployment remains a documented one-time
  infrastructure action because credentials are external to the repository.

## 5. Execution Log (append-only)

- 2026-07-18 p1-1 started: compared the `ouro-pass` site workflow/configuration with the static
  `ouro-ops` onboarding artifact and selected the same Cloudflare preview/production topology.
- 2026-07-18 p1-1 completed: added deterministic static staging, the assets-only Worker config,
  path-filtered build/preview/production workflow, and one-time infrastructure documentation.

## 6. Validation Evidence (append-only)

- TC-1 | stack: other | command: `(cd /tmp && <repo>/web/onboarding/build.sh); find/cmp dist` | result: pass | note: build is cwd-independent and produced one byte-identical index.html
- TC-2 | stack: other | command: `go run github.com/rhysd/actionlint/cmd/actionlint@latest .github/workflows/site.yml` | result: pass | note: workflow syntax and GitHub expressions validated
- TC-3 | stack: node | command: `npx --yes wrangler@4.107.0 deploy --dry-run` (Node 22) | result: pass | note: Wrangler read exactly one asset from dist for ouro-ops-site and performed no deployment
- TC-4 | stack: other | command: `shellcheck web/onboarding/build.sh && git diff --check && git status --short` | result: pass | note: script and patch are clean; pool-spec.yaml remains the sole unrelated untracked user file

## 7. Change Requests (append-only)

- 2026-07-18 user requested a frontend publication pipeline modeled on
  `/Users/caiyufu/Dev/projects/bubble-studio/ouro-pass`.
