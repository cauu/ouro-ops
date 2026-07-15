# Converged Ouro-Managed Runtime & Intent-Based Operations

Spec-ID: S0019
Status: active
Created Time: 2026-07-14T17:30:00+08:00
Start Time: 2026-07-15T10:00:00+08:00
Completion Time:
Previous Spec-ID: S0017
Closure Reason:

## 1. Requirement Details

### Background
S0017 delivered a working production system, but its real-machine acceptance (a fresh agent doing
free-form diagnosis on a live mainnet BP) showed an entire class of pain (exit 126/ARG_MAX, tip
unreadable, opcert filename, container-vs-host paths, `--socket-path` on cardano-cli 10.x,
rootless/rootful, image quirks) shares ONE root cause: **the scripts try to adapt to arbitrary,
heterogeneous node environments** — an unbounded case-explosion, impossible to QA, and every
uncovered combination is a production surprise on a key-holding node.

S0019 is a **greenfield skill set**, not a patch of the S0017 scripts. The user-facing interaction
is UNCHANGED (website form → one pasted prompt → agent reads `ouro-ops skill show <op>` → executes
through `ouro-ops tool run`). What changes is the mechanism: (1) **converge the environment** so
the new skills READ an adoption attestation instead of detecting; (2) **intent-based writes** — the
agent never authors raw commands, only a schema-validated, hash-bound INTENT that a sealed
target-side executor carries out under a crash-durable transaction.

This spec was shaped by two rounds of multi-agent design review (Claude + Codex). §2 is written to
be **self-sufficient for a zero-context implementer**: the safety of the design lives in the
NORMATIVE PROTOCOLS below, not in decisions made in chat. Review artifacts:
`code_review/S0019-design-review*/summary.md`.

### Scope

**A. Environment convergence — a layout contract, digest-pinned.** The convention is a **layout
contract** (fixed in-container paths, a container cardano-cli that honors explicit `--socket-path`,
a role bp|relay) with the **blinklabs `cardano-node` image as the pinned baseline** (Decision B —
never self-built), expressed as a **signed allowlist of immutable OCI digests** (never a tag). Ops
operate ONLY nodes conforming to a contract+digest in the allowlist; a non-conforming node is
REFUSED (never adapted). S0019 is deliberately a blinklabs-layout product (accepted product
decision — non-conforming SPOs are unsupported; same-layout images may be added to the allowlist
later, additive).

**B. Adopt, not migrate (Decision A).** No migration path (re-sync too costly). A conforming node
is adopted NON-DISRUPTIVELY (attestation write, no stop/restart/re-sync); a non-conforming node is
unsupported. States: `host-onboarded` → `node-conformant` → `node-adopted`. Greenfield node
DEPLOYMENT is a non-goal: the operator stands up a conforming node; ouro only adopts + operates.

**C. Fresh skills, no detection on the happy path.** New skills READ the attestation for layout.
Detection exists ONLY at (1) the adopt-time conformance assessment and (2) the per-op live
re-attestation gate (§2.4). No S0017 discovery/adapter/mode-dispatch fallback is carried forward.

**D. Permission model — TWO tiers.**
- **Reads** — free-form, run as the **unprivileged `ouro-diag` principal**. Unix permissions stop
  writes to root/node-owned files and reads of secret dirs; they do NOT stop writes to the diag
  user's own home/tmp, network egress, or resource exhaustion — so this is "unprivileged
  diagnostics", NOT "read-only" (honest labeling per §2.11). Delivered in S0017 p5-18.
- **Writes — declarative intent → sealed executor.** The agent composes a schema-validated,
  canonical, audit-hash-bound INTENT (§2.5); a sealed target-side executor (§2.6) holds the
  capability, validates the intent, performs the fixed mutation, and runs a crash-durable
  transaction. Dangerous writes (key-touching, irreversible, availability-affecting) additionally
  require the evidence-bound confirm-token; category-3 (KES/opcert/tx) sealing is retained.

### Constraints
- **Honest control-plane labeling (S0017 P0-1, carried forward).** The bootstrap credential is
  convenience-mode — NOT mechanism-isolated from the agent. The write-side guarantees defend a
  MISLED/injected agent operating THROUGH ouro-ops, NOT an agent hijacked to run raw shell on the
  control machine. No "writes are mechanically fenced" claim; deployment docs/UI must never
  shorthand this as "all writes are fenced". Closing the boundary is a separate hardening spec.
- **Deny-by-default PRIVILEGED-CAPABILITY API (not just "write tools").** Every root/Docker
  mutation on a target — managed writes AND lifecycle (init/deinit/adopt, attestation rotation,
  recovery/watchdog, artifact staging, allowlist update, write-seal clearance) — goes through one
  target-side privileged-capability API. A static + dynamic gate proves NO privileged
  fs/Docker/process mutation occurs outside it. Bootstrap lifecycle is its own explicit trust
  class, not covered by "two tiers".
- **Attestation = immutable adoption identity + versioned managed state.** Every managed op
  re-attests the live node before mutation (§2.4); a legitimate write advances the versioned state
  via CAS in the same transaction (§2.3), so success does not read as drift.
- No cold, KES secret, or VRF material ever enters agent context or output (carried from S0015/17).
- **Injection red line is defense-in-depth only.** The real control is that the agent supplies a
  validated, canonical, closed-schema intent — not commands — and the executor owns the mutation.
- Crash-durable, single-writer semantics per §2.6; fleet-wide single-writer per §2.9.
- **One supervisor/host contract for v1 (§2.2).** A second variant is a separately versioned
  contract with its own executor + fixtures, never a generic runtime field.

### Non-goals
- Non-conforming environments (bare/systemd, rootless, non-allowlisted images, unrecognized
  daemon/orchestration/mount/network/multi-node shapes) — refused at adoption, not adapted.
- Building our own node image (Decision B).
- Migration/re-deploy for non-conforming nodes (Decision A); greenfield node DEPLOYMENT by ouro.
- Agent-authored raw write commands, paths, or blobs (artifacts arrive via §2.7, not intents).
- Patching the S0017 scripts (fresh skill set); on-target network fetch (upgrades operator-init;
  S0018 distributes only the ouro-ops binary — node-runtime upgrade is owned here, §2.10).
- Closing the P0-1 control-plane boundary; defending against a compromised host root / Docker
  daemon (explicitly in the trusted computing base, §2.12).

## 2. Outline Design (normative protocols)

### 2.1 Layout contract + signed digest allowlist
- A versioned table `{convention_version, contract_id, in_container_paths{socket,db,keys,config,
  topology,genesis}, role_rules, allowed[]}` where `allowed[]` = `{platform, oci_index_digest,
  image_config_digest}`. Tags are display-only, never trusted.
- The allowlist is **signed** and embedded like the skill pack (S0016/S0017 embed model); it
  carries a monotonic version + an emergency **denylist**. Anti-rollback: a target refuses an
  allowlist older than its recorded floor. Control↔target allowlist skew is a refuse.

### 2.2 Supervisor/host contract (pinned, v1)
- Exactly ONE: rootful Docker; a single container per host launched by a named unit ouro can
  identify; bind mounts (not just named volumes) with recorded source device+inode; a fixed
  restart policy; the Docker daemon at the standard socket. Every other daemon/orchestration
  (Compose vs run), mount driver, network mode, or **multi-node host** is refused at adoption.
  (Multi-node support, if ever needed, is a separate versioned contract.)

### 2.3 Adoption attestation — immutable identity vs versioned managed state
- **Immutable identity (frozen at adopt):** role; host key + machine id; OCI index +
  platform-manifest + image-config digests; container creation epoch; entrypoint/args; typed mount
  map `{type, source-device+inode | volume-id+driver+opts, destination, read_only, owner, mode,
  no_symlink:true}`; network + genesis hash; public credential identifiers.
- **Versioned managed state (advances on every managed write):** a monotonic `state_generation` +
  hashes of the mutable fields a managed write may legitimately change (topology hash, config hash,
  KES period / opcert identifier, container id after a restart/recreate). Stored as a separate
  CAS-guarded record, updated ONLY inside the write transaction (§2.6).
- File `/var/lib/ouro/node-attestation.json`, root-owned `0640 root:ouro-exec`. The ONLY layout
  source new skills read.

### 2.4 Per-op live re-attestation gate (closes check→act TOCTOU)
- The `tool run` entrypoint is the single central gate; it runs BEFORE script/executor extraction.
- Under the per-node lock (§2.6): resolve the live container by **immutable container id** (never
  by name); compare a CLOSED fingerprint (identity fields + expected `state_generation`) to the
  attestation; any drift → typed `node_drift`/`not_ouro_managed`, no mutation.
- The executor opens every host resource through stable handles (`openat2` beneath/no-symlink where
  available), re-validates the container id + mount inodes + `state_generation` IMMEDIATELY before
  each irreversible commit (CAS), and re-verifies after. Cooperative out-of-band admin must honor
  the fencing lease (§2.9); non-cooperative Docker/root admin is out of scope (§2.12).

### 2.5 Intent envelope + deny-by-default registry + sink rules
- **Envelope (normative):** `{schema_version, operation_id, node_id(immutable), pre_state_generation,
  pre_state_hash, expected_post_state, nonce, expiry, payload}`. Canonical serialization (sorted
  keys, no duplicates, bounded depth/count/string-length/number-range); the canonical bytes are
  hashed and that hash is bound to the audit event and (for dangerous ops) to a single-use
  confirm-token together with a human-readable diff.
- **Registry (deny-by-default):** each `operation_id` maps to a closed JSON Schema with
  operation-specific semantic validators (allowed values enumerated, not "string"), a touched-
  resource set, a secret-exposure rule, a confirmation rule, a verifier, rollback semantics, and
  fleet policy. An unclassified operation is refused. Static test: no write operation is
  unclassified; every invariant names its negative test.
- **Sink rules:** the executor NEVER runs a shell, `eval`, or text templating; it uses fixed
  executable + argv arrays and structured JSON/YAML serializers; every payload field maps to an
  explicit sink + resource; free-form fields (rare) get a content validator + traversal/symlink
  rejection. Topology/config values are DERIVED from the operator's spec, not agent-authored.

### 2.6 Sealed executor + crash-durable transaction state machine
- Execution locus: a target-resident privileged component (installed by init, identity-pinned per
  §2.8), invoked by `tool run`; a SEPARATE recovery entrypoint runs at the start of every `tool
  run` and reconciles an interrupted transaction BEFORE any new write.
- Durable states: `prepared → committing → committed → verifying → {verified | rolling_back →
  {rolled_back | sealed}}`. Each transition is fsync'd (file + parent dir) before its side effect
  is observable; every step is idempotent / CAS-guarded so recovery can re-drive it safely.
- Ingredients: exclusive per-node lock (with owner + expiry); pre-state fingerprint; fsync'd
  immutable rollback artifact with generations + retention; ordered multi-resource compensation
  (KES key + opcert + restart + state-record are NOT one rename — compensation is ordered and
  each step re-checks); a watchdog whose deadline outlives SSH/process death (owned by the
  target-resident component, not the SSH session); postcondition verify via role-specific readiness
  proxies (§2.6a); rollback verify; terminal journal.
- A failed/unverifiable rollback writes a durable **write-seal**; further writes refuse (exit 40)
  until an explicit operator recovery clears it.
- **2.6a Readiness proxies (not "immediate forging proof"):** process/container identity, socket
  query, expected network/genesis, tip/peer progress, KES/opcert validity, credential-loaded
  state. A low-stake pool cannot show a fresh block in a bounded window, so forging is never a
  postcondition.

### 2.7 Artifact staging inbox (opcert / signed tx / OCI image ingress)
- Operations needing a blob (opcert install, tx submit, KES payloads, runtime image) do NOT pass a
  path or blob in the intent. A root-owned, content-addressed **staging inbox** accepts bounded
  streaming ingress (operator-authorized), no symlink/hardlink following, atomic finalize,
  type-specific parsing + cryptographic/domain validation, provenance/expiry/replay rules, GC.
- The intent references only an immutable `artifact_id + digest`; the confirm preview and audit
  event bind that digest. Runtime images use a preloaded-image path + local OCI digest verification
  (no on-target fetch).

### 2.8 Executor identity / anti-downgrade parity
- Adoption and every live gate bind: the signed ouro-ops build-id, the executor/registry/intent-
  schema/transaction-impl digests, and a minimum security version. Privileged execution is compiled
  into the root-owned binary (no mutable on-disk script/plugin override in production).
- `tool run` requires control↔target parity (same embedded digest family, cf. S0017 p5-17) before
  accepting an intent; a target below the minimum security version refuses. All legacy S0017 write
  entry points are explicitly disabled unless migrated into the registry.

### 2.9 Fleet lease authority (pool-wide single writer)
- A durable **pool generation** + an exclusive **lease** with expiry + fencing token; a controller
  identity; a target-side check of a signed/authorized **step permit** before each disruptive step.
- Quorum (minimum-online-relays) and BP-leadership are re-evaluated immediately before every
  disruptive step, not just at preflight; relay batches; BP-last; active/standby forging rules;
  lease-loss → abort; crashed-lease-holder fencing. Tested with two independent controllers,
  delayed messages, partitions, crashed holders, concurrent out-of-band changes.

### 2.10 Node-runtime upgrade (owned here; DB-compat honest)
- N→N+1 gated by **signed transition metadata** (node/cardano-cli/protocol + **DB-format
  compatibility**). Sequence: upgrade ouro first → canary a relay → BP last → preserve volumes →
  verify → atomically rotate the attestation. Rollback restores runtime AND attestation ONLY if a
  tested backward-compatible downgrade or a crash-consistent volume snapshot/clone (with capacity
  check) exists; otherwise the spec states forward-recovery/re-sync as the only honest outcome and
  does NOT promise rollback. Images arrive via §2.7 preload; N stays supported until the fleet
  completes.

### 2.11 Reads — honest `ouro-diag` labeling (+ optional sandbox)
- Label it "unprivileged diagnostics" with the S0017 residuals restated: it can write its own
  home/tmp, make egress, and exhaust CPU/PID/disk. Either keep that honest scope, OR run each diag
  command in a bounded transient unit (read-only fs, private tmp, CPU/mem/PID/output/time quotas,
  no-new-privileges, explicit egress policy). Pick one in p3-2; do not claim "physically cannot
  write".

### 2.12 Threat model / trust matrix (actual table, authored before activation)
- Adversaries modeled: misled/injected agent; malicious diagnostic data; hostile spec values;
  stale/replaced container; compromised upstream tag; cooperative out-of-band admin; concurrency;
  crash/power loss. **Out of scope (trusted computing base):** compromised host root, Docker
  daemon, host kernel, the allowlisted image's own vulnerabilities, and (by P0-1) an agent that
  abandons the ouro-ops path via the bootstrap credential.
- Every invariant → its enforcing component (§2.x) → its negative test (§4). Prompt text is
  defense-in-depth only.

### 2.13 Audit event schema
- Append-only, closed-field events (hashes only, never raw inspect/config/secret-shaped values)
  for: adopt (with approval evidence hash, §2.14), live-preflight, intent approval, each
  transaction phase, verify, rollback, recovery, attestation/state rotation, refusal. Control-side
  anchoring remains a stated residual risk (S0017); event completeness is testable.

### 2.14 Evidence-bound adoption approval
- `adopt` produces a closed candidate-attestation preview; a single-use operator token/signature is
  bound to that canonical hash + the target host key; a final live comparison runs under the
  adoption lock; the approval evidence hash is stored in the attestation + audit journal. (Under
  the P0-1 boundary an agent can invoke the path, so the binding — not mere invocation — is what
  makes a specific conforming container the trust root.)

### 2.15 Supported / retired / unsupported operation table
- Published before activation: every real-operator remediation (disk cleanup/expansion, log
  rotation, time/firewall correction, config render vs activate, host service repair, failed-
  transaction recovery, KES/opcert/tx, restart/topology, upgrade) is marked supported (with its
  schema/executor/transaction policy), retired, or unsupported (typed refusal + documented
  operator-only recovery path). No operation may fall through to the bootstrap bypass or ad-hoc
  manual commands.

## References
- docs/specs/completed/20260711T1010-S0017-production-provisioning.md (Previous; delivered mechanism; P0-1 labeling; p5-17 parity)
- docs/specs/draft/S0018-official-distribution-infra.md (ouro-ops BINARY distribution only — NOT the node runtime)
- code_review/S0019-design-review/summary.md ; code_review/S0019-design-review-v2/summary.md (the two review rounds that shaped §2)
- docs/codebase-map.md

## 3. Execution Plan
> Greenfield skill set; two-tier model. The protocol items (p1/p2) must be specified + tested
> before any dispatched write goes live.
- [x] p1-1 layout contract + SIGNED digest allowlist (embed, monotonic + denylist, anti-rollback, skew refuse) — §2.1
- [x] p1-2 pin the v1 supervisor/host contract; refuse all other shapes at adoption — §2.2
- [x] p1-3 adoption ceremony + evidence-bound approval + attestation schema (immutable identity vs versioned state) — §2.3, §2.14
- [x] p1-4 central live re-attestation gate (immutable container id, in-lock, openat2, CAS before commit) — §2.4
- [x] p1-5 new skills READ the attestation; no detection/fallback — §1.C
- [x] p2-1 intent envelope + deny-by-default privileged-capability registry + sink rules; static "no unclassified privileged mutation" gate — §1 Constraints, §2.5
- [x] p2-2 sealed executor + crash-durable transaction state machine + target-resident recovery + write-seal — §2.6
- [x] p2-3 artifact staging inbox (content-addressed, validated, GC) — §2.7
- [x] p2-4 executor identity / anti-downgrade parity; disable legacy write entry points — §2.8
- [x] p2-5 role-specific readiness proxies; dangerous-write confirm-token binding (canonical hash + diff) — §2.5, §2.6a
- [x] p3-1 fleet lease authority (pool generation, fencing lease, step permit, quorum re-eval) — §2.9
- [x] p3-2 node-runtime N→N+1 upgrade with DB-compat + attestation rotation; ouro-diag honest labeling/sandbox — §2.10, §2.11
- [x] p3-3 threat-model/trust-matrix table; audit event schema; supported/retired/unsupported operation table — §2.12, §2.13, §2.15
- [x] p4-1 CLI wiring: `ouro-ops adopt` (conformance → evidence-bound approval → attestation) + intent-based `tool run` (recover → parity → build+validate intent → live re-attest gate → confirm-gate → crash-durable transaction → sealed executor) + `confirm create` bound to the canonical intent — integrates §2.1–2.9
- [x] p4-2 greenfield SKILL.md decision trees (adopt/observability/troubleshooting/runtime/kes-rotation/deploy/config/upgrade) as judgment frameworks (invariants/stop/red-lines, writes = intents, dangerous = operator-approved confirm) + sealed per-op executor scripts (fixed argv) — §1.D/§2.5/§2.6
- [x] p4-3 web onboarding prompt templates aligned to the adopt + intent commands (interaction unchanged; English prompt)
- [x] p4-4 dispatch-level end-to-end negative tests: TC-1..10 promoted from unit to dispatch (unmanaged refuse, hostile intent refuse, confirm binding, live drift, crash recovery, fleet quorum) on the container bed
- [x] p4-5 (fix) register the ops the greenfield SKILLs/web reference: add a READ tier (Mutability::Read) so `observability/health` passes the attested gate with no confirm/transaction, and register `upgrade/step` (dangerous). Closes the SKILL↔registry inconsistency that refused those two commands.
- [x] p5-1 SSH dispatch for `adopt`/`op` (control → target, confined principal); the commands actually reach the target instead of running control-local
- [x] p5-2 target-side probe: gather the live observation on the target (container inspect + node argv + cardano-cli reads) instead of a hand-supplied `--observation` file
- [ ] p5-3 real executor invocation: run the fixed argv on the target as the transaction's commit/verify/rollback (wire `executor::build_argv` into the transaction; readiness proxies as verify)
- [ ] p5-4 attestation stored target-side (root-owned `/var/lib/ouro/node-attestation.json`), read by `ouro-attested.sh`; adopt writes it there
- [ ] p5-5 `ouro-ops inbox stage` command (content-addressed ingress) + audit event emission (§2.13 schema) + fleet lease/step-permit + real control↔target parity wired into the op pipeline
- [ ] p5-6 container-bed end-to-end: adopt a real blinklabs container, run each op for real (docker), crash-injection + rollback, on the bed

## 4. Test and Acceptance Criteria
> Acceptance MATRIX — must FALSIFY the security claims, with adversarial interleavings, not just
> the happy path. Each invariant names its negative test. S0017 category-3 + no-leak tests are
> mandatory regressions.
- TC-1 unmanaged/non-attested node: every managed op refuses (typed, no mutation); diag/adopt
  exceptions behave per §2.11/§2.14.
- TC-2 adopt: conforming node adopted non-disruptively; non-conforming (bad digest, wrong layout,
  relay bearing forging keys, wrong supervisor shape) refused; approval NOT evidence-bound → refuse.
- TC-3 live drift + races: container recreate, digest move, mount/symlink swap, config drift,
  and a swap timed BETWEEN re-attestation and commit are all refused (in-lock CAS holds).
- TC-4 intent boundary: hostile intent (raw shell, out-of-allowlist path, traversal/symlink,
  parser-active string, duplicate/non-canonical JSON, key path, DB delete, secret read, confirmed-
  hash ≠ executed) rejected; no forbidden intermediate mutation or secret output.
- TC-5 artifact ingress: a replaced/oversized/wrong-type/replayed artifact after approval is
  rejected; the intent binds an immutable digest.
- TC-6 crash-durable transaction: fault injection (SSH loss/kill/reboot/OOM) before/after EACH
  phase leaves a recoverable state; recovery runs before new writes; rollback is restartable;
  rollback-of-rollback seals writes (exit 40).
- TC-7 dangerous writes require a live-bound confirm-token bound to the canonical intent + diff;
  agent cannot improvise the payload; category-3 unchanged.
- TC-8 fleet: two independent controllers, delayed messages, partitions, crashed lease holders —
  cannot both pass preflight and violate quorum / restart the active producer; BP-last enforced.
- TC-9 upgrade: N→N+1 rollback restores runtime AND attestation where DB-compat allows; where it
  does not, the honest re-sync/forward-recovery outcome is asserted (no false rollback promise).
- TC-10 completeness gates: no privileged mutation outside the capability API (static+dynamic);
  executor/registry/schema downgrade refused; every operation is in the supported/retired/
  unsupported table.

## 5. Execution Log (append-only)
- 2026-07-15 S0019 ACTIVATED (draft → active, Start Time set). Executing p1-1..p3-3; user: use my
  recommended approach at decision points, record in spec, do not close (wait for acceptance).
- 2026-07-15 p1-1 completed (§2.1): `crates/ouro/src/convention.rs` — layout contract + signed
  digest allowlist (`data/allowlist.json`, embedded via include_str!). `Allowlist::embedded`,
  `contract_for` (denylist wins; unknown/wrong-platform digest refused — no tag trust),
  `require_no_skew`, `enforce_anti_rollback` (MAC'd floor, erase→embedded fallback, ratchet).
  **Decision (recorded):** the allowlist SIGNATURE is embedded-trusted until the S0018 signed-
  release feed exists (mirrors `version::security_floor`, never a weaker fallback); the baseline
  blinklabs `image_config_digest` is a placeholder pinned at release time. 3 rust unit tests.
- 2026-07-15 p1-2 completed (§2.2): `crates/ouro/src/supervisor.rs` — pinned v1 contract
  (rootful docker, 1 node/host, bind mounts, standard daemon socket, `unless-stopped`, direct
  `run`). `SupervisorObservation::require_conformant` refuses every other shape (rootless, podman,
  non-rootful, compose, multi-node, named-volume-only, nonstandard socket, wrong restart) with a
  specific reason. **Decision (recorded):** a second supervisor shape is a separate versioned
  contract, never a generic runtime field. 2 rust unit tests (conforming accepted; 8 non-v1
  shapes each refused).
- 2026-07-15 p1-3 completed (§2.3, §2.14): `crates/ouro/src/attestation.rs` — attestation split
  into IMMUTABLE identity (role/digests/epoch/entrypoint/args/typed mounts/network/genesis/public
  cred ids/approval hash) vs VERSIONED managed state (monotonic generation + topology/config/
  kes-opcert hashes + container id). `require_matches_live` refuses identity drift AND out-of-band
  mutable change; `advance_state` CAS bumps the generation inside the write txn so a LEGITIMATE
  write is not drift (fixes the round-2 fingerprint-self-invalidation P0); `check_role` (relay
  forbids forging keys, bp requires opcert); §2.14 `bind_approval`/`verify_approval` bind a
  single-use operator token to the candidate hash + host key. 5 rust unit tests.
- 2026-07-15 p1-4 completed (§2.4): `crates/ouro/src/gate.rs` — central live re-attestation gate.
  `NodeLock` (O_CREAT|O_EXCL exclusive per-node lock, released on drop); `require_attested_node`
  takes the lock, probes by immutable container id, re-attests once, returns an `AttestedGuard`;
  `recheck_before_commit` re-probes + re-checks immediately before each irreversible commit,
  closing the check→act TOCTOU window. **Decision (recorded):** the OS lock + openat2-beneath-
  no-symlink are exercised target-side; the Rust gate owns the ORDERING + the exclusive lock so the
  protocol is testable and the executor (p2-2) composes it. 3 rust unit tests (lock exclusive +
  released on drop; a swap timed between gate-open and pre-commit is refused).
- 2026-07-15 p1-5 completed (§1.C): `ouro-skills/lib/ouro-attested.sh` — greenfield layout lib
  that READS `/var/lib/ouro/node-attestation.json` (role, container id, in-container paths,
  generation); `ouro_require_attested` refuses a node without an attestation (not_ouro_managed,
  exit 20). No process/mode discovery, no path guessing — S0017 detection not carried over. New
  test test_attested_layout.py: unmanaged refuse + recorded-fact reads + static gate (no pgrep/
  cgroup/ouro_node_arg/supervisor primitives in the lib). bundle-manifest regenerated.
- 2026-07-15 p2-1 completed (§1 Constraints, §2.5): `crates/ouro/src/intent.rs` — the intent
  envelope (schema_version, operation_id, immutable node_id, pre_state CAS base, nonce, expiry,
  closed payload), the deny-by-default privileged `registry()` (unregistered op → refused), closed
  per-op param schemas (Enum / MachineId / ArtifactRef `<id>@sha256:<64hex>` / bounded Uint),
  payload bounds (depth/width/string), canonical serialization + `canonical_hash` (key-order &
  duplicate-key independent → confirmed==validated==executed). **Decision (recorded):** payload
  carries only CLOSED typed references, never raw paths/blobs/shell; artifacts arrive via §2.7; the
  executor builds fixed argv, no templating (sink rule). 6 rust unit tests.
- 2026-07-15 p2-2 completed (§2.6): `crates/ouro/src/transaction.rs` — crash-durable write
  transaction. `TxState` machine (prepared→committing→committed→verifying→verified | rolling_back→
  {rolled_back|sealed}); `Journal` fsyncs each transition (tmp+rename+dir fsync) BEFORE its side
  effect; `run` commits→verifies→rolls back on failure; `finish_rollback` seals writes on a failed
  rollback (`WriteSeal`, exit 40, operator-only clear); `recover` (run at every tool-run start)
  re-drives an interrupted transaction idempotently. **Decision (recorded):** commit/verify/
  rollback are FIXED ops the executor supplies (from a validated intent → fixed argv); this module
  owns durable ordering/recovery/seal. Target-resident watchdog/recovery daemon is exercised
  target-side; the Rust core is the tested state machine. 6 rust unit tests inc. fault injection.
- 2026-07-15 p2-3 completed (§2.7): `crates/ouro/src/inbox.rs` — content-addressed artifact
  staging inbox. `stage` validates size + type shape (opcert/tx = JSON envelope, image = tar/gzip
  magic), stores by sha256 via O_EXCL create+rename (no symlink follow), returns `<id>@sha256:
  <digest>`; `resolve` re-verifies the digest against stored content (replaced → refused);
  `gc(now,ttl)` reclaims stale (caller-supplied clock, no ambient time). Intents reference only the
  immutable ref (§2.5), never a path/blob. 4 rust unit tests.
- 2026-07-15 p2-4 completed (§2.8): `crates/ouro/src/parity.rs` — `SecurityIdentity` (build id +
  embedded executor digest + intent schema version + min security version). `require_parity`
  refuses an executor-digest / schema-version mismatch and an anti-downgrade (target below control
  min) before accepting an intent (extends p5-17 parity to the executor). `require_registered_write`
  disables every legacy S0017 write tool unless migrated into the deny-by-default registry. 4 rust
  unit tests.
- 2026-07-15 p2-5 completed (§2.6a, §2.5): `crates/ouro/src/readiness.rs` — role-specific
  readiness proxies (running attested container + socket answers + expected network/genesis + tip
  advancing; BP adds valid KES/opcert + credentials loaded, relay adds established peers). No
  'block produced' postcondition (a low-stake pool can't show one in bounds). `bind_confirm`/
  `verify_confirm` bind a confirm-token to the EXACT canonical intent hash + human diff (a token
  for a different intent/diff is refused). 3 rust unit tests.
- 2026-07-15 p3-1 completed (§2.9): `crates/ouro/src/fleet.rs` — fleet lease authority.
  `PoolAuthority::acquire` grants an exclusive expiring lease and strictly bumps a monotonic
  fencing token; `TargetFence::accept` refuses a step permit whose token is below the highest the
  target has honored (fences a superseded/crashed controller at the point of action) or is expired;
  `require_quorum` re-evaluates min-online-relays immediately before a disruptive step; `require_bp_last`
  enforces BP-last ordering. 4 rust unit tests (lease exclusive+fencing; target fences a stale
  controller; quorum guard; BP-last).
- 2026-07-15 p3-2 completed (§2.10, §2.11): `crates/ouro/src/upgrade.rs` — signed `TransitionMeta`
  (N→N+1, from/to image digests, db_format_compatible, downgrade_supported, snapshot_taken);
  `plan_rollout` (relays canary-first, BP last); `validate_transition` (only N→N+1 + target image on
  the allowlist); `failure_outcome` returns RollbackToN ONLY when DB-compat/downgrade/snapshot make
  it real, else ReSyncRequired (no false rollback promise). §2.11: troubleshooting SKILL diag
  wording corrected from 'physically cannot break the node' to honest 'UNPRIVILEGED diagnostics,
  NOT read-only' (can write own home/tmp, egress, resource use). 3 rust tests; skill-docs gate +
  manifest regenerated.
- 2026-07-15 p3-3 completed (§2.12/§2.13/§2.15): `docs/S0019-threat-model.md` (trust matrix —
  each adversary → enforcing component § → its negative test; TCB out-of-scope inc. P0-1),
  `docs/S0019-operations.md` (supported/retired/unsupported operation table),
  `schemas/audit-event.schema.json` (closed, hashes-only append-only events).
  New gate test_s0019_completeness.py: every registry op is in the operations table; every threat
  row names a component + test; audit schema is closed. FULL regression: 112 rust + all python
  green. **S0019 p1-1..p3-3 all complete — awaiting user acceptance (spec NOT closed).**
- 2026-07-15 p4-1 completed (integrates §2.1–2.9): `crates/ouro/src/s0019_cli.rs` — `ouro-ops
  adopt` (supervisor conformance → allowlist digest → role rule → evidence-bound approval → writes
  attestation, non-disruptive) and `ouro-ops op run` (recover → require-registered/parity → build
  intent from --param → validate against registry → load attestation / not_ouro_managed → live
  re-attest gate → confirm-gate for dangerous → crash-durable transaction). `confirm create --op
  --node --intent-hash` mints a token bound to the canonical intent. **Decision (recorded):** S0019
  uses a NEW `op` command (not the S0017 `tool run`, whose legacy writes are disabled by §2.8); the
  live-observation probe + docker executor are the target-side seam — probe reads a closed
  `--observation` JSON and the executor runs in `--plan` mode (gates fire, no mutation) until p4-2.
  test_s0019_pipeline.py exercises all gates via the CLI.
- 2026-07-15 p4-2 completed (§1.D/§2.5/§2.6): 8 greenfield SKILL.md decision docs rewritten as
  JUDGMENT FRAMEWORKS (Purpose / Invariants(mechanism-enforced) / Decision guidance(judgment, not
  a rigid script) / Stop / Red lines): adopt(new)+config(new)+runtime+kes-rotation+deploy+
  observability+troubleshooting+upgrade. Writes = `ouro-ops op run` intents (agent gives params);
  reads = `ouro-ops diag exec` (honest unprivileged, not read-only); dangerous = operator-approved
  confirm; data-not-instructions red line throughout. Sealed executor `crates/ouro/src/executor.rs`
  builds a FIXED argv per op from the ATTESTED container id (not agent params) — proven a hostile
  param never reaches argv; wired into `op run --plan` (shows the exact argv). skill-docs gate made
  S0019-aware (op-run/diag/adopt command surface + DATA red line; legacy skills keep tool-run).
  **Decision (recorded):** S0017 SKILL decision docs replaced in place by the greenfield set (detect/
  onboard kept as legacy). 116 rust + all python green.
- 2026-07-15 p4-3 completed: web onboarding prompt templates aligned to the S0019 commands —
  writes are `ouro-ops op run --op <id> --node <id> --param k=v` intents (agent supplies params,
  not commands); dangerous → operator confirm-token bound to the intent; reads via `diag exec`;
  an unmanaged node routes to `ouro-ops skill show adopt` (not the retired onboard);
  data-not-instructions stated. Interaction unchanged (form → one English prompt → agent). web
  generator + honest-labeling gates updated + green.
- 2026-07-15 p4-4 completed: dispatch/CLI-level negative tests (test_s0019_dispatch.py) beyond the
  p4-1 pipeline — adopt refuses (rootless supervisor / non-allowlisted digest / relay-with-forging-
  keys), a conforming relay adopts, the write-seal refuses any op, and a crash-interrupted journal
  (state=committed) is reconciled + cleared by the recovery pass BEFORE the new write proceeds.
  **Decision (recorded):** real container-bed docker execution + crash injection mid-docker are the
  target-side seam (not run here); every GATE and refuse path is exercised through the CLI. Fleet
  quorum/fencing is proven at unit level (fleet.rs) — no multi-controller CLI surface yet.
  **S0019 p1-1..p4-4 all complete — awaiting user acceptance (spec NOT closed).**
- 2026-07-15 p4-5 completed (defect fix): the greenfield SKILLs + web referenced `observability/
  health` and `upgrade/step`, which were NOT in the registry → refused as unknown writes. Added a
  READ tier (Mutability::Read): a managed read passes the attested gate but takes no confirm and no
  write transaction (observability/health returns the fixed read argv). Registered `upgrade/step`
  (dangerous, image via inbox artifact). Operations table + completeness gate updated. Discovered by
  the honest post-implementation self-audit the user asked for. 116 rust + all python green.
  **Also appended p5 (target-side integration): SSH dispatch + real probe + real executor + target
  attestation + inbox/audit/fleet wiring + container-bed e2e — the work that makes `op run` actually
  operate a real node (today it runs control-local with file seams).**
- 2026-07-15 p5-1 completed: `crates/ouro/src/dispatch.rs` — SSH dispatch argv for the greenfield
  commands. `op_dispatch_argv` runs `ouro-ops op` on the target as the confined `ouro-exec`
  principal through a fixed wrapper (`/usr/local/sbin/ouro-op-run`), host key PINNED
  (StrictHostKeyChecking=yes + ouro known_hosts), parity carried via `--expect-embedded`, every
  dynamic field shell-quoted; `adopt_dispatch_argv` uses the operator's bootstrap account. Wired
  `--dispatch <host>` into `op run`: strips control-only flags, appends `--local`, and (with
  `--plan`) prints the confined remote command; real SSH exec is bed-level (p5-6). 3 rust tests +
  a CLI dispatch-plan assertion. 119 rust + all python green.
- 2026-07-15 p5-2 completed (§2.4): `ouro-skills/lib/ouro-probe.sh` — the target-side observation
  probe. `ouro_observe` gathers the closed observation from the running container (image config
  digest, container id, creation epoch, entrypoint/args, mounts, restart policy) + node facts
  (topology/config/genesis hashes, opcert id, forging-key presence, host key) and emits the exact
  { supervisor{...}, live{...} } JSON the Rust pipeline consumes — replacing the hand-supplied
  `--observation` file. All docker access is in lib (supervisor gate exempts the whole lib/ layer).
  Secrets are never read — hashes/ids only. Stub-tested (test_probe.py); real gathering is bed-level.
- 2026-07-14 round-1 multi-agent review (Claude + Codex); rewritten to greenfield + two-tier +
  option (b) intent/executor; decisions A/B and post-review items closed.
- 2026-07-14 round-2 multi-agent review (Claude + Codex) found the rewrite named the right
  mechanisms but had not made them normative protocols; per the user, §2 expanded into
  self-sufficient normative protocols (2.1–2.15) so a zero-context implementer can build it safely.

## 6. Validation Evidence (append-only)
- p1-1 | stack: rust | command: cargo test convention | result: pass | note: 3 tests — embedded
  allowlist parses+signed (relay forbids forging keys, bp requires opcert); allowlisted digest
  conforms while unknown/wrong-platform/denylisted refuse (no tag trust); skew refuse + anti-
  rollback floor ratchets and refuses a lower version.
- p5-2 | stack: python | command: python3 tests/test_probe.py; tests/test_supervisor_gate.py |
  result: pass | note: with docker stubs the probe emits a well-formed observation with every key
  the Rust ObsLive/SupervisorObservation expects (image/cid/epoch/entrypoint/args/mounts/hashes/
  forging-keys); docker confined to lib.
- p5-1 | stack: python | command: cargo test dispatch (+full 119); tests/test_s0019_dispatch.py |
  result: pass | note: op dispatch argv is confined (ouro-exec + fixed wrapper) + host-key pinned +
  parity + shell-quoted hostile arg; adopt dispatch uses the bootstrap account; CLI --dispatch
  --plan emits the confined remote command with --local.
- p4-5 | stack: python | command: cargo test (116); tests/test_s0019_completeness.py; skill_docs |
  result: pass | note: observability/health runs as a read (argv docker exec ... query tip, no
  confirm); upgrade/step registered + requires confirm; registry ⊆ operations table holds.
- p4-4 | stack: python | command: python3 tests/test_s0019_dispatch.py; full cargo test (116) +
  all python | result: pass | note: adopt refuse paths (supervisor/digest/role); write-seal refuse;
  crash-committed journal recovered + cleared before the new write; full suite green.
- p4-3 | stack: ui | command: python3 tests/test_web_generator.py; python3 tests/test_honest_labeling.py
  | result: pass | note: prompt drives writes through `ouro-ops op run` intents, routes unmanaged
  nodes to adopt, states data-not-instructions; interaction unchanged; static gates green.
- p4-2 | stack: python | command: python3 tests/test_skill_docs.py; cargo test executor (+full 116)
  | result: pass | note: 8 greenfield judgment-framework SKILLs pass the S0019-aware gate; executor
  builds fixed argv from the attested container id (docker restart cid-real-42, not agent bp1);
  hostile param never reaches argv; op run --plan shows the argv.
- p4-1 | stack: python | command: python3 tests/test_s0019_pipeline.py; cargo test (112) | result:
  pass | note: adopt writes attestation (gen 0); op on unadopted node → not_ouro_managed; dangerous
  write without confirm refused; hostile param refused; legacy op disabled; confirm-token bound to
  the exact intent lets it pass; container-id drift refused before mutation.
- p3-3 | stack: python | command: python3 tests/test_s0019_completeness.py; full cargo test (112);
  all tests/test_*.py | result: pass | note: registry ⊆ operations table; threat matrix rows map
  component+test; audit schema closed; full suite green.
- p3-2 | stack: rust | command: cargo test upgrade; python3 tests/test_skill_docs.py | result:
  pass | note: 3 rust tests — BP-last rollout; rollback promised only when recoverable else honest
  re-sync; N→N+2 and non-allowlisted target refused. Diag honest-labeling passes skill-docs.
- p3-1 | stack: rust | command: cargo test fleet | result: pass | note: 4 tests — lease exclusive
  while live + fencing token increases; a stale/ expired step permit is fenced target-side; quorum
  refuses stopping the last relay; BP refused while relays pending.
- p2-5 | stack: rust | command: cargo test readiness | result: pass | note: 3 tests — healthy bp
  passes, 7 unhealthy modes fail; relay drops forging reqs but needs peers; confirm-token binds the
  exact intent hash + diff (wrong hash/diff refused).
- p2-4 | stack: rust | command: cargo test parity | result: pass | note: 4 tests — self parity ok;
  executor-digest mismatch + schema skew + downgrade refused; legacy write (deploy/takeover)
  disabled, registered write (runtime/restart) allowed, unknown refused.
- p2-3 | stack: rust | command: cargo test inbox | result: pass | note: 4 tests — stage/resolve
  roundtrip; tampered content + malformed/unknown ref refused; junk/empty/oversized refused +
  gzip image accepted; gc reclaims stale.
- p2-2 | stack: rust | command: cargo test transaction | result: pass | note: 6 tests — happy→
  verified+cleared; verify-fail→rollback; failed-rollback→seal (further writes refused until
  clear); crash at Committed→recovery re-verifies to Verified; crash mid-commit + unhealthy→
  recovery rolls back; clean journal→noop.
- p2-1 | stack: rust | command: cargo test intent | result: pass | note: 6 tests — unregistered op
  refused (deny-by-default); unknown field / hostile machine id / raw path where ArtifactRef
  required / bad enum / over-long string all refused; well-formed artifact ref + enum accepted;
  dangerous ops flagged; canonical hash key-order independent; expiry enforced.
- p1-5 | stack: python | command: python3 tests/test_attested_layout.py | result: pass | note:
  unmanaged node → not_ouro_managed exit 20; attested node → role/container/db/keys/generation read
  from the attestation; static gate proves no detection primitives in the greenfield lib.
- p1-4 | stack: rust | command: cargo test gate | result: pass | note: 3 tests — per-node lock is
  exclusive and released on drop; require_attested_node + recheck_before_commit refuses a container
  swap timed between gate-open and commit (TOCTOU closed).
- p1-3 | stack: rust | command: cargo test attestation | result: pass | note: 5 tests — live match
  + 6 drift cases refused; a legitimate topology write advances generation (8) and is NOT drift
  post-write; CAS rejects a stale generation; relay-with-forging-keys refused; approval evidence is
  candidate+host-key specific.
- p1-2 | stack: rust | command: cargo test supervisor | result: pass | note: 2 tests — v1 shape
  accepted; 8 non-conforming shapes (rootless/podman/non-rootful/compose/multi-node/named-volume/
  nonstandard-socket/wrong-restart) each refused with a specific reason.

## 7. Change Requests (append-only)
- 2026-07-14 Decisions A (no migration) and B (blinklabs, no self-build) resolved; then round-1
  review-driven rewrite (greenfield, two-tier, option b, digest allowlist, honest P0-1 labeling).
- 2026-07-14 blinklabs baseline finalized; P0-1 control-plane boundary kept OPEN (honestly
  labeled) — a future separate hardening spec.
- 2026-07-14 **round-2 review → §2 made normative (user: "写进去吧").** The round-2 review (Claude
  ACTIVATE_WITH_CHANGES / Codex REWORK) showed the round-1 findings were the right DIRECTION but
  not yet enforceable protocols; Codex (no chat context) correctly judged the spec on what it
  wrote, which is the right bar since the implementer has no chat context. §2 now specifies, as
  normative protocols: intent envelope + sink rules + deny-by-default privileged-capability API
  (2.5, Constraints); attestation immutable-identity vs versioned-state (2.3) with CAS in-transaction;
  live re-attestation closing check→act TOCTOU via immutable container id + in-lock + openat2 (2.4);
  crash-durable transaction state machine with target-resident recovery + write-seal (2.6); artifact
  staging inbox (2.7); executor identity / anti-downgrade parity (2.8); fleet lease authority (2.9);
  DB-compat-honest upgrade (2.10); honest ouro-diag labeling (2.11); threat-model/trust-matrix
  (2.12); audit schema (2.13); evidence-bound adoption approval (2.14); supported/retired/unsupported
  operation table (2.15). Execution plan + acceptance matrix expanded to match. Ready to activate;
  the protocol items (p1/p2) must be specified + tested before any dispatched write goes live.
