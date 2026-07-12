#!/usr/bin/env bash
# S0017 p4-6 — REAL offline-path KES install, executed ON the block producer (dispatched).
# The second half of the offline rotation: the operator has cold-signed the staged kes.vkey on
# the air-gapped machine and returned node.cert (placed at $STAGE/node.cert.signed). This script
# installs that opcert TOGETHER WITH the matching staged kes.skey (they are a matched pair), then
# restarts the node onto them and ground-truths the result. cold.skey never touched here.
#
# Authority note: the opcert issue COUNTER lives with the cold key on the air-gapped machine, so
# this BP-side script cannot read it. The authoritative check is the node's own view: after
# install, `query kes-period-info` on-disk counter must ADVANCE (a replayed/stale opcert has a
# non-increasing counter and the node refuses to forge) AND the node must restart + forge past the
# pre-restart block. Any failure ROLLS BACK to the previous key + opcert and restarts onto them.
#
# Destructive → confirm-bound (see cli.rs CONFIRM_BOUND_TOOLS).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
POOL="$DEVNET/pools-keys/pool1"
SOCK="$DEVNET/node.socket"
MAGIC="${OURO_NETWORK_MAGIC:-1}"
STAGE="$POOL/offline-stage"
SIGNED="$STAGE/node.cert.signed"
export CARDANO_NODE_SOCKET_PATH="$SOCK"

command -v cardano-cli >/dev/null || ouro_emit_error 20 "no_cardano_cli" "cardano-cli not on target"
[ -f "$SIGNED" ]                  || ouro_emit_error 20 "no_signed_opcert" "no cold-signed node.cert at $SIGNED"
[ -s "$STAGE/kes.skey.staged" ]   || ouro_emit_error 20 "no_staged_key" "no staged KES skey (run generate-offline first)"
[ -s "$STAGE/kes.vkey.staged" ]   || ouro_emit_error 20 "no_staged_key" "no staged KES vkey (run generate-offline first)"

# On-disk opcert counter from the live node (kes-period-info prints "✓ …" lines before the JSON).
ondisk_counter() {
  cardano-cli query kes-period-info --op-cert-file "$1" --testnet-magic "$MAGIC" 2>/dev/null \
    | python3 -c 'import json,sys; s=sys.stdin.read(); i=s.find("{"); print(json.loads(s[i:]).get("qKesOnDiskOperationalCertificateNumber",-1) if i>=0 else -1)' 2>/dev/null || echo -1
}
tip_block() {
  cardano-cli query tip --testnet-magic "$MAGIC" 2>/dev/null | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1
}

BEFORE=$(ondisk_counter "$POOL/opcert.cert")
# The pre-install ground-truth read MUST succeed, else "AFTER > BEFORE" could false-pass.
[ "${BEFORE:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "kes_precheck_failed" "could not read on-disk opcert counter before install"
PRE_PID="$(ouro_node_pid)"
PRE_BLOCK="$(tip_block)"
[ "${PRE_BLOCK:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "kes_precheck_failed" "could not read tip before install"

# Snapshot the current live key + opcert so we can roll back if the new pair does not forge.
BACKUP="$STAGE/rollback"
mkdir -p "$BACKUP"; chmod 700 "$BACKUP"
cp -f "$POOL/opcert.cert" "$BACKUP/opcert.cert"
cp -f "$POOL/kes.skey"    "$BACKUP/kes.skey"
[ -f "$POOL/kes.vkey" ] && cp -f "$POOL/kes.vkey" "$BACKUP/kes.vkey" || true

rollback_and_die() {  # $1=code $2=detail — restore the previous pair, restart onto it, then error.
  local code="$1" detail="$2"
  echo "[kes] ROLLBACK: $detail — restoring previous opcert/KES key" >&2
  cp -f "$BACKUP/opcert.cert" "$POOL/opcert.cert"
  cp -f "$BACKUP/kes.skey"    "$POOL/kes.skey"
  [ -f "$BACKUP/kes.vkey" ] && cp -f "$BACKUP/kes.vkey" "$POOL/kes.vkey" || true
  local mode
  mode="$(ouro_node_effective_mode "$(ouro_declared_mode "${OURO_SPEC:-}" "$MACHINE")")"
  # Best-effort restart onto the restored pair; if even that is not actionable, escalate unknown.
  case "$mode" in
    none|ambiguous|mismatch) ouro_emit_unknown "rollback_restart_unresolved" \
        "$detail; rollback files restored but could not restart onto them (mode=$mode)";;
    *) ouro_node_restart_mode "$mode" || true ;;
  esac
  ouro_emit_error "$code" "kes_push_rolled_back" "$detail"
}

echo "[kes] installing cold-signed opcert + staged KES key (prior on-disk counter=$BEFORE)" >&2
# Install atomically: promote the staged KES skey/vkey and the cold-signed opcert together.
mv -f "$STAGE/kes.skey.staged" "$POOL/kes.skey"
mv -f "$STAGE/kes.vkey.staged" "$POOL/kes.vkey"
cp -f "$SIGNED" "$POOL/opcert.cert"

echo "[kes] restarting node onto the installed opcert" >&2
MODE="$(ouro_node_effective_mode "$(ouro_declared_mode "${OURO_SPEC:-}" "$MACHINE")")"
ouro_node_guard_mode "$MODE"
ouro_node_restart_mode "$MODE"

# Ground-truth #1: a genuine restart (new PID). #2: the node forges PAST the pre-restart block
# (rejects a node that starts but cannot forge on a bad opcert). #3: the on-disk opcert counter
# advanced (rejects a replayed/stale opcert). Any failure rolls back.
NEW_PID="$(ouro_node_pid)"
if [ -n "$PRE_PID" ] && [ "$NEW_PID" = "$PRE_PID" ]; then
  rollback_and_die 30 "node PID unchanged ($NEW_PID) after opcert install"
fi
FORGED=0; BLK=-1
for _ in $(seq 1 40); do
  sleep 3
  BLK=$(tip_block)
  [ "${BLK:--1}" -gt "${PRE_BLOCK:--1}" ] 2>/dev/null && { FORGED=1; break; }
done
[ "$FORGED" = 1 ] || rollback_and_die 30 "node did not forge past block $PRE_BLOCK after opcert install"

AFTER=$(ondisk_counter "$POOL/opcert.cert")
[ "${AFTER:--1}" -gt "${BEFORE:--1}" ] 2>/dev/null \
  || rollback_and_die 30 "on-disk opcert counter did not advance ($BEFORE -> $AFTER) — stale/replayed cert"

# Success: the staged key is now live; clean the staging hand-off artifacts (keep rollback backup).
rm -f "$SIGNED" "$STAGE/kes.period.staged"

python3 - "$MACHINE" "$BEFORE" "$AFTER" "$BLK" "${OURO_AUDIT_ID:-}" <<'PY'
import json, sys
mid, before, after, blk, audit_id = sys.argv[1:6]
before, after, blk = int(before), int(after), int(blk)
checks = []
def add(name, ok, detail):
    checks.append({"name": name, "pass": bool(ok), "severity": "info" if ok else "critical",
                   "exit_class": 0 if ok else 30, "rollback_safe": True, "detail": detail})
add(f"{mid}.opcert_counter_advanced", after > before, f"on-disk opcert counter {before} -> {after}")
add(f"{mid}.node_forging_after_install", blk > 0, f"tip block after restart = {blk}")
add(f"{mid}.kes_period_info_valid", after >= 0, f"query kes-period-info returned counter {after}")
exit_code = max([c["exit_class"] for c in checks if not c["pass"]], default=0)
payload = {"tool": "kes-rotation/push-offline", "machine": mid,
           "status": "ok" if exit_code == 0 else "error", "changed": True, "checks": checks,
           "data": {"counter_before": before, "counter_after": after, "tip_block_after": blk},
           "duration_s": 0.0, "audit_id": (audit_id or None)}
if exit_code:
    payload["error"] = {"code": f"exit_{exit_code}", "detail": "kes offline push ground-truth failed",
                        "hint": "inspect node log /var/log/cardano-node.log"}
print(json.dumps(payload, separators=(",", ":")))
sys.exit(exit_code)
PY
