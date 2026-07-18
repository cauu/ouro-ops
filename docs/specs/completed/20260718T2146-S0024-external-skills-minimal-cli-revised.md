# External Skills and Minimal CLI Boundary — Revised

Spec-ID: S0024
状态: completed
创建时间: 2026-07-18T21:46:22+08:00
开始时间: 2026-07-18T21:46:22+08:00
完成时间: 2026-07-18T21:58:58+08:00
前一个 Spec-ID: S0023
结项原因: replaced

## 1. Requirement Details

### Background

The current CLI compiles the agent decision documents (`SKILL.md`) together with the execution
mechanism. A compatible wording, sequencing, or evidence-guidance change therefore changes the
binary manifest/security identity and appears to require a new CLI release. This is a real
ownership defect: agent guidance and typed enforcement evolve for different reasons.

The product boundary is:

- the website publishes one complete canonical Skill prompt for a fresh agent;
- the Skill invokes a small signed CLI through stable typed commands;
- the CLI owns observation, validation, planning, candidate binding, confirmation, permits,
  execution, signed image policy, and its matching ephemeral target runner;
- compatible Skill-only changes do not require a CLI release.

This change deliberately accepts a different trust statement. The website becomes the source of
agent decision text. The CLI constrains its typed execution path, but cannot guarantee that a
terminal-capable agent will not bypass Ouro. No claim that website content is cryptographically
bound to the CLI is permitted.

### Public Skill set

The public website distributes exactly these six Skills in this iteration:

- Observability
- Troubleshooting
- Runtime
- Upgrade
- KES Rotate
- Deploy

`config`, `detect`, `onboard`, `adopt`, and legacy script names are not standalone public website
Skills. Supporting helpers may still exist as typed CLI internals. A new public Skill requires an
explicit spec change.

### Scope

- Generate each complete website prompt from its canonical repository `SKILL.md`; do not maintain a
  second copy and do not call `ouro-ops skill show`.
- Remove decision-document embedding and the public `skill list/show` CLI surface.
- Preserve only the self-contained execution assets actually required by current typed operations:
  Linux/x86_64 runner, target probe, schemas, stable layout/policy inputs, and verification keys.
- Add one pure compatibility preflight that checks the Skill's minimum CLI version and exact CLI
  contract version before the Skill performs any other CLI/SSH operation.
- Remove Skill text/digests from executable identity and release description. Do not replace them
  with a second exhaustive integrity framework already covered by the signed binary.
- Audit `tool run` production reachability. If no named current production consumer exists, delete
  its public routing and embedded legacy script executor in this spec. Tests and historical fixtures
  do not count as production consumers. If a real consumer is found, stop and expose the retention
  trade-off to the user instead of silently preserving or deleting it.
- Update current documentation, tests, CI, and fresh-agent acceptance to the new boundary.

### Constraints

- Existing accepted typed `op run`, `diag exec`, `release select`, `inbox preview`, confirmation,
  fleet-permit, target validation, and ephemeral-runner semantics must not be weakened.
- A compatible Skill edit is one that uses the same CLI contract and commands. A Skill that needs a
  new command/argument must raise its required CLI version and/or contract; users with an older CLI
  must receive a clean update-required refusal before SSH or state mutation.
- Compatibility checking is not anti-rollback. It must not read credentials, contact the network,
  create secrets/floors/audit rows, or modify candidate hashes. CLI release/security floor behavior
  remains independently owned by CLI distribution.
- The website must treat Skill content as text/data. Skill Markdown may not become executable HTML
  or script through build-time interpolation or browser rendering.
- Existing user-owned `pool-spec.yaml` remains untouched and uncommitted; this is an execution
  hygiene constraint, not a product acceptance feature.

### Non-goals

- Rewriting the accepted operational procedures or redesigning the onboarding UI.
- Implementing formal CLI GitHub Release, Homebrew, npm, self-update, or signed Skill distribution.
- Implementing digest-pinned GHCR image pull or changing Upgrade image transport.
- Proving cross-environment byte-for-byte reproducible Rust builds. This spec proves build-input and
  executable-identity independence from Skill prose instead.
- Repeating real KES rotation, real image activation, valid Deploy submission, or an unnecessary
  production restart when the corresponding execution mechanism did not change.
- Rewriting completed historical specs; current docs receive an explicit new trust statement.

## 2. Outline Design

### 2.1 Website delivery

```text
canonical ouro-skills/<name>/SKILL.md
              ↓ deterministic validation/build
safe serialized complete Skill + skill_version + requires_ouro + requires_contract
              ↓ copy from website
fresh agent
```

The build owns an explicit six-entry operation-to-file mapping. It validates required front matter,
rejects missing/duplicate/unknown entries, and serializes Skill content as inert text. The copied
prompt contains the complete Skill plus operator-supplied pool data/disclosures. It has no
`skill show` or legacy `tool run` bootstrap step.

### 2.2 Minimal CLI and mechanism assets

`build.rs` no longer treats `ouro-skills/` as a single binary input. `SKILL.md` files are website
inputs only. The current target probe is retained under an execution-oriented asset/module (or
implemented directly in Rust) because stateless live observation genuinely consumes it. Schemas,
stable signed policy inputs, and the Linux runner remain self-contained.

Generic hashing and version parsing move out of the decision-Skill module. The signed binary itself
covers all embedded execution bytes; the CLI exposes only a compact descriptor such as:

```json
{
  "ouro_version": "0.1.0",
  "cli_contract": 1,
  "runner_platform": "linux/x86_64",
  "runner_sha256": "<sha256>"
}
```

The old decision/Skill hashes and Skill-derived minimum version are removed. A separate exhaustive
mechanism hash taxonomy is not introduced unless a concrete release consumer requires it.

### 2.3 Compatibility preflight

Each public Skill starts with one command equivalent to:

```text
ouro-ops contract check --requires-ouro '>=0.1.0' --requires-contract 1
```

The command is pure: it parses the requirement, compares it with the running CLI and exact public
contract, returns structured success/refusal, and performs no filesystem, credential, network,
audit, confirmation, or remote operation. A malformed requirement fails closed. An incompatible
Skill may be visible on the website, but cannot progress past this step; its error must direct the
operator to install a compatible published CLI.

Minimum semver alone is not treated as proof of backwards compatibility. `requires_contract` is an
exact coarse-grained public command contract. Within one contract, CLI changes must remain backwards
compatible; a breaking command change increments the contract.

### 2.4 Legacy reachability decision

Before asset restructuring, inventory every current product entry point that can reach `tool run`.
Old tests, fixtures, completed specs, and comments are migration evidence, not consumers. If no
current public/recovery contract names it, remove:

- public `tool run`/`tool verify-context` routing and help;
- embedded legacy Skill scripts and script extraction state;
- old parity fields whose only purpose is script-pack equality;
- CI gates that validate only the retired path.

Do not delete shared probe/schema logic still consumed by typed operations. If an explicit supported
recovery operation still requires `tool run`, implementation pauses for a user decision between
retaining that recovery contract and achieving a minimal release CLI.

### 2.5 Release and rollback boundary

CLI and website releases are independent and compatibility-checked, not published as a fixed pair.
A compatible Skill can be rolled back by redeploying older website content. CLI rollback follows its
own signed release/security policy. A Skill requiring a newer CLI fails cleanly on older installs;
the system does not restore `skill show` as a fallback decision source.

## 3. Execution Plan

- [ ] p1-1 inventory the six public Skill journeys and all production reachability of `skill show`,
  `tool run`, embedded scripts, decision hashes, and Skill-derived version floors; stop for a user
  decision only if a named current production consumer of `tool run` exists
- [ ] p2-1 implement deterministic, safe website delivery directly from the six canonical Skill
  files, including complete clipboard content and front-matter validation
- [ ] p2-2 remove `SKILL.md` build inputs and `skill list/show`; isolate only runner/probe/schema/
  policy execution assets and generic version/hash utilities
- [ ] p2-3 remove the legacy `tool run` public/script path if p1-1 finds no production consumer, and
  retire tests/fixtures that assert only the obsolete architecture
- [ ] p3-1 add the pure `contract check` compatibility preflight and update all six Skills to invoke
  it exactly once before other CLI or SSH actions
- [ ] p3-2 remove decision/Skill hashes and Skill-derived floors from manifest, identity, packaging,
  and self-description; retain only the minimal CLI contract/runner descriptor actually consumed
- [ ] p4-1 update current README/operations/threat/release documentation and automated gates to the
  external-decision/internal-enforcement trust boundary
- [ ] p5-1 run full automated regression plus fresh-agent acceptance for all six Skills and the
  reduced real/safe-boundary checks defined below

## 4. Test And Acceptance Criteria

- TC-1 Source fidelity: the website's operation mapping is exactly the six declared Skills; copied
  content contains the complete canonical Skill and correct front matter with no duplicate second
  copy, truncation, wrong-operation content, `skill show`, or positive `tool run` instruction.
- TC-2 Build refusal and injection safety: missing/duplicate/unknown Skill mapping, malformed or
  absent `skill_version`/`requires_ouro`/`requires_contract`, and unsafe serialization fail the site
  build; Markdown/HTML/script-like fixture content remains inert text under the site's CSP.
- TC-3 Decision independence: `SKILL.md` files are absent from Cargo/build-script inputs and release
  CLI contents; a compatible prose-only Skill edit leaves the CLI descriptor, executable security
  identity, and runner digest unchanged without requiring byte-identical cross-environment builds.
- TC-4 Minimal surface: release CLI has no `skill list/show`; help/errors do not direct agents to an
  embedded decision source. Generic version/hash utilities and live probe behavior remain available
  without a decision-Skill module dependency.
- TC-5 Pure compatibility: `contract check` accepts equal/newer compatible CLI, refuses older,
  malformed, and wrong-contract inputs, and produces no filesystem change, network/SSH attempt,
  credential access, audit row, confirmation, permit, or candidate state.
- TC-6 Compatibility failure routing: every generated Skill runs the preflight first and an
  incompatible CLI stops with an actionable update-required result before any other command. A
  Skill using a new CLI contract explicitly raises `requires_contract`.
- TC-7 Legacy reachability: all current product surfaces have a recorded consumer classification.
  If none names `tool run`, its public route, embedded script executor, script-pack parity, and
  obsolete-only CI gates are absent. If a real consumer is found, implementation is paused and this
  TC is replaced only after explicit user resolution.
- TC-8 Mechanism preservation: automated tests for runner transport, target probe, schema/policy
  validation, live candidate binding, confirmation, permit, artifact binding, target revalidation,
  and signed release selection retain their accepted outcomes.
- TC-9 Fresh-agent flow: a fresh agent given only each website-generated Skill reaches the accepted
  endpoint for Observability, Troubleshooting, Runtime, Upgrade, KES Rotate, and Deploy without
  inventing `skill show`, `tool run`, onboarding, adoption, raw SSH writes, or unapproved capability
  creation.
- TC-10 Proportionate real regression: Observability and Troubleshooting return real conclusions on
  the provided nodes. Runtime reaches the same live final restart plan without applying it. Upgrade,
  KES Rotate, and Deploy reach their previously agreed plan/mock/guaranteed-invalid-safe stopping
  boundaries without a real image activation, KES change, valid chain submission, or unrelated
  production mutation.
- TC-11 Trust/documentation: current website and docs explicitly state that website content is the
  agent decision source, the CLI constrains only typed execution, and a terminal-capable agent can
  bypass Ouro; no obsolete claim says the website cannot alter the procedure.
- TC-12 Repository quality: Rust/Python/web/contract checks pass, generated site artifacts contain
  the canonical Skills, and unrelated user files remain outside commits.
- Pass/fail: TC-1 through TC-12 pass, or execution stops at TC-7 with a concrete named production
  consumer requiring a user decision. Compatible Skill prose evolves without a CLI release, while
  new CLI capabilities fail cleanly until a compatible contract is installed.

## 5. Execution Log (append-only)

- 2026-07-18 S0024 activated as the reviewed replacement for S0023. No implementation item has
  started; p1-1 is the first pending item.
- 2026-07-18 S0024 closed as replaced before implementation started. S0025 restates the complete
  scope with exact-digest Blink Labs GHCR pulls, live catalog publication, release-candidate quality,
  and local-site acceptance while deferring formal CLI and production-site publication.

## 6. Validation Evidence (append-only)

## 7. Change Requests (append-only)

- 2026-07-18 replaces S0023 after adversarial review. The review confirmed the real problem but
  rejected per-command stateful version floors, exhaustive replacement manifests, paired rollback,
  unnecessary production restart, and default preservation of legacy `tool run`.
- 2026-07-18 operator changed the delivery boundary: Ouro must never host image archives; Upgrade
  pulls an exact signed digest from Blink Labs GHCR; the signed catalog is published and accepted
  live now; formal CLI publication and production-site acceptance move to a following spec. These
  constraints replace S0024 with S0025 rather than rewriting the active specification.
