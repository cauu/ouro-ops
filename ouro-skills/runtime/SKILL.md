---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Runtime Skill

## Purpose
Apply runtime topology/config changes and verify the node returns healthy.

## Decision Tree
- PREREQUISITE — `--dispatch` steps need the target ONBOARDED (`ouro-exec` + tool-run wrapper). If a
  dispatch cannot connect, onboard first: `ouro-ops skill show onboard`. If you lack the target
  host/machine id or the operator's access, ASK the operator before dispatching.
- Validate spec with `ouro-ops spec validate`.
- Render config with `ouro-ops config render`.
- Apply topology with `runtime/topology-apply`.
- Restart only with `runtime/restart`.
- Verify with `runtime/verify`.

## Stop Conditions
- Stop on verify failure and use L3 read-only diagnostics.
- Stop on repeated restart failure and do not retry writes.

## Red Lines
- No direct file copy or container mutation outside tool entrypoints.
- Writes only through `ouro-ops tool run`.
- L3 diagnostics are read-only and have no secret directory access.
- No secret paths in output.
- No cold, KES secret, or VRF material enters context or output.
- Verify after every topology or restart change.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
