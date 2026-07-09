---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Observability Skill

## Purpose
Install and verify the Prometheus/Grafana gateway path for relay telemetry.

## Decision Tree
- Validate spec with `ouro spec validate`.
- Install gateway with `observability/install-gateway`.
- Verify with `observability/verify`.
- If install partially changes state and verify fails, use `observability/rollback`.

## Stop Conditions
- Stop on missing telemetry credential reference.
- Stop on verify failure after rollback and request human review.

## Red Lines
- Relay telemetry credentials are referenced only by `creds://`.
- Do not print basic-auth values.
- Do not replace Grafana/Prometheus with an in-app monitoring surface.
- Writes only through `ouro tool run`.
- L3 diagnostics are read-only and have no secret directory access.
- No cold, KES secret, or VRF material enters context or output.
- Every change step is followed by verify.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
