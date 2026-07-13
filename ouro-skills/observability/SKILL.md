---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Observability Skill

## Purpose
Read the REAL health of every machine (BP and relays) through the audited dispatch channel and
report CONCLUSIONS to the operator — not raw metrics: whether the node is up and forging-capable,
whether KES rotation is due, whether sync or disk needs attention, and which operation to run next.

## Decision Tree
PREREQUISITE — every `--dispatch` step needs the target ONBOARDED (`ouro-exec` + tool-run wrapper).
If a dispatch cannot connect, onboard first: `ouro-ops skill show onboard`. If you lack the target
host/machine id or the operator's access, ASK the operator before dispatching.

Health sweep (primary path — read-only, covers the BP as a first-class target):
- Run `ouro-ops tool run observability/health --dispatch <machine> --spec pool-spec.yaml` for EVERY
  machine in the spec, BP included. It is read-only; a non-zero exit means "findings to report",
  not a failed operation.
- Interpret the returned facts and REPORT conclusions to the operator in plain language. The
  interpretation table:
  - `node_running: false` → the node is DOWN. Say so plainly and recommend the troubleshooting
    skill (read-only diagnostics) before any restart.
  - `tip_advancing: false` (with node running) → the node is up but STUCK — likely a fault or a
    peer/topology problem. Recommend troubleshooting; do not restart without the operator's say.
  - `sync_progress < 99.99` → still syncing; report the percentage and that operations needing a
    synced node should wait.
  - `kes.remaining_periods <= 30` (BP) → KES rotation is DUE. Recommend the kes-rotation skill and
    offer to start it (its own confirm gates still apply).
  - `kes.opcert_present: false` on the BP → the BP has no operational certificate where expected —
    surface it; the pool cannot forge without one.
  - `disk.chain_db_used_pct >= 90` → disk pressure on the chain-db filesystem; recommend the
    operator grow or clean the volume before it fills.
- Summarize per machine, worst finding first. If everything passes, say exactly that: node up,
  tip advancing, KES has N periods left, disk at N% — no vague "all good".

Optional external monitoring (relays ONLY):
- If the operator wants their own Prometheus/Grafana to scrape a relay, install the authenticated
  gateway on that relay: `observability/install-gateway`, then `observability/verify`; if install
  partially changes state and verify fails, use `observability/rollback`.
- NEVER install the gateway on the BP: the BP exposes no extra surface. Its health is read
  through the confined dispatch above — that is the designed path, not a limitation.

## Stop Conditions
- Stop and report when health shows the node down or the tip stuck — diagnosis before any write.
- Stop on missing telemetry credential reference (gateway path).
- Stop on verify failure after rollback and request human review.

## Red Lines
- The BP never gets a public telemetry endpoint; BP health is read only via the audited dispatch.
- Relay telemetry credentials are referenced only by `creds://`.
- Do not print basic-auth values.
- Do not replace Grafana/Prometheus with an in-app monitoring surface.
- Writes only through `ouro-ops tool run`.
- L3 diagnostics are read-only and have no secret directory access.
- No cold, KES secret, or VRF material enters context or output.
- Every change step is followed by verify.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
