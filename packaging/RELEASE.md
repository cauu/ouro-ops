# Ouro CLI release boundary

This documents the distribution mechanism S0016 specifies. The **code + verification logic +
packaging artifacts** live in this repo; the parts that need external infrastructure (a
hardware-held CLI signing key, published Homebrew tap / package, a hosted CLI release channel, a
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
deadline, and removes it. Public KES opcert or Deploy transaction artifacts may be appended to that
same one-shot stream. Node images never are: approved Upgrade preparation makes the target Docker
daemon pull the signed exact Blink Labs GHCR platform-manifest digest directly.

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

## Current release standard

Run `make release-candidate` on macOS. It performs locked paired builds, embeds the exact static
Linux/x86_64 runner into the native control CLI, verifies the compact descriptor against the runner
bytes, checks the live signed release catalog, packages only `ouro-ops`, validates SHA256SUMS and
refuses Skill Markdown or node-image payloads. The resulting manifest says
`release-standard-not-published`.

Formal CLI signing and publication are deliberately deferred to the next spec. The current GitHub
workflow runs this release-standard validation but creates no GitHub Release, tag publication,
Homebrew update or hosted CLI artifact.

For S0025 local website acceptance, invoke the built control candidate explicitly as
`./target/release-candidate-control/release/ouro-ops`. `make release-candidate` validates and packages
the candidate but intentionally neither installs it nor replaces an older `ouro-ops` on `PATH`.

The signed node-image release catalog is a different public artifact. It is served at the fixed
`data/releases.json` HTTPS source, signed by Ouro's pinned Ed25519 catalog key, and authorizes only
exact `ghcr.io/blinklabs-io/cardano-node` OCI tuples, one recommended target per platform, and
optional exact-pair rollback declarations. It never hosts image layers.

## Formal release process (next spec)

1. **Controlled paired build** of the control `ouro-ops` binary (no Skills) and its matching static
   Linux/x86_64 runner. `ouro-ops contract` reports the CLI version/contract and the digest of the
   exact embedded runner; release checks compare that digest with the paired artifact.
2. **Sign** the binary and release checksums with the hardware-held key (cosign keyless +
   minisign for the offline fallback). *(INFRA: signing key.)*
3. **Publish** the signature + a Rekor transparency entry, the tarballs, the Homebrew formula
   (`packaging/homebrew/ouro.rb`, sha filled), and the npm wrapper. *(INFRA: publish.)*
4. **Signed release metadata** (`latest_version`, sequence number, timestamp/expiry,
   revocation/security floor) is published to the stable channel. *(INFRA: channel.)*

## Future formal first install (not available yet)

The next spec must replace every placeholder signing identity/URL and activate one official install
vector whose package verifies the signature before trusting the binary. After installation,
`ouro-ops version` and `ouro-ops contract` must be cross-checked against the official site. No
current Homebrew, npm or install-script placeholder is a supported production channel.

## Self-update (steady state)

`ouro-ops self-update --check [--against <signed-metadata>]` (implemented) reports the running
version and — given release metadata — whether a strictly newer version exists (it never flags a
downgrade).

The apply path (INFRA-gated) MUST, before swapping the binary:
- verify the new release's signature against the pinned identity AND its Rekor entry;
- enforce signed distribution rollback/revocation policy and never install below the current
  version;
- require the signed metadata to be **fresh** (unexpired) for write operations; fail closed
  past expiry except for an explicitly verified offline bundle;
- refuse versions revoked by verified release metadata.

## Offline fallback

Near-offline BPs: a signed offline bundle carries its own minisign public key / Sigstore
bundle so it is verifiable WITHOUT the network; `ouro-ops install --offline <bundle>` (INFRA)
runs the SAME signature, revocation, and no-downgrade checks. Never skip verification because the
network is unavailable.

## What is runnable in-repo today

- `ouro-ops contract` — compact CLI contract and paired-runner descriptor.
- `make release-candidate` — complete paired build/package/check without publication.
- `ouro-ops self-update --check` — version/update reporting, no unverified apply (p2-3, partial).
- `packaging/` — SIGNING_IDENTITY, Homebrew formula, install.sh (verification logic).

## What is INFRA (not in-repo)

Signing key custody, published tap/npm/channel, Rekor entries, the network fetch + in-place
swap. These are wired at release time; the repo carries the verification code they plug into.
