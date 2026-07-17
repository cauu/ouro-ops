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

## 14. Fresh-Agent Troubleshooting Acceptance (append-only)
- [x] p4-6 give a context-free agent the current embedded troubleshooting Skill, website-shaped
  temporary pool spec and existing named credentials, require diagnostic-only iteration on the real
  BP/relay, and accept only a command-supported operational conclusion rather than SSH reachability.
- ERTC-13 troubleshooting conclusion: a fresh agent must use only audited `diag exec`, distinguish
  remote command findings from transport success, tie material facts to commands, state evidence
  limits, and either identify a supported typed repair or explicitly conclude that no repair is
  justified. No remote mutation, secret access or repository-owned pool spec access is allowed.
- 2026-07-16T22:22+0800 [~] p4-6 started. Spawn a no-context subagent outside the repository with
  the current CLI path, BP/relay identities and diagnostic goal. Require it to load the embedded
  Skill and independently choose one observational command at a time.
- 2026-07-16T22:22+0800 [x] p4-6 completed. After receiving the same complete declarative spec shape
  that the website prompt supplies, the fresh agent validated both existing named credentials and
  reached a bounded conclusion: both nodes are operating normally on available evidence and no
  typed repair is justified. Its initial attempt from field facts alone was intentionally discarded;
  troubleshooting requires the operator-supplied full spec rather than agent-invented schema.
- ERTC-13 | stack: fresh-agent bed | command: current `ouro-ops skill show troubleshooting`, then 21
  real `ouro-ops diag exec --dispatch <machine-id> --spec <temporary-spec> -- <diagnostic-command>`
  invocations | result: pass | note: BP `cardano-node` existed under containerd and advanced from
  block 13685932 to 13685935; relay container `10.5.4-1` was running with restart count zero and
  advanced from 13685933 to 13685936; relay port 3001 was listening and reachable from BP. Both had
  substantial free disk/memory. Relay disk use at 75% was labeled monitoring information, not an
  incident. BP Docker metadata remained an explicit permission-limited evidence gap.
- ERTC-10/13 | stack: audit | command: `ouro-ops audit log --limit 100` | result: pass | note: all 21
  fresh-agent diagnostic invocations have paired machine-bound `start` and `finish` records. Remote
  exit 1 for BP Docker permission and exit 3 for inactive host systemd were preserved as delivered
  diagnostic DATA, while later process/container evidence prevented false failure conclusions.
- ERTC-13 | stack: other | command: inspect subagent command ledger and remove its unique `/tmp`
  workspace | result: pass | note: no `op run`, install, restart, signal, permission change, secret
  path or repository `pool-spec.yaml` access occurred; the temporary spec/workspace was removed.

## 15. Role-aware Troubleshooting Baseline (append-only)
- 2026-07-17T09:50+0800 change request: the p4-6 conclusion treated an advancing, synced BP as
  operating normally without checking its KES/opcert. Live follow-up evidence showed that the BP's
  opcert had expired, so sync health alone was insufficient to support a block-production-health
  conclusion.
- [~] p4-7 add a fixed, stateless `troubleshooting/snapshot` read through the ephemeral runner.
  Bind the target and role to the pool spec, expose role-aware liveness/sync/network and KES/opcert
  facts in normalized typed output, and require free-form diagnostics to start from that baseline.
- ERTC-14 role correctness: a BP snapshot never reports block-production readiness or overall BP
  health without available, valid KES/opcert and forging-credential evidence; an expired or
  unavailable KES state is explicit and blocks a healthy BP conclusion. Relay snapshots mark KES as
  not applicable and use peer evidence instead.
- ERTC-15 operational safety: the snapshot is a read-only, no-resident-install operation with the
  same pool-spec-bound host, principal, credential, pinned-host-key, bounded-output and audit
  properties as stateless observability. It does not expose secret key contents or mutate the
  target.
- ERTC-16 product guidance: the troubleshooting Skill runs the typed snapshot before ad hoc
  `diag exec`, distinguishes facts from inference, uses targeted diagnostics only for remaining
  evidence gaps, and forbids a `BP healthy` conclusion when KES/opcert evidence is absent or invalid.
- 2026-07-17T10:10+0800 [x] p4-7 completed. `troubleshooting/snapshot` is a registered stateless
  read whose target role is derived from the pool spec and sealed into the ephemeral runner argv.
  Its output separates bounded `role_readiness` from overall health, exposes liveness/tip/peers and
  BP forging/KES facts, and reports explicit remaining evidence gaps. The Skill requires this
  baseline first and reserves free-form diagnostics for symptom-relevant gaps.
- ERTC-14 | stack: python+probe | command: `python3 tests/test_probe.py`; `python3
  tests/test_s0020_troubleshooting_snapshot.py` | result: pass | note: valid, expired and unavailable
  BP KES states and relay KES-not-applicable behavior are covered. When an expired opcert causes
  `cardano-cli kes-period-info` to omit JSON, the probe derives the current period from tip slot and
  public `slotsPerKESPeriod`, matches semantic Prometheus start/expiry aliases, ignores the observed
  unreliable zero `currentKESPeriod`, and leaves unavailable certificate counters explicit.
- ERTC-14/15 | stack: real host | command: freshly cross-built Linux runner; `ouro-ops op run --op
  troubleshooting/snapshot` against bp1 and relay1 with a temporary declarative spec | result: pass |
  note: bp1 was synced at block 13688085 / slot 192687906 but returned `role_readiness:not_ready`,
  `opcert_expired`, current/start/end KES 1486/1342/1404, remaining -82 and
  `block_production_ready:false`; relay1 returned `role_readiness:ready`, 77 established peers and
  KES `not_applicable`. Both outputs set `overall_health_claimed:false`; no remote mutation or
  persistent target install occurred and the temporary spec was removed.
- ERTC-15/16 | stack: regression | command: `cargo test -p ouro`; `make python-test`; `bash
  ci/l2-integration.sh`; `cargo clippy -p ouro --lib --tests -- -D warnings`; `ouro-ops manifest
  verify --against packaging/bundle-manifest.json`; `git diff --check` | result: pass | note: 186
  Rust tests, the full Python suite, L2 integration, zero-warning Clippy, manifest parity and
  whitespace gates pass. The operator-owned untracked `pool-spec.yaml` remained untouched.

## 16. Website Troubleshooting Prompt Parity (append-only)
- 2026-07-17T10:55+0800 change request: the website still generated the pre-p4-7 diagnostic-only
  troubleshooting prompt. It delegated directly to `diag exec`, omitted the typed role-aware
  snapshot and BP KES/opcert conclusion gate, and then contradicted diagnostics with a shared
  `op run`-only/no-raw-command instruction.
- [~] p4-8 align the generated troubleshooting prompt, product documentation and static contract
  tests with the implemented snapshot-first workflow. Generate one spec-host-bound snapshot command
  per declared machine, preserve machine-id-bound `diag exec` only for remaining evidence gaps, and
  make the postamble accurately distinguish the typed baseline from diagnostic command arguments.
- ERTC-17 prompt parity: a copied troubleshooting prompt requires every-machine
  `troubleshooting/snapshot` before any ad hoc diagnostic, states that `role_readiness: ready` is not
  an overall-health claim, and forbids a BP-health conclusion unless KES/opcert evidence is available,
  valid and `block_production_ready` is true.
- ERTC-18 selector and command truth: generated snapshot commands use each spec SSH host for
  `--dispatch` while diagnostic commands use the machine id; the troubleshooting branch does not
  claim that all inputs go through `op run` or that diagnostic commands are forbidden.
- ERTC-19 product gate: website static tests fail if snapshot-first guidance, BP conclusion safety,
  selector semantics or the troubleshooting-specific postamble regress. Current operations docs and
  README list the typed snapshot before `diag exec`.
- ERTC-17 clarification: the prompt includes a concrete snapshot command for every declared machine
  so no selector must be invented, but executes only the operator's exact symptom target unless the
  operator explicitly asks for a fleet-wide investigation.
- 2026-07-17T11:03+0800 [x] p4-8 completed. The website troubleshooting path now generates the
  target's typed role-aware snapshot before bounded `diag exec`, explains the different snapshot
  host and diagnostic machine-id selectors, carries the KES/opcert BP conclusion gate, and uses an
  operation-specific execution boundary instead of the contradictory shared `op run`-only text.
- ERTC-17/18 | stack: browser | command: serve `web/onboarding/index.html` locally; select
  troubleshooting; generate the default BP + two-relay prompt in the in-app browser; inspect
  `#prompt-out` and browser error logs | result: pass | note: all three concrete snapshot choices,
  selector semantics, bounded readiness, KES/opcert + `block_production_ready` gate and diagnostic
  command boundary were present; the obsolete parameters-only statement was absent and the browser
  emitted no errors.
- ERTC-19 | stack: regression | command: `make python-test`; extract the inline website script and
  run `node --check -`; `ouro-ops manifest verify --against packaging/bundle-manifest.json`; `git
  diff --check` | result: pass | note: the full Python suite, onboarding contract gates, JavaScript
  syntax, unchanged embedded bundle manifest and whitespace gate pass. The operator-owned untracked
  `pool-spec.yaml` remained untouched.

## 17. Current Website Observability Acceptance (append-only)
- 2026-07-17T11:13+0800 change request: align the website's observability prompt with the current
  embedded Skill, then give the prompt generated by the website to a fresh context-free subagent for
  real BP/relay acceptance. The existing generic stop text incorrectly treated unsupported signed
  image policy as fatal even though `observability/health` deliberately returns it as informational
  read evidence.
- [~] p4-9 align the website observability branch and its static contract tests with the current
  stateless read semantics, generate the prompt in a real browser, and require a fresh subagent to
  execute that exact prompt against the declared BP and relay without repository context.
- ERTC-20 prompt truth: the generated prompt runs the exact spec-host-bound health command for every
  declared machine, reports node/socket/tip/sync/runtime-policy evidence, labels KES, forging, peers,
  disk and overall health as unmeasured, and never interprets one sample as tip advancement.
- ERTC-21 policy and safety: an unsupported image policy is informational for observability and does
  not suppress returned live facts or trigger adoption/installation; access, credential, host-key or
  live-query failure still stops. No confirmation, target mutation, ad hoc diagnostic or persistent
  target installation occurs.
- ERTC-22 fresh-agent real-host acceptance: a context-free subagent receives the website-generated
  prompt and independently uses the installed Skill and named credentials to return bounded evidence
  from BP `84.247.139.72` and relay `31.220.95.72`. It must not read the repository-owned
  `pool-spec.yaml`, access secrets, broaden into troubleshooting or leave an ephemeral runner behind.
- 2026-07-17T11:21+0800 [x] p4-9 completed. The website now presents observability as a bounded
  fleet-wide read, requests the exact live/socket/tip/sync/runtime-policy fields, forbids advancement
  and overall-health inference from one sample, hands insufficient evidence to a separate operation,
  and treats unsupported image policy as informational instead of a fatal read prerequisite.
- ERTC-20/21 | stack: browser | command: serve the website locally; select observability; set BP
  `84.247.139.72` and relay `31.220.95.72`; generate and inspect `#prompt-out` in the in-app browser |
  result: pass | note: both exact spec-bound commands, bounded fields, single-sample warning,
  informational policy handling and no-diagnostic boundary were present; the stale generic signed-
  policy stop was absent and browser error logs were empty.
- ERTC-22 | stack: fresh-agent real host | command: give the exact browser-generated prompt to a
  no-context subagent in an isolated `/tmp` workspace; after its required summary/wait boundary,
  authorize exactly the two generated `observability/health` reads | result: pass | note: both exited
  0. BP returned block 13688310 / slot 192692039 and relay block 13688311 / slot 192692074; both were
  Conway, sync 100.00, node running, socket answering and signed Blink Labs policy supported. The
  agent claimed only `tip query healthy`, labeled KES/forging/peers/disk/overall health unmeasured,
  ran no diagnostics or writes, accessed no credential contents or repository file, and removed its
  temporary spec/workspace.
- ERTC-21/22 | stack: real-host cleanup | command: separate audited diagnostic `find /tmp -maxdepth
  1 -type d -name 'ouro-run.*' -print` on bp1 and relay1 using an isolated temporary spec | result:
  pass | note: both commands exited 0 with empty stdout; no ephemeral runner directory remained and
  the temporary control spec was removed.
- ERTC-20/21 | stack: regression | command: `make python-test`; extract the inline website script
  and run `node --check -`; `ouro-ops manifest verify --against packaging/bundle-manifest.json`;
  `git diff --check` | result: pass | note: full Python/static website gates, JavaScript syntax,
  unchanged embedded manifest and whitespace checks pass. The operator-owned untracked repository
  `pool-spec.yaml` remained untouched.

## 18. Current Website Upgrade Safe-Stop Acceptance (append-only)
- 2026-07-17T11:37+0800 change request: apply the website-generated fresh-agent acceptance standard
  to Upgrade without loading an image, rebuilding a container or performing an upgrade. The current
  signed allowlist has no N→N+1 transition, so a real `upgrade/step --plan` must safely refuse before
  any recreate. The installed control binary also predates p4-5's honest archive-binding wording;
  acceptance must use a current repository-built control + embedded runner without installing it.
- [~] p4-10 align the website upgrade prompt with the current digest/transition and deferred
  archive-binding contract, generate the real BP+relay prompt in a browser, and give it to a fresh
  context-free subagent. Permit only local archive preview, target `--plan` and typed refusal; forbid
  confirmation, fleet permit, artifact transfer, image load, restart, recreate and apply.
- ERTC-23 prompt truth: the selected node-version label is explicitly descriptive and cannot select
  or authorize an image. The exact target image config digest, signed policy and signed N→N+1
  transition are authoritative; tags, latest release and allowlist membership alone are insufficient.
- ERTC-24 plan truth and safety: archive preview reports only local artifact identity/size. A preload
  plan shows archive reference, operator-declared config digest, policy result and candidate
  separately and states that archive↔config binding remains pending until approved apply preflight.
  Plan/refusal carries no capability and performs no artifact transfer, image load or container
  restart/recreate.
- ERTC-25 fresh-agent real-host acceptance: a no-context subagent receives the exact browser prompt,
  reads the current repository-built embedded Skill, previews an operator-provided local acceptance
  archive, obtains a real relay preload plan if policy/live state permit, and receives a typed
  no-transition refusal for `upgrade/step --plan`. It stops without confirmation, permit or apply,
  accesses no credential contents/repository pool spec, and leaves both target containers unchanged.
- 2026-07-17T11:48+0800 [x] p4-10 completed. The website now labels its upstream node release as
  descriptive only, identifies config digest + signed policy + exact signed transition as upgrade
  authority, makes both preload and step explicitly capability-free `--plan` phases, and states
  that archive↔config binding is pending until an approved apply preflight. No apply was authorized.
- ERTC-23/24 | stack: browser | command: serve the website locally; select upgrade; generate the
  latest prompt with BP `84.247.139.72`, relay `31.220.95.72` and upstream label 11.0.1; inspect
  `#prompt-out` and browser error logs | result: pass | note: descriptive-label, digest/transition
  authority, preload/step plan-only, deferred binding, typed-stop and no-write boundaries were all
  present; browser errors were empty.
- ERTC-24/25 | stack: release+fresh-agent real host | command: embed the current static Linux runner
  into a current release control; give the exact browser prompt to a no-context subagent in isolated
  `/tmp`; preview a Docker-save archive exported from an already-local test image; run relay1
  `upgrade/preload-image --plan` twice | result: pass | note: preview returned staged=false and
  artifact `image-a0115d10@sha256:a0115d10…27e2d` (170246144 bytes). Both live target plans exited 0
  with candidate `b29a8ddb…e178`, live hash `7328c303…9504` and container epoch 1773457345 unchanged;
  changed=false, persistent_target_state_written=false and apply_revalidation_required=true. The
  docker-load argv remained unexecuted plan DATA, archive↔config binding remained pending and no
  archive bytes crossed SSH.
- ERTC-23/25 | stack: fresh-agent real-host refusal | command: acceptance-only relay1
  `upgrade/step --plan` from running config `sha256:a3223d93…c0c7a` to allowed config
  `sha256:0bb21e45…f468f` | result: expected refusal | note: exit 10 stated that no signed N→N+1
  transition exists and allowlisting images alone is insufficient. Refusal occurred before executor,
  confirmation, permit or mutation. No image load, restart/recreate, apply, BP contact, diagnostic,
  repository file or credential-content access occurred; temporary archive/spec/workspace were
  removed.
- ERTC-23/24 | stack: regression | command: `make python-test`; inline website `node --check -`;
  current release `manifest verify --against packaging/bundle-manifest.json`; `git diff --check` |
  result: pass | note: full Python/website gates, JavaScript syntax, embedded manifest and whitespace
  checks pass. The operator-owned untracked repository `pool-spec.yaml` remained untouched.

## 19. Current Website Runtime Restart Apply Acceptance (append-only)
- 2026-07-17T11:58+0800 change request: apply the website-generated fresh-agent acceptance standard
  to Runtime and authorize one real restart of relay `relay1` (`31.220.95.72`). The BP must not be
  restarted or mutated. Acceptance must preserve the exact plan/approval boundary, use the current
  repository-built control and ephemeral runner, and independently establish that the relay returned
  ready after the disruptive operation.
- [~] p4-11 align the website Runtime prompt and typed apply output with the current Skill contract,
  generate the real prompt in a browser, and give it to a fresh context-free subagent. Review the
  final live candidate before consuming the operator's authorization; mint confirmation only for
  that candidate, mint its short-lived fleet permit last, execute it once, and collect terminal audit
  plus bounded post-restart evidence.
- ERTC-26 prompt and plan truth: the operator selects exactly one machine. The capability-free plan
  reports its final candidate, target, current container identity, fixed restart executor,
  availability impact and fleet policy, then waits without confirmation, permit or mutation.
- ERTC-27 exact apply safety: only the approved relay candidate can receive a one-time confirmation;
  the 30-second live fleet permit is minted last and no replan occurs afterward. Apply revalidates
  live state, invokes only the fixed restart argv, verifies the live postcondition, returns that
  evidence, and records an unambiguous terminal control audit. Drift, expired policy, verification
  failure or ambiguous transport must stop without retrying a raw restart.
- ERTC-28 real-host recovery: a no-context subagent receives the exact browser-generated prompt and
  restarts `relay1` once. The returned result is `changed: true`, identifies the approved candidate
  and verified container/readiness state, and a later bounded read confirms the relay's query path is
  answering. No persistent runner remains, BP is not restarted or mutated, and no credential content
  or repository-owned `pool-spec.yaml` is accessed.
- 2026-07-17T12:42+0800 p4-11 acceptance correction: ERTC-27's originally stated 30-second permit
  window is superseded by 180 seconds. Real target preflight safely refused because runner transport
  plus two live probes consumed the entire 30-second window before Docker mutation. The longer
  short-lived permit covers transport/revalidation only: the candidate and target are re-probed,
  and other relay endpoints are still checked immediately before the fixed executor mutation.
- 2026-07-17T12:28+0800 metadata correction: the immediately preceding append was recorded at
  12:28+0800; its 12:42 timestamp was a transcription error. Its technical content is unchanged.
- 2026-07-17T12:32+0800 p4-11 real apply discovery: the first permitted Docker restart executed
  exactly once, but the target sampled readiness immediately after `docker restart` and returned a
  typed socket-not-answering failure while cardano-node was still starting. Reconciliation proved
  the new process start time and preserved container/image identity. Runtime must poll the same
  closed postcondition for at most 300 seconds, tolerate only transient readiness failures, and
  continue to fail identity/policy/layout drift immediately. No second real restart is authorized
  by this correction.
- 2026-07-17T12:46+0800 p4-11 acceptance progress. ERTC-26 passed in the browser and a post-fix
  context-free agent: the website now emits explicit non-YAML BEGIN/END markers, a single-relay
  `min_online_relays: 0` policy, 180-second permit/last-mint wording, immediate other-relay probe and
  300-second readiness behavior. Skill v4 produced candidate `c1ca4f08…b8539` for only relay1 and
  the fixed `docker restart cfabdd36…9bfc` executor, then stopped without capabilities or apply.
- ERTC-27 | stack: mechanism + fake executor | result: pass for current source | note: fleet status
  preserves a KES-unready non-target BP as typed offline evidence instead of blocking relay quorum;
  permits allow 180 seconds for ephemeral transport while retaining the 30-second fleet-collection
  bound and immediate endpoint check; runtime retries transient readiness for at most 300 seconds;
  success returns structured live postcondition; failure after the restart reports `changed: true`,
  `mutation_executed: true` and a no-retry reconciliation instruction. The deterministic test
  observes one unready sample then success and separately proves honest post-mutation failure.
- ERTC-28 | stack: real relay restart + reconciliation | result: partial | note: one real restart was
  executed. Control audit records candidate `c1ca4f08…b8539` `apply_attempt` then typed failure;
  process PID changed to 1680147 with start time 2026-07-17 06:30:48 target-local, while container
  id and Blink Labs image digest stayed unchanged. At roughly four minutes, the independent health
  read returned socket answering, Conway block 13688532 / slot 192696607 / sync 100.00, and no
  ephemeral runner residue remained. This run used the pre-poll target behavior and therefore did
  not return the required post-fix `changed: true` success/live-postcondition record. A second real
  restart requires new explicit operator authorization before ERTC-28 and p4-11 can be completed.
- ERTC-27 | stack: regression | command: `make python-test`; inline website `node --check`; current
  release manifest verify; `cargo test -p ouro` (186 tests); `git diff --check` | result: pass | note:
  the bundle manifest was regenerated for Runtime Skill v4 and the operator-owned untracked
  repository `pool-spec.yaml` remained untouched.
- 2026-07-17T13:05+0800 p4-11 final real-host rerun authorized. The operator explicitly approved a
  second relay1 restart for a full fresh-subagent simulation using the corrected website contract.
  The new agent must independently read Skill v4, use an isolated prompt-owned spec, obtain and show
  a fresh capability-free candidate, and stop for exact-hash approval before minting a new
  confirmation and 180-second permit. No previous token, permit or candidate authorization may be
  reused; BP remains read-only fleet evidence and must not be restarted or mutated.
- 2026-07-17T13:54+0800 p4-11 pre-mutation discoveries. The operator approved exact candidate
  `c1ca4f08…b8539`, but no second restart has executed yet. Three independently safe refusals exposed
  real contract defects: signed-permit verification still used 30 seconds after control issuance
  moved to 180; fleet collection still required the two serial real-host probes to finish within 30
  seconds; and Docker returned the same typed mount set in nondeterministic array order, which the
  candidate incorrectly treated as state drift. Every refusal returned `changed:false`; independent
  reads preserved container `cfabdd36…9bfc`, an answering socket and an advancing Conway/mainnet tip.
- ERTC-27 | stack: mechanism + regression | result: pass for corrected source | note: one shared
  `LIVE_FACTS_VALIDITY_SECONDS=180` now governs permit issuance, control collection freshness and
  target verification; 31-second facts pass while 181-second/future facts fail. Runtime candidates
  exclude the upgrade-only recreate spec and canonicalize the unique typed mount set by destination
  before hashing. Reversing the Docker mount array and changing only recreate environment data leave
  a restart candidate unchanged, while changing the container identity still changes it. Drift
  refusal now identifies the changed non-sensitive live component without exposing values.
- ERTC-27/28 | stack: fresh-agent read-only real host | command: capture alternating relay1 plans,
  compare non-sensitive binding component hashes, then repeat three plans with typed-mount
  canonicalization | result: pass for determinism, apply pending | note: the only varying component
  was raw mount order (`[/data/db,/ipc,/opt/cardano/config/keys,/opt/cardano/config]` versus
  `[/opt/cardano/config,/data/db,/ipc,/opt/cardano/config/keys]`); the canonical mount hash was
  identical. After the fix, all three plans returned candidate `42af6d6e…8af909`, live hash
  `b3076873…279c7`, the same container/epoch and the same fixed restart executor. No confirmation,
  permit or mutation was produced for the new hash; exact operator approval is required before the
  authorized second restart can continue.
- ERTC-27 | stack: regression | command: `python3 tests/test_s0020_stateless_plan.py`; `python3
  tests/test_s0020_stateless_apply.py`; targeted shared-facts Rust tests; static Linux release runner
  + embedded control rebuild; manifest verify; `git diff --check` | result: pass | note: repository-
  wide `cargo fmt --all -- --check` remains red on pre-existing formatting across unrelated files,
  so no broad mechanical rewrite was made. The operator-owned untracked `pool-spec.yaml` remained
  untouched.
- 2026-07-17T13:57+0800 p4-11 exact candidate authorization: the operator explicitly approved
  `42af6d6e9a1485e1346c44907fff678db34774a6a843bdde85b7c10e3d8af909` for one real relay1
  restart. The fresh agent must reject any different current candidate, discard every prior
  confirmation/permit, mint a new exact confirmation and mint the 180-second fleet permit last.
  This authorization covers only relay1's fixed restart executor; BP remains read-only evidence.
- 2026-07-17T14:05+0800 [x] p4-11 completed. The fresh agent revalidated exact candidate
  `42af6d6e…8af909`, minted a new confirmation and last-moment permit, and executed only the fixed
  relay1 restart once. The tracked apply exited 0 with `changed:true`,
  `approved_candidate_revalidated`, no persistent target state, and a typed live postcondition for
  the same container/image/network/genesis. No raw or retry path ran and BP remained read-only.
- ERTC-26/27 | stack: fresh-agent real-host apply | command: current Runtime Skill v4 plan → exact
  operator approval → fresh confirmation → fresh `rt-p411-final2` fleet permit → immediate apply |
  result: pass | note: candidate `42af6d6e…8af909`; permit facts online relays 1, minimum 0; target
  result `ouro.op.apply`, exit 0, changed true. Postcondition preserved container
  `cfabdd36…9bfc`, creation epoch 1773457345 and image `sha256:a3223d93…c0c7a`; mainnet/genesis
  matched, socket answered, Conway block 13688778 / slot 192701841 / sync 100.00. A successful
  control dispatch is emitted only after its terminal `apply_succeeded` audit append succeeds; the
  target record's null audit_id reflects the no-permanent-target-journal design, not a missing
  control terminal event.
- ERTC-28 | stack: fresh-agent real-host reconciliation | command: independent bounded Ouro health
  reads for relay1 and bp1 plus relay1 `/tmp/ouro-run.*` residue check | result: pass | note: relay1
  advanced beyond the typed postcondition to block 13688781 / slot 192701876 with socket answering
  and sync 100.00; bp1 independently answered at block 13688770 / slot 192701599. Container IDs
  remained relay `cfabdd36…9bfc` and BP `d50c302c…f97d`. The residue check exited 0 with empty output,
  proving no ephemeral runner directory remained. These bounded reads do not claim BP forging,
  public peer reachability or disk safety.
- ERTC-26/27/28 | stack: final regression | command: `make python-test`; `cargo test -p ouro`;
  `cargo clippy -p ouro --lib --tests -- -D warnings`; current release manifest verify; `git diff
  --check` | result: pass | note: all Python/static website gates, 187 Rust tests, warning-free
  Clippy, embedded bundle integrity and whitespace checks pass after the successful real restart.
  The operator-owned untracked `pool-spec.yaml` remained untouched and is excluded from p4-11.

## 20. KES Air-gap And No-write Preflight Acceptance (append-only)
- 2026-07-17T14:22+0800 change request: validate KES as two explicit phases. Phase A must generate
  the fixed Ouro cold-sign script from public KES vkey + period, execute it in an isolated air-gap
  simulation with disposable cold key/counter, prove counter backup/advance and return only the
  public node.cert. Phase B must preview and deeply validate a protocol-valid mock node.cert against
  a matching mock node environment, produce the final BP-only install plan, and stop before any
  opcert backup/install/restart or confirmation/permit issuance.
- [x] p4-12 add a typed one-shot `kes-rotation/install-opcert` artifact preflight that reuses the
  production opcert parser, target public KES key and live protocol checks but is capability-free,
  returns changed=false and has no executor; align the Skill/website with Phase A → manual air-gap
  handoff → Phase B, then give the exact prompt to a context-free subagent for full mock acceptance.
- ERTC-29 Phase A: the generated executable script embeds only public KES vkey/period, has an
  out-of-band SHA-256, runs the fixed `cardano-cli node issue-op-cert` against disposable in-place
  cold key/counter with networking disabled, backs up and advances the counter exactly once, emits
  a valid public node.cert and returns no secret/counter material.
- ERTC-30 Phase B: preview and target preflight reopen the exact public artifact bytes, validate
  opcert shape/signature, hot KES key, counter and KES window against a matching sealed mock node,
  return a stable install candidate plus changed=false validation evidence, and stop before
  confirmation, permit, backup, copy or restart. Wrong key/counter/period and byte/path swaps refuse.
- ERTC-31 product truth: website and embedded Skill start from evidence that renewal is due, generate
  the Ouro-owned script, require human air-gap execution/digest review, accept only returned public
  node.cert, use stateless preview/preflight/plan, and no longer direct the operator to legacy
  `kes push`.
- ERTC-32 quality: fresh-subagent command ledger, sealed Phase A/B fixtures, Rust/Python/web tests,
  Clippy, manifest verification and `git diff --check` pass; no production BP opcert/container is
  changed, no capability is minted and all temporary files are removed.
- 2026-07-17T15:13+0800 p4-12 completed. The public `--artifact-preflight` path now reopens the
  exact operator-named opcert, sends only release-selected runner bytes plus that public artifact,
  rebuilds the candidate on the target, validates the cold signature, target public KES key,
  counter and live period window, then re-probes for drift. Its typed success is `changed:false`,
  `executor_available:false`, consumes no confirmation/permit and writes no persistent target state.
- ERTC-29/30 | stack: executable sealed Phase A/B fixture | command: `python3
  tests/test_s0020_kes_airgap_preflight.py` | result: pass | note: the generated fixed script's
  separately emitted SHA-256 matched its bytes; its disposable offline command double observed
  exactly one `cardano-cli node issue-op-cert`; the counter advanced 6→7 with backup 6 and returned
  only a protocol-valid public node.cert. Preview, final plan and deep preflight returned stable
  candidate `300103bd…9435c`; the captured public dispatch was exactly `runner || node.cert` with
  closed `target preflight` argv. No apply, backup/copy/install or restart ran. The fixture is
  network-free by construction but does not claim OS-level network namespace isolation; physical
  air-gap/network-off enforcement remains the human ceremony boundary described by the script.
- ERTC-30 | stack: concrete refusal scenes | result: pass | note: a different target hot KES key,
  an expired live KES period, a corrupted cold signature and an artifact byte/path swap all refused
  before any executor or SSH mutation. Candidate drift is re-probed after protocol validation.
- ERTC-31 | stack: product parity | result: pass | note: KES Skill v5, website prompt, generated
  script guidance and CLI help now describe Phase A → human air-gap handoff → public node.cert
  preview/final plan/deep preflight → separate exact approval. No current Skill or website prompt
  directs an agent to legacy `kes push`.
- ERTC-32 | stack: context-free subagent | result: pass | note: the agent received only the current
  website-style prompt plus disposable-fixture boundary, independently loaded embedded KES Skill
  v5 and executed the full mock chain. It reported the exact script/artifact digests, stable
  candidate, validation fields, closed transport argv and a bounded PASS; it minted no capability,
  contacted no production host, left no temporary tree and never opened the operator-owned root
  `pool-spec.yaml`.
- ERTC-32 | stack: final regression | command: `cargo test --workspace`; `make python-test`;
  `cargo clippy --workspace --all-targets -- -D warnings`; `ouro-ops manifest verify --against
  packaging/bundle-manifest.json`; `git diff --check` | result: pass | note: 187 Rust tests, all
  Python/security/website gates, warning-free Clippy, embedded manifest verification and whitespace
  checks pass. Repository-wide `cargo fmt --all -- --check` remains red on pre-existing formatting
  outside this item, so no unrelated mechanical rewrite was made. The operator-owned untracked
  `pool-spec.yaml` remained untouched and is excluded from p4-12.

## 21. Single-prompt Upgrade Workflow Acceptance (append-only)
- 2026-07-17 change request: accept Upgrade to the same website-prompt → fresh-agent standard while
  keeping it one user-visible workflow. Internally `upgrade/preload-image` prepares an exact image
  without disrupting the node and `upgrade/step` activates it with fleet ordering/readiness; they
  retain distinct candidates and approvals. Production hosts must not receive archive bytes, load
  an image, recreate/restart a container or mint capabilities during this acceptance.
- [x] p4-13 align the Upgrade Skill/website with one workflow + two internal transaction boundaries;
  prove production-safe preview/plan/refusal/reconciliation, and prove positive/refusal/rollback
  behavior in a signed-policy sealed bed before a context-free subagent gives a final verdict.
- ERTC-33 product truth: one website prompt and one Upgrade Skill explain prepare → activate → verify
  → next target, while exposing two separate candidate/approval gates. Canary relay runs first,
  remaining relays next and BP last; no target automatically advances after a refused/failed phase.
- ERTC-34 production-safe evidence: local archive preview is no-stage; repeated preload plans are
  stable and send no bytes; step accepts only an exact signed N→N+1 transition and safely refuses
  the current absent transition. Before/after container id, image config, readiness/tip and runner
  residue reconcile on relay/BP without confirm, permit, image load, recreate or restart.
- ERTC-35 sealed preload: a signed test policy plus Docker-save fixture prove exactly one declared
  config digest before mutation, exactly one fixed `docker load --input` on success, and exact target
  image removal after injected post-load failure. Malformed/multi-image/wrong-config/archive byte
  swap and live candidate drift refuse before image-store mutation.
- ERTC-36 sealed activation: a signed N→N+1 fixture proves deterministic redacted recreate binding,
  target-image presence, canary/quorum/BP-last gates, preservation of the prior container until new
  readiness, finalize only after verification, and verified restoration of N after injected failure
  when backward-compatible. A forward-only transition reports re-sync/forward-recovery rather than
  claiming rollback.
- ERTC-37 quality: exact website-style prompt to a fresh context-free subagent, complete command
  ledger, sealed positive/refusal/recovery evidence, Rust/Python/web tests, Clippy, manifest verify
  and `git diff --check` pass; all temporary fixtures are removed and the operator-owned root
  `pool-spec.yaml` remains unread and untouched.
- 2026-07-17T17:39+0800 p4-13 completed. Upgrade Skill v4 and the website now expose one workflow
  with two independently approved internal boundaries per target: non-disruptive image preparation,
  then signed transition activation. Preparation cannot authorize activation or the next target;
  canary relay, remaining relays and BP-last ordering remain explicit.
- ERTC-33/34 | stack: website-style prompt + context-free agent + production-safe reads | result:
  pass | note: the fresh agent independently loaded the embedded Upgrade Skill, previewed a local
  disposable archive without staging, then obtained the same relay1 preload candidate twice:
  `a243b85e…a78dc`, live hash `b3076873…279c7`, `changed:false`. No archive path or bytes were sent.
  The step plan then exited 10 with `changed:false` because production policy has no exact signed
  transition from `sha256:a3223d93…c0c7a` to `sha256:0bb21e45…f468f`; it explicitly refused to
  treat image allowlisting alone as authorization and emitted no activation candidate.
- ERTC-34 | stack: production reconciliation | result: pass | note: relay1 preserved container
  `cfabdd36…9bfc`, BP preserved `d50c302c…f97d`, both retained the running config
  `sha256:a3223d93…c0c7a`, socket answers and sync 100%. Relay advanced from block 13689379 / slot
  192714345 to block 13689390 / slot 192714549; BP advanced from block 13689377 / slot 192714284 to
  block 13689395 / slot 192714635. A bounded target read found no `/tmp/ouro-run.*` residue. No
  confirmation, permit, apply, archive transport, image load, pull/tag, recreate or restart ran.
- ERTC-35/36 | stack: signed-policy sealed executor | command: `python3
  tests/test_s0020_upgrade_workflow.py` | result: pass | note: a valid single-image archive caused
  exactly one fixed `docker load --input`; injected post-load failure removed only the exact newly
  loaded target. Wrong config, multiple images and byte/path swap refused before load. Signed
  transition and rollback metadata changed the candidate; recreate secrets remained redacted; N was
  retained until N+1 readiness. Backward-compatible failure restored and verified N, while a
  forward-only failure retained recovery evidence and reported forward recovery/re-sync without
  falsely claiming rollback.
- ERTC-33/35/36 | stack: defects corrected before acceptance | result: pass | note: the signed
  allowlist digest and full transition are now candidate-bound; v2 contract observations are
  reachable; preload re-probes and proves the running container unchanged; successful activation
  returns a typed live postcondition; rollback verification accepts the restored original container;
  forward-only transitions never auto-rollback after activation begins.
- ERTC-37 | stack: fresh-agent cleanup + final regression | result: pass | note: the disposable
  `/tmp/ouro-upgrade-accept.*` spec/archive was removed and the root operator-owned `pool-spec.yaml`
  was never opened or modified. `cargo test --workspace`, `make python-test`, `cargo clippy
  --workspace --all-targets -- -D warnings`, current static Linux runner build, embedded manifest
  verification and `git diff --check` pass. Production Ed25519 policy still has no N→N+1 transition,
  so real activation correctly remains unavailable until a separately reviewed policy release.

## 22. Signed Runtime Upgrade Policy Release (append-only)
- 2026-07-17T18:13+0800 change request: update the production allowlist after reviewing current
  Blink Labs releases. Retain live `10.5.4-1` and target `11.0.1-1`; add only final linux/amd64
  `10.6.4-1` and `10.7.1-3` OCI identities. Exclude superseded `10.6.3-1`, `10.7.1-1/-2`, mutable
  tags and prereleases. Authorize only reviewed adjacent edges, never allowlist membership alone.
- [x] p4-14 decouple immutable image-to-image runtime transitions from the stable Docker layout
  convention, publish signed allowlist v3 with the four-version support window and adjacent upgrade
  graph, and prove signature, policy selection, refusal and sealed Upgrade behavior without touching
  production hosts.
- ERTC-38 data model: a transition is an exact directed pair of allowlisted image config digests;
  it no longer fabricates a layout convention bump when entrypoint, paths and role rules remain the
  same. Self-loops, duplicate edges, unknown images and a non-forward-compatible selected edge fail
  closed.
- ERTC-39 release policy: allowlist v3 retains exact `10.5.4-1` and `11.0.1-1` OCI tuples, adds exact
  linux/amd64 `10.6.4-1` and `10.7.1-3` tuples, and contains only the directed chain
  `10.5.4-1 → 10.6.4-1 → 10.7.1-3 → 11.0.1-1`. Direct skips and reverse edges refuse.
- ERTC-40 custody and integrity: the strict release candidate is signed atomically by the existing
  macOS Keychain authority whose public key matches the pinned verifier. No private material is
  accepted through argv/stdin, printed, written to the repository or read from 1Password; tamper,
  wrong-key and anti-rollback tests pass.
- ERTC-41 Upgrade behavior: sealed plans accept each exact adjacent edge under the unchanged Blink
  layout contract; target digest and full signed transition remain candidate-bound. Unknown,
  skipped, reverse or byte-swapped policy/archive inputs refuse before mutation, while existing
  preload/activation/rollback/forward-recovery acceptance remains green.
- ERTC-42 quality: policy/source tests, sealed Upgrade test, full Rust/Python/web regression,
  warning-free Clippy, current static Linux runner/control rebuild, manifest verification and
  `git diff --check` pass. No production image is loaded and no production container is restarted or
  recreated; the operator-owned root `pool-spec.yaml` remains unread and untouched.
- 2026-07-17T23:17+0800 p4-14 completed. `data/allowlist.json` v3 was atomically signed by the
  existing Keychain Ed25519 authority; signer inspection validated canonical SHA-256
  `3c8976983a442a3a5c8b7c0da9fe3bc74fb8c845f9289ac26de1b249e9ede4a9` against the pinned release
  verifier without accessing 1Password or exposing private material.
- ERTC-38/39 | stack: Rust policy + signed release | result: pass | note: all four linux/amd64 image
  config digests resolve to the same convention v1 contract; the signed graph contains exactly
  `10.5.4-1 → 10.6.4-1 → 10.7.1-3 → 11.0.1-1`. Duplicate/self/unknown edges fail validation, while
  direct `10.5.4-1 → 11.0.1-1` and reverse selection refuse. The first edge is honestly marked
  forward-only because exact-version rollback evidence is absent; the two later edges authorize
  rollback.
- ERTC-40/41 | stack: release signer + sealed Upgrade executor | commands:
  `ouro-allowlist-signer inspect --input data/allowlist.json`; `python3
  tests/test_s0020_upgrade_workflow.py`; `python3 tests/test_s0020_stateless_plan.py`; `python3
  tests/test_s0020_stateless_apply.py` | result: pass | note: each adjacent production edge produced
  a candidate-bound plan under the stable layout contract, while skipped and reverse edges refused
  before an executor ran. Existing preload, activation, rollback and forward-recovery cases remain
  green.
- ERTC-42 | stack: full regression/build | commands: `cargo test --workspace`; `make python-test`;
  `cargo clippy --workspace --all-targets -- -D warnings`; `cargo zigbuild --release --target
  x86_64-unknown-linux-musl -p ouro`; `cargo build -p ouro`; `ouro-ops manifest verify --against
  packaging/bundle-manifest.json`; `git diff --check` | result: pass | note: 187 Rust tests, all
  Python/Skill/website gates, warning-free lint, a statically linked x86-64 runner, current embedded
  control policy and manifest integrity passed. No production host was contacted or mutated, and
  root `pool-spec.yaml` remained unread and untouched.
