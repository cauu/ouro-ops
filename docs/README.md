# Documentation Entry Point

The project now uses the immutable-spec workflow for active delivery.

## Current Active Spec

- [2026-03-06-phase4-active.md](./specs/2026-03-06-phase4-active.md)

Read the active spec first for current requirements, outline design, execution plan, and acceptance criteria.

## 已完成归档 Spec

- [2026-03-06-phase1-phase3-completed.md](./specs/2026-03-06-phase1-phase3-completed.md)
- [2026-03-06-phase3-5-deploy-readiness-and-sync-monitoring.md](./specs/2026-03-06-phase3-5-deploy-readiness-and-sync-monitoring.md)
- [2026-03-07-phase3-6-mithril-bootstrap-active.md](./specs/2026-03-07-phase3-6-mithril-bootstrap-active.md)

These completed specs archive the already delivered Phase 1 to Phase 3 scope, the completed Phase 3.5 deploy readiness / sync monitoring work, and the completed Phase 3.6 Mithril bootstrap work.

## 下一阶段草稿 Spec

- 暂无。下一份草稿 spec 将在新增范围时创建。

Phase 4 已切回当前活动 spec。

## Document Layout

- `docs/specs/`
  - Active and historical immutable specs. Only one spec may be active at a time.
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

- Start all new work from the current active spec under `docs/specs/`.
- If scope changes materially, create a new dated spec under `docs/specs/` and record the transition in the old spec change log.
- Keep legacy documents unchanged unless they are explicitly being preserved as historical evidence.
- Do not use `2026-03-06-project-baseline.md` as an execution spec. It is a superseded bootstrap record kept only for traceability.
