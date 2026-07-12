#!/usr/bin/env bash
# S0017 p4-2 — REAL pool-registration BUILD (online, dispatched). First stage of the staged
# cold-sign deploy: gather the live chain snapshot and build the UNSIGNED registration tx, plus
# the ONLINE witnesses (payment + owner stake). The pool COLD key is NOT used here — its public
# cold.vkey goes into the pool-reg cert, and the cold witness is produced OFFLINE by the script
# `ouro-ops deploy cold-sign-script` emits. Only the public tx body + pool id leave here.
#
# Faithful key split: cold.skey stays offline (never read here); the operator pre-creates the new
# pool's cold/vrf/stake keys and stages the PUBLIC vkeys (+ the operational stake.skey) at
# $DEVNET/deploy-stage. Non-destructive (builds only; does not submit) → not confirm-bound.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
SOCK="$DEVNET/node.socket"
MAGIC="${OURO_NETWORK_MAGIC:-1}"
STAGE="$DEVNET/deploy-stage"
UTXO="$DEVNET/utxo-keys/utxo1"
export CARDANO_NODE_SOCKET_PATH="$SOCK"
CLI=(ouro_cardano_cli conway)

ouro_cardano_cli_available || ouro_emit_error 20 "no_cardano_cli" "ouro_cardano_cli not on target"
for f in cold.vkey vrf.vkey stake.vkey stake.skey; do
  [ -s "$STAGE/$f" ] || ouro_emit_error 20 "missing_staged_key" "staged pool key $STAGE/$f not present"
done
[ -s "$UTXO/utxo.skey" ] && [ -s "$UTXO/utxo.vkey" ] || ouro_emit_error 20 "no_funding_key" "funding utxo key not present"

cd "$STAGE"
# Pool parameters: staged params.json (pledge/cost/margin) with safe defaults.
read PLEDGE COST MARGIN < <(python3 -c '
import json,os
p={}
try: p=json.load(open("params.json"))
except Exception: pass
print(int(p.get("pledge",0)), int(p.get("cost",340000000)), p.get("margin",0.05))')

# stake registration deposit from live protocol params (avoid a hardcoded amount).
DEP=$("${CLI[@]}" query protocol-parameters --testnet-magic "$MAGIC" 2>/dev/null \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["stakeAddressDeposit"])') \
  || ouro_emit_error 30 "protocol_params_failed" "could not read protocol parameters"

"${CLI[@]}" stake-address registration-certificate \
  --stake-verification-key-file stake.vkey --key-reg-deposit-amt "$DEP" --out-file stake.reg.cert
"${CLI[@]}" stake-pool registration-certificate \
  --cold-verification-key-file cold.vkey --vrf-verification-key-file vrf.vkey \
  --pool-pledge "$PLEDGE" --pool-cost "$COST" --pool-margin "$MARGIN" \
  --pool-reward-account-verification-key-file stake.vkey \
  --pool-owner-stake-verification-key-file stake.vkey \
  --testnet-magic "$MAGIC" --out-file pool.reg.cert

# Funding: derive the utxo address and pick a tx-in.
"${CLI[@]}" address build --payment-verification-key-file "$UTXO/utxo.vkey" \
  --testnet-magic "$MAGIC" --out-file u1.addr
ADDR="$(cat u1.addr)"
TXIN=$("${CLI[@]}" query utxo --address "$ADDR" --testnet-magic "$MAGIC" --out-file /dev/stdout 2>/dev/null \
  | python3 -c 'import json,sys
u=json.load(sys.stdin)
print(sorted(u, key=lambda k:-u[k]["value"]["lovelace"])[0] if u else "")')
[ -n "$TXIN" ] || ouro_emit_error 30 "no_utxo" "funding address has no utxo"

# Build the UNSIGNED tx — `build` computes the fee against live params (3 witnesses: payment+stake+cold).
"${CLI[@]}" transaction build \
  --tx-in "$TXIN" --change-address "$ADDR" \
  --certificate-file stake.reg.cert --certificate-file pool.reg.cert \
  --witness-override 3 --testnet-magic "$MAGIC" --out-file tx.raw >/dev/null \
  || ouro_emit_error 30 "tx_build_failed" "transaction build failed"
[ -s tx.raw ] || ouro_emit_error 30 "tx_build_failed" "no unsigned tx produced"

# Online witnesses: payment (funds) + owner stake. The COLD witness is produced offline.
"${CLI[@]}" transaction witness --tx-body-file tx.raw --signing-key-file "$UTXO/utxo.skey" \
  --testnet-magic "$MAGIC" --out-file w.pay
"${CLI[@]}" transaction witness --tx-body-file tx.raw --signing-key-file stake.skey \
  --testnet-magic "$MAGIC" --out-file w.stake

POOLID=$("${CLI[@]}" stake-pool id --cold-verification-key-file cold.vkey)

python3 - "$MACHINE" "$POOLID" "$STAGE/tx.raw" "${OURO_AUDIT_ID:-}" <<'PY'
import json, sys
mid, pool_id, txraw, audit_id = sys.argv[1:5]
tx_body = open(txraw).read()
checks = [{"name": f"{mid}.register_tx_built", "pass": True, "severity": "info",
           "exit_class": 0, "rollback_safe": True, "detail": f"unsigned registration tx built for {pool_id}"}]
payload = {"tool": "deploy/register-build", "machine": mid, "status": "ok", "changed": True,
           "checks": checks,
           "data": {"pool_id": pool_id, "tx_body": tx_body, "cold_roles": ["cold"],
                    "online_witnesses": ["w.pay", "w.stake"], "signed": False},
           "duration_s": 0.0, "audit_id": (audit_id or None)}
print(json.dumps(payload, separators=(",", ":")))
PY
