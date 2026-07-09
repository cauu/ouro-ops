#!/usr/bin/env bash
# p2-3 REAL KES rotation, executed ON the block producer (dispatched). Uses the actual
# cardano-cli opcert lifecycle against the live devnet: generate a fresh KES key, issue a
# new operational certificate with the INCREMENTED counter (signed by the cold key), install
# opcert + KES skey, restart the node onto them, and ground-truth via `query kes-period-info`
# + resumed forging. The counter file is the persisted monotonic counter.
#
# Security caveat (bed only): a real deployment issues the opcert OFFLINE (cold key never on
# the BP / in agent context) and pushes ONLY the opcert. The bed co-locates pool1's cold key
# for test convenience; the ground-truth being proven — counter increments, the node accepts
# the new opcert, forging continues — is independent of where issuance runs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
POOL="$DEVNET/pools-keys/pool1"
SOCK="$DEVNET/node.socket"
MAGIC="${OURO_NETWORK_MAGIC:-1}"
export CARDANO_NODE_SOCKET_PATH="$SOCK"

command -v cardano-cli >/dev/null || ouro_emit_error 20 "no_cardano_cli" "cardano-cli not on target"
[ -f "$POOL/cold.skey" ] || ouro_emit_error 20 "no_cold_key" "pool cold key not present on target"

# Current KES period = tip slot / slotsPerKESPeriod.
SPK=$(python3 -c 'import json;print(json.load(open("'"$DEVNET"'/shelley-genesis.json"))["slotsPerKESPeriod"])')
SLOT=$(cardano-cli query tip --testnet-magic "$MAGIC" 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["slot"])')
PERIOD=$(( SLOT / SPK ))

# On-disk opcert counter from the live node. `query kes-period-info` prints human-readable
# "✓ …" lines to stdout BEFORE the JSON, so parse from the first '{'.
ondisk_counter() {
  cardano-cli query kes-period-info --op-cert-file "$1" --testnet-magic "$MAGIC" 2>/dev/null \
    | python3 -c 'import json,sys; s=sys.stdin.read(); i=s.find("{"); print(json.loads(s[i:]).get("qKesOnDiskOperationalCertificateNumber",-1) if i>=0 else -1)' 2>/dev/null || echo -1
}
BEFORE=$(ondisk_counter "$POOL/opcert.cert")
# The pre-rotation ground-truth read MUST succeed, else `after == before+1` could false-pass
# (e.g. BEFORE=-1, AFTER=0 → 0 == -1+1). Also snapshot the pre-restart PID + block so the
# post-restart check proves a genuine restart + NEW forging (not a stale db replay).
[ "${BEFORE:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "kes_precheck_failed" "could not read on-disk opcert counter before rotation"
PRE_PID="$(pgrep -f 'cardano-node run' | head -1 || true)"
PRE_BLOCK="$(cardano-cli query tip --testnet-magic "$MAGIC" 2>/dev/null \
  | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1)"
[ "${PRE_BLOCK:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "kes_precheck_failed" "could not read tip before rotation"

echo "[kes] issuing new opcert (period=$PERIOD, prior on-disk counter=$BEFORE)" >&2
cardano-cli node key-gen-KES --verification-key-file "$POOL/kes.vkey.new" --signing-key-file "$POOL/kes.skey.new" >/dev/null
# issue-op-cert consumes the counter file and writes counter+1 back (persisted, monotonic).
cardano-cli node issue-op-cert \
  --kes-verification-key-file "$POOL/kes.vkey.new" \
  --cold-signing-key-file "$POOL/cold.skey" \
  --operational-certificate-issue-counter-file "$POOL/opcert.counter" \
  --kes-period "$PERIOD" --out-file "$POOL/opcert.cert.new" >/dev/null

# Install atomically, then restart the node onto the new opcert/KES skey.
mv -f "$POOL/kes.skey.new"  "$POOL/kes.skey"
mv -f "$POOL/kes.vkey.new"  "$POOL/kes.vkey"
mv -f "$POOL/opcert.cert.new" "$POOL/opcert.cert"

echo "[kes] restarting node onto rotated opcert" >&2
pkill -f 'cardano-node run' 2>/dev/null || true
sleep 2
# KEEP the existing db: the chain is unchanged (only the forging credential rotates). Wiping
# it would restart the node at the current wall-clock slot with no blocks to bridge the Praos
# forecast window -> NoLedgerView -> it never forges (the p2-0 cold-start trap).
setsid cardano-node run \
  --config "$DEVNET/config.json" --topology "$DEVNET/topology.json" \
  --database-path "$DEVNET/db" --socket-path "$SOCK" \
  --shelley-kes-key "$POOL/kes.skey" --shelley-vrf-key "$POOL/vrf.skey" \
  --shelley-operational-certificate "$POOL/opcert.cert" --port 3001 \
  >/var/log/cardano-node.log 2>&1 < /dev/null &

# Ground-truth: the node must be a NEW process (restarted) AND forge PAST the pre-restart block
# with the new opcert. `block > PRE_BLOCK` (not `> 0`) rejects a node that merely replays the
# preserved db without producing a new block (e.g. a bad opcert that starts but cannot forge).
NEW_PID="$(pgrep -f 'cardano-node run' | head -1 || true)"
if [ -n "$PRE_PID" ] && [ "$NEW_PID" = "$PRE_PID" ]; then
  ouro_emit_error 30 "node_did_not_restart" "node PID unchanged ($NEW_PID) after opcert rotation"
fi
FORGED=0
for _ in $(seq 1 40); do
  sleep 3
  BLK=$(cardano-cli query tip --testnet-magic "$MAGIC" 2>/dev/null \
        | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1)
  [ "${BLK:--1}" -gt "${PRE_BLOCK:--1}" ] 2>/dev/null && { FORGED=1; break; }
done
[ "$FORGED" = 1 ] || ouro_emit_error 30 "node_did_not_resume_forging" "node did not forge past block $PRE_BLOCK after opcert rotation"

AFTER=$(ondisk_counter "$POOL/opcert.cert")

python3 - "$MACHINE" "$PERIOD" "$BEFORE" "$AFTER" "$BLK" "${OURO_AUDIT_ID:-}" <<'PY'
import json, sys
mid, period, before, after, blk, audit_id = sys.argv[1:7]
before, after, blk = int(before), int(after), int(blk)
checks = []
def add(name, ok, detail=""):
    checks.append({"name": name, "pass": bool(ok), "severity": "info" if ok else "critical",
                   "exit_class": 0 if ok else 30, "rollback_safe": True,
                   "detail": detail or ("pass" if ok else "fail")})
add(f"{mid}.opcert_counter_incremented", after == before + 1, f"on-disk opcert counter {before} -> {after}")
add(f"{mid}.node_forging_after_rotation", blk > 0, f"tip block after restart = {blk}")
add(f"{mid}.kes_period_info_valid", after >= 0, f"query kes-period-info returned counter {after}")
exit_code = max([c["exit_class"] for c in checks if not c["pass"]], default=0)
payload = {"tool": "kes-rotation/rotate", "machine": mid,
           "status": "ok" if exit_code == 0 else "error", "changed": True, "checks": checks,
           "data": {"kes_period": int(period), "counter_before": before, "counter_after": after,
                    "tip_block_after": blk},
           "duration_s": 0.0, "audit_id": (audit_id or None)}
if exit_code:
    payload["error"] = {"code": f"exit_{exit_code}", "detail": "kes rotation ground-truth failed",
                        "hint": "inspect node log /var/log/cardano-node.log"}
print(json.dumps(payload, separators=(",", ":")))
sys.exit(exit_code)
PY
