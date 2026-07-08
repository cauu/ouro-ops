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
