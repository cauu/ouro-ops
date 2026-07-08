#!/usr/bin/env bash
# S0015 p2-2 — REAL node-state collection E2E. Dispatches deploy/status to the forging
# bp1 and asserts the status is built from a LIVE `cardano-cli query tip` (NOT an injected
# OURO_STATUS_SNAPSHOT): tip block>0, era Conway, network-magic guard, genesis
# self-consistency, and block-height MONOTONIC increase across two dispatches. Also proves
# wrong-network → query fails. Rebuilds base first (never test stale ouro) and tears down.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-node-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }
jqpy() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

SPEC=/opt/ouro/pool-spec.yaml

echo "[bed] anti-placeholder precheck: the collector must not READ an injected snapshot"
# Match an actual variable expansion (`${OURO_STATUS_SNAPSHOT…`), not the word in a comment.
grep -qE '\$\{?OURO_STATUS_SNAPSHOT' ouro-skills/deploy/scripts/status.sh \
  && fail "deploy/status expands OURO_STATUS_SNAPSHOT (would defeat E2E-11)" \
  || pass "E2E-11 deploy/status reads no injected snapshot (real cardano-cli only)"

echo "[bed] rebuild base + up (bp1 forging) + provision (project=$OURO_E2E_PROJECT)"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

echo "[status] dispatch #1 — real cardano-cli query tip on bp1"
out1=$(ctl ouro tool run deploy/status --dispatch bp1 --spec "$SPEC")
echo "$out1" | jqpy "d['status']" | grep -qx ok || fail "deploy/status not ok: $out1"
b1=$(echo "$out1" | jqpy "d['data']['tip']['block']")
era=$(echo "$out1" | jqpy "d['data']['era']")
mag=$(echo "$out1" | jqpy "d['data']['network_magic']")
gh=$(echo "$out1" | jqpy "d['data']['genesis_hash']")
adv=$(echo "$out1" | jqpy "d['data']['slot_advancing']")
[ "$b1" -gt 0 ] 2>/dev/null || fail "tip block not > 0 (got $b1)"
[ "$era" = Conway ] || fail "era not Conway (got $era)"
[ "$mag" = 1 ] || fail "network_magic not 1 (got $mag)"
[ "${#gh}" = 64 ] || fail "genesis hash not 64-hex (got '$gh')"
[ "$adv" = True ] || fail "slot not advancing"
echo "$out1" | jqpy "all(c['pass'] for c in d['checks'])" | grep -qx True || fail "not all checks passed: $out1"
pass "status #1 REAL tip: block=$b1 era=$era magic=$mag genesis=${gh:0:12}… advancing=$adv (all checks pass)"

echo "[status] dispatch #2 — block height MONOTONIC increase (proves live, not a fixture)"
sleep 6
out2=$(ctl ouro tool run deploy/status --dispatch bp1 --spec "$SPEC")
b2=$(echo "$out2" | jqpy "d['data']['tip']['block']")
[ "$b2" -gt "$b1" ] 2>/dev/null || fail "block not monotonic increasing ($b1 -> $b2)"
pass "status #2 block advanced $b1 -> $b2 across dispatches (live chain, not injected)"

echo "[status] wrong-network guard — query with the WRONG magic must fail"
if dc exec -T bp1 bash -lc 'CARDANO_NODE_SOCKET_PATH=/opt/devnet/node.socket cardano-cli query tip --testnet-magic 42' >/dev/null 2>&1; then
  fail "query tip with wrong magic 42 unexpectedly SUCCEEDED"
fi
pass "wrong-network: query tip --testnet-magic 42 fails (magic guard is real)"

echo "[status] genesis self-consistency — reported hash matches the node's own genesis file"
node_gh=$(dc exec -T bp1 bash -lc 'cardano-cli hash genesis-file --genesis /opt/devnet/shelley-genesis.json' | tr -d '\r\n')
[ "$node_gh" = "$gh" ] || fail "collector genesis $gh != node genesis $node_gh"
pass "genesis self-consistent: collector hash == node's shelley genesis hash"

echo "p2-2 node-status E2E: ALL PASSED"
