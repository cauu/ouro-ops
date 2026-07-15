#!/usr/bin/env bash
# S0019 p5-6 — container-bed end-to-end. Builds a stand-in conforming node, adopts it for REAL,
# runs a real `runtime/restart` op (real docker restart), and asserts the container actually
# restarted. Skips cleanly if docker is unavailable. Real crash-injection is noted where infeasible.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
BIN="$ROOT/target/debug/ouro-ops"
IMG="ouro-s0019-bed:local"
IMG2="ouro-s0019-bed-v2:local"
NAME="ouro-s0019-bed-$$"
WORK="$(mktemp -d)"
export OURO_HOME="$WORK/home"
export OURO_ATTESTATION="$WORK/node-attestation.json"
export OURO_READINESS_SAMPLE_DELAY=0
mkdir -p "$OURO_HOME"

pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; cleanup; exit 1; }
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; docker rmi "$IMG" "$IMG2" >/dev/null 2>&1 || true; rm -rf "$WORK"; }
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

# p6-2: adopt/op auto-run the embedded probe when no --observation is given. Point it at the lib.
export OURO_PROBE_LIB="$ROOT/ouro-skills/lib/ouro-probe.sh"

echo "== adopt the real container (auto-probe, non-disruptive) =="
"$BIN" adopt --local --node bp1 --role bp --approve-token op-tok >/dev/null || fail "adopt (auto-probe) refused a conforming container"
[ -f "$OURO_ATTESTATION" ] || fail "attestation not written"
pass "adopt wrote the attestation; container untouched"
# node still running after adopt (non-disruptive)
[ "$(docker inspect --format '{{.State.Running}}' "$NAME")" = "true" ] || fail "adopt disrupted the node"
pass "adopt was non-disruptive (node still running)"

fleet_permit() {
  "$BIN" fleet permit create --pool-id bedpool --node bp1 --op "$1" --role bp \
    --online-relays 1 --min-online-relays 1 --relays-remaining 0 --holder bedctl \
    | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["fleet_permit"])'
}
RPERMIT="$(fleet_permit runtime/restart)" || fail "restart fleet permit"

echo "== dangerous write without confirm → refused =="
OUT="$("$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --fleet-permit "$RPERMIT" 2>&1)"
echo "$OUT" | grep -q "dangerous write" || fail "restart without confirm was not refused; got: $OUT"
pass "restart refused without an operator confirm-token"

echo "== mint the intent-bound confirm-token, then run the real restart =="
IH="$("$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --fleet-permit "$RPERMIT" 2>&1 | grep -o 'intent-hash [0-9a-f]*' | awk '{print $2}')"
[ -n "$IH" ] || fail "no intent hash"
TOK="$("$BIN" confirm create --op runtime/restart --node bp1 --intent-hash "$IH" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["confirm_token"])')"
START0="$(docker inspect --format '{{.State.StartedAt}}' "$NAME")"
"$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --fleet-permit "$RPERMIT" --confirm-token "$TOK" >/dev/null || fail "confirmed restart failed"
START1="$(docker inspect --format '{{.State.StartedAt}}' "$NAME")"
[ "$START0" != "$START1" ] || fail "container did not actually restart"
pass "REAL docker restart executed (StartedAt advanced) through the sealed executor"
REPLAY="$("$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --fleet-permit "$RPERMIT" --confirm-token "$TOK" 2>&1)"
echo "$REPLAY" | grep -qiE 'stale|replay|already used' || fail "fleet/confirm replay was not refused: $REPLAY"
pass "target-side fencing refused replay of the same disruptive-step permit"

echo "== live drift is refused (swap the container id in the observation) =="
python3 -c "import json;o=json.load(open('$WORK/obs.json'));o['live']['container_id']='deadbeef';json.dump(o,open('$WORK/drift.json','w'))"
DOUT="$("$BIN" op run --op runtime/restart --local --node bp1 --param machine=bp1 --observation "$WORK/drift.json" --fleet-permit "$RPERMIT" --confirm-token "$TOK" 2>&1)"
echo "$DOUT" | grep -q "drift" || fail "drift not refused; got: $DOUT"
pass "live drift refused before mutation"

# p7-4 — a REAL artifact-consuming op sequence (kes-rotation): the sealed executor must (1) refuse
# when no opcert is staged, then (2) docker cp the DIGEST-RESOLVED opcert (public node.cert) into the
# keys mount AND restart. Proves build_plan's multi-step sequence runs for real, not just restart.
echo "== kes-rotation without a staged opcert → refused (no silent restart) =="
RREF="kes-not-staged@sha256:$(printf 'x%.0s' {1..64})"
KPERMIT="$(fleet_permit kes-rotation/rotate)" || fail "KES fleet permit"
NOUT="$("$BIN" op run --op kes-rotation/rotate --local --node bp1 --param machine=bp1 --param opcert="$RREF" --fleet-permit "$KPERMIT" --confirm-token "$TOK" 2>&1)"
echo "$NOUT" | grep -qiE 'confirm|dangerous|artifact|refus' || fail "kes with unstaged opcert not refused; got: $NOUT"
pass "kes-rotation refused a non-staged / unconfirmed opcert (no silent restart)"

echo "== stage a real opcert artifact, then run the REAL kes-rotation sequence =="
printf '{"type":"NodeOperationalCertificate","description":"","cborHex":"82008278"}' > "$WORK/opcert.json"
OREF="$("$BIN" inbox stage --type opcert --file "$WORK/opcert.json" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["artifact_ref"])')"
[ -n "$OREF" ] || fail "opcert not staged"
KIH="$("$BIN" op run --op kes-rotation/rotate --local --node bp1 --param machine=bp1 --param opcert="$OREF" --fleet-permit "$KPERMIT" 2>&1 | grep -o 'intent-hash [0-9a-f]*' | awk '{print $2}')"
[ -n "$KIH" ] || fail "no kes intent hash"
KTOK="$("$BIN" confirm create --op kes-rotation/rotate --node bp1 --intent-hash "$KIH" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["confirm_token"])')"
KSTART0="$(docker inspect --format '{{.State.StartedAt}}' "$NAME")"
"$BIN" op run --op kes-rotation/rotate --local --node bp1 --param machine=bp1 --param opcert="$OREF" --fleet-permit "$KPERMIT" --confirm-token "$KTOK" >/dev/null || fail "confirmed kes-rotation failed"
KSTART1="$(docker inspect --format '{{.State.StartedAt}}' "$NAME")"
INSTALLED="$(docker exec "$NAME" cat /opt/cardano/config/keys/node.cert 2>/dev/null)"
echo "$INSTALLED" | grep -q 'NodeOperationalCertificate' || fail "opcert was NOT docker-cp'd into the keys mount; got: $INSTALLED"
[ "$KSTART0" != "$KSTART1" ] || fail "kes-rotation did not restart the container after installing the opcert"
# The op must COMMIT (advance the managed state), not silently roll back: the attestation's opcert id
# must now match the installed cert AND the generation must have bumped. (Without the advance, the
# next op would drift-refuse — proving the earlier "restart happened" assertion was not enough.)
INSTALLED_ID="$(docker exec "$NAME" sh -c 'sha256sum /opt/cardano/config/keys/node.cert' | awk '{print $1}')"
python3 -c "import json;a=json.load(open('$OURO_ATTESTATION'));assert a['state']['kes_opcert_id']=='$INSTALLED_ID',('opcert id not advanced',a['state']['kes_opcert_id']);assert a['state']['state_generation']>=1,a['state']['state_generation']" || fail "attestation managed state was NOT advanced (op rolled back?)"
pass "REAL kes-rotation sequence: opcert installed, restarted, AND attestation advanced (committed, not rolled back)"

# p8 — a REAL N→N+1 upgrade: recreate the container onto a new allowlisted image digest, preserving
# the observed run-spec (name + the /data/db bind), then rotate the attestation. Build a v2 image
# (distinct config digest via a label), allowlist BOTH, and drive upgrade/step.
echo "== build a v2 image (distinct digest) and allowlist v1 + v2 =="
docker build -q --label ouro.upgrade=v2 -t "$IMG2" "$HERE" >/dev/null || fail "v2 image build"
V2CFG="$(docker inspect --format '{{.Id}}' "$IMG2")"
[ -n "$V2CFG" ] && [ "$V2CFG" != "$IMG_CFG" ] || fail "v2 digest not distinct from v1"
python3 - "$ROOT/data/allowlist.json" "$IMG_CFG" "$V2CFG" > "$WORK/allowlist.json" <<'PY'
import json, sys
a = json.load(open(sys.argv[1])); v1, v2 = sys.argv[2], sys.argv[3]
c1 = a["contracts"][0]
base = c1["allowed"][0]
base["image_config_digest"] = v1; base["oci_index_digest"] = v1
c1["allowed"] = [base]
c2 = json.loads(json.dumps(c1))
c2["convention_version"] = c1["convention_version"] + 1
c2["contract_id"] = c1["contract_id"] + "-v2"
nxt = json.loads(json.dumps(base)); nxt["image_config_digest"] = v2; nxt["oci_index_digest"] = v2
c2["allowed"] = [nxt]
a["contracts"] = [c1, c2]
a["transitions"] = [{
  "from_convention_version": c1["convention_version"],
  "to_convention_version": c2["convention_version"],
  "from_image_config_digest": v1, "to_image_config_digest": v2,
  "db_forward_compatible": True, "db_backward_compatible": True,
  "snapshot_taken": False,
}]
json.dump(a, sys.stdout)
PY
pass "v2 image built with a distinct config digest; allowlist pins both baselines"

echo "== upgrade v1 → v2: recreate onto the new digest, preserve the db bind, rotate attestation =="
UPERMIT="$(fleet_permit upgrade/step)" || fail "upgrade fleet permit"
UIH="$("$BIN" op run --op upgrade/step --local --node bp1 --param machine=bp1 --param image="$V2CFG" --fleet-permit "$UPERMIT" 2>&1 | grep -o 'intent-hash [0-9a-f]*' | awk '{print $2}')"
[ -n "$UIH" ] || fail "no upgrade intent hash"
UTOK="$("$BIN" confirm create --op upgrade/step --node bp1 --intent-hash "$UIH" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["confirm_token"])')"
"$BIN" op run --op upgrade/step --local --node bp1 --param machine=bp1 --param image="$V2CFG" --fleet-permit "$UPERMIT" --confirm-token "$UTOK" >/dev/null || fail "confirmed upgrade failed"
NEWIMG="$(docker inspect --format '{{.Image}}' "$NAME" 2>/dev/null)"
[ "$NEWIMG" = "$V2CFG" ] || fail "container not recreated onto v2 (got: $NEWIMG)"
docker inspect --format '{{range .Mounts}}{{.Destination}} {{end}}' "$NAME" | grep -q '/data/db' || fail "db bind mount not preserved across the upgrade"
python3 -c "import json;a=json.load(open('$OURO_ATTESTATION'));assert a['immutable']['image_config_digest']=='$V2CFG',a['immutable']['image_config_digest'];assert a['state']['state_generation']>=1" || fail "attestation not rotated to v2"
pass "REAL upgrade: container recreated onto v2, /data/db bind preserved, attestation rotated"

echo "== a non-allowlisted target image is refused =="
BADIMG="sha256:$(printf 'f%.0s' {1..64})"
BOUT="$("$BIN" op run --op upgrade/step --local --node bp1 --param machine=bp1 --param image="$BADIMG" --fleet-permit "$UPERMIT" --confirm-token "$UTOK" 2>&1)"
echo "$BOUT" | grep -qiE 'allowlist|not on|refus|denied' || fail "non-allowlisted upgrade target not refused; got: $BOUT"
pass "upgrade to a non-allowlisted image digest refused"

echo ""
echo "S0019 container-bed e2e: ALL PASS"
