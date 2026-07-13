---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# KES Rotation Skill

## Purpose
Rotate KES by generating BP-local KES vkey metadata and installing opcert-only payloads.

## Decision Tree
PREREQUISITE — every `--dispatch` step needs the BP ONBOARDED (`ouro-exec` + tool-run wrapper). If a
dispatch cannot connect, onboard first: `ouro-ops skill show onboard`. If you lack the target
host/machine id or the operator's access, ASK the operator before dispatching.

Production path (cold key kept OFFLINE — preferred):
- Validate spec with `ouro-ops spec validate`.
- Inspect the live opcert counter via a read-only dispatch: `ouro-ops tool run deploy/status
  --dispatch <bp> --spec <pool-spec>` (reports the node's on-disk counter). (`ouro-ops kes counter
  status --state <counter-state.json>` reads a LOCAL offline counter-state file, not the live node.)
- Generate + stage a fresh KES key on the BP with
  `ouro-ops tool run kes-rotation/generate-offline --dispatch <bp> --spec <pool-spec>`. The new KES
  signing key is STAGED on the BP (the running node keeps forging on the old key); the tool returns
  the public `kes_vkey` + the target `kes_period` in its `data`. NO private key leaves the BP.
- Write the returned `kes_vkey` to a file and generate the offline signing script with
  `ouro-ops kes cold-sign-script --kes-vkey <that file> --kes-period <kes_period>`. It embeds ONLY
  public data; it contains NO private key. Hand this ONE script to the operator.
- The operator carries the script to the AIR-GAPPED machine and runs it there. It reads `cold.skey`
  and the opcert counter IN PLACE (paths set via `COLD_SKEY=`/`COUNTER=`/`OUT=`) and issues
  `node.cert`. cold.skey never moves; only the public `node.cert` comes back.
- Place the returned `node.cert` on the BP at the staging path (`<pool-keys>/offline-stage/node.cert.signed`).
- Request an evidence-bound confirmation: detect the live target with `ouro-ops tool run
  detect/runtime --dispatch <bp>`, then `ouro-ops confirm create --action kes-rotation/push-offline
  --machine <bp> --runtime-evidence <evidence_hash>`.
- Install with `ouro-ops tool run kes-rotation/push-offline --dispatch <bp> --spec <pool-spec>
  --confirm-token <tok>`. It promotes the staged KES key + the cold-signed opcert together, restarts
  onto them, and rolls back to the previous pair if the node does not restart, forge, and advance
  the on-disk counter.
- Verify status with `ouro-ops status --diff-spec`.

Single-operation path (managed node where the cold key is co-located, e.g. the containerized
bed): the whole lifecycle — new KES key, opcert issuance with the INCREMENTED counter, install,
node restart, and forging ground-truth (`query kes-period-info` + tip advancing) — is performed
as one audited, dispatched operation. `rotate` is destructive, so it REQUIRES a target-bound
confirmation token — dispatch it in three steps:
- Detect the live target fingerprint: `ouro-ops tool run detect/runtime --dispatch <bp> --spec <pool-spec>`
  (read `data.evidence_hash` from the output).
- Mint a one-time token bound to it: `ouro-ops confirm create --action kes-rotation/rotate
  --machine <bp> --runtime-evidence <evidence_hash>` (read `data.token`).
- Run with the token: `ouro-ops tool run kes-rotation/rotate --dispatch <bp> --spec <pool-spec>
  --confirm-token <token>`. (Without the token the run is refused, fail-closed.)
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
