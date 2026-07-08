#!/usr/bin/env bash
# S0015 p2-3 — REAL KES rotation E2E. Dispatches kes-rotation/rotate to the forging bp1
# and asserts the actual cardano opcert lifecycle: the on-disk opcert counter INCREMENTS,
# the node restarts onto the new opcert and RESUMES forging, and `query kes-period-info`
# reports the new counter. A second rotation proves the counter is monotonic + persisted.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-kes-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }
jqpy() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

SPEC=/opt/ouro/pool-spec.yaml

echo "[bed] rebuild base + up (bp1 forging) + provision (project=$OURO_E2E_PROJECT)"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

# On-disk opcert counter before any rotation (ground-truth from the live node).
# kes-period-info prints "✓ …" lines before the JSON; parse from the first '{'.
kes_counter() {
  dc exec -T bp1 bash -lc 'CARDANO_NODE_SOCKET_PATH=/opt/devnet/node.socket cardano-cli query kes-period-info \
    --op-cert-file /opt/devnet/pools-keys/pool1/opcert.cert --testnet-magic 1 2>/dev/null' \
    | python3 -c 'import json,sys; s=sys.stdin.read(); i=s.find("{"); print(json.loads(s[i:]).get("qKesOnDiskOperationalCertificateNumber",-1) if i>=0 else -1)'
}
c0=$(kes_counter); pass "initial on-disk opcert counter = $c0"

echo "[kes] rotation #1 — dispatch kes-rotation/rotate to bp1 (real opcert issuance + restart)"
out1=$(ctl ouro tool run kes-rotation/rotate --dispatch bp1 --spec "$SPEC")
echo "$out1" | jqpy "d['status']" | grep -qx ok || fail "rotate #1 not ok: $out1"
ca=$(echo "$out1" | jqpy "d['data']['counter_after']")
cb=$(echo "$out1" | jqpy "d['data']['counter_before']")
blk=$(echo "$out1" | jqpy "d['data']['tip_block_after']")
[ "$ca" -eq $((cb + 1)) ] 2>/dev/null || fail "counter not incremented by 1 ($cb -> $ca)"
[ "$blk" -gt 0 ] 2>/dev/null || fail "node not forging after rotation (block=$blk)"
echo "$out1" | jqpy "all(c['pass'] for c in d['checks'])" | grep -qx True || fail "rotate #1 checks not all pass: $out1"
pass "rotation #1: opcert counter $cb -> $ca, node forging (block=$blk), all checks pass"

# Ground-truth from the node's OWN kes-period-info (independent of the script's report).
c1=$(kes_counter)
[ "$c1" -eq "$ca" ] 2>/dev/null || fail "node kes-period-info counter ($c1) != reported ($ca)"
pass "node kes-period-info confirms rotated counter = $c1"

echo "[kes] rotation #2 — counter must be monotonic + persisted across rotations"
out2=$(ctl ouro tool run kes-rotation/rotate --dispatch bp1 --spec "$SPEC")
echo "$out2" | jqpy "d['status']" | grep -qx ok || fail "rotate #2 not ok: $out2"
c2a=$(echo "$out2" | jqpy "d['data']['counter_after']")
[ "$c2a" -eq $((c1 + 1)) ] 2>/dev/null || fail "counter not monotonic across rotations ($c1 -> $c2a)"
pass "rotation #2: counter advanced $c1 -> $c2a (monotonic, persisted)"

echo "p2-3 KES rotation E2E: ALL PASSED"
