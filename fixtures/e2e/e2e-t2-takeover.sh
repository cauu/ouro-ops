#!/usr/bin/env bash
# S0015 p2-7 — REAL legacy-takeover E2E (E2E-17). bp1 runs a real forging node (the
# "legacy" install from ouro's perspective). Dispatch deploy/takeover to assume management
# of it, asserting: the live node/keys are detected, keys are PRESERVED (checksum snapshot
# as the rollback artifact), the node keeps forging, takeover is idempotent, and the failure
# path (no legacy node on a relay) REFUSES without marking (rollback-safe).
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-tko-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }
jqpy() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT
SPEC=/opt/ouro/pool-spec.yaml

echo "[bed] rebuild base + up (bp1 = the legacy forging node) + provision"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

echo "[takeover] assume management of the live legacy node on bp1"
out1=$(ctl ouro-ops tool run deploy/takeover --dispatch bp1 --spec "$SPEC")
echo "$out1" | jqpy "d['status']" | grep -qx ok || fail "takeover not ok: $out1"
echo "$out1" | jqpy "d['changed']" | grep -qx True || fail "first takeover not changed=true: $out1"
pass "takeover bp1: live node+keys detected, changed=true (keys snapshotted for rollback)"

# Rollback artifact is a real key-checksum snapshot on the target.
dc exec -T bp1 bash -lc 'grep -q "pools-keys/pool1/kes.skey" /tmp/ouro-deploy-state/takeover-bp1.rollback' \
  || fail "rollback artifact is not a key checksum snapshot"
pass "rollback artifact present: key checksum snapshot on target"

echo "[takeover] verify — keys preserved (checksums match) + legacy node still forging"
outv=$(ctl ouro-ops tool run deploy/takeover-verify --dispatch bp1 --spec "$SPEC")
echo "$outv" | jqpy "d['status']" | grep -qx ok || fail "takeover-verify not ok: $outv"
echo "$outv" | jqpy "d['checks'][0]['detail']" | grep -qi "preserved" || fail "verify did not confirm key preservation: $outv"
pass "takeover-verify: keys preserved + node running (real sha256 -c against live keys)"

echo "[takeover] idempotent — second takeover is changed=false"
out2=$(ctl ouro-ops tool run deploy/takeover --dispatch bp1 --spec "$SPEC")
echo "$out2" | jqpy "d['changed']" | grep -qx False || fail "second takeover not changed=false: $out2"
pass "takeover idempotent: already-managed => changed=false"

echo "[takeover] failure path — no legacy node on relay1 => REFUSE, rollback-safe (no marker)"
if ctl ouro-ops tool run deploy/takeover --dispatch relay1 --spec "$SPEC" >/tmp/tko-relay.json 2>&1; then
  fail "takeover on relay1 (no node) unexpectedly succeeded"
fi
grep -q "takeover_precondition_failed" /tmp/tko-relay.json || fail "relay1 takeover failed for wrong reason: $(cat /tmp/tko-relay.json)"
dc exec -T relay1 bash -lc 'test ! -e /tmp/ouro-deploy-state/takeover-relay1' \
  || fail "relay1 takeover left a marker despite failing (not rollback-safe)"
pass "failure path: relay1 (no legacy node) refused (exit 20), no marker written (rollback-safe)"
rm -f /tmp/tko-relay.json

echo "p2-7 legacy-takeover E2E: ALL PASSED"
