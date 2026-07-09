---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Upgrade Skill

## Purpose
Upgrade pool machines while preserving availability.

## Decision Tree
- Validate spec with `ouro spec validate`.
- Execute cross-machine upgrade only with `upgrade/run`.
- Let the orchestrator enforce machine lock, relay order, BP-last, verify-before-next, and rollback-stop.

## Stop Conditions
- Stop on lock failure and wait for the current invocation to finish.
- Stop on any verify failure; do not continue to the next machine.
- Stop on rollback failure and escalate as state unknown.

## Red Lines
- Do not implement cross-machine loops in the skill.
- Do not bypass `upgrade/run`.
- Do not run writes outside `ouro tool run`.
- Keep BP plus at least one relay available.
- L3 diagnostics are read-only and have no secret directory access.
- No cold, KES secret, or VRF material enters context or output.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
- Every change step is followed by verify.
