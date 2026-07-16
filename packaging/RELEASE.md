# S0016 — Release, Signing & Self-Update (p2-2/p2-3/p2-5)

This documents the distribution mechanism S0016 specifies. The **code + verification logic +
packaging artifacts** live in this repo; the parts that need external infrastructure (a
hardware-held signing key, published Homebrew tap / npm package, a hosted release channel, a
Sigstore/Rekor transparency entry) are called out explicitly as **INFRA** — they are not, and
should not be, runnable from the repo.

## Trust anchors (pinned, repo-controlled)

`packaging/SIGNING_IDENTITY` pins the cosign identity, minisign public key, Rekor URL, and the
**single** official install vectors. The installer verifies against these automatically; the
official site shows the same values for an independent cross-check (≥2 channels). This is what
defeats first-install typosquat / fake-package / bootstrap-key substitution (R2 N4).

## Current target boundary (S0020)

Ordinary non-deploy operations do not provision or update target-resident Ouro software. The
macOS/control release carries its matching static Linux/x86_64 runner; each invocation sends it to a
run-unique private directory through the operator's existing `cardano` SSH credential, verifies the
control-known digest, executes a closed `target` action with a clean environment, bounds output and
deadline, and removes it. Public artifacts are optionally appended to that same one-shot stream.

This design deliberately targets an honest-but-fallible or misled agent using the Ouro command
surface. The existing operator credential is **NOT mechanism-isolated from the agent** when it has
the control terminal; preventing deliberate raw SSH bypass is deferred. Do not claim otherwise.
Mechanically enforced properties are narrower: the agent cannot select runner bytes/path/hash or
sudo argv through public operation flags; host keys are pinned; target commands have a closed
grammar; confirmation/permit checks remain local and exact-intent-bound; key contents do not enter
Ouro JSON/audit output.

Legacy `init`/`onboard`/`adopt` remain for explicit S0017/S0019 migration or recovery only. They are
not ordinary-operation prerequisites and must never be suggested merely because a target lacks an
Ouro binary, adoption metadata, or a matching remote version.

## Release process (per version)

1. **Reproducible paired build** of the control `ouro-ops` binary (Skills embedded) and its matching
   static Linux/x86_64 runner. The control artifact embeds or release-binds that exact runner.
   `ouro-ops manifest show` is captured and committed as `packaging/bundle-manifest.json`; a CI
   test (`skills::committed_manifest_matches_embedded`) fails the build on any drift.
2. **Sign** the binary + the bundle manifest with the hardware-held key (cosign keyless +
   minisign for the offline fallback). *(INFRA: signing key.)*
3. **Publish** the signature + a Rekor transparency entry, the tarballs, the Homebrew formula
   (`packaging/homebrew/ouro.rb`, sha filled), and the npm wrapper. *(INFRA: publish.)*
4. **Signed release metadata** (`latest_version`, sequence number, timestamp/expiry,
   revocation/security floor) is published to the stable channel. *(INFRA: channel.)*

## First install (once, deliberate)

Primary: `brew install ouro/tap/ouro` (macOS). Secondary: `npx @ouro/cli@<pinned>`. Both
verify the signature against the pinned identity before trusting the binary. After install,
`ouro-ops version` + `ouro-ops contract` are cross-checked against the official site.

## Self-update (steady state)

`ouro-ops self-update --check [--against <signed-metadata>]` (implemented) reports the running
version, the built-in required floor, and — given release metadata — whether a strictly newer
version exists (it never flags a downgrade).

The apply path (INFRA-gated) MUST, before swapping the binary:
- verify the new release's signature against the pinned identity AND its Rekor entry;
- enforce **monotonic anti-rollback** (never install below the current version — see
  `version.rs`, the same tamper-evident floor the run-time gate uses, R2 N1);
- require the signed metadata to be **fresh** (unexpired) for write operations; fail closed
  past expiry except for an explicitly verified offline bundle;
- compute the effective floor as `max(prompt_min, embedded_floor, security/revocation floor)`
  and refuse known-vulnerable versions (denylist).

## Offline fallback

Near-offline BPs: a signed offline bundle carries its own minisign public key / Sigstore
bundle so it is verifiable WITHOUT the network; `ouro-ops install --offline <bundle>` (INFRA)
runs the SAME verify + denylist + `max(floor)` checks. Never skip verification because the
network is unavailable.

## What is runnable in-repo today

- `ouro-ops manifest show|verify` — bundle integrity, drift/tamper gate (p2-6).
- `ouro-ops self-update --check` — version/floor reporting, no unverified apply (p2-3, partial).
- `version.rs` gate + tamper-evident monotonic anti-rollback (p3-2/3, R2 N1/P0-2).
- `packaging/` — SIGNING_IDENTITY, Homebrew formula, install.sh (verification logic).

## What is INFRA (not in-repo)

Signing key custody, published tap/npm/channel, Rekor entries, the network fetch + in-place
swap. These are wired at release time; the repo carries the verification code they plug into.
