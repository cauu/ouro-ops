#!/usr/bin/env bash
# S0017 p4-1 — REAL KES cold-signing roundtrip against the bed's cardano environment.
# The bed's bp1 stands in for BOTH the online BP and the air-gapped cold machine (its cold.skey
# is co-located for the test). Proves that the script `ouro-ops kes cold-sign-script` generates,
# run with a real cardano-cli + real cold.skey + real counter, produces a VALID operational
# certificate — and that the script embeds NO private key and cold.skey is never modified/moved.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-cs-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
bp() { dc exec -T bp1 bash -lc "$1"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT
MAGIC=1
POOL=/opt/devnet/pools-keys/pool1
SOCK=/opt/devnet/node.socket

echo "[cs] rebuild base + up (bp1 forging) + provision"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

echo "[cs] on bp1: generate a fresh KES key pair (kes.skey stays; kes.vkey is public)"
bp "cardano-cli node key-gen-KES --verification-key-file /tmp/cs.kes.vkey --signing-key-file /tmp/cs.kes.skey >/dev/null" \
  || fail "kes key-gen failed"
bp "test -s /tmp/cs.kes.vkey" || fail "no kes vkey produced"

echo "[cs] compute the KES period from the live chain tip"
PERIOD=$(bp "SPK=\$(python3 -c 'import json;print(json.load(open(\"/opt/devnet/shelley-genesis.json\"))[\"slotsPerKESPeriod\"])'); \
  SLOT=\$(CARDANO_NODE_SOCKET_PATH=$SOCK cardano-cli query tip --testnet-magic $MAGIC 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)[\"slot\"])'); \
  echo \$(( SLOT / SPK ))" | tr -d '\r')
[ -n "$PERIOD" ] && [ "$PERIOD" -ge 0 ] 2>/dev/null || fail "could not compute KES period (got '$PERIOD')"
pass "fresh KES vkey generated; KES period = $PERIOD"

echo "[cs] generate the cold-sign SCRIPT (ouro-ops, embeds only the public vkey + period)"
bp "ouro-ops kes cold-sign-script --kes-vkey /tmp/cs.kes.vkey --kes-period $PERIOD > /tmp/coldsign.sh" \
  || fail "cold-sign-script generation failed"
bp "test -s /tmp/coldsign.sh" || fail "empty cold-sign script"

echo "[cs] the generated script contains NO private key material"
# the cold key's cborHex must NOT appear in the script; nor any SigningKey marker.
COLD_CBOR=$(bp "python3 -c 'import json;print(json.load(open(\"$POOL/cold.skey\"))[\"cborHex\"])'" | tr -d '\r')
[ -n "$COLD_CBOR" ] || fail "could not read cold.skey cborHex for the leak check"
if bp "grep -qF '$COLD_CBOR' /tmp/coldsign.sh"; then fail "LEAK: cold.skey cborHex is in the generated script"; fi
if bp "grep -qiE 'SigningKey|kes\\.skey' /tmp/coldsign.sh"; then fail "generated script references a signing key"; fi
pass "cold-sign script embeds only public data (no cold cborHex, no SigningKey)"

echo "[cs] run the script on the (stand-in) air-gapped machine → real operational certificate"
COLD_SHA_BEFORE=$(bp "sha256sum $POOL/cold.skey | cut -d' ' -f1" | tr -d '\r')
CTR_BEFORE=$(bp "python3 -c 'import json;print(json.load(open(\"$POOL/opcert.counter\")).get(\"cborHex\",\"\"))' 2>/dev/null || cat $POOL/opcert.counter" | tr -d '\r')
bp "COLD_SKEY=$POOL/cold.skey COUNTER=$POOL/opcert.counter OUT=/tmp/cs.node.cert bash /tmp/coldsign.sh >/dev/null" \
  || fail "cold-sign script execution failed"
bp "test -s /tmp/cs.node.cert" || fail "no node.cert produced by the script"

echo "[cs] the certificate is VALID (real cardano-cli reads it against the chain)"
bp "CARDANO_NODE_SOCKET_PATH=$SOCK cardano-cli query kes-period-info --op-cert-file /tmp/cs.node.cert --testnet-magic $MAGIC >/dev/null 2>&1" \
  || fail "cardano-cli rejected the generated node.cert (invalid opcert)"
pass "script produced a valid operational certificate (kes-period-info accepts it)"

echo "[cs] cold.skey was neither modified nor moved; only the counter advanced"
COLD_SHA_AFTER=$(bp "sha256sum $POOL/cold.skey | cut -d' ' -f1" | tr -d '\r')
[ "$COLD_SHA_AFTER" = "$COLD_SHA_BEFORE" ] || fail "cold.skey CHANGED during cold-signing ($COLD_SHA_BEFORE -> $COLD_SHA_AFTER)"
bp "test -f $POOL/cold.skey" || fail "cold.skey missing after cold-signing (it must never move)"
CTR_AFTER=$(bp "python3 -c 'import json;print(json.load(open(\"$POOL/opcert.counter\")).get(\"cborHex\",\"\"))' 2>/dev/null || cat $POOL/opcert.counter" | tr -d '\r')
[ "$CTR_AFTER" != "$CTR_BEFORE" ] || fail "opcert counter did not advance (issue-op-cert did not consume it)"
pass "cold.skey unchanged + in place; opcert counter advanced (issued in place)"

echo "p4-1 KES cold-signing roundtrip E2E: ALL PASSED"
