# KES Rotation Skill

## Purpose
Rotate KES by generating BP-local KES vkey metadata and installing opcert-only payloads.

## Decision Tree
- Validate spec with `ouro spec validate`.
- Inspect counters with `ouro kes counter status`.
- Generate BP-local KES vkey metadata with `ouro kes generate`.
- Pause for offline certificate signing outside the agent context.
- Request a human confirmation with `ouro confirm create`.
- Install only `node.cert` with `ouro kes push`.
- Verify status with `ouro status --diff-spec`.

## Stop Conditions
- Stop when counter status is behind, equal, or ambiguous.
- Stop when confirmation is missing, expired, reused, or action mismatched.
- Stop when cert metadata does not match the BP KES vkey hash.

## Red Lines
- Never request or print cold, KES secret, or VRF material.
- Do not install VRF or KES secret payloads during rotation.
- Do not continue after counter replay is detected.
- Writes only through `ouro tool run`.
- L3 diagnostics are read-only and have no secret directory access.
- Every change step is followed by verify.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
