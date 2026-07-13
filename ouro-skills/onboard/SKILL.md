---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Onboarding Skill

## Purpose
Bring a target machine under ouro management ONCE, so every per-operation dispatch can run through
the confined `ouro-exec` principal. This is the PREREQUISITE for every `--dispatch` operation: a
dispatch to a machine that is not yet onboarded cannot connect (there is no `ouro-exec` account or
tool-run wrapper on it yet).

## Decision Tree
Before any `--dispatch` operation, confirm the target is ONBOARDED. If a read-only probe
(`ouro-ops tool run detect/runtime --dispatch <id> --spec pool-spec.yaml`) cannot connect as
`ouro-exec`, the target is NOT onboarded — onboard it first:
- Prerequisites the operator supplies (convenience mode, S0017 P0-1 — the bootstrap credential is
  NOT mechanism-isolated from the agent; this is documented, relied on for upstream security):
  - an existing privileged (root-capable) account on the target + its key, referenced as
    `creds://<name>` (a name under the local credentials dir — never an inline key);
  - an `ouro-ops` binary matching the target's OS/arch (init probes the target and REFUSES a
    mismatched binary before writing anything).
- The control key is handled FOR you: when you pass `--machine <id>` and omit `--control-pubkey`,
  init AUTO-PROVISIONS a keypair at `creds://<id>` — the exact path dispatch resolves from the
  spec's `ssh.key_ref: creds://<id>` — and authorizes its public half for `ouro-exec`. Zero key
  handling. (To supply your own instead: pass `--control-pubkey <pub>` and place its PRIVATE half at
  `creds://<id>` yourself, matching `ssh.key_ref`.)
- Onboard: `ouro-ops init --host <target> --port <ssh-port> --bootstrap-user <account>
  --bootstrap-key creds://<name> --ouro-binary <target-arch ouro-ops> --spec pool-spec.yaml
  --machine <id> [--expected-host-key <sha256>]` (`--port` defaults to 22). It installs the confined `ouro-exec` principal +
  the fixed tool-run wrapper + the command allowlist, returns an auditable install manifest, pins
  the target host key (with `--expected-host-key` it verifies out-of-band and refuses a mismatch
  BEFORE any write), and records the declared runtime for later detected↔declared cross-check.
- Verify onboarding succeeded: the read-only probe must now connect —
  `ouro-ops tool run detect/runtime --dispatch <id> --spec pool-spec.yaml`.
- To reverse onboarding later: `ouro-ops deinit` (refuses while a node runs; restores the box and
  removes only ouro-created principals).

## Stop Conditions
- Stop if the bootstrap account/key is absent, or the target host key does not match an
  out-of-band `--expected-host-key` — a first-connect interception is refused with ZERO writes.
- Stop if the `ouro-ops` binary does not match the target OS/arch (init refuses it, no writes).
- Stop on exit 30 (rollback-capable path) or exit 40 (human intervention required).

## Red Lines
- Per-operation writes go only through `ouro-ops tool run` via the confined wrapper; onboarding is
  the ONE-TIME exception, through the audited `ouro-ops init` — never raw remote commands.
- The bootstrap credential is honestly labeled: NOT mechanism-isolated from the agent (convenience
  mode); a poisoned prompt could invoke init via the agent. Relies on upstream control-machine
  security. Do not claim it is isolated.
- No cold, KES secret, or VRF material is requested, printed, or handled during onboarding.
- L3 diagnostics are read-only and have no secret directory access.
- On exit 30, use the rollback-capable path before continuing; on exit 40, stop all writes and
  require human intervention.
