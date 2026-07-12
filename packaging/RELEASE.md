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

## Security boundary — bootstrap credential (S0017 P0-1, convenience mode)

`ouro-ops init` provisions a target using a privileged bootstrap credential (an SSH key + sudo).
That credential is **NOT mechanism-isolated from the agent**: `ouro-ops init` is an ordinary
privileged command with no TTY / out-of-band-authorization gate, and the credential may sit in the
same control-machine credential store as per-operation credentials. A poisoned prompt could
therefore invoke `init` via the agent and use that credential to perform privileged provisioning on
any reachable host. This residual risk is **known and accepted** (P0-1): the tool relies on upstream
control-machine / agent-runtime security for it, not on an in-tool boundary.

Do NOT claim in any documentation that the bootstrap credential is isolated from the agent — that
would be a false statement. What the tool DOES guarantee is narrower and unrelated to isolation:
the bootstrap key path/content never enters `ouro-ops` JSON / audit / `confirm preview` / the
secret-fingerprint corpus (no accidental leakage to output/logs), and per-operation dispatch still
runs only through the confined `ouro-exec` wrapper.

## Release process (per version)

1. **Reproducible build** of the single static `ouro` binary (skills embedded, p2-1).
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
