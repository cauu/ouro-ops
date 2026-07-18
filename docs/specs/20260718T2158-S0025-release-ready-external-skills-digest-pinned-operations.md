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
- [ ] p2-2 remove `SKILL.md` build inputs and `skill list/show`; isolate only runner/probe/schema/
  policy execution assets and generic version/hash utilities
- [ ] p2-3 remove the public legacy `tool run`/script path and obsolete-only parity gates when p1-1
  finds no production consumer; if one exists, obtain and implement the user's explicit disposition
  before completing this item
- [ ] p3-1 add the pure `contract check` preflight and update all six Skills to invoke it exactly once
  before every other CLI, credential, network, SSH, or mutating action
- [ ] p3-2 remove decision/Skill hashes and Skill-derived floors from manifest, identity, packaging,
  and self-description; implement and consume only the compact CLI contract/runner descriptor
- [ ] p4-1 extend release-catalog schema and validation with the signed Blink Labs GHCR repository and
  exact OCI identity tuple; update release selection and transition validation
- [ ] p4-2 replace archive preload with candidate-bound exact-digest GHCR pull and post-pull config/
  platform/repository verification; remove image-tar/artifact-file inputs and leave active containers
  untouched
- [ ] p4-3 regenerate and sign the release catalog with the existing local signer, publish it at the
  canonical live URL, and make online pinned-key verification the Upgrade acceptance path
- [ ] p5-1 implement a release-candidate build/check that pairs the macOS CLI with its Linux/x86_64
  runner, verifies descriptor/package/checksums and smoke behavior, and excludes Skill text and image
  archives without publishing a formal CLI release
- [ ] p5-2 update current README, operation, threat, release, website, and CI documentation/gates to
  the external-decision, exact-upstream-pull, live-catalog, and deferred-publication boundaries
- [ ] p6-1 run all automated regression gates and fresh-agent end-to-end acceptance for all six Skills
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

## 7. Change Requests (append-only)

- 2026-07-18 replaces S0024. The operator requires release-ready code and complete functional
  acceptance now, defers formal CLI publication and production-site acceptance, requires Ouro to
  never host image archives and to pull exact signed digests only from Blink Labs GHCR, and authorizes
  publication/online acceptance of the signed release catalog in this spec.
