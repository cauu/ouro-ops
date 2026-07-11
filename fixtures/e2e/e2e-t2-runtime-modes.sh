#!/usr/bin/env bash
# S0017 p2-9 — supervision-mode E2E against a REAL systemd host. Validates the two legs of the
# p2-5 systemd branch that were previously only stub-tested:
#   (1) detect/runtime reports mode=systemd + the real unit name on a systemd-managed node;
#   (2) the adapter's restart dispatch does `systemctl restart <unit>` and the node PID rotates.
# Runs a systemd container (private cgroupns => a clean /system.slice/<unit> cgroup, i.e. a
# faithful bare-metal/VM systemd host, not a container-in-container). Standalone; self-cleans.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

IMG=ouro-e2e-systemd-node:local
NAME="ouro-e2e-modes-$$"
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT
dex() { docker exec "$NAME" "$@"; }

echo "[modes] build systemd-node image + boot systemd (private cgroupns)"
docker build -q -f fixtures/e2e/systemd-node/Dockerfile -t "$IMG" . >/dev/null
docker rm -f "$NAME" >/dev/null 2>&1 || true
# --privileged + the cgroup mount let systemd run as PID1; DEFAULT (private) cgroupns keeps the
# service cgroup free of the outer docker id — the whole point of the fixture.
docker run -d --name "$NAME" --privileged \
  -v /sys/fs/cgroup:/sys/fs/cgroup:rw --tmpfs /run --tmpfs /run/lock "$IMG" >/dev/null

# Wait for systemd + the node unit to be active.
i=0
until dex systemctl is-active cardano-node.service >/dev/null 2>&1 || [ $i -ge 30 ]; do sleep 1; i=$((i+1)); done
dex systemctl is-active cardano-node.service | grep -qx active \
  || fail "cardano-node.service not active: $(dex systemctl status cardano-node.service 2>&1 | head -5)"
pass "systemd host up; cardano-node.service active"

echo "[modes] (1) detect/runtime reports mode=systemd on a real systemd node"
OUT=$(dex bash -c 'OURO_MACHINE=sysnode bash /opt/ouro/ouro-skills/detect/scripts/runtime.sh')
MODE=$(printf '%s' "$OUT" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["mode"])')
UNIT=$(printf '%s' "$OUT" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["evidence"]["unit"])')
CID=$(printf '%s' "$OUT" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["evidence"]["container_id"])')
[ "$MODE" = systemd ] || fail "detect mode=$MODE (want systemd); out=$OUT"
[ "$UNIT" = cardano-node.service ] || fail "detect unit=$UNIT (want cardano-node.service)"
[ "$CID" = None ] || fail "detect saw a container id ($CID) — private cgroupns leaked the docker id"
pass "detect/runtime: mode=systemd, unit=cardano-node.service, no container id"

echo "[modes] (2) adapter restart dispatch => systemctl restart <unit>, node PID rotates"
PID0=$(dex bash -c 'source /opt/ouro/ouro-skills/lib/ouro-lib.sh; ouro_node_pid')
[ -n "$PID0" ] || fail "no node pid before restart"
# Drive the SAME adapter path the lifecycle scripts use: resolve mode (must be systemd) then
# dispatch. This is the systemd branch of ouro_node_restart_mode (systemctl restart <unit>).
dex bash -c 'source /opt/ouro/ouro-skills/lib/ouro-lib.sh
  MODE="$(ouro_node_effective_mode "")"; ouro_node_guard_mode "$MODE"
  [ "$MODE" = systemd ] || { echo "resolved mode=$MODE not systemd" >&2; exit 3; }
  ouro_node_restart_mode "$MODE"' || fail "restart_mode systemd dispatch failed"
sleep 2
PID1=$(dex bash -c 'source /opt/ouro/ouro-skills/lib/ouro-lib.sh; ouro_node_pid')
[ -n "$PID1" ] || fail "no node pid after restart (unit did not come back)"
[ "$PID1" != "$PID0" ] || fail "node PID unchanged ($PID0) — systemctl restart did not rotate the process"
# Ground-truth: systemd itself reports the unit active with the new MainPID.
dex systemctl is-active cardano-node.service | grep -qx active || fail "unit not active after restart"
MAINPID=$(dex systemctl show -p MainPID --value cardano-node.service)
[ "$MAINPID" = "$PID1" ] || fail "systemd MainPID ($MAINPID) != detected pid ($PID1)"
pass "systemd restart: PID rotated $PID0 -> $PID1, unit active, MainPID matches"

echo "[modes] docker/compose — container upgrade = image re-pin + recreate + rollback (TC-7)"
CPROJ="ouro-e2e-cup-$$"
CDIR="$(pwd)/tmp/compose-node-$$"; rm -rf "$CDIR"; mkdir -p "$CDIR"
CFG="$CDIR/docker-compose.yaml"
compose_cleanup() { docker compose -p "$CPROJ" -f "$CFG" down >/dev/null 2>&1 || true; rm -rf "$CDIR"; }
trap 'cleanup; compose_cleanup' EXIT

# Two DISTINCT node image versions (different content ids) — no apt, so no flaky fetch.
docker build -q --build-arg V=1 -f fixtures/e2e/compose-node/Dockerfile -t ouro-e2e-cnode:v1 . >/dev/null
docker build -q --build-arg V=2 -f fixtures/e2e/compose-node/Dockerfile -t ouro-e2e-cnode:v2 . >/dev/null
V1_ID=$(docker image inspect --format '{{.Id}}' ouro-e2e-cnode:v1)
V2_ID=$(docker image inspect --format '{{.Id}}' ouro-e2e-cnode:v2)
[ "$V1_ID" != "$V2_ID" ] || fail "v1/v2 images have the same id — upgrade check would be vacuous"

printf 'services:\n  cardano-node:\n    image: ouro-e2e-cnode:v1\n    command: run\n' > "$CFG"
docker compose -p "$CPROJ" -f "$CFG" up -d >/dev/null 2>&1 || fail "compose up (v1) failed"
CID=$(docker compose -p "$CPROJ" -f "$CFG" ps -q cardano-node | head -1)
[ -n "$CID" ] || fail "no compose container id"
[ "$(docker inspect --format '{{.Image}}' "$CID")" = "$V1_ID" ] || fail "compose node not on v1"
pass "compose node up on v1 (project=$CPROJ)"

# Upgrade to v2 via the REAL adapter path against real docker + compose (adapter runs here,
# reading the container's own compose labels to locate the project + compose file).
OURO_MACHINE=bp1 OURO_TOOL_NAME=modes/upgrade bash -c '
  source ouro-skills/lib/ouro-lib.sh
  ouro_node_upgrade_container docker "'"$CID"'" ouro-e2e-cnode:v2' \
  || fail "container upgrade v1->v2 failed"
grep -q 'image: ouro-e2e-cnode:v2' "$CFG" || fail "compose file NOT re-pinned to v2"
NEWCID=$(docker compose -p "$CPROJ" -f "$CFG" ps -q cardano-node | head -1)
[ -n "$NEWCID" ] || fail "no container after v2 recreate"
[ "$(docker inspect --format '{{.Image}}' "$NEWCID")" = "$V2_ID" ] || fail "recreated container NOT on v2 image"
pass "upgrade v1->v2: compose re-pinned + container recreated on v2 image id"

# Rollback: upgrade to a NONEXISTENT image => must restore the compose file to v2 and fail.
if OURO_MACHINE=bp1 OURO_TOOL_NAME=modes/upgrade bash -c '
     source ouro-skills/lib/ouro-lib.sh
     ouro_node_upgrade_container docker "'"$NEWCID"'" ouro-e2e-cnode:does-not-exist-9x' >/dev/null 2>&1; then
  fail "upgrade to a nonexistent image unexpectedly succeeded"
fi
grep -q 'image: ouro-e2e-cnode:v2' "$CFG" || fail "compose file NOT restored to v2 after failed upgrade"
STILL=$(docker compose -p "$CPROJ" -f "$CFG" ps -q cardano-node | head -1)
[ -n "$STILL" ] && [ "$(docker inspect --format '{{.Image}}' "$STILL")" = "$V2_ID" ] \
  || fail "node not still on v2 after rolled-back upgrade"
pass "rollback: failed upgrade restored compose to v2, node still on v2"

compose_cleanup
docker rmi ouro-e2e-cnode:v1 ouro-e2e-cnode:v2 >/dev/null 2>&1 || true

echo "supervision-mode (systemd + docker/compose) E2E: ALL PASSED"
