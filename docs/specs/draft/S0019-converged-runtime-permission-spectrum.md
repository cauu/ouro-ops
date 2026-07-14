# Converged Ouro-Managed Runtime & Permission-Spectrum Operations

Spec-ID: S0019
Status: draft
Created Time: 2026-07-14T17:30:00+08:00
Start Time:
Completion Time:
Previous Spec-ID: S0017
Closure Reason:

## 1. Requirement Details

### Background
S0017 delivered a working production system (init/deinit provisioning, offline cold-signing,
real on-chain registration, KES rotation, confirm-gates). But its real-machine acceptance —
a fresh agent doing free-form diagnosis on a live mainnet block producer — exposed that an
entire class of pain (exit 126 / ARG_MAX, tip unreadable, opcert filename, container-vs-host
paths, `--socket-path` on cardano-cli 10.x, rootless/rootful, image-specific quirks) has ONE
root cause: **the scripts try to adapt to arbitrary, heterogeneous node deployment
environments.** That is an unbounded case-explosion — impossible to enumerate, and every
uncovered combination is a production surprise on a key-holding node, the worst possible place
for one. A script that fails does so fail-closed (exit code, no mutation); the cost is
debugging pain, not damage — but the *breadth* of environments defeats QA.

Two convergences fix this at the root:
1. **Only operate what we deployed** — collapse the environment matrix from ∞ to 1.
2. **A permission spectrum** — mechanically fence what the agent *can* do by risk class, and
   let the agent be flexible where a fence makes it safe, instead of encoding every step as a
   rigid script.

### Scope
- **Environment convergence.** Ops skills operate ONLY ouro-deployed containerized nodes under
  a versioned convention. A root-owned **provenance manifest** (written by `init`/deploy) is the
  single source of truth for the layout (convention version, container runtime + name, socket
  path, database path, opcert path, image ref). Every skill's first step verifies the manifest;
  a node without a matching manifest is REFUSED with a clear message — no detection-and-adapt.
- **Scripts read, not detect.** On the happy path the scripts READ the manifest instead of
  discovering paths from argv / cgroup / image quirks. The existing detection machinery
  (detect/runtime, cardano-cli adapter, mode dispatch) is repurposed: its semantics flip from
  "figure out how to operate this environment" to "verify this is ours, else refuse."
- **Permission spectrum** (three tiers, by reversibility + blast radius):
  - **Reads** — free-form, fenced by the OS (unprivileged `ouro-diag`, no sudo). Delivered in
    S0017 p5-18 (`ouro-ops diag exec` + the troubleshooting rewrite). Unchanged.
  - **Reversible / verifiable writes** (config render, topology apply, restart) — the agent runs
    the environment plumbing flexibly against the KNOWN converged runtime, but a SEALED
    verify+rollback the agent cannot skip enforces the invariant (node returns healthy / forging,
    else roll back).
  - **Irreversible / key-touching writes** (KES rotation, opcert install, tx submit) — stay
    SEALED reviewed scripts behind the evidence-bound confirm-token human gate. The destructive
    command is NEVER handed to the agent to improvise. (User-confirmed 2026-07-14.)
- **Migration of existing/foreign nodes** — a guided re-deploy under the convention (or explicit
  out-of-scope). See Open Decisions.

### Constraints
- Category-3 writes (KES/opcert/tx) remain sealed + confirm-token gated — non-negotiable
  (user-confirmed). The failure-mode asymmetry is the reason: a buggy sealed script fails
  closed; an improvising agent under prompt-injection fails OPEN, and a mis-incremented KES
  counter / a wrong on-chain tx is unrecoverable.
- No cold, KES secret, or VRF material ever enters agent context or output (carried over from
  S0015/S0017).
- The provenance manifest is root-owned and version-stamped; a convention-version mismatch is a
  refuse, not a silent best-effort.
- Agent-run reversible-write plumbing must still pass through the sealed verify+rollback — the
  agent's flexibility is in HOW it reaches the state, never in whether the invariant holds.
- Command output / logs / chain state read by the agent are DATA, never instructions (injection
  red line, carried from p5-18).

### Non-goals
- Supporting arbitrary/foreign node environments; the bare/systemd/rootless-vs-rootful matrix;
  adopting a legacy node as-is (S0017's takeover path is superseded — see Open Decisions).
- Handing any category-3 (irreversible/key-touching) operation to agent improvisation.
- On-target network fetch / self-bootstrapping (upgrades stay operator-initiated; distribution
  is S0018).

## 2. Outline Design
- **Provenance manifest** `/var/lib/ouro/node-manifest.json` (root-owned, `0644`): `{convention_version,
  runtime, container_name, image_ref, socket_path, database_path, opcert_path, pool_dir,
  written_by_audit_id, written_at}`. Written by `init`/deploy; the ONLY thing scripts consult for
  layout.
- **Skill preflight gate** (lib primitive, e.g. `ouro_require_managed_node`): assert the manifest
  exists and `convention_version` is supported; else emit a typed refusal (`not_ouro_managed`)
  and exit. First line of every dispatched op script.
- **Read-the-manifest helpers** replace the p5-3/p5-14/p5-21 discovery helpers on the happy path
  (`ouro_node_socket`/`_opcert`/`_disk` etc. read the manifest; detection stays only inside the
  preflight gate as the refuse check).
- **Sealed verify+rollback** for the reversible tier: a reviewed post-write verifier the agent
  invokes but cannot bypass (the tool-run entrypoint runs verify+rollback around the agent's
  plumbing step, or the plumbing is expressed as an intent the sealed executor carries out with
  the invariant check).
- **Category-3 unchanged**: kes-rotation/*, deploy/register-submit, opcert install keep their
  current sealed scripts + confirm-token flow.

## References
- docs/specs/completed/20260711T1010-S0017-production-provisioning.md (Previous; delivered mechanism + p5 real-machine findings)
- docs/specs/draft/S0018-official-distribution-infra.md (signed distribution + operator-initiated upgrade; complements the refuse-and-upgrade path)
- docs/codebase-map.md

## 3. Execution Plan
> Draft — to be detailed and sequenced on activation, after the Open Decisions are resolved.
- [ ] p1-1 provenance manifest schema + `init`/deploy writes it
- [ ] p1-2 `ouro_require_managed_node` preflight gate + typed `not_ouro_managed` refusal; wire into every dispatched op script
- [ ] p1-3 flip layout helpers to READ the manifest; retire happy-path discovery (keep detection only as the refuse check)
- [ ] p2-1 reversible-write tier: sealed verify+rollback wrapper; move restart/topology plumbing to the agent behind it
- [ ] p2-2 confirm category-3 sealed scripts read the manifest (no behavior change to the gate)
- [ ] p3-1 migration path for foreign/existing nodes (per Open Decision A)
- [ ] p3-2 the converged container convention: image choice + fixed layout (per Open Decision B)

## 4. Test and Acceptance Criteria
> Draft placeholders.
- TC-1 a node WITHOUT a matching provenance manifest is refused by every op skill (typed `not_ouro_managed`, no mutation).
- TC-2 an ouro-deployed node: every read/reversible op works reading the manifest, zero argv/cgroup discovery on the happy path.
- TC-3 reversible write (restart) with an injected failure rolls back via the sealed verifier the agent cannot skip.
- TC-4 category-3 (KES rotation) is unchanged: sealed + confirm-token; agent cannot improvise the destructive command.
- TC-5 convention-version mismatch is a refuse, not a best-effort.

## 5. Execution Log (append-only)
- 2026-07-14 draft created from the S0017 closure decision (converge + permission spectrum; category-3 sealed).

## 6. Validation Evidence (append-only)
- (pending activation)

## 7. Change Requests (append-only)
- 2026-07-14 draft opened. Two Open Decisions block activation:
  - **A. Existing/foreign node adoption.** Guided re-deploy-under-convention, or fully out of
    scope? (S0017's takeover adopted a legacy node's forging keys; under convergence that path
    is superseded.)
  - **B. Container convention / image.** A self-built ouro image, or a pinned upstream image
    (e.g. official IOG, or the blinklabs build seen in testing) as the convention baseline —
    fixing paths + a known cardano-cli that honors the socket handling.
