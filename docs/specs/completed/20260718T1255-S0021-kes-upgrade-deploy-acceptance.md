# KES, Upgrade, And Deploy Fresh-Agent Acceptance

Spec-ID: S0021
Status: completed
Created Time: 2026-07-17T14:14:21+08:00
Start Time: 2026-07-18T12:55:27+08:00
Completion Time: 2026-07-18T15:34:12+08:00
Previous Spec-ID: S0020
Closure Reason: achieved

## 1. Requirement Details

### Background
- Runtime acceptance proved the website-prompt → context-free agent → exact live plan → explicit
  approval → one-shot typed apply → independent reconciliation chain on real hosts.
- KES Phase A/B was accepted under S0020 p4-12 with a disposable air-gap ceremony and typed
  capability-free target preflight. Upgrade already uses the S0020 ephemeral runner, but acceptance
  must not load an image or recreate a real node.
- Deploy is currently excluded from S0020, removed from the website, and still depends on the legacy
  adopted-node/persistent-inbox path. It cannot meet the Runtime standard until migrated.

### Scope
- Retain S0020 p4-12 as the KES acceptance prerequisite, then execute fresh-agent acceptance for
  `upgrade/preload-image`, `upgrade/step`, and `deploy/register-submit`.
- Use real bp1/relay1 only for capability-free plans, typed safe-stops and independent no-mutation
  reconciliation unless the operator separately authorizes an exact production mutation.
- Exercise positive apply, rollback and ambiguity paths with sealed fake executors/fixtures that use
  the production parser, candidate, confirmation, permit and audit code.
- Migrate Deploy to the same stateless one-shot artifact transport before accepting it.

### Constraints
- Fresh agents receive the website-equivalent prompt and no prior conversation context.
- No credential contents, repository `pool-spec.yaml`, cold key, KES signing key or VRF key enter
  agent context or output.
- Public KES vkey/opcert, Docker-save archive and signed transaction are operator-named regular
  files. Public artifacts are previewed without staging and streamed only by typed one-shot paths.
- KES and Upgrade production acceptance performs no real install, image load or container recreate.
- Deploy is irreversible. Acceptance may send exactly one guaranteed-invalid mock transaction to the
  real BP after candidate-specific approval, proving the terminal rejection path without ledger
  mutation. Only an explicitly authorized real transaction plus independent ledger evidence can
  prove production submit success.

### Non-goals
- Generating or rotating a KES signing key.
- Trusting image tags, `latest`, or allowlist membership without an exact signed transition.
- Claiming ledger inclusion from successful `transaction submit` alone.
- Retrying an ambiguous irreversible submission.

## 2. Outline Design

### 2.1 Shared acceptance harness
- Generate the current website prompt and give it verbatim to a fresh isolated agent.
- Require current embedded Skill loading, an isolated temporary pool spec, named credential
  references, pinned host identity, current control-selected Linux runner and empty runner residue.
- Require three identical capability-free candidates for identical semantic live state. Unordered
  Docker/API collections must be canonicalized; content or identity drift must change the hash.
- Exact confirmation is minted only after the operator approves the complete candidate. Fleet
  permits are minted last where required. All one-time material is discarded on any stop.
- Positive and negative terminal paths must produce truthful control audit events. Post-mutation
  failures distinguish changed/rolled-back/ambiguous outcomes and never invite a blind retry.

### 2.2 KES acceptance prerequisite (completed in S0020 p4-12)
- Treat the command as fixed cold-script generation plus public opcert installation, never
  signing-key rotation. Phase A generates the Ouro-owned script from only public KES vkey/current
  period; the human executes it air-gapped and returns only public node.cert.
- Phase B previews the exact returned bytes, obtains the stable BP-only final plan, then runs the
  typed `--artifact-preflight`. It validates cold signature, target hot KES key, counter and live
  window with `changed:false` and no capability/executor, stopping before opcert update.
- The sealed fresh-agent acceptance proved counter backup/advance, exact runner+artifact transport,
  stable candidate and wrong-key/period/signature/byte-swap refusal. Positive production install and
  rollback were explicitly outside the operator-approved boundary.

### 2.3 Upgrade acceptance boundary
- Treat preload and step as two separately approved operations.
- Real-host acceptance previews one local Docker-save archive, obtains a stable preload plan without
  loading it, and accepts the current typed no-transition refusal for step without recreating the
  node. Container/image/tip and target residue are reconciled afterward.
- Signed-policy fixtures prove archive↔single config digest validation before image-store mutation,
  exact N→N+1 authorization, redacted and deterministic recreate binding, canary relay/quorum order,
  BP-last enforcement, new-image readiness, old-container retention until verification, verified
  rollback, and honest re-sync when no inverse is supported.

### 2.4 Deploy migration and irreversible boundary
- Replace the adopted-node/persistent-inbox prerequisite with a pool-spec-bound stateless BP plan and
  one-shot public signed-transaction payload. Restore a website Deploy prompt only after that product
  contract is true.
- The capability-free plan exposes tx artifact reference, txid and a reviewable transaction-body
  summary: network, registration certificate/pool id, inputs, outputs/change, fee, validity interval,
  withdrawals/deposits and required witnesses. It binds those facts plus BP/network/genesis and live
  submit target into the exact candidate.
- Apply reopens the exact file, revalidates bytes/semantics and invokes only fixed
  `cardano-cli transaction submit` argv once. There is no rollback. Rejection is terminal;
  untyped/transport interruption is ambiguous and must never auto-resubmit.
- A successful submit means only node/mempool acceptance. Separate bounded ledger queries must report
  inclusion and pool registration as confirmed, pending or absent. Production Deploy acceptance
  requires an intended real transaction and a second explicit operator authorization; otherwise the
  release can pass only the no-submit flow-readiness tier.

## 3. Execution Plan
- [x] p1-1 KES Phase A/B no-write acceptance completed under S0020 p4-12; do not repeat or broaden
  it without a new operator request.
- [x] p2-1 accept Upgrade preload/step safe-stops on real hosts and positive rollout/rollback behavior
  in the signed-policy bed.
- [x] p3-1 migrate Deploy to stateless plan/apply, update Skill/website, and accept the irreversible
  mock-submit flow-readiness tier.
- [x] p3-2 leave production Deploy unexecuted in this iteration; any intended signed transaction
  requires a separate operator request, exact candidate authorization and independent ledger
  verification.

## 4. Test And Acceptance Criteria

- KATC-1 Phase A custody: a fresh agent accepts only public KES vkey/current period, generates the
  fixed digest-addressed script and returns only public node.cert from the human air-gap boundary.
- KATC-2 Phase B no-write boundary: public preview/final plan/deep preflight are candidate-stable;
  the preflight has no capability/executor and proves signature/key/counter/window compatibility.
- KATC-3 concrete refusal: artifact/path swap, wrong signature/key/counter/period and live drift
  refuse before mutation. No real BP opcert/container changes during this acceptance tier.

- UATC-1 policy truth: target config digest and exact signed N→N+1 transition, not tag/latest or
  membership alone, authorize a step.
- UATC-2 preload safety: archive preview is no-stage; apply preflight proves a single matching config
  before `docker load`; positive fixture changes only the image store and rollback removes only the
  newly loaded digest.
- UATC-3 step safety: recreate plan is stable/redacted and binds the complete reproducible run spec;
  candidate drift, absent image/transition, unsupported shape, quorum violation and BP-before-relay
  all refuse before mutation.
- UATC-4 recovery: positive fixture verifies new digest/readiness before old-container cleanup;
  injected failures restore and verify the old container or honestly require re-sync. Real hosts show
  typed safe-stop with unchanged container/image/tip continuity and no runner residue.

- DATC-1 migration truth: Deploy has no adoption, resident CLI or durable remote inbox prerequisite;
  Skill, website and code all use the stateless pool-spec-bound one-shot path.
- DATC-2 review truth: preview/plan validate and display exact public tx reference, txid and semantic
  transaction summary matching the intended pool/network before approval; malformed, mismatched,
  expired or changed bytes refuse before submit.
- DATC-3 exactly-one invocation boundary: after exact approval the sealed bed observes one fixed
  submit argv; rejection is terminal, transport ambiguity is recorded and never retried, and no
  success path claims rollback or ledger inclusion.
- DATC-4 real-host mock tier: a fresh agent reaches the complete stable final plan on the real BP,
  waits for exact approval, then makes one fixed submit attempt using a protocol-valid signed
  transaction whose referenced input was independently proven absent immediately beforehand. The
  node must reject it, Ouro must report a terminal non-ambiguous refusal without automatic retry,
  and transaction/runner residue plus node/ledger/pool state must reconcile unchanged.
- DATC-5 production tier: only when separately authorized, the exact signed tx is submitted once and
  independent ledger evidence reports tx inclusion and resulting pool registration. Without this
  evidence, production Deploy remains pending even if the node accepted submission.

- XATC-1 product parity: current website prompts, embedded Skills, CLI help and implementation agree
  on commands, artifact custody, approval boundaries, stop conditions and honest outcome language.
- XATC-2 regression: Rust/Python/security/web tests, Clippy, manifest verification and whitespace
  checks pass; temporary control/target files are removed and operator-owned files remain untouched.
- Pass/fail: KES and Upgrade pass without real production mutation only when both real safe-stop and
  sealed positive/recovery evidence pass. Deploy mechanism readiness requires real-host Phase A plus
  the approved guaranteed-invalid Phase B refusal and sealed positive/ambiguity evidence; production
  submit success remains a separate verdict and must never be conflated.

## 5. Execution Log (append-only)
- Draft only; execution has not started.
- 2026-07-17T17:39+0800 p2-1 was completed under active spec S0020 p4-13. The website-style fresh
  agent reached stable relay preload plans and the signed-transition safe stop without production
  mutation; sealed signed-policy fixtures proved preload, activation, rollback and forward-only
  recovery behavior. Deploy remains pending and is not broadened by this completion.
- 2026-07-18 p3-1 implementation migrated Deploy to the stateless runner/payload path, retired the
  legacy `tool run` entry point, restored the website prompt, added target-derived transaction
  review/input-UTxO evidence and classified exactly-once submit outcomes. The first fresh-agent live
  plan exposed a concrete defect: `live_slot` made the candidate drift under normal chain progress.
  The slot remains displayed and is rechecked by apply but was removed from the semantic candidate.
- 2026-07-18 p3-1 Phase A retry used a second context-free agent and the rebuilt Linux runner. Four
  real-BP plans across slots 192787613..192787725 produced the same candidate
  `38f6d10ae18ddb5cbd19e4c384bb107f74ed43cec2d5149e689ca59670e9869e` and independently reported
  the all-zero fixture input `absent/all_absent`. No confirmation, apply or submit occurred; both
  control and target runner residue checks were empty. Phase B waits for exact operator approval.
- 2026-07-18T15:34:12+08:00 p3-1 Phase B completed after the operator approved the exact stable
  candidate. The context-free agent minted one candidate-bound confirmation and executed one fixed
  `/dev/stdin` submit. The node returned a normal typed rejection; no retry or second apply occurred.
  Independent reconciliation found the input and pool state absent, the same running container and
  image, advancing synced tip, unchanged artifact digest and no control/target runner or tx residue.

## 6. Validation Evidence (append-only)
- None; criteria proposed for operator review.
- Upgrade evidence is recorded in S0020 ERTC-33 through ERTC-37. Production containers/images were
  unchanged, no archive bytes or capabilities were sent, and temporary runner/spec/archive residue
  was removed.
- DATC-1..3 sealed evidence: `python3 tests/test_s0021_deploy_workflow.py` passed for stable planning,
  changed bytes/network/extra-effect/expiry refusal, exact stdin bytes, one normal rejection, one
  accepted-by-node result and one signal-derived ambiguity; every terminal branch forbids retry and
  writes no persistent target state. `make python-test`, 188 Rust tests, Clippy with warnings denied,
  bundle manifest verification and `git diff --check` passed before live Phase A.
- DATC-2/4 Phase A evidence: public artifact
  `tx-09ed0665@sha256:09ed066539e809a439c93bbcc559f12cc0ea10bdfff7262ed687ac703ee7fdd2`,
  txid `9d8a58be66d7c190534c0c407b03c041f60d9beff19744071e231c3a7dd211a0`, BP container
  `d50c302cd08774707784023ceaa846880ede3f7d7c014aef436e42c71abbf97d`, one pool-registration
  certificate, two witnesses, no extra effects, and exact input UTxO state `absent`. Tip advanced
  while the fixed executor and candidate remained stable. The repository-root operator spec and
  credential contents were not read.
- DATC-3/4 Phase B evidence: approved candidate
  `38f6d10ae18ddb5cbd19e4c384bb107f74ed43cec2d5149e689ca59670e9869e`;
  apply return code 10, `error.code=submission_rejected`, `changed=false`,
  `submission_attempted=true`, `accepted_by_node=false`, `outcome=node_rejected`,
  `retry_allowed=false`, `persistent_target_state_written=false`. The rejection named
  `BadInputsUTxO` for the all-zero input, value-not-conserved and missing witness evidence. It was
  not ambiguous and produced no ledger/pool success claim.
- DATC-4 reconciliation: exact input query `{}`, pool-state query `{}`, container
  `d50c302cd08774707784023ceaa846880ede3f7d7c014aef436e42c71abbf97d` remained running on
  `ghcr.io/blinklabs-io/cardano-node:10.5.4-1`; tip advanced to block 13693316 / slot 192793513 at
  100% sync. Control/target `/tmp/ouro-run.*` and target 1207-byte transaction residue were empty;
  the operator-named control artifact remained 1207 bytes at digest
  `09ed066539e809a439c93bbcc559f12cc0ea10bdfff7262ed687ac703ee7fdd2`.
- XATC-2 final gates: `make python-test`, `cargo test -p ouro` (188 passed),
  `cargo clippy -p ouro --lib --tests -- -D warnings`, bundle manifest verification and
  `git diff --check` all passed after the final Skill/runner rebuild.
- Final cleanup removed the control-generated temporary spec, metadata, generation script and signed
  mock fixture under `/tmp/ouro-s0021-deploy-acceptance`; the directory no longer exists. The
  operator-owned repository-root `pool-spec.yaml` remains untouched and untracked.

## 7. Change Requests (append-only)
- 2026-07-17 operator requested Runtime-equivalent acceptance standards for KES rotate, Upgrade and
  Deploy. Deploy's current legacy architecture requires migration before the standard is attainable.
- 2026-07-18T12:55:27+08:00 operator accepted the bounded impact of one guaranteed-invalid mock
  submission against the already-running real BP and requested Skill migration plus context-free
  subagent acceptance. The mock uses disposable keys and an independently absent input, never a
  production credential or UTxO. Real-host acceptance must observe one attempt, typed rejection, no
  retry, temporary-artifact cleanup and unchanged node/ledger/pool state; accepted/included behavior
  remains sealed unless a separate intended production transaction is explicitly authorized.
