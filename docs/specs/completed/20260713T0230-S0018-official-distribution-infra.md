# Official Distribution, Signing & Self-Update INFRA

Spec-ID: S0018
Status: completed
Created Time: 2026-07-13T02:30:00+08:00
Start Time:
Completion Time: 2026-07-24T11:05:03+08:00
Previous Spec-ID: S0017
Closure Reason: replaced

## 1. Requirement Details

### Background
S0016 built the **verification logic** for a signed, single-binary distribution (`ouro-ops manifest
verify`, `self-update --check`, `version.rs` monotonic anti-rollback, `packaging/SIGNING_IDENTITY`,
`packaging/install.sh`, the Homebrew formula) but **deliberately left the release INFRA out of the
repo** — the hardware-held signing key, published tap / npm / release channel, Rekor transparency
entries, and the network fetch + in-place swap are all marked INFRA in `packaging/RELEASE.md`.

Today the binary is installable ONLY by building from source (`cargo build --release` → copy to
PATH). `packaging/SIGNING_IDENTITY` and `install.sh` carry `PLACEHOLDER` identities and fake
download URLs (`github.com/ouro/ouro`, `release@ouro.example`). This spec stands up the real
distribution so an operator can `brew install …` / `npx …` / `curl | sh` a **verified** binary and
`ouro-ops self-update` safely, WITHOUT weakening the pinned-identity trust model.

### Scope
- A reproducible release build producing the four target binaries: `x86_64/aarch64-unknown-linux-musl`
  and `x86_64/aarch64-apple-darwin` (static, skills embedded, byte-identical across rebuilds).
- Real signing: cosign keyless (GitHub Actions OIDC identity) + minisign for the offline fallback,
  with a Rekor transparency entry per release. Private key custody is external (never in the repo).
- Publishing vectors: GitHub Releases (tarballs + `.sig` + bundle), a Homebrew tap, an npm wrapper,
  and a signed release-metadata channel (`latest_version`, sequence number, timestamp/expiry,
  revocation/security floor).
- Wire the REAL pinned identity + URLs into `packaging/SIGNING_IDENTITY` and `install.sh` (replace
  placeholders); the official site cross-check (≥2 channels) stays the human-independent anchor.
- The `self-update` APPLY path (fetch → verify sig + Rekor → monotonic anti-rollback → freshness →
  denylist → atomic swap), gated behind the release channel.

### Constraints
- Do NOT weaken the S0016 trust model: verification is against the PINNED identity, never fetched
  from wherever the user landed; a spoofed site cannot produce a signature under the pinned key.
- The signing private key is hardware-held / OIDC-keyless; it is NEVER committed, printed, or
  requested by any tool. CI signs via short-lived OIDC credentials, not a stored key.
- Reproducible builds: `ouro-ops manifest show` must match `packaging/bundle-manifest.json` for the
  released binary (the existing drift gate), and rebuilds must be byte-identical.
- Anti-rollback + freshness + denylist checks from `version.rs` are mandatory on every apply; fail
  closed past metadata expiry except for an explicitly verified offline bundle.
- Cross-platform binaries must be self-contained (musl-static on Linux; no dynamic cardano deps —
  ouro-ops itself has none).

### Non-goals
- Not re-implementing the verification logic (S0016 already has it) — this spec wires the infra it
  plugs into.
- Not the cardano-node/cardano-cli runtime on targets (that is a deployment concern, see S0017 and
  the container-cardano-cli follow-up).
- Not a GUI installer; CLI + Homebrew + npx + curl only.

## 2. Outline Design
- **CI release workflow** (GitHub Actions): matrix build the 4 targets in a reproducible container;
  capture `ouro-ops manifest show`; cosign-sign (keyless OIDC) + minisign-sign each artifact; push
  a Rekor entry; attach tarballs + `.sig` + offline bundle to a GitHub Release; publish the signed
  release-metadata JSON to the channel.
- **Homebrew tap**: a `homebrew-tap` repo with the formula (sha256 filled from the release), which
  runs `install.sh`'s verify path before linking.
- **npm wrapper**: a thin `@ouro/cli` package whose postinstall downloads + verifies the
  platform binary against the pinned identity.
- **SIGNING_IDENTITY + install.sh**: replace placeholders with the real cosign identity/issuer,
  minisign pubkey, Rekor URL, and the real release base URL + version.
- **self-update apply**: implement the INFRA-gated fetch+swap in `self-update`, reusing the S0016
  verify + `max(floor)` + denylist + freshness checks; atomic replace (temp → rename) with a
  rollback-on-failure.
- **Risk/rollback**: a bad release is handled by the denylist + revocation floor in the channel
  metadata (operators refuse it on next check); never a silent downgrade.

## References
- packaging/RELEASE.md (the INFRA callouts this spec implements)
- packaging/SIGNING_IDENTITY, packaging/install.sh, packaging/homebrew/ouro-ops.rb
- crates/ouro/src/version.rs (monotonic anti-rollback / floor), skills.rs (bundle manifest / drift)
- docs/specs/completed/…S0016… (release/signing/self-update verification logic)

## 3. Execution Plan
- [ ] p1-1 reproducible cross-platform release build (4 targets, musl-static, skills embedded, byte-identical)
- [ ] p1-2 CI release workflow: build matrix + capture manifest + drift gate on the released binary
- [ ] p2-1 cosign keyless signing (GitHub Actions OIDC) + Rekor transparency entry per artifact
- [ ] p2-2 minisign offline-fallback signing + self-verifiable offline bundle
- [ ] p2-3 wire REAL identity/URLs into SIGNING_IDENTITY + install.sh (replace placeholders); site cross-check
- [ ] p3-1 Homebrew tap repo + formula (sha filled), verify-before-link
- [ ] p3-2 npm wrapper `@ouro/cli` (postinstall download + pinned-identity verify)
- [ ] p3-3 signed release-metadata channel (latest_version, sequence, expiry, revocation/security floor)
- [ ] p4-1 `self-update` APPLY path: fetch → verify sig+Rekor → anti-rollback → freshness → denylist → atomic swap → rollback-on-fail
- [ ] p4-2 revocation/denylist honored on apply + `--offline <bundle>` verified path

## 4. Test and Acceptance Criteria
- TC-1 each of the 4 target binaries is byte-identical on rebuild and its `manifest show` matches the committed bundle manifest
- TC-2 a released artifact verifies against the pinned cosign identity + has a resolvable Rekor entry; a tampered artifact FAILS
- TC-3 `install.sh` / brew / npx install a binary ONLY if it verifies against the pinned identity (spoofed URL/binary refused)
- TC-4 `self-update` applies ONLY a strictly-newer, verified, unexpired, non-denylisted version; refuses a downgrade / expired / revoked one
- TC-5 offline bundle verifies WITHOUT network (self-contained minisign/Sigstore) and runs the same checks
- TC-6 SIGNING_IDENTITY + install.sh carry NO `PLACEHOLDER` values; the official site shows the same identity (≥2-channel cross-check)
- Pass/fail: an operator can install a verified binary via each vector and self-update safely; no path trusts an unverified binary.

## 5. Execution Log (append-only)

- 2026-07-24T11:05:03+08:00 draft retired without activation：S0028 replaces this proposal
  with a narrower GitHub immutable release/artifact-attestation chain, external canonical Skills,
  four control artifacts, an x86_64-only remote target boundary, and no first-phase Homebrew/npm/
  self-update apply/minisign infrastructure.

## 6. Validation Evidence (append-only)

## 7. Change Requests (append-only)

- 2026-07-24T11:05:03+08:00 operator accepted the adversarial S0028 simplification and explicitly
  replaced this never-activated draft. Historical assumptions remain here as evidence only and
  must not drive current release implementation.
