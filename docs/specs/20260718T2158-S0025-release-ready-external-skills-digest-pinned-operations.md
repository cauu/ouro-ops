# Release-Ready External Skills And Digest-Pinned Operations

Spec-ID: S0025
状态: active
创建时间: 2026-07-18T21:58:58+08:00
开始时间: 2026-07-18T21:58:58+08:00
完成时间:
前一个 Spec-ID: S0024
结项原因:

## 1. Requirement Details

### Background

S0024 correctly separated agent decision guidance from the typed CLI mechanism, but excluded the
image-distribution change required by the intended Upgrade workflow and mixed production publication
with functional readiness. This iteration must finish the functional refactor and proportionate
end-to-end acceptance so the product is ready to publish, while keeping the actual CLI distribution
and production website rollout as a small, auditable follow-up.

The product boundary is:

- the website publishes one complete canonical Skill prompt for a fresh agent;
- the Skill invokes a small CLI through stable typed commands;
- the CLI owns observation, validation, planning, candidate binding, confirmation, permits,
  execution, signed image policy, and its matching ephemeral target runner;
- compatible Skill-only changes do not require a CLI release;
- Ouro never hosts or distributes container image archives;
- Upgrade resolves a signed release entry and pulls the exact approved OCI digest directly from
  Blink Labs GHCR;
- the signed release catalog is published and exercised through its canonical live URL in this
  iteration;
- the CLI release candidate and locally served website must reach release quality, but uploading the
  formal CLI release and deploying the production website belong to the next spec.

### Public Skill set

The public website distributes exactly these six Skills in this iteration:

- Observability
- Troubleshooting
- Runtime
- Upgrade
- KES Rotate
- Deploy

`config`, `detect`, `onboard`, `adopt`, image preload, and deployment steps are not additional public
prompts. Supporting stages remain typed commands within the owning Skill journey. A new public Skill
requires an explicit spec change.

### Scope

- Generate each complete website prompt from its canonical repository `SKILL.md`; do not maintain a
  second copy and do not call `ouro-ops skill show`.
- Remove decision-document embedding and the public `skill list/show` CLI surface.
- Preserve only the self-contained execution assets required by current typed operations: the
  Linux/x86_64 runner, target probe, schemas, stable policy inputs, and verification keys.
- Add one pure compatibility preflight that checks the Skill's minimum CLI version and exact public
  CLI contract before any other CLI, credential, network, SSH, or mutating action.
- Remove Skill text/digests from executable identity and release description. Do not replace them
  with a second exhaustive integrity framework already covered by the signed binary.
- Audit `tool run` production reachability and remove its public routing and embedded legacy script
  executor when no named current production consumer exists. Tests, historical specs, fixtures, and
  comments are not consumers. A real consumer is an unresolved product decision and blocks S0025
  completion until the user chooses its disposition.
- Extend the signed release catalog with the fixed Blink Labs GHCR repository identity and the exact
  OCI digests needed for typed image selection and verification.
- Replace operator-provided Docker archives with a typed, candidate-bound exact-digest pull directly
  from Blink Labs GHCR. Remove the Upgrade image-tar/artifact-file path from public commands, Skills,
  help, tests, and current documentation.
- Re-sign and publish the release catalog to its canonical live URL, then exercise Upgrade acceptance
  using that online signed catalog rather than a fixture or local override.
- Produce and validate a release-mode CLI candidate containing its matching Linux/x86_64 runner and
  a compact contract/runner descriptor. Validate packaging inputs and candidate checksums locally or
  in CI without publishing a formal CLI release.
- Serve the production-form website build from a local HTTP service and use prompts copied from that
  service for fresh-agent acceptance of all six Skills.
- Update current documentation, tests, CI, and end-to-end acceptance to this boundary.

### Constraints

- Ouro must not build, mirror, cache for distribution, upload, publish, or require an operator to
  provide a Docker image tar. It may only instruct the target container runtime to pull an approved
  exact digest from `ghcr.io/blinklabs-io/cardano-node`.
- Image identity is never selected by a mutable tag alone. The signed catalog binds the upstream
  repository, release/index digest, platform manifest digest, image config digest, supported
  platform, layout contract, and allowed transitions. Execution pulls
  `ghcr.io/blinklabs-io/cardano-node@sha256:<platform-manifest-digest>` and independently verifies the
  resulting image config digest before producing an activation candidate.
- Image preparation is non-disruptive: planning performs no pull; application requires the bound
  candidate and explicit confirmation, pulls only the exact signed digest, verifies it, and does not
  stop, recreate, or switch the active container.
- The signed catalog remains fail-closed: pinned verification key, signature and schema validation,
  freshness/rollback rules, no insecure online-to-embedded fallback, and no unverified cache.
- Existing accepted typed `op run`, `diag exec`, `release select`, confirmation, fleet-permit, target
  validation, and ephemeral-runner semantics must not be weakened.
- A compatible Skill edit uses the same CLI contract and commands. A Skill requiring a new command
  or argument raises its required CLI version and/or contract; an older CLI refuses before SSH or
  mutation.
- Compatibility checking is not anti-rollback and must not read credentials, contact the network,
  create local state, or alter candidates. CLI release/security-floor behavior remains owned by CLI
  distribution.
- Website Skill Markdown is inert text/data and cannot become executable HTML or script through
  build interpolation or browser rendering.
- Catalog signing may use the existing local macOS Keychain-backed release signer. CI receives only
  the public verification key; no private signing key or 1Password access is introduced.
- Existing user-owned `pool-spec.yaml` remains untouched and uncommitted.

### Non-goals

- Uploading the CLI to GitHub Releases, publishing final release signatures/checksums/installers,
  updating a live installer, Homebrew/npm packaging, or self-update delivery. These are next-spec
  publication tasks after S0025 proves the candidate release-ready.
- Deploying or accepting the production website, configuring Cloudflare production secrets,
  switching the public domain, or validating the production CDN. These are next-spec tasks; this
  iteration validates the production-form build through a local HTTP service.
- Hosting any container image or Docker archive under Ouro-controlled release infrastructure.
- Pulling an image from a repository other than Blink Labs GHCR, selecting a tag without an exact
  signed digest, or accepting a locally supplied image tar as the normal Upgrade path.
- Actually activating a new node image, installing a KES certificate, submitting a valid Deploy
  transaction, or changing unrelated production state.
- Rewriting onboarding UI or completed historical specs.
- Proving byte-for-byte reproducible Rust builds across arbitrary host toolchains. The required
  release check proves controlled release inputs, the embedded runner pairing, contract identity,
  packaging contents, and executable smoke behavior.

## 2. Outline Design

### 2.1 External Skills and local website delivery

```text
canonical ouro-skills/<name>/SKILL.md
              ↓ deterministic validation/build
safe serialized complete Skill + skill_version + requires_ouro + requires_contract
              ↓ locally served production-form site
fresh agent
```

The build owns an explicit six-entry operation-to-file mapping, rejects missing, duplicate, or
unknown entries, validates required front matter, and serializes the full Markdown as inert text.
The copied prompt contains the complete Skill plus operator-supplied pool data and disclosures. It
does not bootstrap through `skill show` or legacy `tool run`.

Production hosting is deliberately outside this spec. Acceptance starts the built site on a local
HTTP origin, uses the rendered copy action, and gives that exact prompt to a fresh agent.

### 2.2 Minimal CLI and compatibility boundary

`build.rs` no longer treats `ouro-skills/` as a CLI input. The target probe remains an execution
asset/module because stateless live observation consumes it. Schemas, stable policy/key material,
and the Linux runner remain self-contained. Generic hashing and version parsing move out of any
decision-Skill module.

The CLI exposes a compact machine-readable descriptor such as:

```json
{
  "ouro_version": "0.1.0",
  "cli_contract": 1,
  "runner_platform": "linux/x86_64",
  "runner_sha256": "<sha256>"
}
```

Every public Skill begins with one pure command equivalent to:

```text
ouro-ops contract check --requires-ouro '>=0.1.0' --requires-contract 1
```

It parses and compares locally, produces structured success/refusal, and has no filesystem,
credential, network, audit, confirmation, permit, candidate, or remote side effect. Contract mismatch
or malformed requirements fail with an actionable install-a-compatible-CLI result.

### 2.3 Legacy reachability decision

All current product entry points reaching `tool run`, `tool verify-context`, embedded legacy scripts,
or script-pack parity are classified before removal. Shared probe/schema logic used by typed commands
is retained. If no current supported product/recovery contract consumes the legacy executor, those
routes, assets, states, and obsolete-only gates are deleted. If a real consumer is found, work pauses
for an explicit user decision; S0025 cannot pass with an unresolved conditional legacy surface.

### 2.4 Signed release catalog

The catalog's signed contract names the upstream repository
`ghcr.io/blinklabs-io/cardano-node`. Each approved release/platform entry already binds, or is extended
to bind, the exact release/index manifest digest, platform manifest digest, image config digest,
platform, layout contract, and permitted transition. Repository identity is covered by the catalog
signature rather than supplied ad hoc by a Skill or operator.

Catalog publication is a public-data release:

```text
local Keychain signer → canonical data/releases.json in main → raw canonical HTTPS URL
                                               ↓
                                    pinned-key CLI verification
```

CI verifies signature, schema, semantic digest relations, transitions, and canonical repository. It
does not hold the private key. Online load refuses signature failure, malformed content, rollback, or
unexpected repository and does not silently substitute an older embedded allowlist.

### 2.5 Exact-digest image preparation

```text
signed catalog target
       ↓ release select / transition validation
repository@platform_manifest_digest
       ↓ typed prepare plan (read-only)
candidate hash + disclosed pull/inspect actions
       ↓ explicit confirmation
docker pull ghcr.io/blinklabs-io/cardano-node@sha256:<platform-manifest>
       ↓ inspect RepoDigests/platform/config
exact repository + manifest + platform + signed config digest verified
       ↓
prepared-image evidence bound into the later Upgrade activation candidate
```

The public operation may retain an internal name such as `upgrade/preload-image` for compatibility,
but its contract is exact remote pull and verification, not archive preload. It takes a signed release
selection/candidate, never a Docker tar path. A successful preparation changes only local container
runtime image storage and Ouro audit/candidate state; the running node remains unchanged.

### 2.6 Release-ready boundary

S0025 produces a controlled release-mode macOS CLI candidate with its matching embedded
Linux/x86_64 runner, validates the compact descriptor, packaging contents, checksum generation, and
smoke commands, and proves that no Skill Markdown or image archive is packaged. The candidate may be
an unpublished local/CI artifact.

The next spec performs irreversible/publication-facing distribution work: final version selection,
formal CLI signatures and GitHub Release upload, live installer wiring, and production website
deployment/acceptance. It must not reopen S0025 functional design unless publication testing exposes
a concrete defect.

### 2.7 Risk and rollback

- Catalog rollback is a new signed higher-sequence catalog entry; published history is not rewritten.
- A bad catalog is contained by signature/schema/freshness checks and exact digest verification.
- A failed image pull or verification leaves the active container unchanged and cannot produce a
  valid activation candidate.
- Skill rollback redeploys prior compatible website content; CLI rollback remains governed by the
  independent signed CLI release/security policy.
- CLI and production-site publication are deferred, so S0025 rollback is a normal code/catalog
  forward fix except for the intentionally live public catalog, which follows signed forward-only
  correction.

## 3. Execution Plan

- [x] p1-1 inventory all six public Skill journeys and every current production reachability of
  `skill show`, `tool run`, embedded scripts, decision hashes, Skill-derived version floors, Docker
  image archives, and image repository/digest selection; record the disposition of each surface
- [x] p2-1 implement deterministic, injection-safe website delivery directly from the six canonical
  Skill files, including complete clipboard content, front-matter validation, and a production-form
  local HTTP acceptance service
- [x] p2-2 remove `SKILL.md` build inputs and `skill list/show`; isolate only runner/probe/schema/
  policy execution assets and generic version/hash utilities
- [x] p2-3 remove the public legacy `tool run`/script path and obsolete-only parity gates when p1-1
  finds no production consumer; if one exists, obtain and implement the user's explicit disposition
  before completing this item
- [x] p3-1 add the pure `contract check` preflight and update all six Skills to invoke it exactly once
  before every other CLI, credential, network, SSH, or mutating action
- [x] p3-2 remove decision/Skill hashes and Skill-derived floors from manifest, identity, packaging,
  and self-description; implement and consume only the compact CLI contract/runner descriptor
- [x] p4-1 extend release-catalog schema and validation with the signed Blink Labs GHCR repository and
  exact OCI identity tuple; update release selection and transition validation
- [x] p4-2 replace archive preload with candidate-bound exact-digest GHCR pull and post-pull config/
  platform/repository verification; remove image-tar/artifact-file inputs and leave active containers
  untouched
- [x] p4-3 regenerate and sign the release catalog with the existing local signer, publish it at the
  canonical live URL, and make online pinned-key verification the Upgrade acceptance path
- [x] p5-1 implement a release-candidate build/check that pairs the macOS CLI with its Linux/x86_64
  runner, verifies descriptor/package/checksums and smoke behavior, and excludes Skill text and image
  archives without publishing a formal CLI release
- [x] p5-2 update current README, operation, threat, release, website, and CI documentation/gates to
  the external-decision, exact-upstream-pull, live-catalog, and deferred-publication boundaries
- [~] p6-1 run all automated regression gates and fresh-agent end-to-end acceptance for all six Skills
  from prompts copied from the locally served site, including the proportionate real/safe boundaries
  defined below

## 4. Test And Acceptance Criteria

### Website and external-Skill boundary

- TC-1 Source fidelity: the site maps exactly the six declared Skills; copied content contains each
  complete canonical Skill and correct front matter with no second copy, truncation, wrong-operation
  content, `skill show`, or positive `tool run` instruction.
- TC-2 Local production-form service: the release-form site build succeeds, is served over local
  HTTP, renders every operation, and its actual copy action yields the content used by fresh-agent
  tests. No acceptance depends on a production domain or Cloudflare deployment.
- TC-3 Build refusal and injection safety: missing, duplicate, or unknown mapping; malformed/absent
  `skill_version`, `requires_ouro`, or `requires_contract`; and unsafe serialization fail the site
  build. HTML/script-like Skill fixtures remain inert under the site's CSP.
- TC-4 Decision independence: `SKILL.md` files are absent from Cargo/build-script inputs and CLI
  packages. A compatible prose-only edit leaves the CLI descriptor, executable security identity,
  and runner digest unchanged without requiring cross-toolchain byte-identical builds.
- TC-5 Minimal surface: release CLI has no `skill list/show`; help/errors do not direct agents to an
  embedded decision source. Generic version/hash and live probe behavior remain available without a
  decision-Skill module dependency.
- TC-6 Pure compatibility: `contract check` accepts compatible requirements, refuses older CLI,
  malformed syntax, and wrong contracts, and produces no filesystem change, network/SSH attempt,
  credential access, audit row, confirmation, permit, or candidate state. Every generated Skill runs
  it first and stops incompatibility with an actionable message.
- TC-7 Legacy reachability: every product surface has a recorded consumer classification. No unnamed
  or test-only compatibility claim preserves `tool run`; the final CLI either has no public legacy
  executor or contains the explicitly user-approved supported consumer and corresponding contract.
  An unresolved consumer decision fails S0025.

### Signed catalog and exact upstream pull

- TC-8 Catalog identity: the schema and signature cover the exact repository
  `ghcr.io/blinklabs-io/cardano-node`, release/index digest, linux/amd64 platform manifest digest,
  image config digest, layout contract, and transition. Mutable tag text alone cannot authorize an
  image, and any altered repository/digest/signature is refused.
- TC-9 Live catalog: the canonical public HTTPS URL returns the newly signed catalog; the actual
  release CLI verifies it with the pinned public key and successfully selects the intended release.
  404, malformed JSON, bad signature, rollback sequence/freshness failure, or unexpected repository
  fails closed without an insecure embedded/cache fallback.
- TC-10 No Ouro image hosting: repository code, packages, CI artifacts, release-candidate contents,
  Skills, CLI help, and current docs neither contain nor require an image tar upload/download path.
  Upgrade does not accept an operator image archive or `--artifact-file` for image preparation.
- TC-11 Exact preparation plan: the read-only plan resolves only a signed allowed transition and
  discloses the exact `repository@platform-manifest-digest`, expected platform/config digest, target,
  and candidate hash without pulling or modifying either host.
- TC-12 Exact preparation apply: after bound candidate and explicit confirmation, the relay pulls
  the exact Blink Labs GHCR digest and verifies repository digest, linux/amd64 platform, and signed
  config digest. A tag-only reference, alternate repository, wrong manifest/config/platform, failed
  pull, or changed target refuses without producing valid prepared-image evidence. The running
  container ID, image, command, mounts, network, health, and uptime remain unchanged.

### Release candidate and regressions

- TC-13 Paired release candidate: a controlled release-mode build produces a macOS control CLI with
  its matching Linux/x86_64 embedded runner; the descriptor's contract/platform/runner digest matches
  extracted bytes, package/checksum verification and smoke commands pass, and neither Skill Markdown
  nor any image archive is included. No TC requires publishing this artifact.
- TC-14 Mechanism preservation: automated tests for runner transport, target probe, schema/policy
  validation, candidate binding, confirmation, permit, target revalidation, signed release selection,
  KES artifacts, and Deploy submission retain their accepted fail-closed behavior.
- TC-15 Observability: a fresh agent using the locally copied prompt returns evidence-backed real BP
  and relay health conclusions without remote mutation.
- TC-16 Troubleshooting: a fresh agent reaches a defensible real diagnosis/conclusion using relevant
  host/container logs, sync/tip/peer evidence, Prometheus metrics, and KES/opcert state; absence or
  inapplicability of a metric is disclosed rather than invented.
- TC-17 Runtime: a fresh agent completes live inspection, planning, candidate binding, approval, one
  real relay restart, and post-restart health verification. It must not restart the BP or widen the
  approved candidate/target.
- TC-18 Upgrade: a fresh agent uses the live signed catalog, performs the exact non-disruptive GHCR
  preparation on the relay, verifies the prepared identity, and reaches a final activation plan.
  It does not recreate or switch the running container.
- TC-19 KES Rotate: a fresh agent completes Phase A, accepts a controlled mock `node.cert`, validates
  the remaining Phase B preflight and candidate, and stops before installing or activating the
  operational certificate.
- TC-20 Deploy: a fresh agent completes both phases with a controlled mock signed transaction and
  makes one explicitly approved guaranteed-invalid submission to the BP; chain rejection is the
  expected success boundary and no valid chain state is created.
- TC-21 Fresh-agent discipline: across all six journeys, agents do not invent `skill show`, `tool
  run`, onboarding/adoption, raw SSH writes, tag-only image selection, image tar transport, unapproved
  mutations, or capabilities not granted by the copied prompt and CLI contract.
- TC-22 Repository/release quality: Rust, Python, web, shell, schema, contract, signature, and release
  checks pass; current docs describe the actual trust and publication boundaries; generated files
  are current; and unrelated user files remain outside commits.

Pass/fail: TC-1 through TC-22 must all pass. Completion means the six functions, minimal CLI,
exact-digest Upgrade transport, live signed catalog, release candidate, and local production-form
website are ready for publication. Formal CLI distribution and production website deployment are
explicitly deferred to the next spec and are not hidden alternatives for a failed S0025 acceptance.

## 5. Execution Log (append-only)

- 2026-07-18 S0025 drafted to replace S0024 after the operator clarified the release boundary. No
  implementation item has started; p1-1 is the first pending item.
- 2026-07-18 S0025 activated as the only execution and acceptance source. No implementation item has
  started; p1-1 remains the first pending item.
- 2026-07-18 p1-1 started: inventorying the six public journeys, legacy decision/script surfaces,
  release identity, and Docker archive versus signed-digest image paths before changing behavior.
- 2026-07-18 p1-1 completed: `docs/review/S0025-p1-1-surface-inventory.md` records every public
  journey and classifies legacy decision/script, release identity, website, catalog, and image
  transport surfaces. No named current production/recovery consumer of `tool run` exists; it may be
  retired. Upgrade archive transport is a current path that S0025 explicitly replaces.
- 2026-07-18 p2-1 started: replacing the website's `skill show` bootstrap with deterministic,
  front-matter-validated inert serialization of the six complete canonical Skills and a local HTTP
  acceptance service.
- 2026-07-18 p2-1 completed: the site build now owns an exact six-entry canonical mapping, validates
  and safely serializes complete Skills, includes the selected Skill in the copied prompt, and ships
  a loopback-only production-form service used by the website acceptance test.
- 2026-07-18 p2-2 started: removing decision documents and public Skill routing from the Rust build
  while preserving the runner, target probe, schemas, policy/key inputs, and generic hash/version
  utilities consumed by typed execution.
- 2026-07-18 p2-2 completed: Rust decision assets moved to an execution-only `assets` module,
  `SKILL.md` and directory-wide Skill rebuild triggers are absent, the public `skill` route/help is
  removed, and probe/schema/runner consumers plus the transitional legacy script executor still
  compile pending its dedicated p2-3 removal.
- 2026-07-18 p2-3 started: deleting the unconsumed public legacy script executor, resident-target
  dispatch wrapper/parity path, embedded script pack and obsolete-only CI fixtures while retaining
  shared code reached by current typed operations.
- 2026-07-18 p2-3 completed: the public `tool`/`init`/`deinit` routes, script resolver/extractor,
  legacy SSH wrapper dispatch, old provisioning recipe, embedded shell pack, and obsolete-only E2E
  fixtures are removed. Current onboarding cleanup, typed operations, probe/schema assets, durable
  transactions, confirmation, and target parity remain reachable and covered.
- 2026-07-18 p3-1 started: adding a compile-time-only external-Skill compatibility comparison and
  making its exact invocation the mandatory first action in every canonical public Skill.
- 2026-07-18 p3-1 completed: `contract check` strictly parses the version floor and exact contract,
  emits one typed success/refusal, and reaches no stateful subsystem. All six Skills invoke their
  front-matter requirements exactly once before pool-spec, credential, network, SSH, or other CLI
  work.
- 2026-07-18 p3-2 started: replacing the legacy asset-taxonomy manifest and local version-floor
  state with one compact CLI/runner descriptor while preserving internal execution-identity checks.
- 2026-07-18 p3-2 completed: `ouro-ops contract` now exposes only version, contract, runner platform
  and compiled-runner digest; debug builds honestly report a missing runner as null. The manifest
  route/file, Skill-derived required floor, local version-floor MAC state, and parity floor field are
  removed. Packaging and installer smoke checks consume the compact descriptor.
- 2026-07-19 p4-1 started: binding the canonical Blink Labs GHCR repository into release-catalog v5,
  adding strict schema/semantic validation, and carrying repository identity through selection and
  transition results. The existing Keychain-backed authority signed the local candidate after an
  explicit one-time operator authorization; publication remains p4-3.
- 2026-07-19 p4-1 completed: catalog v5 and its JSON schema bind the exact Blink Labs repository,
  index digest, platform manifest digest, image config digest, platform, layout and directed
  transitions. Release signing refuses a missing/alternate repository, selection returns the signed
  repository, and the Keychain signer produced a pinned-key-verifiable local document.
- 2026-07-19 p4-2 started: replacing Upgrade archive preview/payload/load with a candidate-bound
  exact GHCR pull, target-side repository/platform/config verification, and active-container
  before/after invariance checks.
- 2026-07-19 p4-2 completed: removed the image inbox/type/archive schema and every Upgrade
  `artifact`/`--artifact-file` transport; preload planning now binds and discloses only the signed
  Blink Labs repository plus exact linux/amd64 manifest/config tuple, while approved apply performs
  one target-side digest pull, verifies Docker's repository/platform/config evidence, and requires
  role readiness plus the complete modeled active-container state to remain unchanged.
- 2026-07-19 p4-3 started: publishing the already Keychain-signed catalog v5 to the canonical main-
  branch HTTPS source, then exercising the release CLI's production no-cache fetch and pinned-key
  verification rather than the local signed-file test seam.
- 2026-07-19 p4-3 completed: published byte-identical catalog v5 as GitHub main commit `3d92fe1`;
  the canonical raw URL returned the same SHA-256 bytes, and the current CLI with all local catalog/
  test-key environment seams explicitly unset verified the Ed25519 signature online and selected
  both the linux/amd64 deployment recommendation and the signed 10.5.4-1 → 10.6.4-1 transition.
- 2026-07-19 p5-1 started: defining one deterministic local release-candidate builder/checker for
  the macOS control binary paired with its exact embedded Linux/x86_64 runner, checksums,
  descriptor/package smoke evidence, and negative package inventory for Skills and image payloads.
- 2026-07-19 p5-1 completed: `make release-candidate` now performs a locked x86_64-musl runner build,
  embeds those exact bytes into a locked native macOS release build, packages only `ouro-ops`, and
  emits/validates descriptor, version, live catalog smoke, candidate manifest and SHA256SUMS
  evidence. The local arm64 candidate passed extraction/compatibility checks and explicitly remains
  `release-standard-not-published`; neither the CLI nor its tarball was published.
- 2026-07-19 p5-2 started: reconciling current README/operation/threat/release/site documentation and
  CI with the six external canonical Skills, pure compatibility preflight, no target-resident Ouro,
  signed live catalog, exact upstream pull, local-site acceptance and deferred formal CLI/site
  publication boundaries.
- 2026-07-19 p5-2 completed: current product/release/threat/site documents now describe the external
  Skill and ephemeral mechanism as separate trust surfaces, the live signed catalog and exact GHCR
  pull, and honest terminal-bypass/custody limits. The website removed its unrelated upstream
  release fetch, desired-version input and placeholder install claims. CI now validates the local
  production-form site and paired CLI candidate on `next`/PR/manual triggers while intentionally
  using no deploy/signing secrets, hosted artifacts, GitHub Release or production-site action.
- 2026-07-19 p6-1 started: running the complete repository/release gate set while isolated fresh
  agents consume the actual locally generated website prompts and exercise all six real/safe
  acceptance boundaries against the declared BP/relay fleet.

## 6. Validation Evidence (append-only)

- TC-7 | stack: other | command: inspect CLI routes, six canonical Skills, website generator,
  build inputs, release identity, wrappers, tests and current docs; record
  `docs/review/S0025-p1-1-surface-inventory.md` | result: pass | note: six typed journeys are mapped;
  `tool run` has no named current consumer and is approved for removal
- TC-10 | stack: other | command: inventory `artifact-file`, `inbox preview --type image`, Docker
  archive, `docker load`, release catalog and OCI digest call sites | result: pass | note: all current
  positive image-archive surfaces and their exact signed-GHCR replacement boundary are enumerated
- TC-22 | stack: other | command: `git diff --check` and inventory-table/search assertions | result:
  pass | note: inventory is internally complete and unrelated `pool-spec.yaml` remains untracked
- TC-1 | stack: ui | command: `python3 -m pytest -q tests/test_web_generator.py` | result: pass |
  note: exact six canonical files and their complete bytes/front matter appear once in generated
  payloads and the copied-prompt path consumes the selected content without `skill show`
- TC-2 | stack: ui | command: `web/onboarding/serve-local.sh <ephemeral-port>` via
  `test_built_site_is_served_over_local_http` | result: pass | note: built release-form `index.html`
  is byte-identical over loopback HTTP and retains the real copy path
- TC-3 | stack: python | command: malformed/missing/unknown front-matter and script-terminator
  generator tests | result: pass | note: invalid sources fail the build and `</script>` content is
  escaped in HTML while round-tripping exactly as inert Skill text
- TC-4 | stack: python | command: `python3 -m pytest -q tests/test_external_skill_boundary.py` |
  result: pass | note: compiled manifest has no Skill documents/decision hash, build triggers do not
  cover Skill prose, and probe/schema execution assets remain present
- TC-5 | stack: rust | command: invoke CLI help and retired `skill list` through
  `tests/test_external_skill_boundary.py` | result: pass | note: help has no embedded decision source
  and `skill` is an unknown command
- TC-14 | stack: rust | command: `cargo test -q -p ouro --lib` | result: pass | note: 187 runner,
  probe, schema, transport, policy, confirmation and typed-mechanism unit tests pass after the asset
  boundary change
- TC-5, TC-7 | stack: rust+python | command: external-boundary source/CLI assertions plus searches
  for legacy routes, resolvers, wrappers and embedded scripts | result: pass | note: `tool` is an
  unknown command, no executable legacy script path remains, and the only wrapper-name references
  are explicit negative assertions or current onboarding removal of an old installed artifact
- TC-14 | stack: rust | command: `cargo test -q -p ouro --lib` | result: pass | note: 168 current
  typed-operation, transport, probe, schema, confirmation, transaction and policy tests pass after
  obsolete-only modules and fixtures are retired
- TC-22 | stack: python+ui | command: `make python-test && python3 -m pytest -q
  tests/test_web_generator.py tests/test_external_skill_boundary.py && git diff --check` | result:
  pass | note: all current direct Python gates and 9 pytest website/boundary cases pass; unrelated
  `pool-spec.yaml` remains untracked
- TC-6 | stack: rust | command: `cargo test -q -p ouro --lib` | result: pass | note: 171 tests pass,
  including compatible, too-old, wrong-contract and closed requirement-syntax cases for the pure
  compatibility module
- TC-6 | stack: python | command: `python3 tests/test_contract_check.py` | result: pass | note: five
  success/refusal subprocess cases emit one typed record and leave HOME, OURO_HOME, XDG config,
  cache and state trees byte-for-byte unchanged
- TC-1, TC-6 | stack: python+ui | command: `python3 tests/test_skill_docs.py && python3 -m pytest
  -q tests/test_web_generator.py tests/test_contract_check.py` | result: pass | note: 11 cases prove
  the exact six generated Skills each contain one front-matter-derived compatibility command as
  their first Ouro action
- TC-4, TC-5 | stack: rust+python | command: compact descriptor unit tests and
  `python3 tests/test_external_skill_boundary.py` | result: pass | note: public self-description has
  exactly `ouro_version`, `cli_contract`, `runner_platform`, and `runner_sha256`; legacy manifest is
  unknown and its committed taxonomy file is absent
- TC-4 | stack: other | command: source/build dependency assertions for decision documents,
  `required_ouro`, version-floor state, and parity identity | result: pass | note: Skill prose is not
  a build or identity input; signed distribution metadata remains the owner of revocation policy
- TC-14, TC-22 | stack: rust+python | command: `cargo test -q -p ouro --lib && cargo clippy -q -p
  ouro --lib --tests -- -D warnings && make python-test && python3 -m pytest -q
  tests/test_web_generator.py tests/test_external_skill_boundary.py tests/test_contract_check.py` |
  result: pass | note: 169 Rust tests, clippy, all direct Python gates and 12 pytest cases pass
- TC-8 | stack: rust | command: `cargo test -q -p ouro --lib` | result: pass | note: 169 tests pass;
  release-candidate validation refuses missing/alternate repositories and selection binds all three
  OCI digests to the fixed repository and platform
- TC-8 | stack: python | command: `python3 tests/test_release_catalog.py` | result: pass | note:
  Draft 2020-12 schema validation, deploy/transition selection, exact repository/tuple output,
  signed semantic refusal and signature-tamper refusal all pass without cache state
- TC-8 | stack: other | command: `ouro-allowlist-signer inspect --input data/releases.json` after
  Keychain-backed sign | result: pass | note: catalog v5 validates under pinned public key
  `3ceb1920…165dd`; signed canonical payload digest is `203fea05…a98d83`
- TC-10 | stack: rust+python+docs | command: source inventory plus `python3
  tests/test_skill_docs.py` and `python3 tests/test_s0019_dispatch.py` within `make python-test` |
  result: pass | note: image ArtifactType, archive parser, inbox image preview/stage, Upgrade artifact
  parameter/payload and image `--artifact-file` path are absent; help, current docs and Skill expose
  only exact target-side GHCR preparation
- TC-11, TC-12 | stack: python | command: `python3 tests/test_s0020_upgrade_workflow.py` |
  result: pass | note: plan emits the exact signed
  `ghcr.io/blinklabs-io/cardano-node@<platform-manifest>` tuple without Docker access; approved apply
  verifies repo digest/linux-amd64/config and unchanged identity/image/command/mounts/network/
  creation-epoch/readiness; tag, alternate repo, wrong manifest/config/platform and pull failure all
  refuse without container activation
- TC-10, TC-14 | stack: rust+python | command: `cargo test -p ouro --lib && cargo clippy -q -p
  ouro --lib --tests -- -D warnings && make python-test` | result: pass | note: 169 Rust tests,
  clippy and the complete direct Python suite pass after exact-pull migration; the control apply
  transport test proves Upgrade sends runner bytes only and rejects a supplied image artifact before
  SSH
- TC-9 | stack: other | command: publish `data/releases.json` to `origin/main` as `3d92fe1`, then
  compare `shasum -a 256 data/releases.json` with no-cache canonical HTTPS bytes | result: pass |
  note: local and live documents are byte-identical at
  `85e7474475575357def24f540943511e1134bb0d672122a9741c65ee0900b66e`; remote main resolves to
  `3d92fe100c4e1225152055d4b80286a94fb4c393`
- TC-9 | stack: rust+network | command: `env -u OURO_RELEASES_FILE -u OURO_ALLOWLIST_TEST_KEY
  target/debug/ouro-ops release select --platform linux/amd64` and the same command with `--from
  sha256:a3223d…c0c7a` | result: pass | note: production no-cache HTTPS/pinned-Ed25519 path selected
  11.0.1-1 for deploy and the exact 10.5.4-1 → 10.6.4-1 transition, reported catalog v5/fixed GHCR
  repository, and wrote no cache
- TC-13 | stack: other | command: `make release-candidate && python3
  tests/test_release_candidate.py` | result: pass | note: locked macOS arm64 control package contains
  only `ouro-ops`; paired statically linked Linux/x86_64 ELF SHA-256
  `29dcaaa6e05ce6a427de17ac0833c62d2ad95db300199a9d593f1127c75c2c92` equals the release
  descriptor, extracted-binary descriptor/contract smoke and all SHA256SUMS checks pass, live
  catalog v5 selection succeeds, and package/candidate inventories contain neither Skill Markdown
  nor node-image payloads; manifest status is `release-standard-not-published`
- TC-1, TC-2, TC-3 | stack: ui+python | command: `./web/onboarding/build.sh && python3 -m pytest
  -q tests/test_web_generator.py` | result: pass | note: eight generator/source-fidelity/copy/local-
  HTTP cases pass; CSP now has `connect-src 'none'`, no ambient fetch exists, Upgrade carries no
  operator-selected `node_version`, and copied prompts use the release-paired runner boundary
- TC-4, TC-5, TC-10, TC-13 | stack: other | command: `python3 tests/test_skill_docs.py && python3
  tests/test_release_candidate.py && python3 tests/test_release_surfaces.py` | result: pass | note:
  current docs and workflows contain no embedded-Skill/legacy tool/Tauri/placeholder-install path;
  release CI builds/checks without publication, site CI builds/tests without deployment or secrets,
  and the locally built paired candidate retains its verified no-Skill/no-node-image inventory
- TC-22 | stack: other | command: YAML-parse current workflows, stale-surface inventory and `git
  diff --check` | result: pass | note: README, operation, threat, release and website documentation
  agree on external decision custody, pure compatibility, target-side ephemeral execution, signed
  live catalog, exact upstream pull, and deferred formal CLI/production-site publication

## 7. Change Requests (append-only)

- 2026-07-18 replaces S0024. The operator requires release-ready code and complete functional
  acceptance now, defers formal CLI publication and production-site acceptance, requires Ouro to
  never host image archives and to pull exact signed digests only from Blink Labs GHCR, and authorizes
  publication/online acceptance of the signed release catalog in this spec.

## 8. Follow-up Execution Plan (append-only)

- [x] p6-1-fix1 make copied-operation selection unambiguous and make the disclosure copy result
  truthful before handing the remaining real-machine acceptance to the operator

## 9. Follow-up Test And Acceptance Criteria (append-only)

- TC-23 Prompt handoff integrity: after generating different operations in succession, the real
  disclosure action copies the currently displayed operation prompt, never the previous prompt. The
  UI reports success only after the clipboard write completes. Generated instructions distinguish
  the literal SSH host required by `op run --dispatch` from the machine id required by
  `diag exec --dispatch`.

## 10. Follow-up Execution Log (append-only)

- 2026-07-19 p6-1-fix1 started: two fresh agents independently substituted a machine id where the
  typed operation requires the spec's literal SSH host, and browser acceptance observed a stale
  previous-operation clipboard value while the page already displayed `Copied`. The operator will
  perform the remaining real-machine journeys after this development defect is fixed and automated
  release gates pass.
- 2026-07-19 p6-1-fix1 completed: generated prompts now state the distinct typed selectors, and the
  disclosure approval performs a synchronous user-gesture copy before the modern clipboard write,
  awaits completion, reports failure without closing, and reports success only after at least one
  copy mechanism succeeds. The operator can now use the site for the remaining real-machine
  acceptance while p6-1 remains active.

## 11. Follow-up Validation Evidence (append-only)

- TC-23 | stack: ui | command: local HTTP browser flow with clipboard preloaded to a stale sentinel;
  generate/copy Observability once, then generate/copy KES Rotate once | result: pass | note: each
  first disclosure approval copied bytes exactly equal to the currently displayed prompt, the
  second operation replaced the first, success appeared only afterward, and the copied KES prompt
  distinguishes literal SSH host dispatch from diagnostic machine-id dispatch
- TC-23 | stack: python+ui | command: `./web/onboarding/build.sh && python3 -m pytest -q
  tests/test_web_generator.py tests/test_release_surfaces.py` | result: pass | note: eight cases
  cover the direct approval handler, awaited copy, selector wording, canonical Skill source, local
  HTTP form and current release surfaces
- TC-14, TC-22 | stack: rust+python+shell | command: `cargo test -q`, `cargo clippy -q -p ouro
  --lib --tests -- -D warnings`, `make python-test`, `python3 -m pytest -q`, and `bash
  ci/l2-integration.sh` | result: pass | note: 169 Rust tests, clippy, every direct Python gate, 12
  pytest cases and the complete L2 integration gate pass after the handoff fix
- TC-13, TC-22 | stack: other | command: `make release-candidate` | result: pass | note: the paired
  release candidate rebuilt successfully and its package, runner, descriptor, version, live release
  selection, candidate manifest and checksums all verify

## 12. Second Follow-up Execution Plan (append-only)

- [x] p6-1-fix2 bind locally copied acceptance prompts to the paired release-candidate CLI instead
  of an unrelated pre-existing PATH installation

## 13. Second Follow-up Test And Acceptance Criteria (append-only)

- TC-24 Candidate execution binding: after `make release-candidate`, every generated local-acceptance
  prompt invokes the exact repository-local control candidate for compatibility and all subsequent
  Skill commands. A stale bare `ouro-ops` earlier on PATH cannot receive an operation, and the
  binding neither overwrites nor installs into the operator's existing CLI location.

## 14. Second Follow-up Execution Log (append-only)

- 2026-07-19 p6-1-fix2 started: operator-run Troubleshooting stopped before SSH because the copied
  prompt invoked bare `ouro-ops`; PATH resolved `/Users/caiyufu/.local/bin/ouro-ops`, whose help still
  exposes retired embedded `skill`, `tool`, and `manifest` surfaces, instead of the newly built
  repository-local candidate. The current candidate recognizes `troubleshooting/snapshot`; candidate
  construction succeeded but candidate selection was absent from the website handoff contract.
- 2026-07-19 p6-1-fix2 completed: every local acceptance prompt now replaces the Skill's bare
  command name with `./target/release-candidate-control/release/ouro-ops`, including the mandatory
  first contract check, and explicitly refuses PATH fallback or installation overwrite. The final
  inline JavaScript now has a syntax gate, and clipboard success uses available readback to retry or
  refuse a stale value instead of reporting success.

## 15. Second Follow-up Validation Evidence (append-only)

- TC-24 | stack: other | command: compare `command -v ouro-ops`, PATH binary help/SHA-256 and paired
  candidate help/SHA-256; run candidate Troubleshooting `--transport-plan` against operator spec |
  result: pass | note: PATH resolves an older distinct binary with retired surfaces; the exact paired
  candidate accepts `troubleshooting/snapshot`, binds the current linux/x86_64 runner and produces a
  no-SSH transport plan
- TC-23, TC-24 | stack: ui | command: build and serve local site, preload a stale clipboard, then
  generate and approve one Troubleshooting copy in the real browser | result: pass | note: the first
  copy exactly equals the displayed Troubleshooting prompt, names the repository-local candidate for
  the mandatory contract check, forbids PATH fallback and reports copied only after success
- TC-3, TC-24 | stack: python+ui | command: `./web/onboarding/build.sh && python3 -m pytest -q
  tests/test_web_generator.py tests/test_release_surfaces.py && python3 tests/test_skill_docs.py` |
  result: pass | note: nine focused cases include final inline-JavaScript syntax parsing and exact
  candidate-binding assertions; release-surface and canonical Skill gates pass
- TC-14, TC-22, TC-24 | stack: python+shell | command: `make python-test` | result: pass | note: all
  maintained contract, release-candidate, site, schema, policy, stateless operation, KES, Upgrade and
  Deploy gates pass after the candidate-selection repair

## 16. Third Follow-up Execution Plan (append-only)

- [x] p6-1-fix3 replace opcert-only renewal with a genuine staged KES key rotation whose Phase A
  derives live period and paths without operator-supplied metadata, keeps the new signing key on the
  BP, and binds Phase B activation to that exact staged public key

## 17. Third Follow-up Test And Acceptance Criteria (append-only)

- TC-25 Genuine staged KES rotation: a read-only BP plan derives current KES period, active key/cert
  identity and a fixed private staging destination. Only an exact approved candidate may generate a
  new KES pair through a closed target command. Apply returns only the public verification-key
  envelope/hash and period; the staged signing key is mode `0600`, remains on the BP, and no active
  key, certificate, container identity or readiness state changes.
- TC-26 Bound activation: Phase B accepts only a cold-signed public `node.cert` whose hot KES key
  equals the exact staged key and whose counter/window/signature pass current live checks. The final
  candidate binds both staged-key and certificate identities. Approved activation backs up and
  atomically promotes the staged signing key plus certificate, restarts once and either verifies
  readiness or restores the prior pair; no key bytes enter control output, audit, model context or
  persistent target Ouro state.
- TC-27 Minimal operator input: the copied KES Prompt asks for no vkey path, period, output path or
  separate permission to write the inlined pool spec/script. The operator approves only the staged
  key candidate, executes the generated script on the air-gapped machine and returns public
  `node.cert`, then separately approves activation. Deterministic local paths and current period are
  mechanism-derived; incompatible layouts stop rather than falling back to manual secret handling.

## 18. Third Follow-up Execution Log (append-only)

- 2026-07-19 p6-1-fix3 started: operator acceptance exposed that the existing `KES Rotate` Skill
  requested vkey/period/path metadata and only installed an opcert for an externally supplied key.
  Cardano's KES lifetime requires a fresh key pair at rotation, so reusing the hot key embedded in
  the current opcert would optimize the wrong operation. The repair keeps cold signing offline while
  adding candidate-bound BP staging and paired key/certificate activation.
- 2026-07-19 p6-1-fix3 completed: the typed registry now separates non-disruptive
  `kes-rotation/stage-key` from disruptive `kes-rotation/install-opcert`. The first candidate derives
  the live KES period, generates a fresh pair only in the fixed BP-private stage and returns only
  public evidence. The second binds the cold-signed certificate to that staged key, backs up and
  promotes the active signing key, verification key and certificate as one recoverable operation,
  restarts once and verifies readiness. Skill v6 and the website prompt derive all paths/periods and
  pre-authorize only the deterministic local public handoff files; remote writes retain separate
  exact approvals.

## 19. Third Follow-up Validation Evidence (append-only)

- TC-25 | stack: rust+python | command: `cargo test -p ouro` and `python3
  tests/test_s0025_kes_rotation.py` | result: pass | note: 170 Rust tests plus a stateful fake-target
  flow prove fixed-path key generation, mode 0600, public-only output and active
  container/key/certificate invariance during Phase A
- TC-26 | stack: python | command: `python3 tests/test_s0020_kes_airgap_preflight.py` and `python3
  tests/test_s0025_kes_rotation.py` | result: pass | note: mock cold-signed node.cert is bound to the
  exact staged hot key; signature/counter/window preflight passes without mutation, then the fake
  target backs up/promotes the three-file set, restarts once, verifies the exact active key/cert and
  removes stage/rollback residue without exposing signing-key bytes
- TC-27 | stack: ui+python | command: local HTTP browser KES form plus `python3
  tests/test_skill_docs.py` and `python3 -m pytest -q tests/test_web_generator.py` | result: pass |
  note: copied prompt contains canonical Skill v6, stage-key and paired candidate CLI; it derives
  the current period and deterministic paths, explicitly authorizes local public files, contains no
  old vkey/period/output-path questions and produced zero browser console errors
- TC-14, TC-22, TC-25, TC-26, TC-27 | stack: rust+python+shell | command: `make python-test`,
  `cargo clippy -q -p ouro --lib --tests -- -D warnings`, `python3 -m pytest -q`, and `bash
  ci/l2-integration.sh` | result: pass | note: every maintained contract/stateless-operation/site
  gate, 13 pytest cases, clippy with warnings denied and complete L2 regression pass
- TC-13, TC-22 | stack: other | command: `make release-candidate` | result: pass | note: final
  source rebuilt the paired macOS control CLI and linux/x86_64 ephemeral runner; package, descriptor,
  candidate, version, release selection and checksums all verify

## 20. Fourth Follow-up Execution Plan (append-only)

- [x] p6-1-fix4 allow Phase A to stage a replacement KES pair when the unchanged existing
  KES/opcert is already invalid, while still refusing any active-state or availability regression

## 21. Fourth Follow-up Test And Acceptance Criteria (append-only)

- TC-28 Expired-KES recovery staging: when the BP is running, its socket supplies a typed current
  period, and its existing KES/opcert is already invalid, `kes-rotation/stage-key` may plan and stage
  a replacement pair. Phase A succeeds only if container identity, active KES key, active opcert and
  the complete pre-existing readiness evidence remain unchanged; it must not reinterpret an
  unchanged pre-existing forging failure as a regression. A real Phase-A-caused drift still cleans
  the staged pair and refuses. Phase B activation continues to require full BP forging readiness.

## 22. Fourth Follow-up Execution Log (append-only)

- 2026-07-19 p6-1-fix4 started: operator Phase A generated then cleaned the staged pair with
  `KES/opcert invalid (bp cannot forge)`. The stage postcondition incorrectly called the generic BP
  write-readiness evaluator, which requires the old certificate to be valid even though replacing
  an expired/invalid KES certificate is the purpose of this workflow. The fix will compare Phase A
  against its bound pre-state and reserve full forging readiness for activation.
- 2026-07-19 p6-1-fix4 completed: Phase A now requires an answering BP container/socket and compares
  post-stage availability plus KES validity/counter/period evidence against the candidate-bound
  pre-state without requiring the old certificate to forge. It reports the pre-existing
  KES/opcert/forging booleans as facts, preserves natural moving tip/peer evidence, and still cleans
  the staged pair on any active-state or readiness regression. Phase B retains the generic full BP
  forging-readiness gate after activation. Skill v7 explicitly directs agents to continue the
  offline handoff when old credentials were already invalid rather than demanding circular recovery.

## 23. Fourth Follow-up Validation Evidence (append-only)

- TC-28 | stack: python | command: `python3 tests/test_s0025_kes_rotation.py` | result: pass | note:
  the stateful target begins with `kes_opcert_valid=false`, `forging_credentials_ready=false` and
  typed KES `valid=false`; Phase A stages successfully with those pre-state facts unchanged, a
  deliberately induced Phase-A drift is still cleaned/refused, and Phase B succeeds only after the
  mock activation restores all forging readiness
- TC-27, TC-28 | stack: ui+python | command: local HTTP browser KES form plus `python3
  tests/test_skill_docs.py` and `python3 -m pytest -q tests/test_web_generator.py` | result: pass |
  note: copied prompt contains canonical Skill v7, explicitly permits already invalid/expired active
  credentials, labels them non-gating Phase-A facts, forbids circular recovery and emits zero browser
  console errors
- TC-14, TC-22, TC-28 | stack: rust+python+shell | command: `cargo test -q -p ouro`, `make
  python-test`, `cargo clippy -q -p ouro --lib --tests -- -D warnings`, `python3 -m pytest -q`, and
  `bash ci/l2-integration.sh` | result: pass | note: 170 Rust tests, all maintained Python gates, 13
  pytest cases, warnings-denied clippy and the complete L2 regression pass
- TC-13, TC-22 | stack: other | command: `make release-candidate` | result: pass | note: the fixed
  source rebuilt and verified the paired macOS control CLI, linux/x86_64 ephemeral runner, package,
  descriptor, candidate, version, release selection and checksums

## 24. Fifth Follow-up Requirement And Design (append-only)

Phase A currently hands the operator a public `kes.vkey` and a script that assumes `cardano-cli`
is already installed on the air-gapped machine. A fresh air-gapped environment has no reason to
meet that hidden dependency, so the handoff is not directly executable. Phase A must instead create
one platform-specific public bundle containing the script, vkey, an official Intersect
`cardano-cli` binary, a manifest and checksums. Ouro does not host or publish this dependency: the
online control machine downloads the exact official release asset on demand and verifies the
release checksum before writing the bundle.

The operator chooses the air-gapped device using one plain-language option: Apple-silicon Mac,
Intel Mac, Intel/AMD Linux, or ARM Linux. The Skill maps those choices to `aarch64-darwin`,
`x86_64-darwin`, `x86_64-linux`, or `aarch64-linux`; if the operator is unsure it requests only
`uname -s` and `uname -m`. The target `cardano-cli --version` is typed Phase-A evidence and selects
the matching official CLI release. The generated script resolves the bundled binary relative to
its own directory and verifies its recorded digest and version before reading or backing up the
cold counter. Windows and multi-platform bundles are out of scope.

## 25. Fifth Follow-up Execution Plan (append-only)

- [~] p6-1-fix5 produce a checksum-verified, platform-specific KES air-gap bundle that requires no
  preinstalled `cardano-cli`, and align the external Skill, website prompt and operating contract

## 26. Fifth Follow-up Test And Acceptance Criteria (append-only)

- TC-29 Platform-specific air-gap bundle: all four supported plain-language device choices map to
  the exact official Intersect release asset; unknown platform/version, download failure, malformed
  checksum, checksum mismatch, archive traversal, missing/multiple binaries or reported-version
  mismatch fail without a completed output directory. A successful bundle contains only public
  `kes.vkey`, `cold-sign.sh`, executable `cardano-cli`, `manifest.json` and `SHA256SUMS`, and uses an
  atomic directory promotion.
- TC-30 Offline direct execution: with networking unavailable and no system `cardano-cli`, the
  generated script selects its adjacent verified binary, checks the manifest-bound digest and
  version before reading the counter, invokes exactly one era-neutral `node issue-op-cert`, advances
  the in-place counter and emits only public `node.cert`. A tampered binary or manifest fails before
  the counter backup or signing command.
- TC-31 Prompt parity: the canonical KES Skill and locally served website prompt use friendly device
  names, accept natural device descriptions, fall back only to `uname -s` plus `uname -m`, invoke
  the typed bundle generator, and no longer instruct the agent to assemble a two-file handoff that
  assumes a preinstalled CLI.

## 27. Fifth Follow-up Execution Log (append-only)

- 2026-07-19 p6-1-fix5 started: operator accepted a single-platform bundle after real Phase-A use
  exposed the hidden cold-machine `cardano-cli` prerequisite. Implementation will preserve Ouro's
  two formal release artifacts and fetch the public signing dependency only while constructing the
  operator-selected handoff.

## 28. Fifth Follow-up Completion Status (append-only)

- [x] p6-1-fix5 produce a checksum-verified, platform-specific KES air-gap bundle that requires no
  preinstalled `cardano-cli`, and align the external Skill, website prompt and operating contract

## 29. Fifth Follow-up Completion Log (append-only)

- 2026-07-19 p6-1-fix5 completed: Phase A now binds the BP container's exact four-component
  `cardano-cli_version` into the typed candidate and returns it as public evidence. The local
  `kes airgap-bundle` command maps four plain-language device choices to official Intersect release
  assets, verifies the asset against the release checksum list, extracts exactly one expected
  executable and atomically emits the five-file public bundle. The cold script resolves only its
  adjacent executable and verifies manifest, vkey, binary and reported version before touching the
  cold counter. Skill v8 and all website locales request only a device description, with
  `uname -s`/`uname -m` as the sole fallback; no Ouro release artifact or image-hosting path was
  added.
- Cross-platform version validation is intentionally split at the executable boundary: the online
  controller executes `--version` before promotion when the selected binary matches its own host;
  for a foreign platform, official release identity plus checksum is verified online and the cold
  script executes and validates `--version` before counter backup or signing. This satisfies the
  no-mutation failure contract without attempting to execute a foreign binary.

## 30. Fifth Follow-up Validation Evidence (append-only)

- TC-29 | stack: rust+python+network | command: `python3 tests/test_kes_airgap_bundle.py` plus a
  real `cardano-cli-10.14.0.0-aarch64-darwin.tar.gz` bundle build from the official Intersect GitHub
  release | result: pass | note: all four friendly device choices select the exact release asset;
  closed platform/version, missing release, malformed/mismatched checksum, unsafe path,
  missing/duplicate executable and same-host version mismatch refuse with no output or partial
  directory. The real archive checksum was
  `2564ebea25ce7466f08e6fa15b339768438bd35d7674dffce2ff748d80662bb5` and the extracted executable
  checksum was `40d62fa37a2878a745702c24e29bd0319465afa6391ca412083ae4d9ceb1c6fa`.
- TC-30 | stack: python+shell | command: `python3 tests/test_kes_airgap_bundle.py` plus real bundle
  `cold-sign.sh` pre-key execution | result: pass | note: a mock cold machine with no PATH
  `cardano-cli` and no network used only the adjacent executable, invoked one `node issue-op-cert`,
  advanced the counter and emitted public `node.cert`; manifest or executable tampering stopped
  before backup/signing. The real Apple-silicon binary passed integrity/version validation and then
  stopped at the deliberately absent cold key before reading or backing up a counter.
- TC-31 | stack: ui+python | command: `./web/onboarding/build.sh`, local website KES prompt browser
  inspection, `python3 -m pytest -q tests/test_web_generator.py`, and `python3
  tests/test_skill_docs.py` | result: pass | note: copied prompt embeds canonical Skill v8, all four
  friendly device descriptions, the two-command uname fallback, typed version and bundle command;
  the legacy script-only canonical path is absent and the browser reported no console errors.
- TC-14, TC-22, TC-28, TC-29, TC-30, TC-31 | stack: rust+python+shell | command: `cargo test -q -p
  ouro`, `cargo clippy -q -p ouro --lib --tests -- -D warnings`, `make python-test`, `python3 -m
  pytest -q`, and `bash ci/l2-integration.sh` | result: pass | note: 172 Rust tests, all maintained
  Python contract gates, 13 pytest cases, warnings-denied clippy and complete L2 regression pass.
- TC-13, TC-22 | stack: other | command: `make release-candidate` | result: pass | note: the final
  source rebuilt and verified the paired macOS control CLI, linux/x86_64 ephemeral runner, package,
  descriptor, candidate, version, release selection and checksums without publishing.

## 31. Sixth Follow-up Requirement And Design (append-only)

After a successful Phase A, a fresh agent invocation currently runs `kes-rotation/stage-key` again
and is correctly refused because the fixed staging directory already exists. The refusal prevents
secret-key replacement but leaves no typed continuation path, so an otherwise valid rotation cannot
be resumed from the website prompt. Phase A planning must be idempotent across agent invocations:
when no staged directory exists it keeps the existing generate-and-approve behavior; when a complete
staged pair exists it returns the existing PUBLIC verification-key envelope/hash, current typed KES
period and container `cardano-cli_version` without mutation, confirmation or fleet permit. The Skill
must build/rebuild the local public bundle from that evidence and continue the offline handoff.

An existing but incomplete, unreadable or incorrectly permissioned staged pair remains a hard stop.
It must never be overwritten, auto-deleted or treated as resumable. Explicit destructive recovery is
out of scope for this repair and requires a separately approved typed operation if later needed.

## 32. Sixth Follow-up Execution Plan (append-only)

- [~] p6-1-fix6 make KES Phase A safely resumable from a complete existing staged pair and align
  the external Skill and website prompt

## 33. Sixth Follow-up Test And Acceptance Criteria (append-only)

- TC-32 Resumable Phase A: after an approved Phase A has staged a complete pair, a fresh invocation
  of the canonical website prompt obtains a successful `stage-key --plan` whose typed evidence
  identifies `resume_existing`, includes only the public staged vkey/hash plus current period and
  CLI version, declares no executor steps and requires no confirmation or fleet permit. The agent
  regenerates only the local public bundle and proceeds to the cold handoff; no target write command,
  new key generation, approval capability or raw SSH occurs. A missing stage preserves the existing
  approved generation flow, while an incomplete stage still refuses without cleanup or overwrite.

## 34. Sixth Follow-up Execution Log (append-only)

- 2026-07-19 p6-1-fix6 started: real manual acceptance restarted the website prompt after Phase A
  and hit `a staged KES rotation already exists`. The target guard behaved correctly, but the Skill
  had modeled Phase A as one-shot rather than a resumable transaction. The repair will expose only
  the already-public staged key through the existing typed plan boundary and leave all secret and
  destructive recovery behavior unchanged.

## 35. Sixth Follow-up Change Request (append-only)

- 2026-07-19 operator refinement: detecting a complete staged pair must not silently choose the
  continuation path. The agent must present the bound public key/hash and ask the operator to choose
  either continuing that pending rotation or discarding it and starting a new Phase A. Discard is a
  separate candidate-bound typed write requiring exact confirmation; it may not be implemented as
  raw SSH, implicit cleanup or delete-and-regenerate in one step. This supersedes section 31's
  statement that continuation occurs directly without operator decision.
- A successfully completed Phase B must leave no remote rotation transaction residue. Acceptance
  must verify removal of the fixed staging directory and all `.ouro-prev` key/certificate backups,
  while preserving the newly active triple and readiness. The deterministic temporary local public
  vkey must also be removed after bundle creation; the operator-owned bundle and returned public
  certificate are outputs, not hidden target state.

## 36. Sixth Follow-up Additional Acceptance Criteria (append-only)

- TC-33 Explicit pending-rotation decision: a complete staged pair makes the Skill stop for an
  operator choice before bundle creation. Continue performs no target mutation and uses the exact
  typed public pair. Restart first previews `kes-rotation/discard-stage`, waits for exact approval,
  applies only the candidate-bound stage removal, proves the active key/cert/container unchanged and
  stage absent, then begins a separate normal Phase A with a separate approval. Neither branch reads
  the signing key or uses raw SSH. An incomplete stage cannot be continued or discarded through the
  complete-stage decision path.
- TC-34 Successful cleanup: after a verified `install-opcert` activation, the fixed stage directory
  and all prior-key/opcert backup files are absent, the new KES key/certificate remains active and
  readiness passes. A subsequent `stage-key --plan` reports no pending rotation and returns the
  normal new-generation candidate. Skill instructions remove their temporary public-vkey input
  after bundle creation and do not confuse the operator-owned air-gap bundle with remote residue.

## 37. Sixth Follow-up Completion Status (append-only)

- [x] p6-1-fix6 make KES Phase A explicitly resumable or discardable from a complete existing
  staged pair, and prove successful activation leaves no transaction residue

## 38. Sixth Follow-up Completion Log (append-only)

- 2026-07-19 p6-1-fix6 completed: `stage-key --plan` now distinguishes an absent stage from one
  complete pending pair. A pending plan contains only its PUBLIC envelope/hash, current period and
  CLI version, has no executor steps and requires no confirmation, but Skill v10 stops for an
  explicit operator continue/discard decision. Continue builds the public bundle without target
  mutation. Discard uses the new BP-only `kes-rotation/discard-stage` typed write, binds the exact
  pending public-key hash, requires confirmation, removes only the fixed stage and proves active
  container/key/opcert unchanged. A new pair remains a separate candidate and approval.
- Successful activation now executes and verifies cleanup of the fixed stage plus all three
  `.ouro-prev` rollback files before reporting success. Its typed postcondition exposes both staging
  and rollback cleanup. The Skill removes its temporary local public-vkey input after bundle
  creation while preserving the operator-owned bundle and returned public certificate as explicit
  outputs. Incomplete or unsafe staging residue continues to refuse both continuation and the
  complete-stage discard path.

## 39. Sixth Follow-up Validation Evidence (append-only)

- TC-32, TC-33, TC-34 | stack: python+rust | command: `python3
  tests/test_s0025_kes_rotation.py` and `cargo test -q -p ouro` | result: pass | note: a stateful fake
  BP proves initial generation, fresh-agent pending detection with public-only evidence and zero
  executor steps, incomplete-stage refusal, explicit candidate-bound discard, stale-candidate
  refusal before deletion, separate regeneration approval, matched activation, verified residue
  removal and a subsequent clean new-generation plan; 173 Rust tests include the fixed four-path
  cleanup verifier.
- TC-33 | stack: ui+python | command: `./web/onboarding/build.sh`, `python3
  tests/test_skill_docs.py`, and `python3 -m pytest -q tests/test_web_generator.py` | result: pass |
  note: the locally built website embeds canonical Skill v10 and exact pending decision,
  `discard-stage`, no-silent-choice, cleanup and no-raw-SSH requirements; all nine generator cases
  pass.
- TC-14, TC-22, TC-32, TC-33, TC-34 | stack: rust+python+shell | command: `cargo clippy -q -p ouro
  --lib --tests -- -D warnings`, `make python-test`, `python3 -m pytest -q`, and `bash
  ci/l2-integration.sh` | result: pass | note: all maintained contract/operation/site gates, 13
  pytest cases, warnings-denied clippy and complete L2 regression pass.
- TC-13, TC-22 | stack: other | command: `make release-candidate` | result: pass | note: final source
  rebuilt and verified the paired macOS control CLI, linux/x86_64 ephemeral runner, package,
  descriptor, candidate, version, release selection and checksums without publishing. No real host
  was contacted and the operator's current pending staged pair was intentionally left untouched.

## 40. Seventh Follow-up Requirement And Design (append-only)

The air-gap return handoff must be one deterministic local transaction instead of an
operator-invented path. For BP `<bp>`, Ouro derives the public handoff directory from the directory
containing the selected pool spec as `ouro-kes-rotation/<bp>/pending`; its returned certificate is
always `<pending>/node.cert`. The operator copies the complete pending directory to the air-gapped
device, runs the adjacent `cold-sign.sh`, and copies only the resulting public `node.cert` back to
that exact pending directory. The operator therefore only needs to say that the certificate has
been returned; neither its bytes nor an arbitrary path are pasted into the model conversation.

The local bundle command must be safely resumable. Repeating it for the same staged public key,
period, platform and CLI version validates and reuses the complete existing bundle instead of
renaming it or producing a second directory. A mismatched, incomplete, tampered or unexpected
pending directory is refused and never overwritten. The returned public certificate is read by the
local Ouro process and streamed through the typed operation; agent-visible output remains limited
to its reference, hash and size.

The pending directory is transaction residue, not an operator archive. After a confirmed typed
discard proves the remote stage absent, or after activation proves the new key/certificate ready
and remote rollback residue absent, Ouro removes only the bound deterministic public directory.
Failed bundle generation, cold signing, preflight or activation preserves it for retry. Cleanup is
bound to the staged public-key hash, accepts only the fixed bundle files plus optional `node.cert`,
and refuses symlinks, nested directories or unknown files. No `.discarded-*` archive is created.

## 41. Seventh Follow-up Execution Plan (append-only)

- [~] p6-1-fix7 implement deterministic, resumable and lifecycle-clean KES public handoff; align
  the external Skill and locally generated website prompt

## 42. Seventh Follow-up Test And Acceptance Criteria (append-only)

- TC-35 Deterministic return: the canonical Skill derives exactly
  `<pool-spec-dir>/ouro-kes-rotation/<bp>/pending/node.cert`, never asks the operator to name a path
  or paste/attach certificate bytes, and Phase B previews and applies that same file through Ouro.
- TC-36 Resumable bundle: a repeated bundle command with identical public inputs reports reuse and
  validates every fixed bundle file; conflicting inputs, modification, missing files, symlinks,
  nested entries and unknown entries refuse without overwrite or deletion.
- TC-37 Bound local cleanup: after typed discard success or verified activation success, local
  cleanup removes only a pending bundle whose manifest public-key hash matches the operation. A
  failure before those postconditions preserves it, and cleanup never creates or preserves a
  `.discarded-*` directory.

## 43. Seventh Follow-up Completion Status And Log (append-only)

- [x] p6-1-fix7 implement deterministic, resumable and lifecycle-clean KES public handoff; align
  the external Skill and locally generated website prompt
- 2026-07-19 p6-1-fix7 completed: `kes airgap-bundle --spec <pool-spec> --node <bp>` now derives the
  single absolute `<pool-spec-dir>/ouro-kes-rotation/<bp>/pending` path, reports its fixed
  `node.cert` return path, and validates/reuses a matching complete bundle. Manifest schema 2 binds
  the generation time needed to reconstruct and byte-check the signing script; reuse additionally
  checks all fixed entries, the exact public vkey, executable provenance/digest and checksum file.
- The new local `kes airgap-cleanup` command loads and validates the same spec/BP path, requires the
  exact staged public-key hash and refuses symlinks, nested or unknown entries before removing the
  public transaction directory. Skill v11 invokes it only after typed remote discard proof or
  verified activation success. Failures retain the handoff for retry and no discarded archive is
  created.
- Skill v11 and the locally built site now tell the operator to return only public `node.cert` to
  the fixed local path and reply that it is ready. The agent never requests a path, attachment or
  certificate bytes; local preview/preflight/apply use that same path and expose only metadata.

## 44. Seventh Follow-up Validation Evidence (append-only)

- TC-35, TC-36, TC-37 | stack: rust+python | command: `python3
  tests/test_kes_airgap_bundle.py`, `cargo test -q -p ouro`, and `cargo clippy -q -p ouro --lib
  --tests -- -D warnings` | result: pass | note: four platform bundles, canonical path creation,
  byte-validated idempotent reuse, fixed certificate return, missing/unknown/nested/symlink/content
  refusal, wrong-hash cleanup refusal, permitted public-cert cleanup and absent-idempotence all pass;
  173 Rust tests and warnings-denied clippy pass.
- TC-35 | stack: ui+python | command: `./web/onboarding/build.sh`, `python3
  tests/test_skill_docs.py`, and `python3 -m pytest -q tests/test_web_generator.py` | result: pass |
  note: locally built website embeds exact canonical Skill v11, deterministic return and cleanup
  commands, and contains no operator-named opcert path; all nine generator/local-service cases pass.
- TC-14, TC-22, TC-35, TC-36, TC-37 | stack: rust+python+shell | command: `make python-test`,
  `python3 -m pytest -q`, and `bash ci/l2-integration.sh` | result: pass | note: all maintained
  contract, operation, site and release-boundary gates plus 13 pytest cases and full L2 pass.
- TC-13, TC-22 | stack: other | command: `make release-candidate` | result: pass | note: the source
  rebuilt and verified the paired macOS control CLI, linux/x86_64 ephemeral runner, package,
  descriptors, release selection and checksums without publishing or contacting a real host.

## 45. Eighth Follow-up Requirement And Design (append-only)

The operator is now entering real production acceptance. Historical TC-19 deliberately stopped
after KES Phase-B preflight to bound an earlier simulation, but that acceptance boundary must not be
interpreted by a fresh agent as a runtime restriction. The canonical KES Skill must state plainly
that, after a real returned certificate passes preflight and the operator gives exact approval, the
next invocation performs the actual BP mutation: backup, staged KES key promotion, public opcert
installation, container restart, readiness verification, rollback-residue cleanup and local public
handoff cleanup. It must not stop merely because activation is consequential or because an older
test used a mock certificate.

No mechanism change is required: the typed `kes-rotation/install-opcert` executor already performs
that production transaction and rolls back the prior active triple when commit or readiness fails.
Artifact preflight, candidate-bound confirmation, a last-minted fleet permit, live-state recheck,
rollback and typed postconditions remain mandatory production controls; they are not simulation
restrictions and must not be removed.

## 46. Eighth Follow-up Execution Plan And Acceptance (append-only)

- [~] p6-1-fix8 remove the historical simulation ambiguity from the production KES Skill and site
- TC-38 Production KES commit: the canonical website prompt explicitly says the historical
  stop-before-activation boundary does not apply, identifies the real mutation/restart/rollback
  behavior after exact approval, and still requires preflight, confirmation, last-minted fleet
  permit and typed success proof. The production CLI continues to expose and execute
  `kes-rotation/install-opcert`; no mock flag or test fixture is part of its runtime decision.

## 47. Eighth Follow-up Completion And Evidence (append-only)

- [x] p6-1-fix8 remove the historical simulation ambiguity from the production KES Skill and site
- 2026-07-19 p6-1-fix8 completed: code inspection confirmed the stateless production executor
  already backs up the active KES signing key, verification key and opcert, promotes the staged
  pair plus candidate-bound public certificate, performs a real container restart, verifies the
  active vkey/opcert and readiness, rolls back on failure, and removes transaction residue on
  success. There is no runtime mock or stop-before-activation switch to remove.
- TC-38 | stack: ui+python | command: `./web/onboarding/build.sh`, `python3
  tests/test_skill_docs.py`, `python3 -m pytest -q tests/test_web_generator.py`, and `make
  python-test` | result: pass | note: Skill v12 and the locally built site explicitly distinguish
  the historical preflight-only acceptance boundary from the real production commit, disclose the
  active-triple replacement and BP restart, forbid mock substitution, and retain every production
  safety gate; nine website cases and all maintained Python contract gates pass.
- TC-38 | stack: other | command: production executor inspection plus
  `target/release-candidate-control/release/ouro-ops kes --help` | result: pass | note: the release
  candidate exposes the real staged-key → deterministic handoff → install-opcert workflow; website
  prompt contains confirmation, last-minted fleet permit and actual-activation postconditions and
  contains no mock or stop-before-activation instruction.

## 48. Ninth Follow-up Requirement And Design (append-only)

Production KES artifact preflight against `cardano-cli 10.14.0.0` returned a valid, parseable
`kes-period-info` record whose `qKesNodeStateOperationalCertificateNumber` was JSON `null`. Ouro
currently collapses absent, null and malformed values into `omitted`, refusing a valid
`OpCertNoBlocksMintedYet` protocol state. The probe independently calls `int(null)`, discards the
entire KES evidence object and would make post-activation generic readiness fail and roll back even
after a corrected preflight.

Ouro must parse the node-state counter as a closed evidence type: unsigned integer `Present(n)`, or
JSON null `NoBlocksMintedYet`. An absent key is a cardano-cli schema incompatibility; any other type,
negative number or out-of-range number is malformed output. Present-counter validation retains the
existing candidate/on-disk/window and `node_state <= on_disk <= node_state + 1` contract.

The null path must remain fail-closed through an independent public pool identity binding. The typed
operation reads only the fixed, public active `/opt/cardano/config/keys/node.cert` through a bounded
container read, parses and verifies its cold signature, and requires the candidate and active
certificates to have the same cold verification key, candidate counter strictly greater than the
active certificate counter, candidate counter equal to the on-disk counter, valid KES window and
candidate hot key equal to the staged KES key. `pool_binding.pool_id` is not a Cardano pool ID and
must not be used for this check. Missing, malformed or signature-invalid active opcert remains a
refusal.

The probe must preserve null as `opcert_counter_node_state: null` and
`counter_status: no_blocks_minted_yet`, while keeping ordinary `kes_opcert_valid` and
`forging_credentials_ready` fail-closed. Only `install-opcert` postcondition may combine its
retained candidate-bound null-path preflight evidence with the installed artifact digest, activated
staged vkey, fixed credential files, container/socket/network/genesis/tip evidence and period/on-disk
facts to satisfy activation readiness. Any mismatch must execute the existing rollback.

Recovery constraints: do not contact the production BP during implementation; do not re-run cold
signing, advance the counter, regenerate/stage keys or alter the operator's current `node.cert`.
After rebuilding, the operator re-plans and preflights that same returned file and staged pair, then
gives a fresh activation approval only after the complete preflight passes.

## 49. Ninth Follow-up Execution Plan And Acceptance (append-only)

- [~] p6-1-fix9 model no-blocks-minted KES counter evidence with cold-identity binding and scoped
  activation readiness
- TC-39 KES counter parsing and preflight: integer node-state retains current behavior; absent is a
  schema-incompatible refusal; null plus matching verified active cold key and strictly advanced
  counter passes; wrong cold key, non-advanced counter, invalid active signature and malformed
  values refuse. Every preflight remains `changed:false`, executor-free and consumes neither
  confirmation nor permit.
- TC-40 Probe and activation: probe preserves `no_blocks_minted_yet` and the remaining KES facts
  without globally declaring the BP ready. Candidate-bound install can verify and accept a post-state
  that remains null when artifact/vkey/cold identity/counter/window and ordinary BP readiness all
  match. Artifact, key, digest, credential or readiness mismatch still invokes rollback.

## 50. Ninth Follow-up Completion And Evidence (append-only)

- [x] p6-1-fix9 model no-blocks-minted KES counter evidence with cold-identity binding and scoped
  activation readiness
- 2026-07-19 p6-1-fix9 completed: the typed KES validator now distinguishes integer, null, absent
  and malformed node-state counter records. Null reads and verifies only the fixed public active
  opcert, binds its exact observed digest and cold key, and requires candidate counter monotonicity;
  present integers retain the original protocol-state range checks. The synthetic Ouro fleet
  `pool_id` is not consulted.
- The probe now emits `counter_status`, `period_valid` and preserved null counter evidence. It keeps
  ordinary KES/forging readiness false for no-blocks-minted. Only the candidate-bound install
  postcondition may combine its retained preflight evidence with exact installed digest/vkey,
  credential/layout and live node facts. Failed scoped readiness restores the original active
  triple and verifies rollback against the bound pre-state rather than requiring it to become newly
  ready.
- Skill v13 and the locally built website prompt explicitly accept the null status only with the
  typed cold-identity/active-counter evidence and forbid null-as-zero or fleet-ID substitution.
- TC-39 | stack: rust+python | command: `python3
  tests/test_s0020_kes_airgap_preflight.py`, `cargo test -q -p ouro`, and `cargo clippy -q -p ouro
  --lib --tests -- -D warnings` | result: pass | note: integer path, valid matching null, absent,
  malformed, wrong cold key, equal counter, invalid active signature, digest binding and
  capability-free preflight assertions pass; 173 Rust tests and warnings-denied clippy pass.
- TC-40 | stack: python+shell | command: `python3 tests/test_probe.py`, `python3
  tests/test_s0020_stateless_apply.py`, and `python3 tests/test_s0025_kes_rotation.py` | result: pass |
  note: probe preserves no-blocks-minted without global readiness, null-path production activation
  succeeds when fully bound, and a deliberate post-mutation readiness mismatch performs and proves
  live-state rollback before an independent success case.
- TC-39, TC-40 | stack: ui+python | command: `./web/onboarding/build.sh`, `python3
  tests/test_skill_docs.py`, `python3 -m pytest -q tests/test_web_generator.py`, `make python-test`,
  `python3 -m pytest -q`, and `bash ci/l2-integration.sh` | result: pass | note: canonical Skill v13,
  all maintained operation/site gates, nine website cases, 13 pytest cases and full L2 pass.
- TC-13, TC-22, TC-39, TC-40 | stack: other | command: `make release-candidate` | result: pass |
  note: production control CLI and linux/x86_64 runner rebuilt and all package, descriptor, release
  selection and checksum evidence verified without contacting the real BP or changing the retained
  staged pair, node.cert or counter.
