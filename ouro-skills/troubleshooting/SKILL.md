---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Troubleshooting Skill

## Purpose
Diagnose failures after an L1/L2 tool returns non-zero.

## Decision Tree
- Read the failed JSON output and audit id.
- Use `ouro audit log` for invocation history.
- Use `ouro status --diff-spec` for drift and network identity checks.
- Use read-only diagnostic principal outputs only.
- Propose a repair that can be executed through an L1/L2 tool.

## Stop Conditions
- Stop on exit 40 or unknown state.
- Stop if a diagnostic attempts to read restricted key material.
- Stop if the next action would be a write without a tool entrypoint.

## Red Lines
- L3 is read-only.
- Diagnostic principal has no secret directory access and no write capability.
- Do not turn a diagnosis into an ad hoc mutation.
- Writes only through `ouro tool run`.
- No cold, KES secret, or VRF material enters context or output.
- Every change step is followed by verify.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
