#!/usr/bin/env bash
# S0017 p4-2 — REAL staged pool-registration cold-sign roundtrip against the bed. Registers a
# SECOND pool on the live devnet through the full staged flow, with the pool COLD key signing
# only via the generated cold-sign script:
#
#   0. operator pre-creates the new pool's cold/vrf/stake keys (offline) -> staged public vkeys.
#   1. dispatch deploy/register-build -> bp1 builds the UNSIGNED registration tx + online
#      witnesses (payment + owner stake), returns the public tx body + pool id. cold.skey untouched.
#   2. the agent turns the returned tx body into a cold-sign script via `ouro-ops deploy
#      cold-sign-script`; the operator runs it on the air-gapped machine to witness with cold.skey.
#   3. evidence-bound confirm, then dispatch deploy/register-submit -> bp1 assembles all witnesses,
#      submits, and ground-truths the pool id is registered on chain.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-reg-$$}"
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
D=/opt/devnet
STAGE="$D/deploy-stage"

echo "[reg] rebuild base + up (bp1 forging) + provision (project=$OURO_E2E_PROJECT)"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

echo "[reg] step 0 — operator pre-creates the new pool's keys OFFLINE (staged public vkeys)"
bp "set -e; rm -rf $STAGE; mkdir -p $STAGE; cd $STAGE
  cardano-cli conway node key-gen --cold-verification-key-file cold.vkey --cold-signing-key-file cold.skey --operational-certificate-issue-counter-file cold.counter >/dev/null
  cardano-cli conway node key-gen-VRF --verification-key-file vrf.vkey --signing-key-file vrf.skey >/dev/null
  cardano-cli conway stake-address key-gen --verification-key-file stake.vkey --signing-key-file stake.skey >/dev/null
  printf '{\"pledge\":0,\"cost\":340000000,\"margin\":0.05}' > params.json" \
  || fail "staging the new pool keys failed"
COLD_SHA_BEFORE=$(bp "sha256sum $STAGE/cold.skey | cut -d' ' -f1" | tr -d '\r')
pass "staged new pool cold/vrf/stake keys + params"

echo "[reg] step 1 — dispatch deploy/register-build (build unsigned tx + online witnesses)"
build=$(ctl ouro-ops tool run deploy/register-build --dispatch bp1 --spec "$SPEC")
echo "$build" | jqpy "d['status']" | grep -qx ok || fail "register-build not ok: $build"
POOLID=$(echo "$build" | jqpy "d['data']['pool_id']")
[ -n "$POOLID" ] || fail "no pool_id returned"
echo "$build" | jqpy "d['data']['signed']" | grep -qx False || fail "register-build must return an UNSIGNED tx"
# register-build must NOT have touched cold.skey (cold key only signs offline).
COLD_SHA_MID=$(bp "sha256sum $STAGE/cold.skey | cut -d' ' -f1" | tr -d '\r')
[ "$COLD_SHA_MID" = "$COLD_SHA_BEFORE" ] || fail "register-build modified cold.skey (must never touch it)"
pass "built unsigned registration tx for $POOLID; cold.skey untouched"

echo "[reg] step 2 — agent turns the returned tx body into a cold-sign script; operator witnesses"
echo "$build" | python3 -c 'import json,sys; sys.stdout.write(json.load(sys.stdin)["data"]["tx_body"])' > /tmp/reg.tx.raw
[ -s /tmp/reg.tx.raw ] || fail "no returned tx body"
cat /tmp/reg.tx.raw | ctl bash -c 'cat > /tmp/reg.tx.raw'
ctl ouro-ops deploy cold-sign-script --tx-body /tmp/reg.tx.raw --cold-key cold --testnet-magic "$MAGIC" > /tmp/reg.coldsign.sh
grep -q 'transaction witness' /tmp/reg.coldsign.sh || fail "cold-sign script missing transaction witness"
grep -qi 'SigningKey' /tmp/reg.coldsign.sh && fail "cold-sign script leaked a signing-key marker"
# operator runs it on the air-gapped machine (bp1 holds cold.skey); witness lands in the stage dir.
cat /tmp/reg.coldsign.sh | bp "cat > /tmp/reg.coldsign.sh && COLD_SKEY=$STAGE/cold.skey COLD_WITNESS=$STAGE/cold.witness bash /tmp/reg.coldsign.sh >/dev/null" \
  || fail "cold-sign script execution failed"
bp "test -s $STAGE/cold.witness" || fail "no cold witness produced"
pass "cold witness produced from cold.skey (read in place, offline)"

echo "[reg] step 3 — evidence-bound confirm + dispatch deploy/register-submit (assemble + submit)"
fp=$(ctl ouro-ops tool run detect/runtime --dispatch bp1 --spec "$SPEC" | jqpy "d['data']['evidence_hash']")
tok=$(ctl ouro-ops confirm create --action deploy/register-submit --machine bp1 --runtime-evidence "$fp" | jqpy "d['data']['token']")
sub=$(ctl ouro-ops tool run deploy/register-submit --dispatch bp1 --spec "$SPEC" --confirm-token "$tok")
echo "$sub" | jqpy "d['status']" | grep -qx ok || fail "register-submit not ok: $sub"
echo "$sub" | jqpy "d['data']['registered']" | grep -qx True || fail "register-submit did not confirm registration: $sub"
subpool=$(echo "$sub" | jqpy "d['data']['pool_id']")
[ "$subpool" = "$POOLID" ] || fail "submit pool id mismatch ($subpool != $POOLID)"
pass "register-submit: pool $POOLID registered on chain"

echo "[reg] independent ground-truth — the pool id is in the ledger's stake-pool set"
inled=$(bp "CARDANO_NODE_SOCKET_PATH=$D/node.socket cardano-cli conway query stake-pools --testnet-magic $MAGIC 2>/dev/null" \
  | python3 -c 'import json,sys;print("yes" if sys.argv[1] in json.load(sys.stdin) else "no")' "$POOLID")
[ "$inled" = yes ] || fail "pool $POOLID not found in stake-pools"
pass "independent query stake-pools confirms $POOLID is registered"

echo "[reg] the confirm gate is enforced — submit without a token is refused"
noauth=$(ctl ouro-ops tool run deploy/register-submit --dispatch bp1 --spec "$SPEC" 2>&1 || true)
echo "$noauth" | grep -qi 'confirm\|token' || fail "register-submit ran without a confirmation token: $noauth"
pass "register-submit refuses to run without a target-bound confirmation token"

echo "p4-2 REAL pool-registration cold-sign roundtrip E2E: ALL PASSED"
