#!/usr/bin/env bash
# S0017 p4-2 — REAL pool-registration SUBMIT (online, dispatched). Final stage of the staged
# cold-sign deploy: assemble the unsigned tx with its witnesses (the online payment+stake ones
# from register-build plus the OFFLINE cold witness the operator returned) and submit it, then
# ground-truth that the pool id is registered on chain.
#
# Destructive (submits an on-chain tx) → confirm-bound (see cli.rs CONFIRM_BOUND_TOOLS).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
SOCK="$DEVNET/node.socket"
NET="$(ouro_network_args)"   # p5-14: network from the spec (mainnet-aware); OURO_NETWORK_MAGIC was never set
STAGE="$DEVNET/deploy-stage"
export CARDANO_NODE_SOCKET_PATH="$SOCK"
CLI=(ouro_cardano_cli conway)

ouro_cardano_cli_available || ouro_emit_error 20 "no_cardano_cli" "ouro_cardano_cli not on target"
for f in tx.raw w.pay w.stake cold.witness cold.vkey; do
  [ -s "$STAGE/$f" ] || ouro_emit_error 20 "missing_stage_artifact" "expected $STAGE/$f (run register-build + cold-sign first)"
done
cd "$STAGE"

POOLID=$("${CLI[@]}" stake-pool id --cold-verification-key-file cold.vkey)

# Idempotency / replay guard: if the pool is ALREADY registered, do not resubmit.
already() { "${CLI[@]}" query stake-pools $NET 2>/dev/null \
  | python3 -c 'import json,sys;print("yes" if sys.argv[1] in json.load(sys.stdin) else "no")' "$POOLID" 2>/dev/null || echo no; }
if [ "$(already)" = yes ]; then
  ouro_emit_error 20 "already_registered" "pool $POOLID already registered on chain"
fi

"${CLI[@]}" transaction assemble --tx-body-file tx.raw \
  --witness-file w.pay --witness-file w.stake --witness-file cold.witness --out-file tx.signed \
  || ouro_emit_error 30 "assemble_failed" "could not assemble witnessed tx"
TXHASH=$("${CLI[@]}" transaction txid --tx-file tx.signed 2>/dev/null || echo "")
"${CLI[@]}" transaction submit --tx-file tx.signed $NET >/dev/null \
  || ouro_emit_error 30 "submit_failed" "node rejected the registration tx"

# Ground-truth: the pool id must appear in the ledger's stake-pool set.
REGISTERED=no
for _ in $(seq 1 30); do
  sleep 2
  [ "$(already)" = yes ] && { REGISTERED=yes; break; }
done
[ "$REGISTERED" = yes ] || ouro_emit_error 30 "not_registered" "pool $POOLID not in stake-pools after submit"

python3 - "$MACHINE" "$POOLID" "$TXHASH" "${OURO_AUDIT_ID:-}" <<'PY'
import json, sys
mid, pool_id, txhash, audit_id = sys.argv[1:5]
checks = [{"name": f"{mid}.pool_registered", "pass": True, "severity": "info",
           "exit_class": 0, "rollback_safe": False,
           "detail": f"pool {pool_id} registered on chain (tx {txhash})"}]
payload = {"tool": "deploy/register-submit", "machine": mid, "status": "ok", "changed": True,
           "checks": checks,
           "data": {"pool_id": pool_id, "tx_hash": txhash, "registered": True},
           "duration_s": 0.0, "audit_id": (audit_id or None)}
print(json.dumps(payload, separators=(",", ":")))
PY
