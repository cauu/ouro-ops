#!/usr/bin/env bash
# S0015 (gap fill) — REAL runtime E2E. Dispatches runtime/restart, runtime/topology-apply and
# runtime/verify to the forging bp1 and asserts real node behaviour: a restart genuinely rotates
# the node process (PID changes) and forging resumes; topology-apply renders the spec's relay
# peers into the node topology and restarts (idempotent second run); verify proves forging.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-rt-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }
jqpy() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT
SPEC=/opt/ouro/pool-spec.yaml
# p2-5b: dispatch a target-bound destructive op. Detect the target's live fingerprint, issue a
# confirmation token bound to it, then run with the token — the control-side gate re-detects and
# consumes it against the live evidence (a token for a different/changed target is refused).
cdispatch() {
  local tool="$1" m="${2:-bp1}" fp tok
  fp=$(ctl ouro-ops tool run detect/runtime --dispatch "$m" --spec "$SPEC" \
       | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["evidence_hash"])')
  tok=$(ctl ouro-ops confirm create --action "$tool" --machine "$m" --runtime-evidence "$fp" \
        | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["token"])')
  ctl ouro-ops tool run "$tool" --dispatch "$m" --spec "$SPEC" --confirm-token "$tok"
}
node_pid() { dc exec -T bp1 bash -lc "pgrep -f 'cardano-node run' | head -1" | tr -d '\r'; }

echo "[bed] rebuild base + up (bp1 forging) + provision"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

echo "[rt] gate — a token-less destructive dispatch is REFUSED (p2-5b)"
if ctl ouro-ops tool run runtime/restart --dispatch bp1 --spec "$SPEC" >/tmp/rt-notoken.json 2>&1; then
  fail "token-less runtime/restart was NOT refused: $(cat /tmp/rt-notoken.json)"
fi
grep -q 'confirmation token' /tmp/rt-notoken.json \
  || fail "refusal not about a confirmation token: $(cat /tmp/rt-notoken.json)"
pass "gate: token-less restart refused (requires a target-bound confirmation token)"

echo "[rt] restart — real node process rotation"
p0=$(node_pid)
out=$(cdispatch runtime/restart)
echo "$out" | jqpy "d['changed']" | grep -qx True || fail "restart not changed=true: $out"
sleep 6; p1=$(node_pid)
[ -n "$p1" ] && [ "$p1" != "$p0" ] || fail "node PID unchanged ($p0 -> $p1) — not really restarted"
pass "restart: node PID rotated $p0 -> $p1 (genuine restart)"

echo "[rt] verify — node forging after restart"
ctl ouro-ops tool run runtime/verify --dispatch bp1 --spec "$SPEC" | jqpy "d['status']" | grep -qx ok \
  || fail "runtime verify not ok after restart"
pass "verify: node forging (tip advancing)"

echo "[rt] topology-apply — render relay peers into node topology + restart"
p2=$(node_pid)
outt=$(cdispatch runtime/topology-apply)
echo "$outt" | jqpy "d['changed']" | grep -qx True || fail "topology-apply not changed=true: $outt"
dc exec -T bp1 bash -lc 'python3 -c "import json;p=json.load(open(\"/opt/devnet/topology.json\"))[\"Producers\"];print([x[\"addr\"] for x in p])"' \
  | grep -q relay1 || fail "topology.json does not contain the relay peers"
sleep 6; p3=$(node_pid); [ "$p3" != "$p2" ] || fail "topology-apply did not restart the node ($p2 -> $p3)"
pass "topology-apply: relay peers written to topology.json + node restarted ($p2 -> $p3)"

echo "[rt] topology-apply idempotent — second run changed=false"
cdispatch runtime/topology-apply | jqpy "d['changed']" | grep -qx False \
  || fail "second topology-apply not changed=false"
pass "topology-apply idempotent (changed=false)"

echo "[rt] verify — node still forging after topology-apply"
ctl ouro-ops tool run runtime/verify --dispatch bp1 --spec "$SPEC" | jqpy "d['status']" | grep -qx ok \
  || fail "runtime verify not ok after topology-apply"
pass "verify: node forging after topology-apply"

echo "runtime real-node E2E: ALL PASSED"
