# Release catalog signing

`data/releases.json` is the no-cache production release document fetched by deployment and Upgrade
selection. Production binaries accept it only when its canonical payload verifies under the
Ed25519 public key pinned in `crates/ouro/src/convention.rs`.
An allowlisted image is an exact OCI index/platform-manifest/image-config tuple; a mutable tag is
never sufficient. The platform's `recommended` image is the sole automatic Upgrade target.
Transitions are optional exact-pair rollback declarations, not an intermediate-hop upgrade graph.

## Key custody

The macOS-only `tools/allowlist-release-key.swift` creation helper stores the raw Ed25519 seed in
the current user's local login Keychain as:

- service: `io.ouro-ops.allowlist-release`
- account: `production-ed25519-2026-07`
- label: `Ouro Ops Allowlist Release Private Key (2026-07)`

The item is local and its ACL trusts no application. Each reading process therefore triggers an
explicit Keychain authorization prompt. Select the one-time **Allow** action; never select **Always
Allow**, because that would let the signer use the key silently later. The helper never accepts a
private key through argv/stdin, writes it to a file or prints it. The feature-gated
`ouro-allowlist-signer` reads the item through Security.framework and shares the exact
canonicalization and strict semantic validation used by the production Rust verifier. Public keys,
payload hashes and signatures are intentionally public.

This is a local custody mechanism, not an offline or hardware security boundary. A signing process
receives the seed transiently after the user authorizes access. Losing this Mac without an approved
recovery copy requires another trust-root rotation and release of updated binaries. Do not weaken
the Keychain access control or create a plaintext backup.

## One-time authority creation

Run from a reviewed, clean release worktree with the operator physically present:

```sh
swift -suppress-warnings tools/allowlist-release-key.swift create
```

The command refuses to overwrite an existing item and prints only its public identity. Compare that
public key with the reviewed change to `RELEASE_VERIFY_KEY_HEX` before signing.

`data/allowlist.json` is the frozen embedded layout/test fixture. Publishing a new reliable image
changes `data/releases.json`, not the CLI binary or embedded fixture.

## Signing a release catalog

1. Resolve each OCI tuple from the official registry and at least one independent source. Edit only
   `allowlist_version`, the reviewed immutable entries and any separately authorized transitions.
2. Inspect the strict release candidate and its canonical payload digest without using the private
   key:

   ```sh
   cargo run --features release-signer --bin ouro-allowlist-signer -- \
     inspect --input data/releases.json
   ```

3. After review, request an atomic signature update. `--expect-public-key` must be the reviewed key
   pinned in `RELEASE_VERIFY_KEY_HEX`:

   ```sh
   cargo run --features release-signer --bin ouro-allowlist-signer -- \
     sign --input data/releases.json --output data/releases.json \
     --expect-public-key <64-lowercase-hex>
   ```

   Approve the macOS user-presence prompt. The signer checks the Keychain-derived public key,
   self-verifies the signature and replaces the output atomically; it does not print the signature
   or private material.
4. Run `cargo test -p ouro`, the Python suite, Clippy, manifest regeneration/verification and the
   non-mutating target adoption previews required by the active spec. Add a `transition` only when
   that exact source-to-recommended pair has compatibility evidence; its backward-compatible flag
   controls automatic rollback and its absence does not block the forward upgrade.

## Trust-root rotation

A rotation changes both the Keychain authority and `RELEASE_VERIFY_KEY_HEX`. Old signatures stop
verifying immediately in the new binary. Rotation therefore requires explicit operator approval,
a release catalog signed by the new authority, rebuilt control/target binaries, signature-tamper
tests and parity validation. Never replace only the JSON signature or only the pinned public key.
