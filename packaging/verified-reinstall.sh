#!/bin/sh
# Canonical command source rendered by the Ouro Site for both first install and update.
# Users copy these commands from the Site. This file is not a separately published installer.
set -eu

REPO=cauu/ouro-ops
INSTALL_DIR=$HOME/.local/bin
OURO_BIN=$INSTALL_DIR/ouro-ops
SIGNER_WORKFLOW=cauu/ouro-ops/.github/workflows/release-publish.yml
install_tmp=

cleanup() {
  [ -z "${work_dir:-}" ] || rm -rf -- "$work_dir"
  [ -z "${install_tmp:-}" ] || rm -f -- "$install_tmp"
}
trap cleanup EXIT HUP INT TERM

fail() {
  printf 'ouro-ops verified reinstall: %s\n' "$*" >&2
  exit 1
}

command -v gh >/dev/null 2>&1 ||
  fail "GitHub CLI (gh) is required; no binary or PATH change was made"
command -v tar >/dev/null 2>&1 || fail "tar is required; no binary or PATH change was made"

case "$(uname -s)/$(uname -m)" in
  Linux/x86_64|Linux/amd64) target=x86_64-unknown-linux-musl ;;
  Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-musl ;;
  Darwin/x86_64) target=x86_64-apple-darwin ;;
  Darwin/arm64|Darwin/aarch64) target=aarch64-apple-darwin ;;
  *) fail "unsupported control platform $(uname -s)/$(uname -m)" ;;
esac

tag=$(gh release view --repo "$REPO" --json tagName --jq .tagName) ||
  fail "cannot resolve the latest stable release"
case "$tag" in
  v[0-9]*.[0-9]*.[0-9]*) ;;
  *) fail "latest release tag is not stable SemVer: $tag" ;;
esac
version=${tag#v}
printf '%s\n' "$version" |
  grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' ||
  fail "latest release version is not canonical stable SemVer: $version"

archive=ouro-ops-v${version}-${target}.tar.gz
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/ouro-verified-reinstall.XXXXXXXXXX")
download_dir=$work_dir/download
extract_dir=$work_dir/extract
mkdir -p "$download_dir" "$extract_dir"

gh release verify "$tag" --repo "$REPO" ||
  fail "release $tag is not a verified immutable GitHub release"
gh release download "$tag" --repo "$REPO" \
  --pattern "$archive" --pattern SHA256SUMS --dir "$download_dir" ||
  fail "cannot download canonical release assets"
archive_path=$download_dir/$archive
checksums_path=$download_dir/SHA256SUMS
[ -f "$archive_path" ] && [ -f "$checksums_path" ] ||
  fail "release is missing $archive or SHA256SUMS"

gh release verify-asset "$tag" "$archive_path" --repo "$REPO" ||
  fail "$archive does not match the immutable release"
gh release verify-asset "$tag" "$checksums_path" --repo "$REPO" ||
  fail "SHA256SUMS does not match the immutable release"
gh attestation verify "$archive_path" --repo "$REPO" \
  --signer-workflow "$SIGNER_WORKFLOW" >/dev/null ||
  fail "$archive has no valid release-publish workflow attestation"

checksum_line=$(grep -E "^[0-9a-f]{64}  ${archive}$" "$checksums_path" || true)
[ "$(printf '%s\n' "$checksum_line" | grep -c .)" -eq 1 ] ||
  fail "SHA256SUMS has no unique entry for $archive"
expected_sha=${checksum_line%%  *}
if command -v shasum >/dev/null 2>&1; then
  actual_sha=$(shasum -a 256 "$archive_path" | awk '{print $1}')
elif command -v sha256sum >/dev/null 2>&1; then
  actual_sha=$(sha256sum "$archive_path" | awk '{print $1}')
else
  fail "shasum or sha256sum is required; the installed binary was not changed"
fi
[ "$actual_sha" = "$expected_sha" ] || fail "$archive checksum mismatch"

[ "$(tar -tzf "$archive_path")" = ouro-ops ] ||
  fail "$archive must contain exactly one ouro-ops"
tar -xzf "$archive_path" -C "$extract_dir"
candidate=$extract_dir/ouro-ops
[ -f "$candidate" ] && [ ! -L "$candidate" ] || fail "archive binary is not a regular file"
chmod 0500 "$candidate"

version_record=$(OURO_JSON=1 "$candidate" version) ||
  fail "downloaded binary cannot report its version"
printf '%s\n' "$version_record" | grep -q '"binary":"ouro-ops"' ||
  fail "downloaded executable is not Ouro Ops"
candidate_version=$(printf '%s\n' "$version_record" |
  sed -n 's/.*"version":"\([^"]*\)".*/\1/p')
[ "$candidate_version" = "$version" ] ||
  fail "downloaded binary version $candidate_version does not match $version"
OURO_JSON=1 "$candidate" contract check \
  --requires-ouro ">=$version" --requires-contract 1 >/dev/null ||
  fail "downloaded binary contract is incompatible"
OURO_JSON=1 "$candidate" contract >/dev/null ||
  fail "downloaded binary cannot report its contract"

version_cmp() (
  left=$1
  right=$2
  old_ifs=$IFS
  IFS=.
  set -- $left
  left_major=$1 left_minor=$2 left_patch=$3
  set -- $right
  right_major=$1 right_minor=$2 right_patch=$3
  IFS=$old_ifs
  for pair in \
    "$left_major $right_major" "$left_minor $right_minor" "$left_patch $right_patch"
  do
    set -- $pair
    [ "$1" -eq "$2" ] || {
      [ "$1" -lt "$2" ] && printf '%s\n' -1 || printf '%s\n' 1
      exit
    }
  done
  printf '%s\n' 0
)

action=fresh_install
if [ -e "$OURO_BIN" ] || [ -L "$OURO_BIN" ]; then
  [ -f "$OURO_BIN" ] && [ ! -L "$OURO_BIN" ] && [ -x "$OURO_BIN" ] ||
    fail "$OURO_BIN is not an executable regular Ouro binary"
  current_record=$(OURO_JSON=1 "$OURO_BIN" version 2>/dev/null) ||
    fail "existing $OURO_BIN cannot be verified as Ouro Ops"
  printf '%s\n' "$current_record" | grep -q '"binary":"ouro-ops"' ||
    fail "existing $OURO_BIN is not Ouro Ops"
  current_version=$(printf '%s\n' "$current_record" |
    sed -n 's/.*"version":"\([^"]*\)".*/\1/p')
  printf '%s\n' "$current_version" |
    grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' ||
    fail "existing Ouro version is not stable SemVer: $current_version"
  comparison=$(version_cmp "$current_version" "$version")
  if [ "$comparison" -gt 0 ]; then
    fail "refusing downgrade from $current_version to $version"
  fi
  current_sha=
  if command -v shasum >/dev/null 2>&1; then
    current_sha=$(shasum -a 256 "$OURO_BIN" | awk '{print $1}')
    candidate_sha=$(shasum -a 256 "$candidate" | awk '{print $1}')
  else
    current_sha=$(sha256sum "$OURO_BIN" | awk '{print $1}')
    candidate_sha=$(sha256sum "$candidate" | awk '{print $1}')
  fi
  if [ "$comparison" -eq 0 ]; then
    [ "$current_sha" = "$candidate_sha" ] ||
      fail "same-version Ouro binary digest differs; refusing replacement"
    OURO_JSON=1 "$OURO_BIN" contract >/dev/null ||
      fail "existing same-version Ouro contract check failed"
    printf 'ouro-ops %s already verified at %s; no write performed\n' "$version" "$OURO_BIN"
    exit 0
  fi
  action=forward_update
fi

mkdir -p "$INSTALL_DIR"
install_tmp=$(mktemp "$INSTALL_DIR/.ouro-ops.install.XXXXXXXXXX")
install -m 0755 "$candidate" "$install_tmp"
mv -f "$install_tmp" "$OURO_BIN"
install_tmp=
OURO_JSON=1 "$OURO_BIN" version >/dev/null
OURO_JSON=1 "$OURO_BIN" contract >/dev/null
printf 'ouro-ops %s completed at %s (%s); PATH was not modified\n' "$action" "$OURO_BIN" "$version"
