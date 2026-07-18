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

1. **Controlled paired build** of the control `ouro-ops` binary (no Skills) and its matching static
   Linux/x86_64 runner. `ouro-ops contract` reports the CLI version/contract and the digest of the
   exact embedded runner; release checks compare that digest with the paired artifact.
2. **Sign** the binary and release checksums with the hardware-held key (cosign keyless +
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
- `ouro-ops self-update --check` — version/update reporting, no unverified apply (p2-3, partial).
- `packaging/` — SIGNING_IDENTITY, Homebrew formula, install.sh (verification logic).

## What is INFRA (not in-repo)

Signing key custody, published tap/npm/channel, Rekor entries, the network fetch + in-place
swap. These are wired at release time; the repo carries the verification code they plug into.
