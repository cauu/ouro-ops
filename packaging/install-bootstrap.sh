#!/bin/sh
set -eu

repo=cauu/ouro-ops
signer=cauu/ouro-ops/.github/workflows/release-publish.yml
command -v gh >/dev/null 2>&1 || {
  printf '%s\n' "GitHub CLI (gh) is required; nothing was installed" >&2
  exit 1
}
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/ouro-install.XXXXXXXXXX")
trap 'rm -rf -- "$work_dir"' EXIT HUP INT TERM
tag=$(gh release view --repo "$repo" --json tagName --jq .tagName)
gh release download "$tag" --repo "$repo" --pattern ouro-install.sh --dir "$work_dir"
gh release verify-asset "$tag" "$work_dir/ouro-install.sh" --repo "$repo"
gh attestation verify "$work_dir/ouro-install.sh" --repo "$repo" \
  --signer-workflow "$signer" >/dev/null
OURO_RELEASE_TAG="$tag" sh "$work_dir/ouro-install.sh"
