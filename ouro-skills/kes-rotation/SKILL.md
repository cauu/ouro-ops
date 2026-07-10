---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# KES Rotation Skill

## Purpose
Rotate KES by generating BP-local KES vkey metadata and installing opcert-only payloads.

## Decision Tree
Production path (cold key kept OFFLINE — preferred):
- Validate spec with `ouro-ops spec validate`.
- Inspect counters with `ouro-ops kes counter status`.
- Generate BP-local KES vkey metadata with `ouro-ops kes generate`.
- Pause for offline certificate signing outside the agent context.
- Request a human confirmation with `ouro-ops confirm create`.
- Install only `node.cert` with `ouro-ops kes push`.
- Verify status with `ouro-ops status --diff-spec`.

Single-operation path (managed node where the cold key is co-located, e.g. the containerized
bed): the whole lifecycle — new KES key, opcert issuance with the INCREMENTED counter, install,
node restart, and forging ground-truth (`query kes-period-info` + tip advancing) — is performed
as one audited, dispatched operation:
- `ouro-ops tool run kes-rotation/rotate --dispatch <bp> --spec <pool-spec>`.
- Then confirm the node resumed producing blocks (e.g. `ouro-ops tool run deploy/status --dispatch <bp>`).
Prefer the offline production path whenever the cold key can be kept off the block producer.

## Stop Conditions
- Stop when counter status is behind, equal, or ambiguous.
- Stop when confirmation is missing, expired, reused, or action mismatched.
- Stop when cert metadata does not match the BP KES vkey hash.

## Red Lines
- Never request or print cold, KES secret, or VRF material.
- Do not install VRF or KES secret payloads during rotation.
- Do not continue after counter replay is detected.
- Writes only through `ouro-ops tool run`.
- L3 diagnostics are read-only and have no secret directory access.
- Every change step is followed by verify.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
