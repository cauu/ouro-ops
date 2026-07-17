# KES, Upgrade, And Deploy Fresh-Agent Acceptance

Spec-ID: S0021
Status: draft
Created Time: 2026-07-17T14:14:21+08:00
Start Time:
Completion Time:
Previous Spec-ID: S0020
Closure Reason:

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
- Deploy is irreversible. A no-submit acceptance can prove flow readiness, but only an explicitly
  authorized real transaction plus independent ledger evidence can prove production submit success.

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
- [ ] p3-1 migrate Deploy to stateless plan/apply, update Skill/website, and accept the irreversible
  no-submit flow-readiness tier.
- [ ] p3-2 optionally execute production Deploy only with a separately supplied intended signed tx,
  exact candidate authorization and independent ledger verification.

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
- DATC-4 no-submit real-host tier: a fresh agent reaches the complete stable final plan on the real BP,
  waits for approval, and stops without confirmation/apply; node state and runner cleanup reconcile.
- DATC-5 production tier: only when separately authorized, the exact signed tx is submitted once and
  independent ledger evidence reports tx inclusion and resulting pool registration. Without this
  evidence, production Deploy remains pending even if the node accepted submission.

- XATC-1 product parity: current website prompts, embedded Skills, CLI help and implementation agree
  on commands, artifact custody, approval boundaries, stop conditions and honest outcome language.
- XATC-2 regression: Rust/Python/security/web tests, Clippy, manifest verification and whitespace
  checks pass; temporary control/target files are removed and operator-owned files remain untouched.
- Pass/fail: KES and Upgrade pass without real production mutation only when both real safe-stop and
  sealed positive/recovery evidence pass. Deploy no-submit readiness and production submit success
  are separate verdicts and must never be conflated.

## 5. Execution Log (append-only)
- Draft only; execution has not started.
- 2026-07-17T17:39+0800 p2-1 was completed under active spec S0020 p4-13. The website-style fresh
  agent reached stable relay preload plans and the signed-transition safe stop without production
  mutation; sealed signed-policy fixtures proved preload, activation, rollback and forward-only
  recovery behavior. Deploy remains pending and is not broadened by this completion.

## 6. Validation Evidence (append-only)
- None; criteria proposed for operator review.
- Upgrade evidence is recorded in S0020 ERTC-33 through ERTC-37. Production containers/images were
  unchanged, no archive bytes or capabilities were sent, and temporary runner/spec/archive residue
  was removed.

## 7. Change Requests (append-only)
- 2026-07-17 operator requested Runtime-equivalent acceptance standards for KES rotate, Upgrade and
  Deploy. Deploy's current legacy architecture requires migration before the standard is attainable.
