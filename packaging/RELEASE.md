# Ouro CLI Release And Installation

GitHub Releases in `cauu/ouro-ops` are the only supported CLI distribution channel. A formal
release contains:

- `ouro-ops-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz`
- `ouro-ops-vX.Y.Z-aarch64-unknown-linux-musl.tar.gz`
- `ouro-ops-vX.Y.Z-x86_64-apple-darwin.tar.gz`
- `ouro-ops-vX.Y.Z-aarch64-apple-darwin.tar.gz`
- `SHA256SUMS`

Each archive contains exactly one `ouro-ops`. All four controls embed the same static
Linux/x86_64 target runner. GitHub immutable releases bind the assets to the tag, and the
`release-publish.yml` workflow creates GitHub artifact attestations for all four archives.

## Publish

A maintainer starts `Prepare CLI release` on current `main` and chooses exactly one SemVer
component: patch, minor, or major. The workflow:

1. verifies a clean current `main` and the latest stable release baseline;
2. updates only root `Cargo.toml` and `Cargo.lock`;
3. runs release-source tests;
4. creates and non-force pushes `chore(release): vX.Y.Z` plus the matching tag;
5. explicitly dispatches `release-publish.yml` on that tag;
6. builds and executes four native controls around one runner, verifies the aggregate, attests the
   archives, and creates the immutable release.

An exact release commit/tag left unpublished by a transient dispatch failure is resumed without a
second bump. Any mismatched partial state is blocked for operator diagnosis.

## First Install And Update

First install and update are the same verified reinstall. The production Site renders the exact
commands from `packaging/verified-reinstall.sh`; users copy those commands into their control
machine terminal. The command source is not a separately published installer and is never executed
through `curl | sh`.

The flow requires GitHub CLI, resolves the latest stable tag, verifies immutable release and asset
identity, verifies the archive attestation against
`cauu/ouro-ops/.github/workflows/release-publish.yml`, checks `SHA256SUMS`, checks the one-binary
archive shape, and executes the candidate's version/contract checks before any install write.

The only destination is:

```text
$HOME/.local/bin/ouro-ops
```

It does not use sudo or edit PATH. Fresh install writes the verified binary atomically. Update only
replaces an older valid stable Ouro binary. A same-version identical binary is a no-write success.
Downgrade, prerelease, same-version digest mismatch, unknown executable, failed verification, or
unsupported control platform fails closed without replacing the installed binary.

After installation the flow invokes the absolute path for both checks:

```sh
$HOME/.local/bin/ouro-ops version
$HOME/.local/bin/ouro-ops contract
```

## Local Candidate

`make release-candidate` remains a developer-only paired-build check. Its artifacts under
`dist/release-candidate` are not a production install source and are never referenced by the
production Site.
