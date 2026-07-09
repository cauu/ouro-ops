---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Deploy Skill

## Purpose
Deploy or take over one pool from a validated `pool-spec.yaml`.

## Decision Tree
- New machine path: `ouro spec validate` -> `deploy/preflight` -> `deploy/provision` -> `deploy/sync` -> `deploy/start` -> `deploy/verify`.
- Existing node takeover path: `ouro spec validate` -> `deploy/preflight` -> `deploy/takeover` -> `deploy/takeover-verify` -> `deploy/start` -> `deploy/verify`.
- Mithril sync requires snapshot digest and certificate-chain evidence before `deploy/sync` may pass.

## Stop Conditions
- Stop on exit 10 and ask for corrected inputs or missing audit context.
- Stop on exit 20 and run L3 read-only diagnostics only.
- Stop on exit 30 and use rollback-capable path before any further write.
- Stop on exit 40 and require human intervention.

## Red Lines
- Writes only through `ouro tool run`.
- L3 diagnostics are read-only and have no secret directory access.
- No cold, KES secret, or VRF material enters context or output.
- Every change step is followed by verify.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
