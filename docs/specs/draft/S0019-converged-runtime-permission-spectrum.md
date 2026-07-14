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
- **Environment convergence.** The convention is the **blinklabs `cardano-node` container image
  + its standard layout** (`/ipc/node.socket`, `/data/db`, `/opt/cardano/config/keys/…`) — a
  pinned upstream image, NOT a self-built one (Decision B, resolved 2026-07-14). Ops skills
  operate ONLY nodes that conform to this convention.
- **Cheap adoption, no migration (Decision A, resolved 2026-07-14).** Re-running a node is very
  costly for an SPO (re-sync = hours/days, missed-block risk), so we do NOT force a re-deploy and
  do NOT build a migration path. A node ALREADY running the blinklabs convention (the common
  case) is adopted NON-DISRUPTIVELY: `init` verifies it matches the convention and writes a
  root-owned **provenance manifest** alongside the running node (metadata only — the node is not
  stopped, restarted, or re-synced). The manifest is the single source of truth for the layout
  (convention version, container runtime + name, socket/db/opcert paths, image ref). A node that
  does NOT conform is REFUSED with a clear message — unsupported; we do not migrate or reconfigure
  it. Every skill's first step verifies the manifest; a node without one (never adopted, or
  non-conforming) is refused (`not_ouro_managed`) — no detection-and-adapt on the happy path.
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
- **No migration** — non-conforming nodes are simply unsupported (Decision A). ouro adds value to
  a conforming node by adopting it cheaply, never by asking the SPO to rebuild.

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
- Supporting non-conforming environments (bare/systemd, rootless, custom/self-built images, the
  bare/systemd/rootless-vs-rootful matrix). These are refused, not adapted.
- **Building our own node image** — the blinklabs image is the pinned baseline (Decision B).
- **A migration/re-deploy path** for non-conforming nodes (Decision A) — out of scope by design;
  re-syncing a node is too costly to ask of an SPO.
- Handing any category-3 (irreversible/key-touching) operation to agent improvisation.
- On-target network fetch / self-bootstrapping (upgrades stay operator-initiated; distribution
  is S0018).

## 2. Outline Design
- **Convention conformance check** (init, adoption time): verify the running node is the blinklabs
  image with the standard layout (image ref matches the pinned baseline; `/ipc/node.socket`,
  `/data/db`, opcert under `/opt/cardano/config/keys` present in-container). Conforms → write the
  manifest (adopt). Does not conform → refuse, no reconfigure.
- **Provenance manifest** `/var/lib/ouro/node-manifest.json` (root-owned, `0644`): `{convention_version,
  image_ref, runtime, container_name, socket_path, database_path, opcert_path, pool_dir,
  written_by_audit_id, written_at}`. Written by `init` after the conformance check; the ONLY thing
  scripts consult for layout. Adoption is non-disruptive (no node stop/restart/re-sync).
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
- [ ] p1-1 pin the blinklabs image + standard layout as the convention; provenance manifest schema
- [ ] p1-2 `init` conformance check (blinklabs image + standard layout) → adopt (write manifest) non-disruptively, or refuse a non-conforming node
- [ ] p1-3 `ouro_require_managed_node` preflight gate + typed `not_ouro_managed` refusal; wire into every dispatched op script
- [ ] p1-4 flip layout helpers to READ the manifest; retire happy-path discovery (keep detection only as the refuse/conformance check)
- [ ] p2-1 reversible-write tier: sealed verify+rollback wrapper; move restart/topology plumbing to the agent behind it
- [ ] p2-2 confirm category-3 sealed scripts read the manifest (no behavior change to the gate)

## 4. Test and Acceptance Criteria
> Draft placeholders.
- TC-1 a node WITHOUT a matching provenance manifest is refused by every op skill (typed `not_ouro_managed`, no mutation).
- TC-1b `init` on a conforming blinklabs node adopts it non-disruptively (manifest written; node not stopped/restarted/re-synced); on a non-conforming node it refuses without reconfiguring.
- TC-2 an adopted node: every read/reversible op works reading the manifest, zero argv/cgroup discovery on the happy path.
- TC-3 reversible write (restart) with an injected failure rolls back via the sealed verifier the agent cannot skip.
- TC-4 category-3 (KES rotation) is unchanged: sealed + confirm-token; agent cannot improvise the destructive command.
- TC-5 convention-version mismatch is a refuse, not a best-effort.

## 5. Execution Log (append-only)
- 2026-07-14 draft created from the S0017 closure decision (converge + permission spectrum; category-3 sealed).

## 6. Validation Evidence (append-only)
- (pending activation)

## 7. Change Requests (append-only)
- 2026-07-14 draft opened with two Open Decisions.
- 2026-07-14 **both decisions resolved (user):**
  - **A. Existing/foreign node adoption → NO migration.** Re-running a node is too costly for an
    SPO, so we do not force re-deploy and build no migration path. A node already conforming to
    the convention is adopted NON-DISRUPTIVELY (manifest write, no re-sync); a non-conforming node
    is simply unsupported. (Note: this makes the common "SPO already runs blinklabs" case
    first-class — including the bp1 tested under S0017 — rather than requiring a rebuild.)
  - **B. Convention image → the blinklabs `cardano-node` image (pinned upstream), NOT self-built.**
    Its standard layout (`/ipc/node.socket`, `/data/db`, `/opt/cardano/config`) becomes the fixed
    convention scripts assume.
  Both Open Decisions are now closed; the draft is ready to activate on the user's go-ahead.
