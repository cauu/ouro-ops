---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Detect Skill

## Purpose
Read-only detection of HOW a cardano-node is supervised on a target, so the mechanism
can choose the correct lifecycle path (host process, systemd unit, or container runtime)
instead of assuming a bare process. The probe emits a closed, typed projection; it never
mutates anything and never reads key material.

## Decision Tree
- Run `ouro-ops tool run detect/runtime --machine <m>` (dispatched) to collect the
  supervision projection.
- Read `data.mode`: one of `bare`, `systemd`, `docker`, `podman`, `ambiguous`, `none`.
- `bare` — the node is a host process; the mechanism uses process rotation.
- `systemd` — a `*.service` unit owns the node; the mechanism restarts the unit.
- `docker`/`podman` — a container runtime owns the node; upgrade re-pins the image
  digest and recreates the container rather than swapping a host binary.
- Use `data.evidence` (unit basename, container id, image digest) and `data.port` only
  to describe ground truth to the operator; the mechanism re-verifies before acting.
- The projection is advisory. The mechanism re-checks mode and shows ground truth before
  any destructive action (see the lifecycle skills).

## Stop Conditions
- `data.mode == "ambiguous"` (multiple supervisor signals, e.g. a unit that launches a
  container-runtime, nested containers, or two matching nodes): stop and escalate; the
  mechanism must fail closed with exit 40 rather than guess.
- `data.mode == "none"` while an operation expects a running node: stop and diagnose.
- Stop if the detected mode conflicts with the spec's declared runtime.
- Stop if a diagnosis would otherwise become an ad hoc mutation.

## Red Lines
- L3 is read-only; this probe has no secret directory access and no write capability.
- No cold, KES secret, or VRF material enters context or output.
- The projection is closed: booleans, a mode enum, opaque ids, and hashes only — never
  raw environment, argv, mounts, labels, or full inspect output.
- Writes only through `ouro-ops tool run`; detection never performs a change itself.
- On exit 30 from a subsequent change, run the rollback-capable path before continuing.
- On exit 40 (ambiguous or unknown supervision state), stop all writes and require human
  intervention.
