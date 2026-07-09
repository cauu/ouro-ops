#!/usr/bin/env bash
# S0015 p2-5 — REAL multi-machine rolling upgrade E2E (E2E-7). Runs upgrade/rollout on
# control, which dispatches upgrade-one + verify to each target. Asserts the invariants:
#   A. BP-last order + per-machine real state delta + bp1 node really restarts & keeps forging;
#   B. relay quorum: a min_online_relays=2 spec is refused (exit 10) before any target is touched;
#   C. lock: a concurrent rollout is refused (exit 10);
#   D. verify-before-next: an injected verify failure on relay2 STOPS the rollout (exit 30) and
#      the BP is never reached.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-upg-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }
jqpy() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT
SPEC=/opt/ouro/pool-spec.yaml
SPEC_Q2=/opt/ouro/pool-spec-q2.yaml

reset_state() {
  for m in bp1 relay1 relay2; do dc exec -T "$m" rm -rf /tmp/ouro-upgrade-state 2>/dev/null || true; done
  ctl rm -rf /tmp/ouro-rollout-state 2>/dev/null || true
}
upgraded() { dc exec -T "$1" test -f "/tmp/ouro-upgrade-state/upgraded-$1"; }

echo "[bed] rebuild base + up (bp1 forging) + provision"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null
docker cp fixtures/e2e/pool-spec.bed-quorum2.yaml "$(dc ps -q control)":"$SPEC_Q2" >/dev/null

# ---- A. happy path: BP-last order + per-machine delta + bp1 restart keeps forging ----
echo "[A] rollout (control dispatches relays-first, BP-last, real node restart on bp1)"
reset_state
pid_before=$(dc exec -T bp1 bash -lc "pgrep -f 'cardano-node run' | head -1" | tr -d '\r')
blk_before=$(dc exec -T bp1 bash -lc 'CARDANO_NODE_SOCKET_PATH=/opt/devnet/node.socket cardano-cli query tip --testnet-magic 1 2>/dev/null' | jqpy "d.get('block',0)")
outA=$(ctl ouro tool run upgrade/rollout --spec "$SPEC")
echo "$outA" | jqpy "d['status']" | grep -qx ok || fail "rollout not ok: $outA"
order=$(echo "$outA" | jqpy "','.join(d['data']['order'])")
[ "$order" = "relay1,relay2,bp1" ] || fail "not BP-last order (got: $order)"
pass "A: rollout order = $order (relays first, BP last)"
for m in relay1 relay2 bp1; do upgraded "$m" || fail "no upgrade state delta on $m"; done
pass "A: per-machine real state delta present on relay1, relay2, bp1"
# Prove the bp1 node GENUINELY restarted (PID changed) and forged PAST the pre-rollout block —
# not just that some node reports block>0 from the preserved db.
pid_after=$(dc exec -T bp1 bash -lc "pgrep -f 'cardano-node run' | head -1" | tr -d '\r')
[ -n "$pid_after" ] && [ "$pid_after" != "$pid_before" ] || fail "bp1 node PID unchanged ($pid_before->$pid_after) — not really restarted"
blk=$(dc exec -T bp1 bash -lc 'CARDANO_NODE_SOCKET_PATH=/opt/devnet/node.socket cardano-cli query tip --testnet-magic 1 2>/dev/null' | jqpy "d.get('block',0)")
[ "$blk" -gt "$blk_before" ] 2>/dev/null || fail "bp1 tip did not advance past $blk_before after restart (block=$blk)"
pass "A: bp1 node truly restarted (pid $pid_before->$pid_after) and forged past $blk_before (block=$blk)"

# ---- B. quorum: min_online_relays=2 must be refused before touching any target ----
echo "[B] quorum: rollout with min_online_relays=2 must exit 10"
if ctl ouro tool run upgrade/rollout --spec "$SPEC_Q2" >/tmp/upg-q2.json 2>&1; then
  fail "quorum-2 rollout unexpectedly succeeded"
fi
grep -q "relay_quorum_violation" /tmp/upg-q2.json || fail "quorum rollout failed for wrong reason: $(cat /tmp/upg-q2.json)"
pass "B: quorum violation refused (exit 10 relay_quorum_violation)"

# ---- C. lock: a concurrent rollout is refused ----
echo "[C] lock: hold the rollout lock, then a second rollout must exit 10"
ctl bash -c 'mkdir -p /tmp/ouro-rollout-state/rollout.lock.d && printf other-owner > /tmp/ouro-rollout-state/rollout.lock.d/owner'
if ctl ouro tool run upgrade/rollout --spec "$SPEC" >/tmp/upg-lock.json 2>&1; then
  fail "rollout under a held lock unexpectedly succeeded"
fi
grep -q "rollout_lock_held" /tmp/upg-lock.json || fail "locked rollout failed for wrong reason: $(cat /tmp/upg-lock.json)"
ctl rm -rf /tmp/ouro-rollout-state
pass "C: concurrent rollout refused (exit 10 rollout_lock_held)"

# ---- D. verify-before-next: injected relay2 verify failure stops the rollout; BP not reached ----
echo "[D] verify-fail on relay2 => STOP before BP"
reset_state
dc exec -T relay2 bash -c 'mkdir -p /tmp/ouro-upgrade-state && touch /tmp/ouro-upgrade-state/__test_inject_fail__relay2'
if ctl ouro tool run upgrade/rollout --spec "$SPEC" >/tmp/upg-fail.json 2>&1; then
  fail "rollout with injected relay2 failure unexpectedly succeeded"
fi
grep -q "upgrade_verify_failed" /tmp/upg-fail.json || fail "stop reason wrong: $(cat /tmp/upg-fail.json)"
upgraded relay1 || fail "relay1 should have been upgraded before the relay2 failure"
if upgraded bp1; then fail "BP was reached despite the relay2 stop (BP-last/verify-before-next violated)"; fi
dc exec -T relay2 test -f /tmp/ouro-upgrade-state/rollback-relay2 || fail "no rollback dispatched to the failed relay2"
pass "D: stopped at relay2 (exit 30), relay1 upgraded, BP NOT reached, relay2 rolled back"
rm -f /tmp/upg-*.json

echo "p2-5 rolling-upgrade E2E: ALL PASSED"
