# S0025 p1-1 — Product Surface And Retirement Inventory

Date: 2026-07-18

This inventory classifies current product consumers before S0025 changes behavior. Historical specs,
tests, fixtures, comments, and deprecated help are evidence of old architecture, not reasons to keep
it. The supported product is the six-operation website plus the typed CLI journeys named below.

## Public journey map

| Website operation | Canonical decision source | Current typed mechanism | S0025 disposition |
| --- | --- | --- | --- |
| Observability | `ouro-skills/observability/SKILL.md` | `op run --op observability/health` | Publish the complete Skill on the site; retain typed stateless read |
| Troubleshooting | `ouro-skills/troubleshooting/SKILL.md` | `op run --op troubleshooting/snapshot`, bounded `diag exec` | Publish the complete Skill; retain typed snapshot and diagnostic transport |
| Runtime | `ouro-skills/runtime/SKILL.md` | `op run --op runtime/restart`, confirmation, fleet permit | Publish the complete Skill; retain candidate-bound restart |
| Upgrade | `ouro-skills/upgrade/SKILL.md` | `release select`, `upgrade/preload-image`, `upgrade/step` | Replace archive preload with signed exact-digest GHCR preparation; retain separate activation gate |
| KES Rotate | `ouro-skills/kes-rotation/SKILL.md` | cold-sign script, opcert preview/preflight, `op run` install | Publish the complete Skill; retain public-artifact air-gap boundary |
| Deploy | `ouro-skills/deploy/SKILL.md` | transaction preview and `op run --op deploy/register-submit` | Publish the complete Skill; retain one-shot candidate-bound submit |

`config`, `detect`, `onboard`, `adopt`, `init`, image preparation, and Upgrade activation are not
separate public website Skills. Required helpers remain CLI mechanisms only when a current typed
journey consumes them.

## Legacy decision and script surfaces

| Surface | Current reachability | Named current production consumer | Disposition |
| --- | --- | --- | --- |
| `ouro-ops skill list/show` | Public CLI route, help, README, website prompt and trust copy | None after the website carries the complete six canonical Skills | Remove CLI route/help and all positive guidance |
| Embedded `SKILL.md` bytes | `build.rs` includes every Skill document | Only `skill show`, decision hashes, and Skill-derived version floor | Remove from CLI build inputs and package identity |
| `ouro-ops tool run` | Public CLI route; old remote dispatcher and resident-target executor | None of the six current canonical Skills; they use `op run`/`diag exec` | Remove public route, dispatcher, resolver, target wrapper dependency, and obsolete gates |
| `ouro-ops tool verify-context` | Called only by legacy embedded shell scripts | No typed ephemeral-runner operation | Remove with legacy executor |
| Embedded `*/scripts/*.sh` and shared legacy shell libraries | Materialized by `tool run`; old tests/fixtures execute them | None of the six current typed journeys | Remove from CLI build inputs; keep only the standalone target probe that `s0019_cli` consumes |
| `manifest show/verify` decision/script hash taxonomy | Describes embedded Skills/scripts | No external release consumer; current JSON file is repository-local legacy packaging | Replace with compact CLI contract and embedded-runner descriptor |
| `skills::required_ouro()` and version gate floor derived from Skill front matter | Legacy `tool run` anti-rollback and `self-update --check` | No current typed operation requires a mutable Skill-derived binary floor | Remove Skill-derived floor; `contract check` becomes the pure compatibility boundary |
| `parity::SecurityIdentity` contribution from embedded Skill digest | Old resident `tool run` control/target parity | No ephemeral-runner typed operation | Remove Skill digest input; preserve identity only if another current mechanism consumes it |
| S0017 `/usr/local/sbin/ouro-tool-run` wrapper and resident-binary provisioning | Old `init`/fixture path | No six-operation flow; S0019 onboarding installs `ouro-op-run`, and S0020 transports a one-shot runner | Retire product-facing dependency; historical fixtures/docs are updated or deleted where they gate current CI |

Conclusion: there is no named current production or recovery consumer that justifies retaining the
legacy `tool run` executor. S0025 p2-3 may remove it without a new product decision. Shared Rust
confirmation, audit, transaction, SSH quoting, probe parsing, schema, and typed executor logic are
not legacy merely because old comments mention `tool run`; they are retained when current typed
operations consume them.

## Website delivery inventory

The current page generates a short bootstrap prompt and directs the agent to `skill show`. It does
not build from the six canonical `SKILL.md` files, validate their front matter, or include their full
content. Trust copy in four locales asserts that decision text is embedded in the binary. The build
script copies one handwritten HTML file and has no source-fidelity or safe-serialization stage.

S0025 will replace this with an explicit six-entry build mapping, inert serialized Skill data,
front-matter validation, a complete copied prompt, and local HTTP acceptance of the release-form
site. The website owns decision text; the CLI constrains only typed operations.

## Release identity inventory

The current CLI identity/package model includes:

- `decision_hash`, `skills_hash`, `schema_hash`, and an aggregate `embedded_digest`;
- per-file hashes for decision documents and old shell scripts;
- a `required_ouro` floor parsed from embedded Skill front matter;
- the embedded Skill digest in `SecurityIdentity`;
- `packaging/bundle-manifest.json` containing the same legacy taxonomy.

The Linux/x86_64 ephemeral runner is separately embedded by `runner.rs` and is a real current
mechanism asset. S0025 keeps that runner and the target probe, and replaces the old taxonomy with the
compact CLI version/contract/runner-platform/runner-digest descriptor required by release checks.

## Image and signed-release inventory

Current signed entries bind OCI index, platform manifest, config digest, platform, convention and
allowed transitions, but the signed contract does not bind the repository name. Upgrade currently:

1. selects a signed target config digest;
2. asks the operator for a Docker-save archive;
3. previews/hashes it through `inbox preview --type image`;
4. binds an artifact reference into `upgrade/preload-image`;
5. streams archive bytes to the ephemeral runner and invokes `docker load`;
6. later plans `upgrade/step` by config digest.

Positive archive/image-load behavior exists in the Upgrade Skill, `s0019_cli`, target-runner
dispatch, test fixtures, and current documentation. This path is a current product consumer, but its
transport contract is explicitly replaced—not preserved—by S0025.

The replacement owns these decisions:

- the signed catalog names exactly `ghcr.io/blinklabs-io/cardano-node`;
- selection yields `repository@platform_manifest_digest` plus expected config/platform;
- plan performs no pull;
- confirmed apply runs only the fixed exact-digest pull and inspection executor;
- post-pull repository, manifest, platform, and config identity must match signed data;
- the active container is never stopped, recreated, retagged, or switched during preparation;
- `inbox preview --type image`, image `--artifact-file`, archive hashing/binding, and `docker load`
  are removed from the public Upgrade contract.

## Acceptance and publication boundary inventory

- The canonical release URL is already compiled as
  `https://raw.githubusercontent.com/cauu/ouro-ops/refs/heads/main/data/releases.json`; S0025 must
  publish and accept the newly signed document there.
- `data/releases.json` signing uses the existing Keychain-backed local signer. CI uses only the
  pinned public key and must not gain a private key or 1Password dependency.
- The current website has a deterministic static build but not a canonical-Skill generator or local
  browser acceptance harness.
- Existing release files describe published bundles containing Skills. S0025 produces and validates
  an unpublished paired CLI candidate and defers formal CLI/GitHub/install-channel publication.
- Production website/Cloudflare deployment is not required for S0025; local HTTP execution is.

## Required removal versus preservation summary

Remove or redefine in S0025:

- public `skill` and `tool` command families;
- embedded decision documents and legacy shell-script pack;
- Skill/script hashes and Skill-derived compatibility floors;
- website bootstrap through `skill show`;
- Docker archive/image `artifact-file` preparation and `docker load`;
- release documentation that claims Ouro hosts/bundles Skills or image bytes.

Preserve:

- the six canonical external Skill files;
- typed `op run`, `diag exec`, `release select`, confirmation, fleet permit and artifact contracts
  still used by KES/Deploy;
- Linux/x86_64 ephemeral runner, target probe, schemas, signed policy/key inputs, candidate binding,
  target revalidation, audit and fixed executors;
- exact live signed catalog verification with no insecure fallback.
