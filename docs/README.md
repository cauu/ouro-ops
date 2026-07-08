# Documentation Entry Point

The project now uses the immutable-spec workflow for active delivery.

## Current Active Spec

- [20260708T0000-S0014-agent-tooling.md](./specs/20260708T0000-S0014-agent-tooling.md) — Agent 侧工具面拆分：`ouro` CLI + skills 架构，退役旧 Tauri/React/sidecar/Ansible 栈。

如需开始新需求，请先在 `docs/specs/draft/` 创建新的 draft spec，并在启动执行时提升为 `docs/specs/` 根目录中的唯一 active spec。

## Draft Specs

- 暂无。

## Completed Specs

- [20260329T2100-S0013-github-release-and-auto-update.md](./specs/completed/20260329T2100-S0013-github-release-and-auto-update.md)
- [20260329T1900-S0012-pool-staking-overview.md](./specs/completed/20260329T1900-S0012-pool-staking-overview.md)
- [20260329T1630-S0011-frontend-startup-optimization.md](./specs/completed/20260329T1630-S0011-frontend-startup-optimization.md)
- [20260315T2323-S0010-kes-rotate-core-flow.md](./specs/completed/20260315T2323-S0010-kes-rotate-core-flow.md)
- [20260313T1134-S0009-relay-prometheus-aggregation-auth.md](./specs/completed/20260313T1134-S0009-relay-prometheus-aggregation-auth.md)
- [20260312T1446-S0008-phase5-mac-app-delivery.md](./specs/completed/20260312T1446-S0008-phase5-mac-app-delivery.md)
- [20260310T1412-S0007-frontend-prototype-v2.md](./specs/completed/20260310T1412-S0007-frontend-prototype-v2.md)
- [20260309T1602-S0006-stake-pool-onchain-registration.md](./specs/completed/20260309T1602-S0006-stake-pool-onchain-registration.md)
- [20260308T0000-S0005-phase4.md](./specs/completed/20260308T0000-S0005-phase4.md)
- [20260307T0000-S0004-phase3-6-mithril-bootstrap.md](./specs/completed/20260307T0000-S0004-phase3-6-mithril-bootstrap.md)
- [20260306T0020-S0003-phase3-5-deploy-readiness-and-sync-monitoring.md](./specs/completed/20260306T0020-S0003-phase3-5-deploy-readiness-and-sync-monitoring.md)
- [20260306T0010-S0002-phase1-phase3.md](./specs/completed/20260306T0010-S0002-phase1-phase3.md)
- [20260306T0000-S0001-project-baseline.md](./specs/completed/20260306T0000-S0001-project-baseline.md)

These completed specs archive the delivered Phase 1 to Phase 5 scope (project baseline through the Mac app delivery, relay telemetry, KES rotation, and release/auto-update) that has been explicitly closed. S0014 (active) supersedes the desktop-app direction with the CLI + skills architecture.

## Document Layout

- `docs/specs/`
  - Current active spec only. When no delivery is in progress, this directory root can be temporarily empty.
- `docs/specs/draft/`
  - Not-started specs. Draft specs are mutable and use stable `Spec-ID` filenames.
- `docs/specs/completed/`
  - Closed specs. Completed specs retain their start-time-prefixed filenames and record a terminal completion reason.
- `docs/immutable-spec.md`
  - External-facing explanation of the immutable-spec approach used by this repository.
- `docs/prd/`
  - Legacy product requirements reference.
- `docs/high-level-design/`
  - Legacy architectural overview reference.
- `docs/detail-design/`
  - Legacy detailed design reference.
- `docs/development-plan/`
  - Legacy phase-based plan and task tracking reference.
- `docs/test-cases/`
  - Legacy test case catalog reference.
- `docs/test-results/`
  - Legacy validation evidence reference.
- `docs/review/`
  - Historical code review outputs.

## Usage Rules

- Start all active work from the current root spec under `docs/specs/`.
- Create new not-started specs under `docs/specs/draft/` with stable `Spec-ID` filenames.
- When a draft starts execution, move it to `docs/specs/`, rename it with the start-time prefix, and record `开始时间` plus `前一个 Spec-ID`.
- When a spec ends, move it to `docs/specs/completed/` without renaming it again, and fill `完成时间` plus `结项原因`.
- Keep legacy documents unchanged unless they are explicitly being preserved as historical evidence.
