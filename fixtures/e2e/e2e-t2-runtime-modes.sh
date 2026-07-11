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

echo "supervision-mode (systemd) E2E: ALL PASSED"
