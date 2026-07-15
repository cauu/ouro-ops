#!/usr/bin/env bash
# S0019 p5-6 — container-bed end-to-end. Builds a stand-in conforming node, adopts it for REAL,
# runs a real `runtime/restart` op (real docker restart), and asserts the container actually
# restarted. Skips cleanly if docker is unavailable. Real crash-injection is noted where infeasible.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
BIN="$ROOT/target/debug/ouro-ops"
IMG="ouro-s0019-bed:local"
NAME="ouro-s0019-bed-$$"
WORK="$(mktemp -d)"
export OURO_HOME="$WORK/home"
export OURO_ATTESTATION="$WORK/node-attestation.json"
mkdir -p "$OURO_HOME"

pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; cleanup; exit 1; }
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; docker rmi "$IMG" >/dev/null 2>&1 || true; rm -rf "$WORK"; }
trap cleanup EXIT

command -v docker >/dev/null 2>&1 || { echo "SKIP: docker not available"; exit 0; }
[ -x "$BIN" ] || { echo "SKIP: build ouro-ops first (cargo build)"; exit 0; }

echo "== build the stand-in conforming node image =="
docker build -q -t "$IMG" "$HERE" >/dev/null || fail "image build"

echo "== run it (rootful docker run, bind mount, unless-stopped) =="
docker run -d --name "$NAME" --restart unless-stopped -v "$WORK/db:/data/db" "$IMG" >/dev/null || fail "container run"
CID="$(docker inspect --format '{{.Id}}' "$NAME" | cut -c1-64)"
IMG_CFG="$(docker inspect --format '{{.Image}}' "$NAME")"
[ -n "$IMG_CFG" ] || fail "image config digest"

echo "== pin the bed image digest into a test allowlist =="
python3 - "$ROOT/data/allowlist.json" "$IMG_CFG" > "$WORK/allowlist.json" <<'PY'
import json, sys
a = json.load(open(sys.argv[1]))
a["contracts"][0]["allowed"][0]["image_config_digest"] = sys.argv[2]
a["contracts"][0]["allowed"][0]["oci_index_digest"] = sys.argv[2]
json.dump(a, sys.stdout)
PY
export OURO_ALLOWLIST_FILE="$WORK/allowlist.json"

echo "== probe the running container for the observation =="
source "$ROOT/ouro-skills/lib/ouro-probe.sh"
ouro_observe linux/amd64 > "$WORK/obs.json" || fail "probe"
python3 -c "import json;o=json.load(open('$WORK/obs.json'));assert o['live']['container_id'];assert o['live']['image_config_digest']=='$IMG_CFG',o['live']['image_config_digest']" || fail "observation mismatch"
pass "probe gathered the observation from the real container"

echo "== adopt the real container (non-disruptive) =="
"$BIN" adopt --local --node bp1 --role bp --approve-token op-tok --observation "$WORK/obs.json" >/dev/null || fail "adopt refused a conforming container"
[ -f "$OURO_ATTESTATION" ] || fail "attestation not written"
pass "adopt wrote the attestation; container untouched"
# node still running after adopt (non-disruptive)
[ "$(docker inspect --format '{{.State.Running}}' "$NAME")" = "true" ] || fail "adopt disrupted the node"
pass "adopt was non-disruptive (node still running)"

echo "== dangerous write without confirm → refused =="
OUT="$("$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --observation "$WORK/obs.json" 2>&1)"
echo "$OUT" | grep -q "dangerous write" || fail "restart without confirm was not refused; got: $OUT"
pass "restart refused without an operator confirm-token"

echo "== mint the intent-bound confirm-token, then run the real restart =="
IH="$("$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --observation "$WORK/obs.json" 2>&1 | grep -o 'intent-hash [0-9a-f]*' | awk '{print $2}')"
[ -n "$IH" ] || fail "no intent hash"
TOK="$("$BIN" confirm create --op runtime/restart --node bp1 --intent-hash "$IH" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["confirm_token"])')"
START0="$(docker inspect --format '{{.State.StartedAt}}' "$NAME")"
"$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --observation "$WORK/obs.json" --confirm-token "$TOK" >/dev/null || fail "confirmed restart failed"
START1="$(docker inspect --format '{{.State.StartedAt}}' "$NAME")"
[ "$START0" != "$START1" ] || fail "container did not actually restart"
pass "REAL docker restart executed (StartedAt advanced) through the sealed executor"

echo "== live drift is refused (swap the container id in the observation) =="
python3 -c "import json;o=json.load(open('$WORK/obs.json'));o['live']['container_id']='deadbeef';json.dump(o,open('$WORK/drift.json','w'))"
DOUT="$("$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --observation "$WORK/drift.json" --confirm-token "$TOK" 2>&1)"
echo "$DOUT" | grep -q "drift" || fail "drift not refused; got: $DOUT"
pass "live drift refused before mutation"

echo ""
echo "S0019 container-bed e2e: ALL PASS"
