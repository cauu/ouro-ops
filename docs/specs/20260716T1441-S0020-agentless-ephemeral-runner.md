# Agentless Ephemeral Runner Operations

Spec-ID: S0020
Status: active
Created Time: 2026-07-16T14:41:55+08:00
Start Time: 2026-07-16T14:41:55+08:00
Completion Time:
Previous Spec-ID: S0019
Closure Reason:

## 1. Requirement Details

### Background
S0019 proved that a sealed typed operation surface can keep an AI agent inside reviewed workflows,
but its target-resident full CLI, adoption attestation prerequisite and exact control↔target
security-identity parity made ordinary reads and release updates operationally brittle. Live
fresh-agent acceptance repeatedly stopped at `not_ouro_managed` even though SSH access, target
confinement and the node itself were healthy enough to inspect. The operator has explicitly chosen
flow reliability over hostile-terminal hardening for the current product stage.

### Scope
- The complete Ouro CLI and Skills remain on the control machine only.
- Every dispatched operation automatically transfers the control release's static Linux runner to
  a run-unique private target directory, verifies the exact expected SHA-256, executes it through
  the existing operator SSH authority, returns bounded structured output and removes it.
- No operation depends on a target-installed `/usr/local/bin/ouro-ops`, Ouro gate/daemon, remote CLI
  version, global `ouro_managed` flag or adoption attestation.
- Read operations, beginning with observability, derive evidence from current live state and label
  its assurance honestly; absence of prior Ouro metadata is never a read blocker.
- Typed writes derive their candidate from current signed policy + live state on every preview and
  apply. Apply must re-probe and match the operator-approved candidate/intent; durable truth is the
  node/runtime state itself, not a parallel Ouro ownership record.
- Website prompts and embedded Skills keep the agent on typed Ouro commands. Transport, runner
  selection, SSH argv, temporary paths and cleanup are internal implementation details.
- Validate the non-deploy workflows on the real Linux/x86_64 BP and relay without completing an
  upgrade, KES change or restart during acceptance.

### Constraints
- Threat model for S0020 is an honest-but-fallible or misled agent using the provided Ouro command
  surface. Defending against an agent that deliberately bypasses Ouro with raw terminal/SSH access
  is deferred and must not complicate this delivery.
- Continue using the operator's existing named SSH credential references and `cardano` account;
  no dedicated agent key, credential broker, Secure Enclave flow or remote gate is required.
- The runner is supplied only by the control binary/build; the agent cannot select runner bytes,
  remote executable paths, hashes, shell fragments or sudo argv.
- Use run-unique target paths, closed argument grammar, host-key pinning, bounded execution/output,
  clean environment and best-effort cleanup. Never overwrite a shared executable path.
- No cold, KES signing or VRF secret material enters context or output.
- Signed immutable OCI policy remains authoritative for operations that depend on a supported
  runtime. A tag alone is not trusted.
- Operator-owned `pool-spec.yaml` remains outside repository delivery.

### Non-goals
- Hostile-agent/raw-SSH prevention, app-bound credential custody and hardware-backed keys.
- A target-resident Ouro CLI, gate, daemon, auto-update mechanism or Ouro-specific ownership state.
- Greenfield Cardano node deployment or reshaping a non-conforming node.
- Completing a real upgrade, KES rotation, restart or deploy during acceptance.
- Exactly-once guarantees based on a permanent Ouro target journal. Each operation must instead be
  idempotent or recoverable from live target/application state; irreversible deploy remains out of
  this spec.

## 2. Outline Design

### 2.1 Control/runner split
The macOS control binary embeds or is release-manifest-bound to the static Linux/x86_64 runner.
Dispatch probes only the target OS/arch, selects the closed supported artifact, and never asks the
agent for a binary path. Development builds may use the same repository-built static artifact only
through an explicit internal/test configuration unavailable in public prompt parameters.

### 2.2 Ephemeral SSH transport
One dispatch opens a host-key-pinned SSH session as the operator account, creates a 0700 run-unique
directory, receives the runner through stdin, verifies its exact control-known SHA-256, marks it
0500, executes `sudo env -i ... <runner> target ...` with closed args, then removes the directory.
No shared `/tmp/ouro-ops`, PATH entry, installed binary or version floor is created. Transport plans
may disclose bounded argv structure but never credentials or a reusable arbitrary-exec capability.

### 2.3 Stateless observation
`observability/health` runs the sealed live probe before any attestation lookup. It returns actual
tip evidence plus `assurance: live_observation` and an informational management-state field when
useful; missing attestation is not an error. Unsupported images/layouts remain honestly reported,
but safe host/runtime facts are not converted into ownership claims.

### 2.4 Stateless typed writes
The target runner validates the signed allowlist, pool-spec-derived role/network/genesis, closed
operation schema and current runtime facts. Preview emits a candidate/intent hash with no durable
capability. Apply carries the operator-approved hash/token, re-derives the same candidate from live
state and refuses drift. Restart/upgrade/KES outcomes and recovery are derived from container image,
container epoch and public opcert/runtime facts rather than an Ouro attestation. No write is executed
in this spec's production acceptance.

### 2.5 Product flow
Public prompts call only ordinary `ouro-ops` commands. They do not ask the agent to install, onboard,
adopt, synchronize or choose a runner. `not_ouro_managed` is removed from the read flow; any legacy
onboard/adopt commands remain migration-only and are not prerequisites for current operations.

### 2.6 Risk and rollback
Until S0020 is complete, S0019 remains available in Git history and installed target binaries are
left untouched but ignored by new dispatch. Each item is independently committed. A defect in the
new transport is rolled forward by restoring the prior dispatch constructor under a new spec item;
no target state is destroyed merely to test the new path.

## 3. Execution Plan
- [ ] p1-1 implement and test the run-unique ephemeral runner SSH transport with closed artifact,
  argv, hash verification, bounded output and cleanup semantics.
- [ ] p1-2 migrate observability/health to live stateless execution with no attestation prerequisite
  and honest assurance fields.
- [ ] p2-1 migrate target-validated typed plan construction to current live state + pool-spec
  bindings, without target-installed CLI/parity or remote ownership state.
- [ ] p2-2 migrate non-deploy apply plumbing to revalidated approved candidates and live-state
  recovery while preserving the no-real-write acceptance boundary.
- [ ] p3-1 update embedded Skills, website prompts, manifests and docs so agents never onboard,
  adopt, synchronize or reason about a remote Ouro version in the ordinary flow.
- [ ] p3-2 build the final macOS control + static Linux runner and run fresh-agent real-host E2E for
  observability and non-mutating plan/safe-stop paths on BP and relay.

## 4. Test And Acceptance Criteria
- ERTC-1 transport: public dispatch automatically uses only the control-selected Linux runner in a
  run-unique private target directory; exact hash, host key, closed args, bounded output and cleanup
  are proven, and no `/usr/local/bin/ouro-ops` dependency appears in the remote argv.
- ERTC-2 stateless read: a fresh-agent observability prompt obtains real BP and relay tip responses
  without attestation, adoption, onboard, confirm or target installation and makes only supported
  claims.
- ERTC-3 stateless plan: runtime, upgrade and KES target plans bind role/network/genesis, signed
  image policy, current target facts and one stable candidate/intent hash without persistent Ouro
  target state.
- ERTC-4 apply safety: candidate drift, wrong host/role/network/genesis, unsupported image, wrong
  runner digest, unknown flags and approval mismatch are refused before mutation; acceptance does
  not perform the real mutations.
- ERTC-5 release independence: replacing the local control/runner build requires no remote install,
  target CLI update, re-onboard or re-adopt; the next invocation transports the new runner.
- ERTC-6 product contract: Skills, website prompt, tests and docs contain no ordinary-flow
  `not_ouro_managed` recovery, remote CLI sync, onboard or adopt prerequisite.
- ERTC-7 quality: Rust, Python, L2 integration, Clippy, bundle manifest verification and
  `git diff --check` pass; `pool-spec.yaml` is untouched and uncommitted.
- Pass/fail: a fresh agent can paste the website-equivalent non-deploy prompts and reach real typed
  reads/plans through the current local build while targets have no required resident Ouro version
  or adoption metadata.

## 5. Execution Log (append-only)
- 2026-07-16T14:41+0800 [~] p1-1 started. Replace target-installed CLI dispatch with the
  control-selected, run-unique ephemeral Linux runner transport and prove its closed lifecycle.
- 2026-07-16T14:49+0800 [x] p1-1 completed. Added a host-key-pinned ephemeral dispatch constructor
  that accepts only an internally selected runner digest and closed target argv, receives bytes on
  stdin beneath a run-unique 0700 directory, verifies lowercase SHA-256, executes through
  `sudo env -i` and removes the directory through an exit trap. Added bounded concurrent SSH stdin
  streaming so a static runner cannot deadlock against stdout/stderr pipe capacity. No current
  operation is switched in this item; target behavior remains unchanged until p1-2.
- 2026-07-16T15:02+0800 [~] p1-2 started. Route observability through the ephemeral runner and
  derive its typed result directly from the sealed live probe without loading adoption attestation,
  target-installed CLI identity or persistent Ouro management state.
- 2026-07-16T15:05+0800 [x] p1-2 completed. `observability/health` now streams the control-build's
  digest-bound static Linux runner as `cardano`, invokes only `target observe --node <id>`, returns
  live cardano-cli tip fields with `assurance: live_observation` and
  `management_state: not_required`, and treats signed runtime policy as informational for reads
  while retaining it as a write gate. Missing attestation, allowlist floor and installed target CLI
  are absent from the path. Unsupported image policy is reported without suppressing safe facts.
- 2026-07-16T15:07+0800 [~] p2-1 started. Move non-deploy `--plan` construction into the ephemeral
  runner: derive target identity and fleet policy from the operator pool spec on control, then bind
  the final typed candidate to signed image policy and a fresh target-side live observation without
  target ownership metadata or installed CLI parity.
- 2026-07-16T15:24+0800 [x] p2-1 completed. Dispatched non-deploy `--plan` now requires the
  operator pool spec, derives and verifies its host/user/credential/node/role/network/genesis/fleet
  bindings on control, and sends only those closed values to `target plan`. The target validates the
  signed running-image convention, supervisor/layout/role, typed operation schema and stable live
  state hash before returning the final candidate and fixed/redacted executor argv. Restart, KES,
  image preload and upgrade-step plan paths write no target ownership or transaction state; public
  artifacts are candidate-bound by digest and explicitly deferred to apply-time domain validation.
- 2026-07-16T15:29+0800 [~] p2-2 started. Route non-deploy apply through the same ephemeral runner,
  verify and consume control-local operator approval against the exact candidate, stream any public
  artifact in the same private invocation, re-derive the candidate from fresh live state, and
  refuse every approval/drift/artifact/fleet mismatch before a sealed executor mutation.
- 2026-07-16T16:04+0800 [x] p2-2 completed. All dispatched non-deploy writes now require the
  pool-spec-derived S0020 path, a lowercase approved candidate and a locally verified single-use
  confirmation. Disruptive writes additionally verify the short-lived signed fleet permit against
  live ephemeral `target status` facts; fleet snapshot collection no longer uses the installed op
  wrapper or adoption metadata. The target re-plans twice around artifact/domain validation before
  any fixed Docker argv, verifies live postconditions, and performs in-invocation, live-verified
  rollback for KES/preload/upgrade where an inverse exists. Public artifacts travel immediately
  after runner bytes into the same private run directory and are never staged durably.
- 2026-07-16T16:18+0800 [~] p3-1 started. Rewrite the public website prompt, embedded Skills,
  command help and current operations documentation around existing `cardano` SSH access plus the
  automatically transported ephemeral runner. Remove ordinary-flow onboard/adopt/version-sync /
  `not_ouro_managed` recovery, replace durable remote artifact staging with local content preview +
  one-invocation delivery, and make troubleshooting use the operator's existing SSH principal with
  an explicit diagnostic-intent (not OS-confinement) assurance boundary.
- 2026-07-16T16:58+0800 [x] p3-1 completed. All non-deploy Skills and website-generated prompts now
  describe only control-local Ouro plus existing `cardano` access and automatic ephemeral runner
  delivery. Runtime/KES/upgrade show the exact spec-bound plan → operator confirmation → permit-last
  → candidate-bound apply commands. `inbox preview` validates public files locally without durable
  staging; apply performs the single runner+artifact transfer. Diagnostics use the spec account with
  honest `read_only_enforced: false` assurance and retain pinned/bounded/audited transport. Legacy
  onboard/adopt are visibly migration-only. Current README/operations/release docs and bundle
  manifest agree. The live probe now uses Cardano's semantic genesis hash, avoiding JSON-byte-layout
  false drift against website specs.
- 2026-07-16T17:00+0800 [~] p3-2 started. Build the final static Linux/x86_64 runner from this
  source, embed it into the macOS control artifact, then replay website-equivalent prompts through a
  fresh agent against the real BP/relay. Execute only live reads, local previews, target plans,
  diagnostic-only commands and expected safe refusals; do not mint capabilities or perform restart,
  KES, image-load, upgrade or deploy mutations.
- 2026-07-16T17:10+0800 [x] p3-2 completed. Built the final static Linux/x86_64 runner and embedded
  it in the matching arm64 macOS control; the installed control copy is byte-identical. A context-free
  agent given only the website-equivalent observability prompt independently read the installed
  Skill and returned real BP/relay tips without requesting a binary, onboarding or adoption. A
  second fresh agent independently exercised runtime, KES and preload plans, upgrade transition
  safe refusal, diagnostics, and config/detect stop semantics without minting any capability or
  applying a write. Final target cleanup checks found no `ouro-run.*` directory; original BP/relay
  container ids remained unchanged.

## 6. Validation Evidence (append-only)
- ERTC-1 | stack: rust | command: `cargo test -p ouro ephemeral_runner -- --nocapture` | result:
  pass | note: 3/3 prove pinned SSH, random private path, digest verification, clean root env,
  cleanup, no installed CLI/wrapper reference, hostile-argv quoting, and malformed digest/user/empty
  command refusal.
- ERTC-1 | stack: rust | command: `cargo test -p ouro
  bounded_input_stream_reaches_child_and_closes -- --nocapture` | result: pass | note: a concurrent
  512 KiB opaque payload reaches the child exactly and EOF terminates the receiver without deadlock.
- ERTC-7 | stack: rust | command: `cargo clippy -p ouro --lib --tests -- -D warnings` | result: pass |
  note: zero warnings for the transport primitive and tests.
- ERTC-7 | stack: other | command: `git diff --check` | result: pass | note: p1-1 patch is whitespace
  clean and operator-owned `pool-spec.yaml` remains untracked.
- ERTC-2 | stack: integration | command: `python3 tests/test_s0020_observability.py` | result: pass |
  note: no-attestation local and closed target reads return real-shaped tip evidence; unsupported
  image policy is informational; control transport preview has only cardano + random tmp runner;
  an SSH stub consumes the exact selected bytes and its typed result is forwarded unchanged.
- ERTC-2 | stack: bed | command: `target/release/ouro-ops op run --op observability/health --node
  bp1 --dispatch 84.247.139.72 --ssh-key creds://bp1` | result: pass | note: the freshly embedded
  static Linux runner returned mainnet Conway tip block 13684676 / slot 192618980, sync 100.00,
  socket answering and signed Blink Labs 10.5.4-1 policy support without target ownership state.
- ERTC-2 | stack: bed | command: `target/release/ouro-ops op run --op observability/health --node
  relay1 --dispatch 31.220.95.72 --ssh-key creds://relay1` | result: pass | note: the same control
  artifact returned mainnet Conway tip block 13684678 / slot 192618989, sync 100.00 and socket
  answering without target ownership state.
- ERTC-1 | stack: bed | command: pinned read-only SSH `find /tmp -maxdepth 1 -type d -name
  'ouro-run.*' -print` on BP and relay after the reads | result: pass | note: both outputs were empty;
  per-invocation target runner directories were removed.
- ERTC-5 | stack: other | command: `cargo zigbuild --release --target
  x86_64-unknown-linux-musl -p ouro`; then `OURO_EMBED_LINUX_X86_64_RUNNER=<built-elf> cargo build
  --release -p ouro`; release transport preview | result: pass | note: the 5,565,896-byte static
  Linux/x86_64 ELF digest `b7153a0b…abbdf7` is embedded in the 11 MiB arm64 macOS control and
  selected automatically; no public runner path exists.
- ERTC-7 | stack: rust | command: `cargo test -p ouro` | result: pass | note: 180/180 library tests,
  binary tests and doc tests pass; the updated committed bundle manifest matches the embedded live
  probe.
- ERTC-7 | stack: python | command: `python3 tests/test_probe.py`; S0020 observability; S0019
  pipeline/inbox-audit/dispatch regression scripts | result: pass | note: live probe vocabulary,
  new stateless contract and retained write/adoption negative gates all pass together.
- ERTC-7 | stack: rust | command: `cargo clippy -p ouro --lib --tests -- -D warnings`; targeted
  rustfmt check; `git diff --check` | result: pass | note: zero warnings or changed-file formatting /
  whitespace errors; operator-owned `pool-spec.yaml` remains untracked.
- ERTC-3 | stack: integration | command: `python3 tests/test_s0020_stateless_plan.py` | result: pass |
  note: runtime and KES candidates are stable for identical live state, change after container
  drift, redact container env values, reject role/network/host/credential/unknown-arg mismatches,
  and public dispatch carries only the embedded runner plus pool-spec-derived target argv.
- ERTC-3 | stack: bed | command: release ephemeral `runtime/restart --plan --spec
  /tmp/ouro-s0020-live-spec.yaml` on BP and relay | result: pass | note: BP candidate
  `6340c54f…abd94` repeated identically; relay candidate `1a559fbe…90611`; both bind allowlist v2,
  Blink Labs 10.5.4-1 OCI identity, role/mainnet/genesis/spec/fleet policy and unchanged live
  container IDs without restart or target Ouro state.
- ERTC-3 | stack: bed | command: release ephemeral KES install and upgrade preload plans | result:
  pass | note: BP public-opcert candidate `c17dd523…d6889` contains only a digest placeholder and
  fixed docker cp/restart argv; relay image-preload candidate `eb6ae1c7…0888` binds a public archive
  digest + allowlisted config digest and contains only fixed docker load argv. Neither was applied.
- ERTC-4 | stack: bed | command: release ephemeral `upgrade/step --plan` from running 10.5.4-1 to
  the older allowlisted config | result: pass | note: refused because signed N→N+1 transition
  metadata is absent; mere allowlist membership never authorizes an upgrade and no mutation ran.
- ERTC-3 | stack: other | command: `ouro-ops fleet spec identity --spec pool-spec.yaml` | result:
  expected refusal | note: the operator-owned untracked spec has truncated noncanonical genesis
  hashes. It was not edited; bed plan validation used a temporary, valid spec bound to the live
  mainnet genesis file SHA-256 `59cd3932…36961`.
- ERTC-1 | stack: bed | command: pinned read-only tmp-directory inspection after parallel BP/relay
  restart, KES and preload plans | result: pass | note: neither target retained an `ouro-run.*`
  directory and the plan outputs retained their original running container IDs.
- ERTC-7 | stack: regression | command: `cargo test -p ouro`; Clippy `-D warnings`; probe, S0020
  stateless-plan and S0019 dispatch Python suites; `git diff --check` | result: pass | note: 180 Rust
  tests plus all selected integration regressions pass with no warnings or whitespace errors.
- ERTC-1 | stack: rust | command: `cargo test -p ouro` payload transport unit cases | result: pass |
  note: exact runner-then-file stdin streaming, fixed-length Python splitting, dual SHA-256 checks,
  0500/0400 modes, clean internal payload environment and exit-trap cleanup pass; malformed/empty
  digests and sizes refuse. No installed CLI/wrapper path appears in the argv.
- ERTC-4 | stack: integration | command: `python3 tests/test_s0020_stateless_apply.py` | result:
  pass | note: wrong candidate, unknown flag, wrong role, approval-to-apply drift, artifact digest /
  Docker-config mismatch, missing confirmation and token replay all refuse before the fake mutation
  or SSH boundary. The successful target seam executes exactly one fake `docker restart cid-plan`;
  the successful control seam streams byte-exact `runner || artifact` once and consumes approval.
- ERTC-3 | stack: integration | command: S0020 target-status and fake two-node fleet-permit cases
  in `tests/test_s0020_stateless_apply.py` | result: pass | note: BP/relay fleet facts are gathered
  through release-selected ephemeral `target status`, bind role/network/genesis/image/host key/live
  epoch, report `management_state: not_required`, and mint only a control-local lease/permit with no
  target ownership state or old `ouro-op` channel.
- ERTC-4 | stack: other | command: code-path inspection + fake executor apply/rollback tests |
  result: pass | note: runtime, KES, preload and upgrade use fixed argv arrays; target planning is
  repeated after expensive public-artifact validation; post image/role/network/genesis/readiness and
  operation-specific facts are checked; KES/preload/upgrade rollback is verified from live state.
  No real restart, KES install, image load or upgrade ran during acceptance.
- ERTC-7 | stack: regression | command: `cargo test -p ouro`; `make python-test`; `bash
  ci/l2-integration.sh`; bundle manifest verify; Clippy `-D warnings`; `git diff --check` | result:
  pass | note: 183 Rust tests, every Python test, L2 integration, manifest parity, lint and whitespace
  gates pass. S0019 debug-only fixture regressions were retained while release ordinary-flow writes
  cannot fall back without dispatch; operator-owned `pool-spec.yaml` remains untracked and untouched.
- ERTC-6 | stack: product contract | command: Skill/web static gates + source contract scan | result:
  pass | note: non-deploy website prompts and Skills contain no ordinary-flow managed/adopt/onboard /
  remote-version recovery; generated specs use `cardano`; failure routing stops on control access,
  host-key, signed-policy or live-state evidence instead of installing or reshaping a target.
- ERTC-4 | stack: integration | command: `python3 tests/test_s0020_product_flow.py`; S0019 inbox
  regression | result: pass | note: local artifact preview returns the same typed content reference
  without creating inbox state; diagnostic dispatch uses the spec's cardano account, pinned known
  hosts, bounded/audited output, no target Ouro path and honest non-read-only assurance; a legacy
  principal refuses before the SSH boundary.
- ERTC-3 | stack: integration | command: `python3 tests/test_probe.py` | result: pass | note: live
  observation derives the semantic Shelley genesis identity via cardano-cli rather than byte-level
  JSON SHA-256, matching the network identities generated by the website.
- ERTC-7 | stack: regression | command: `cargo test -p ouro`; `make python-test`; `bash
  ci/l2-integration.sh`; `cargo clippy -p ouro --lib --tests -- -D warnings`; manifest verify; `git
  diff --check` | result: pass | note: 183 Rust tests, all Python tests including S0020 product
  seams, L2 integration, zero-warning Clippy, embedded bundle parity and whitespace gates pass.
- ERTC-5 | stack: release | command: `cargo zigbuild --release --target
  x86_64-unknown-linux-musl -p ouro`; embedded arm64 macOS release build; digest comparison | result:
  pass | note: final 5,723,672-byte static runner digest is `674de3e0…8b50`; final control and installed
  `~/.local/bin/ouro-ops` are byte-identical at `8bba9411…70af`. Transport preview selects that runner
  internally and discloses no public runner path parameter.
- ERTC-2 | stack: fresh-agent bed | command: website-equivalent observability prompt to a context-free
  subagent | result: pass | note: the agent read the installed Skill and returned BP block 13685018 /
  slot 192626433 and relay block 13685022 / slot 192626493, Conway, sync 100.00, socket answering and
  signed Blink Labs runtime support. It made only the supported tip claims and never requested
  install/onboard/adopt/version-sync or a Linux binary.
- ERTC-3/4/6 | stack: fresh-agent bed | command: context-free non-deploy Skill acceptance prompt |
  result: pass | note: real relay restart plan, local no-stage opcert preview, real BP KES plan and
  relay preload plan returned changed=false live candidates; upgrade step refused with exit 10
  because exact signed N→N+1 transition metadata is absent. BP `uname -s` and relay `id -un` returned
  Linux/cardano through honest operator-SSH diagnostics. Config stopped unsupported; detect routed to
  live read/plan. No confirmation, permit, artifact transfer, apply or target installation occurred.
- ERTC-1/4 | stack: bed | command: final BP/relay observability + diagnostic `find /tmp -maxdepth 1
  -type d -name 'ouro-run.*' -print` | result: pass | note: final embedded runner returned live tips
  on both targets; cleanup output was empty on both. Container ids remained BP `d50c302c…f97d` and
  relay `cfabdd36…9bfc`, proving no accepted flow restarted or recreated either node.
- ERTC-7 | stack: final regression | command: Rust 183 tests; all Python tests; L2 integration;
  Clippy `-D warnings`; manifest verify; `git diff --check` | result: pass | note: all final source,
  embedded Skills, release manifest and product-contract gates are green; operator-owned untracked
  `pool-spec.yaml` remains untouched.

## 7. Change Requests (append-only)
- 2026-07-16T14:41+0800 operator replaced S0019's persistent target CLI/attestation model with the
  agentless ephemeral runner model and explicitly deferred hostile-terminal credential hardening.
- 2026-07-16T20:12+0800 operator accepted the post-iteration review repairs under the existing
  S0020 boundary. The repair must preserve the agentless model: crash recovery may retain only
  operation-scoped Cardano/Docker application state, never a resident Ouro gate, CLI, daemon or
  transaction database. Fleet collection parallelism remains deferred until measured latency proves
  the current fail-closed retry behavior unusable.

## 8. Review Repair Plan (append-only)
- [ ] p4-1 make KES and upgrade writes recoverable after runner/process interruption by retaining
  minimal application-state rollback material until live postconditions pass.
- [ ] p4-2 carry verified fleet-permit freshness evidence into the closed target action and re-check
  expiry plus relay quorum immediately before the first disruptive mutation.
- [ ] p4-3 bind stateless observability machine identity to pool-spec host, port, cardano principal and
  named credential instead of accepting independently composable selectors.
- [ ] p4-4 make stateless apply and diagnostic audit lifecycles terminally truthful, and reject
  unknown/duplicate diagnostic control flags before SSH.
- [ ] p4-5 remove legacy deploy from the ordinary S0020 website flow and describe upgrade artifact
  evidence at the phase where the mechanism actually validates it.

### Repair Acceptance Criteria
- ERTC-8 crash recovery: KES never stores its only previous opcert beneath the ephemeral cleanup
  directory; upgrade never deletes the prior container before the replacement is live-verified;
  fixed rollback/finalize argv and interruption-residue behavior have regression coverage.
- ERTC-9 commit-time fleet safety: an expired permit or insufficient immediate relay quorum refuses
  inside the target after final live revalidation and before any Docker restart/recreate argv.
- ERTC-10 identity and diagnostics: observability derives its complete SSH target from pool spec;
  diagnostic unknown/duplicate flags refuse before SSH; every begun diagnostic/apply audit reaches an
  accurate success, failure, rollback or ambiguous terminal record.
- ERTC-11 product truth: the ordinary website cannot generate a legacy deploy prompt and the upgrade
  Skill does not claim archive/config validation before apply preflight performs it.
- ERTC-12 repair quality: targeted Rust/Python regressions, the full Rust/Python suites, Clippy,
  manifest verification and `git diff --check` pass without reading or committing `pool-spec.yaml`.

## 9. Repair Execution Log (append-only)
- 2026-07-16T20:12+0800 [~] p4-1 started. Replace ephemeral-only KES backup and destructive
  remove-first upgrade recreate with fixed application-state recovery plans that retain the prior
  public certificate/container until the new state is live-verified.
- 2026-07-16T21:38+0800 [x] p4-1 completed. KES now refuses an existing recovery residue, copies
  the prior public opcert to `node.cert.ouro-prev` beside the live Cardano file, restores it on an
  in-invocation failure and removes it only after verified success/rollback. Stateless upgrade now
  renames the running container to `<name>.ouro-prev`, stops it, creates the replacement under the
  original name, and deletes the prior container only after image/layout/readiness verification;
  rollback removes any partial replacement and renames/starts the preserved container. Neither path
  stores its only rollback material beneath `/tmp/ouro-run.*` or adds Ouro target state.

## 10. Repair Validation Evidence (append-only)
- ERTC-8 | stack: rust | command: `cargo test -p ouro executor::tests -- --nocapture` | result: pass |
  note: 23 executor cases prove KES recovery storage is application-resident, upgrade commit contains
  no remove, rollback restores the preserved name, finalize alone removes it, and approval remains
  environment-redacted.
- ERTC-8 | stack: python | command: `python3 tests/test_s0020_stateless_plan.py`; `python3
  tests/test_s0020_stateless_apply.py` | result: pass | note: target plans expose the fixed KES
  guard/backup/install/restart/finalize sequence without an ephemeral rollback path; stateless apply
  drift, artifact and control approval seams remain green.
- ERTC-12 | stack: rust | command: `cargo clippy -p ouro --lib --tests -- -D warnings`; `git diff
  --check` | result: pass | note: p4-1 compiles without warnings or whitespace errors; the accidental
  repository-wide formatting diff was removed before delivery and `pool-spec.yaml` stayed untouched.
- 2026-07-16T21:44+0800 [~] p4-2 started. Carry only the exact control-authenticated fleet permit
  into an internal closed target flag, strip it from candidate planning, and re-check all bound
  target/pool/candidate fields, the 30-second facts window, permit expiry and immediate public relay
  endpoint quorum after final target revalidation and before any disruptive executor argv.
- 2026-07-16T21:44+0800 [x] p4-2 completed. Control execution now rebuilds the sealed SSH argv only
  after permit verification and adds the authenticated permit as target-internal evidence. Target
  apply refuses missing, mismatched, stale or expired evidence, re-probes the permit's spec-derived
  relay endpoints and re-checks expiry after that probe. Non-disruptive applies reject the internal
  evidence and unchanged plan candidates never include it.
- ERTC-9 | stack: python | command: `python3 tests/test_s0020_stateless_apply.py` | result: pass |
  note: disruptive target apply refuses missing and expired fleet evidence before fake Docker; a
  fresh candidate-bound permit reaches a live test relay endpoint and executes exactly one restart.
- ERTC-9/12 | stack: rust | command: `cargo test -p ouro`; `cargo clippy -p ouro --lib --tests -- -D
  warnings`; `git diff --check` | result: pass | note: 185 Rust tests pass; target permit parsing,
  binding and commit-time quorum checks compile without warnings or whitespace errors.

## 11. p4-3 Execution and Evidence (append-only)
- 2026-07-16T21:47+0800 [~] p4-3 started. Require the operator pool spec for dispatched health
  reads and reject any caller-composed host, port, principal or credential that does not match the
  declared machine id before constructing SSH argv.
- 2026-07-16T21:47+0800 [x] p4-3 completed. Stateless observability now resolves the machine from
  `--spec`, enforces the exact dispatch host, `cardano` user and optional credential selector, uses
  the spec port and resolves only the spec credential. Current Skills, website prompts and README
  include the same spec-bound command. Local/debug fixture reads and the closed target probe remain
  spec-independent internal seams.
- ERTC-10 | stack: python | command: `python3 tests/test_s0020_observability.py`; `python3
  tests/test_s0019_dispatch.py` | result: pass | note: non-default spec port 2222 reaches the sealed
  transport; swapped host/key selectors refuse before SSH; live typed output and legacy negative
  dispatch behavior remain green.
- ERTC-10/12 | stack: rust/python | command: `cargo test -p ouro`; `cargo clippy -p ouro --lib
  --tests -- -D warnings`; `python3 tests/test_skill_docs.py`; `python3 tests/test_web_generator.py`;
  `git diff --check` | result: pass | note: 185 Rust tests, embedded manifest parity, Skills and
  website command gates pass with the spec-bound observability contract.

## 12. p4-4 Execution and Evidence (append-only)
- 2026-07-16T21:57+0800 [~] p4-4 started. Give every dispatched stateless apply an explicit attempt
  and outcome record, distinguish verified success, typed failure, verified rollback and ambiguous
  transport/untyped outcomes, and parse diagnostic control flags as a closed pre-SSH grammar.
- 2026-07-16T21:57+0800 [x] p4-4 completed. Control records `apply_attempt` only after approval is
  consumed and immediately before dispatch, then records exactly one classified terminal apply
  outcome before forwarding the target result. Already-audited failures no longer acquire a false
  outer refusal. Diagnostics reject unknown, duplicate, missing and out-of-range control flags before
  SSH and close every begun invocation with `finish` or `crash`, including SSH exit 255.
- ERTC-10 | stack: python | command: `python3 tests/test_s0020_stateless_apply.py`; `python3
  tests/test_s0020_product_flow.py`; `python3 tests/test_s0019_inbox_audit.py` | result: pass | note:
  a real success produces attempt plus verified-success; diagnostic success produces start plus
  finish; transport exit 255 produces start plus crash; misspelled and duplicate flags never reach
  the fake SSH boundary.
- ERTC-10/12 | stack: rust | command: `cargo test -p ouro`; `cargo clippy -p ouro --lib --tests --
  -D warnings`; `git diff --check` | result: pass | note: 186 Rust tests pass, including rollback
  versus ambiguity classification; the updated audit schema matches the embedded bundle manifest
  and the repair compiles without warnings or whitespace errors.

## 13. p4-5 Execution and Final Repair Evidence (append-only)
- 2026-07-16T22:04+0800 [~] p4-5 started. Remove the legacy deploy selector and generator branch
  from the ordinary website while retaining its explicitly out-of-scope Skill/CLI compatibility
  surface, and align upgrade guidance with the phase that actually binds archive bytes to config.
- 2026-07-16T22:04+0800 [x] p4-5 completed. The website has no deploy card, operation state,
  prompt branch or translation entry and defaults to a supported operation. Upgrade guidance shows
  archive reference, declared digest and policy result separately, explicitly says their binding is
  pending, and assigns the binding to apply preflight before image-store mutation.
- ERTC-11 | stack: web/skill | command: `python3 tests/test_web_generator.py`; `python3
  tests/test_skill_docs.py`; inline JavaScript parse check | result: pass | note: the ordinary page
  contains no selectable/generatable deploy surface; the updated Skill cannot claim the removed
  premature evidence phrase and retains the exact preload/apply flow.
- ERTC-12 | stack: final regression | command: `cargo test -p ouro`; `make python-test`; `bash
  ci/l2-integration.sh`; `cargo clippy -p ouro --lib --tests -- -D warnings`; `ouro-ops manifest
  verify --against packaging/bundle-manifest.json`; `git diff --check` | result: pass | note: 186
  Rust tests, the full Python suite, L2 integration, zero-warning Clippy, embedded bundle parity and
  whitespace gates pass. Operator-owned untracked `pool-spec.yaml` remains untouched.
