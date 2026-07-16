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

## 7. Change Requests (append-only)
- 2026-07-16T14:41+0800 operator replaced S0019's persistent target CLI/attestation model with the
  agentless ephemeral runner model and explicitly deferred hostile-terminal credential hardening.
