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
POOL="$(ouro_node_pool_dir)"
SOCK="$(ouro_node_socket)"
GENESIS="$(ouro_node_genesis_shelley)"
NET="$(ouro_network_args)"   # p5-14: network from the spec (mainnet-aware); OURO_NETWORK_MAGIC was never set
export CARDANO_NODE_SOCKET_PATH="$SOCK"

ouro_cardano_cli_available || ouro_emit_error 20 "no_cardano_cli" "ouro_cardano_cli not on target"
[ -f "$POOL/cold.skey" ] || ouro_emit_error 20 "no_cold_key" "pool cold key not present on target"

# Current KES period = tip slot / slotsPerKESPeriod.
SPK=$(python3 -c 'import json;print(json.load(open("'"$GENESIS"'"))["slotsPerKESPeriod"])')
SLOT=$(ouro_cardano_cli query tip $NET 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["slot"])')
PERIOD=$(( SLOT / SPK ))

# On-disk opcert counter from the live node. `query kes-period-info` prints human-readable
# "✓ …" lines to stdout BEFORE the JSON, so parse from the first '{'.
ondisk_counter() {
  ouro_cardano_cli query kes-period-info --op-cert-file "$1" $NET 2>/dev/null \
    | python3 -c 'import json,sys; s=sys.stdin.read(); i=s.find("{"); print(json.loads(s[i:]).get("qKesOnDiskOperationalCertificateNumber",-1) if i>=0 else -1)' 2>/dev/null || echo -1
}
BEFORE=$(ondisk_counter "$POOL/opcert.cert")
# The pre-rotation ground-truth read MUST succeed, else `after == before+1` could false-pass
# (e.g. BEFORE=-1, AFTER=0 → 0 == -1+1). Also snapshot the pre-restart PID + block so the
# post-restart check proves a genuine restart + NEW forging (not a stale db replay).
[ "${BEFORE:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "kes_precheck_failed" "could not read on-disk opcert counter before rotation"
PRE_PID="$(ouro_node_pid)"
PRE_BLOCK="$(ouro_cardano_cli query tip $NET 2>/dev/null \
  | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1)"
[ "${PRE_BLOCK:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "kes_precheck_failed" "could not read tip before rotation"

echo "[kes] issuing new opcert (period=$PERIOD, prior on-disk counter=$BEFORE)" >&2
ouro_cardano_cli node key-gen-KES --verification-key-file "$POOL/kes.vkey.new" --signing-key-file "$POOL/kes.skey.new" >/dev/null
# issue-op-cert consumes the counter file and writes counter+1 back (persisted, monotonic).
ouro_cardano_cli node issue-op-cert \
  --kes-verification-key-file "$POOL/kes.vkey.new" \
  --cold-signing-key-file "$POOL/cold.skey" \
  --operational-certificate-issue-counter-file "$POOL/opcert.counter" \
  --kes-period "$PERIOD" --out-file "$POOL/opcert.cert.new" >/dev/null

# Install atomically, then restart the node onto the new opcert/KES skey.
mv -f "$POOL/kes.skey.new"  "$POOL/kes.skey"
mv -f "$POOL/kes.vkey.new"  "$POOL/kes.vkey"
mv -f "$POOL/opcert.cert.new" "$POOL/opcert.cert"

echo "[kes] restarting node onto rotated opcert" >&2
# Restart onto the new opcert/KES skey (db preserved — see ouro_node_start's cold-start note),
# dispatched by detected mode + declaration cross-check (p2-5; fail-closed on drift).
MODE="$(ouro_node_effective_mode "$(ouro_declared_mode "${OURO_SPEC:-}" "$MACHINE")")"
ouro_node_guard_mode "$MODE"
ouro_node_restart_mode "$MODE"

# Ground-truth: the node must be a NEW process (restarted) AND forge PAST the pre-restart block
# with the new opcert. `block > PRE_BLOCK` (not `> 0`) rejects a node that merely replays the
# preserved db without producing a new block (e.g. a bad opcert that starts but cannot forge).
NEW_PID="$(ouro_node_pid)"
if [ -n "$PRE_PID" ] && [ "$NEW_PID" = "$PRE_PID" ]; then
  ouro_emit_error 30 "node_did_not_restart" "node PID unchanged ($NEW_PID) after opcert rotation"
fi
FORGED=0
for _ in $(seq 1 40); do
  sleep 3
  BLK=$(ouro_cardano_cli query tip $NET 2>/dev/null \
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
