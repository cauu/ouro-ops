# External Skills and Minimal CLI Boundary

Spec-ID: S0023
状态: active
创建时间: 2026-07-18T21:27:40+08:00
开始时间: 2026-07-18T21:27:40+08:00
完成时间:
前一个 Spec-ID: S0022
结项原因:

## 1. Requirement Details

### Background

The accepted agent workflow currently obtains its decision procedure by running
`ouro-ops skill show <name>`. `build.rs` therefore embeds every `SKILL.md`, mechanism shell asset,
and schema into the CLI, and the binary manifest/security identity changes whenever a Skill is
edited. This couples independently evolving agent guidance to CLI distribution and forces an
otherwise unnecessary CLI release for prompt-only changes.

The desired product boundary is the reverse: the public website delivers one complete,
version-declared Skill prompt to a fresh agent; that Skill directly invokes a small, stateless
`ouro-ops` enforcement CLI. The CLI owns typed observation, planning, confirmation, permits,
execution, policy verification, and its matching ephemeral target runner, but it does not own or
serve the agent's decision document.

### Scope

- Make repository `SKILL.md` files the single source used to generate complete website prompts;
  copied prompts must not call `ouro-ops skill show`.
- Remove decision-layer `SKILL.md` content and the `skill list/show` surface from the CLI release.
- Split required target probe/schema/runtime helpers from the `skills` module into explicitly named
  mechanism assets, preserving current stateless operation behavior.
- Redefine the CLI manifest and security identity around executable mechanism/policy inputs rather
  than Skill text.
- Preserve mechanical Skill↔CLI compatibility by carrying external `requires_ouro` into every
  applicable public operation boundary before observation, planning, state creation, or mutation.
- Update current documentation, automated gates, and fresh-agent acceptance to the new boundary.

### Constraints

- Existing accepted Rust `op run`, `diag exec`, `release select`, `inbox preview`, confirmation,
  fleet-permit, target validation, and ephemeral-runner semantics must not be weakened.
- A Skill-supplied minimum version may only raise the effective requirement; it cannot lower the
  CLI's own signed security floor or monotonic anti-rollback floor.
- Changing only a `SKILL.md` must not alter the CLI binary, executable security identity, mechanism
  manifest, or require a CLI release.
- The signed CLI remains self-contained for its execution mechanism: Linux/x86_64 runner, required
  target probe, schemas, stable layout contract, and release-catalog verification key remain
  available without downloading code.
- `pool-spec.yaml` is user-owned and out of scope; it must remain unmodified and uncommitted.

### Non-goals

- Rewriting the accepted operational procedures or redesigning the onboarding UI.
- Implementing the formal CLI GitHub Release/Homebrew/npm/self-update pipeline.
- Implementing digest-pinned GHCR image pull or changing Upgrade's image transport in this spec.
- Deleting the entire legacy `tool run` implementation in the same change. Necessary legacy shell
  mechanisms may remain temporarily, but must no longer be presented or hashed as decision Skills;
  complete retirement is a separate follow-up.
- Rewriting completed historical specs to describe the new model.

## 2. Outline Design

### 2.1 Decision and execution boundary

```text
repository SKILL.md
        ↓ deterministic website build
complete copyable Skill prompt + requires_ouro
        ↓
fresh agent
        ↓ typed public commands with min-version contract
minimal signed ouro-ops CLI
        ↓
ephemeral digest-bound target runner/mechanism
```

The website is the Skill distribution channel. The CLI is the execution authority. Skill text may
explain sequencing and stop conditions, but all consequential state transitions remain constrained
by typed CLI inputs, target-revalidated plans, exact confirmation, and fleet permits.

### 2.2 Build and asset layout

- Stop treating `ouro-skills/` as one build input class.
- Keep `SKILL.md` as website/test source only.
- Move or expose `ouro-probe.sh`, schemas, hashing helpers, and any temporarily retained legacy
  shell executor through a mechanism-oriented module/path.
- Continue embedding the repository-built Linux/x86_64 runner in release control builds.
- A release build must be unaffected by a decision-document-only edit.

### 2.3 Compatibility contract

- Each generated complete Skill carries its repository `requires_ouro` value.
- Every supported Skill command sequence supplies that requirement to the first CLI boundary that
  could observe remote state or create local authority state; subsequent plan/apply invocations
  retain the same explicit floor.
- The CLI computes `max(external_skill_floor, cli_security_floor, monotonic_floor)` and refuses an
  older binary before network access, permit/confirmation creation, or mutation.
- Semver parsing becomes a generic version facility and no longer depends on embedded Skills.

### 2.4 Manifest and identity

- Replace `decision_hash`, `skills_hash`, and Skill-derived `required_ouro` with explicit hashes for
  the embedded target runner, mechanism assets, schemas, stable policy, and typed operation
  contract/version.
- Control↔target/security identity compares executable decisions only. Skill prose is not an
  executable identity input.
- Release signing covers the CLI and its mechanism manifest; the website release independently
  publishes the complete Skills.

### 2.5 Risk and rollback

- Main risk: Skill/CLI skew after decoupling. Mitigate with the mechanically enforced external
  minimum version and compatibility tests for every generated prompt.
- Main regression risk: accidentally removing the embedded target probe used by current stateless
  operations. Preserve it as an explicit mechanism asset and cover live observation paths.
- Rollback is a forward revert to the last binary-plus-website pair. Do not restore dual decision
  sources (`skill show` plus website prompt) as a compatibility mode because that recreates
  ambiguity over which procedure is authoritative.

## 3. Execution Plan

- [ ] p1-1 define the external Skill prompt contract and deterministic website generation from the
  repository Skill sources, including `requires_ouro`
- [ ] p2-1 split mechanism assets/version utilities from `skills.rs`; remove `SKILL.md` from CLI
  build inputs while preserving runner, probe, schema, and policy behavior
- [ ] p2-2 remove `skill list/show` and all current CLI/help/error paths that direct agents back to
  the binary for decision guidance
- [ ] p3-1 enforce the external minimum CLI version across supported Skill command boundaries and
  preserve monotonic/signed security floors
- [ ] p3-2 redefine the CLI mechanism manifest, build identity, packaging contract, and release
  documentation without decision-layer hashes
- [ ] p4-1 retire or rewrite obsolete embedding/parity/web assertions and update CI to prove Skill
  edits are independent of CLI artifacts while current mechanism tests remain green
- [ ] p5-1 run fresh-agent acceptance for all public Skills plus representative real read/write
  regression at the previously agreed safe stopping boundaries

## 4. Test And Acceptance Criteria

- TC-1 Decision independence: modifying only a `SKILL.md` changes the generated website prompt but
  does not change CLI build inputs, CLI binary digest, executable security identity, or mechanism
  manifest.
- TC-2 Website fidelity: every public operation emits the complete corresponding repository Skill,
  preserves its `requires_ouro`, contains no `skill show` dependency, and passes prompt/content
  drift tests.
- TC-3 Minimal CLI surface: release CLI has no `skill list/show` command and does not contain full
  decision-document text; help and errors consistently point to the supplied Skill rather than an
  embedded copy.
- TC-4 Compatibility gate: an older CLI is refused before remote observation or local authority
  state creation; an equal/newer CLI proceeds; external input cannot lower CLI security or
  monotonic floors.
- TC-5 Mechanism preservation: runner transport, target probe, schema validation, signed policy,
  candidate binding, confirmation, permit, and target revalidation retain their existing automated
  acceptance behavior.
- TC-6 Manifest/identity: the published manifest inventories only runner/mechanism/schema/policy/
  typed-contract inputs; Skill prose edits do not cause parity mismatch or manifest drift.
- TC-7 Fresh-agent flow: a fresh agent given only each generated website Skill correctly reaches
  the accepted endpoint for Observability, Troubleshooting, Runtime, Upgrade, KES Rotate, and
  Deploy without inventing `skill show` or legacy `tool run` steps.
- TC-8 Representative real regression: Observability and Troubleshooting reach real conclusions;
  Runtime completes one approved relay restart; Upgrade, KES Rotate, and Deploy stop at their
  previously agreed safe/mock boundaries without unapproved production effects.
- TC-9 Repository hygiene: Rust/Python/web/manifest checks pass and the unrelated untracked
  `pool-spec.yaml` remains untouched.
- Pass/fail: TC-1 through TC-9 pass. No public Skill depends on decision text embedded in the CLI,
  and the already accepted execution mechanisms retain their prior safety and outcome contracts.

## 5. Execution Log (append-only)

- 2026-07-18 S0023 activated after S0022 was completed as delivered. Execution has not started;
  p1-1 is the first pending item.

## 6. Validation Evidence (append-only)

## 7. Change Requests (append-only)

- 2026-07-18 user approved separating decision Skills from the CLI so Skills call the minimal CLI
  directly, and requested a dedicated spec after completing S0022.
