#!/usr/bin/env bash
# S0017 p4-6 — REAL end-to-end OFFLINE KES rotation against the bed. Exercises the full
# split flow through actual cardano-cli, with cold-signing done from the returned PUBLIC vkey:
#
#   1. dispatch kes-rotation/generate-offline -> bp1 stages a fresh KES key (node keeps forging on
#      the OLD key), returns the public kes_vkey + kes_period in its data.
#   2. the agent (control) turns that returned vkey into a cold-sign script via
#      `ouro-ops kes cold-sign-script`, and the operator runs it on the air-gapped machine
#      (bp1 here — it holds cold.skey) to issue node.cert, placed at the BP staging path.
#   3. evidence-bound confirm, then dispatch kes-rotation/push-offline -> bp1 promotes the staged
#      key + cold-signed opcert together, restarts, and ground-truths.
#
# Asserts the real cardano ground-truth: on-disk opcert counter ADVANCES, the node RESTARTS
# (new PID) and RESUMES forging, and cold.skey never moved.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-off-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }
bp() { dc exec -T bp1 bash -lc "$1"; }
jqpy() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT
SPEC=/opt/ouro/pool-spec.yaml
MAGIC=1
POOL=/opt/devnet/pools-keys/pool1
STAGE="$POOL/offline-stage"
SOCK=/opt/devnet/node.socket

# On-disk opcert counter from the live node (kes-period-info prints "✓ …" before the JSON).
kes_counter() {
  bp "CARDANO_NODE_SOCKET_PATH=$SOCK cardano-cli query kes-period-info --op-cert-file $POOL/opcert.cert --testnet-magic $MAGIC 2>/dev/null" \
    | python3 -c 'import json,sys; s=sys.stdin.read(); i=s.find("{"); print(json.loads(s[i:]).get("qKesOnDiskOperationalCertificateNumber",-1) if i>=0 else -1)'
}

echo "[off] rebuild base + up (bp1 forging) + provision (project=$OURO_E2E_PROJECT)"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

c0=$(kes_counter); pass "initial on-disk opcert counter = $c0"
COLD_SHA_BEFORE=$(bp "sha256sum $POOL/cold.skey | cut -d' ' -f1" | tr -d '\r')
pid_before=$(bp "pgrep -f 'cardano-node run' | head -1" | tr -d '\r')

echo "[off] step 1 — dispatch kes-rotation/generate-offline (stage new KES key; node keeps forging)"
gen=$(ctl ouro-ops tool run kes-rotation/generate-offline --dispatch bp1 --spec "$SPEC")
echo "$gen" | jqpy "d['status']" | grep -qx ok || fail "generate-offline not ok: $gen"
echo "$gen" | jqpy "d['data']['staged']" | grep -qx True || fail "generate-offline did not stage: $gen"
period=$(echo "$gen" | jqpy "d['data']['kes_period']")
vhash=$(echo "$gen" | jqpy "d['data']['kes_vkey_hash']")
# the running node MUST NOT have been disturbed by staging (still the same PID, still forging).
pid_mid=$(bp "pgrep -f 'cardano-node run' | head -1" | tr -d '\r')
[ -n "$pid_mid" ] && [ "$pid_mid" = "$pid_before" ] || fail "generate-offline disturbed the running node ($pid_before->$pid_mid)"
pass "staged new KES key for period $period; live node untouched (pid $pid_mid)"

echo "[off] step 2 — agent turns the RETURNED public vkey into a cold-sign script; operator signs"
# Extract the returned public vkey to a host file, hand it to ouro-ops on control (proves the
# public hand-off works end to end — control never reads bp1's filesystem for the key).
echo "$gen" | python3 -c 'import json,sys; sys.stdout.write(json.load(sys.stdin)["data"]["kes_vkey"])' > /tmp/off.kes.vkey
[ -s /tmp/off.kes.vkey ] || fail "no returned kes_vkey content"
# sanity: the returned content hashes to the advertised kes_vkey_hash.
got_hash=$(python3 -c 'import hashlib,sys;print(hashlib.sha256(open("/tmp/off.kes.vkey","rb").read()).hexdigest())')
[ "$got_hash" = "$vhash" ] || fail "returned kes_vkey content hash mismatch ($got_hash != $vhash)"
cat /tmp/off.kes.vkey | ctl bash -c 'cat > /tmp/off.kes.vkey'
ctl ouro-ops kes cold-sign-script --kes-vkey /tmp/off.kes.vkey --kes-period "$period" > /tmp/off.coldsign.sh
grep -q 'node issue-op-cert' /tmp/off.coldsign.sh || fail "generated cold-sign script missing issue-op-cert"
grep -qi 'SigningKey' /tmp/off.coldsign.sh && fail "cold-sign script leaked a signing-key marker"
# Operator runs the script on the air-gapped machine (bp1 holds cold.skey + counter); node.cert
# lands at the BP staging path push-offline reads.
cat /tmp/off.coldsign.sh | bp "cat > /tmp/off.coldsign.sh && COLD_SKEY=$POOL/cold.skey COUNTER=$POOL/opcert.counter OUT=$STAGE/node.cert.signed bash /tmp/off.coldsign.sh >/dev/null" \
  || fail "cold-sign script execution failed on the air-gapped machine"
bp "test -s $STAGE/node.cert.signed" || fail "no signed node.cert produced"
pass "operator issued node.cert from the returned public vkey (cold.skey read in place)"

echo "[off] step 3 — evidence-bound confirm + dispatch kes-rotation/push-offline (install + restart)"
fp=$(ctl ouro-ops tool run detect/runtime --dispatch bp1 --spec "$SPEC" | jqpy "d['data']['evidence_hash']")
tok=$(ctl ouro-ops confirm create --action kes-rotation/push-offline --machine bp1 --runtime-evidence "$fp" | jqpy "d['data']['token']")
push=$(ctl ouro-ops tool run kes-rotation/push-offline --dispatch bp1 --spec "$SPEC" --confirm-token "$tok")
echo "$push" | jqpy "d['status']" | grep -qx ok || fail "push-offline not ok: $push"
echo "$push" | jqpy "all(c['pass'] for c in d['checks'])" | grep -qx True || fail "push-offline checks not all pass: $push"
ca=$(echo "$push" | jqpy "d['data']['counter_after']")
cb=$(echo "$push" | jqpy "d['data']['counter_before']")
blk=$(echo "$push" | jqpy "d['data']['tip_block_after']")
[ "$ca" -gt "$cb" ] 2>/dev/null || fail "on-disk opcert counter did not advance ($cb -> $ca)"
[ "$blk" -gt 0 ] 2>/dev/null || fail "node not forging after offline install (block=$blk)"

pid_after=$(bp "pgrep -f 'cardano-node run' | head -1" | tr -d '\r')
[ -n "$pid_after" ] && [ "$pid_after" != "$pid_before" ] || fail "bp1 node PID unchanged ($pid_before->$pid_after) — not really restarted"
pass "push-offline: counter $cb -> $ca, node restarted (pid $pid_before->$pid_after) + forging (block=$blk)"

# Node's own kes-period-info confirms the rotated counter (independent of the script's report).
c1=$(kes_counter)
[ "$c1" -eq "$ca" ] 2>/dev/null || fail "node kes-period-info counter ($c1) != reported ($ca)"
pass "node kes-period-info confirms rotated counter = $c1"

echo "[off] cold.skey never moved or changed during the whole roundtrip"
COLD_SHA_AFTER=$(bp "sha256sum $POOL/cold.skey | cut -d' ' -f1" | tr -d '\r')
[ "$COLD_SHA_AFTER" = "$COLD_SHA_BEFORE" ] || fail "cold.skey CHANGED ($COLD_SHA_BEFORE -> $COLD_SHA_AFTER)"
bp "test -f $POOL/cold.skey" || fail "cold.skey missing after rotation"
pass "cold.skey unchanged + in place"

echo "p4-6 REAL offline KES rotation E2E: ALL PASSED"
