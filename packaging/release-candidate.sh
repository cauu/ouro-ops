#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT_ROOT=${1:-"$ROOT/dist/release-candidate"}

if [[ $(uname -s) != Darwin ]]; then
  echo "release-candidate control build requires macOS" >&2
  exit 64
fi
for command in cargo cargo-zigbuild rustc python3 shasum tar file strings cmp find grep awk; do
  command -v "$command" >/dev/null || {
    echo "missing release-candidate dependency: $command" >&2
    exit 64
  }
done

VERSION=$(python3 - "$ROOT/Cargo.toml" <<'PY'
import pathlib, sys, tomllib
print(tomllib.loads(pathlib.Path(sys.argv[1]).read_text())["package"]["version"])
PY
)
HOST_TARGET=$(rustc -vV | sed -n 's/^host: //p')
case "$HOST_TARGET" in
  aarch64-apple-darwin|x86_64-apple-darwin) ;;
  *) echo "unsupported macOS release target: $HOST_TARGET" >&2; exit 64 ;;
esac

CANDIDATE="$OUT_ROOT/v$VERSION-$HOST_TARGET"
LINUX_TARGET=x86_64-unknown-linux-musl
LINUX_BUILD="$ROOT/target/release-candidate-linux"
CONTROL_BUILD="$ROOT/target/release-candidate-control"
RUNNER_BUILD="$LINUX_BUILD/$LINUX_TARGET/release/ouro-ops"
CONTROL_BUILD_BINARY="$CONTROL_BUILD/release/ouro-ops"
PACKAGE_NAME="ouro-ops-$VERSION-$HOST_TARGET.tar.gz"
RUNNER_NAME="ouro-ops-runner-$VERSION-$LINUX_TARGET"

rm -rf "$CANDIDATE" "$LINUX_BUILD" "$CONTROL_BUILD"
mkdir -p "$CANDIDATE/build-evidence" "$CANDIDATE/package-root"

(
  cd "$ROOT"
  cargo zigbuild --locked --release --target "$LINUX_TARGET" \
    --target-dir "$LINUX_BUILD" --bin ouro-ops
  OURO_EMBED_LINUX_X86_64_RUNNER="$RUNNER_BUILD" \
    cargo build --locked --release --target-dir "$CONTROL_BUILD" --bin ouro-ops
)

cp "$RUNNER_BUILD" "$CANDIDATE/build-evidence/$RUNNER_NAME"
cp "$CONTROL_BUILD_BINARY" "$CANDIDATE/package-root/ouro-ops"
chmod 0755 "$CANDIDATE/build-evidence/$RUNNER_NAME" "$CANDIDATE/package-root/ouro-ops"

RUNNER_SHA=$(shasum -a 256 "$CANDIDATE/build-evidence/$RUNNER_NAME" | awk '{print $1}')
OURO_JSON=1 "$CANDIDATE/package-root/ouro-ops" contract >"$CANDIDATE/descriptor.json"
OURO_JSON=1 "$CANDIDATE/package-root/ouro-ops" version >"$CANDIDATE/version.json"
OURO_JSON=1 "$CANDIDATE/package-root/ouro-ops" contract check \
  --requires-ouro ">=$VERSION" --requires-contract 1 >/dev/null
OURO_JSON=1 env -u OURO_RELEASES_FILE -u OURO_ALLOWLIST_TEST_KEY \
  "$CANDIDATE/package-root/ouro-ops" release select --platform linux/amd64 \
  >"$CANDIDATE/release-select.json"

python3 - "$CANDIDATE/descriptor.json" "$VERSION" "$RUNNER_SHA" <<'PY'
import json, pathlib, sys
record = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert record["status"] == "ok", record
data = record["data"]
assert set(data) == {"ouro_version", "cli_contract", "runner_platform", "runner_sha256"}, data
assert data == {
    "ouro_version": sys.argv[2],
    "cli_contract": 1,
    "runner_platform": "linux/x86_64",
    "runner_sha256": sys.argv[3],
}, data
PY

file "$CANDIDATE/package-root/ouro-ops" | grep -q 'Mach-O'
file "$CANDIDATE/build-evidence/$RUNNER_NAME" | grep -Eq 'ELF 64-bit.*x86-64'

COPYFILE_DISABLE=1 tar -C "$CANDIDATE/package-root" -czf "$CANDIDATE/$PACKAGE_NAME" ouro-ops
[[ $(tar -tzf "$CANDIDATE/$PACKAGE_NAME") == ouro-ops ]]

EXTRACTED=$(mktemp -d "${TMPDIR:-/tmp}/ouro-rc.XXXXXX")
trap 'rm -rf "$EXTRACTED"' EXIT
tar -xzf "$CANDIDATE/$PACKAGE_NAME" -C "$EXTRACTED"
cmp "$CANDIDATE/package-root/ouro-ops" "$EXTRACTED/ouro-ops"
OURO_JSON=1 "$EXTRACTED/ouro-ops" contract >"$CANDIDATE/extracted-descriptor.json"
cmp "$CANDIDATE/descriptor.json" "$CANDIDATE/extracted-descriptor.json"
OURO_JSON=1 "$EXTRACTED/ouro-ops" contract check \
  --requires-ouro ">=$VERSION" --requires-contract 1 >/dev/null

PACKAGE_SHA=$(shasum -a 256 "$CANDIDATE/$PACKAGE_NAME" | awk '{print $1}')
python3 - "$CANDIDATE/candidate.json" "$VERSION" "$HOST_TARGET" "$PACKAGE_NAME" \
  "$PACKAGE_SHA" "$RUNNER_NAME" "$RUNNER_SHA" "$CANDIDATE/descriptor.json" \
  "$CANDIDATE/version.json" "$CANDIDATE/release-select.json" <<'PY'
import json, pathlib, sys
descriptor = json.loads(pathlib.Path(sys.argv[8]).read_text())["data"]
version = json.loads(pathlib.Path(sys.argv[9]).read_text())
selection = json.loads(pathlib.Path(sys.argv[10]).read_text())
assert version["status"] == "ok" and version["data"]["version"] == sys.argv[2], version
assert selection["status"] == "ok" and selection["data"]["cache_written"] is False, selection
assert selection["data"]["repository"] == "ghcr.io/blinklabs-io/cardano-node", selection
document = {
    "schema_version": 1,
    "version": sys.argv[2],
    "status": "release-standard-not-published",
    "control": {
        "platform": sys.argv[3],
        "package": sys.argv[4],
        "sha256": sys.argv[5],
    },
    "embedded_runner_evidence": {
        "platform": "linux/x86_64",
        "file": f"build-evidence/{sys.argv[6]}",
        "sha256": sys.argv[7],
    },
    "descriptor": descriptor,
    "release_catalog_smoke": {
        "policy_version": selection["data"]["policy_version"],
        "policy_digest": selection["data"]["policy_digest"],
        "repository": selection["data"]["repository"],
        "source": selection["data"]["source"],
    },
    "formal_cli_publication": "deferred",
}
pathlib.Path(sys.argv[1]).write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
PY

(
  cd "$CANDIDATE"
  shasum -a 256 "$PACKAGE_NAME" "build-evidence/$RUNNER_NAME" candidate.json \
    descriptor.json version.json release-select.json \
    >SHA256SUMS
  shasum -a 256 -c SHA256SUMS
)

if find "$CANDIDATE" -type f \( -name 'SKILL.md' -o -name '*.oci' -o -name '*.img' \
  -o -name '*.docker.tar' -o -name '*image*.tar' \) | grep -q .; then
  echo "release candidate contains a forbidden decision or node-image payload" >&2
  exit 70
fi
if tar -tzf "$CANDIDATE/$PACKAGE_NAME" | grep -Eq 'SKILL\.md|ouro-skills|\.oci$|\.img$|image.*\.tar$'; then
  echo "control package contains a forbidden decision or node-image payload" >&2
  exit 70
fi
if strings "$CANDIDATE/package-root/ouro-ops" | grep -q '^# Upgrade Skill$'; then
  echo "control binary unexpectedly embeds external Skill prose" >&2
  exit 70
fi

rm -rf "$CANDIDATE/package-root"
echo "$CANDIDATE"
