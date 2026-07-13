#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-upgrade-state}"
# Test-only failure injection via a state marker. Env-var hooks are stripped by the
# `ouro-ops tool run` env allowlist (p5-2), so injection must go through allowlisted state.
if [[ -f "$STATE_DIR/__test_inject_fail__$MACHINE" ]]; then
  ouro_emit_error 30 "upgrade_verify_failed" "verification failed for $MACHINE"
fi
# Test-only unknown-state injection (exit 40) via an allowlisted marker — drives the T3
# failure-discipline invariant (exit 40 => stop ALL writes).
if [[ -f "$STATE_DIR/__test_inject_unknown__$MACHINE" ]]; then
  ouro_emit_error 40 "upgrade_state_unknown" "node state unknown for $MACHINE; stop all writes and escalate"
fi

DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
SOCK="$(ouro_node_socket)"
SPEC="${OURO_SPEC:-}"
ROLE=""
NET="$(ouro_network_args)"   # p5-14: network from the spec (mainnet-aware)
if [ -n "$SPEC" ]; then
  ROLE="$(python3 - "$SPEC" "$MACHINE" <<'PY'
import yaml,sys
s=yaml.safe_load(open(sys.argv[1])); mid=sys.argv[2]
print(next((m["role"] for m in s["machines"] if m["id"]==mid),""))
PY
)" || true
fi

# Real node host if a node is running/socketed, OR role=bp on a node-provisioned host
# ($DEVNET/config.json exists — absent on the dev host so unit tests stay in marker mode). On
# such a role=bp host a missing socket/process is a FAILURE, never a marker pass (that is how a
# dead BP slipped through before).
if [ -S "$SOCK" ] || ouro_node_running || { [ "$ROLE" = "bp" ] && [ -f "$DEVNET/config.json" ]; }; then
  export CARDANO_NODE_SOCKET_PATH="$SOCK"
  PRE_PID="$(cat "$STATE_DIR/pre-pid-$MACHINE" 2>/dev/null || echo "")"
  PRE_BLOCK="$(cat "$STATE_DIR/pre-block-$MACHINE" 2>/dev/null || echo -1)"
  # Ground-truth: the node must be a NEW process (restarted) AND forge PAST the pre-restart
  # block. `block > PRE_BLOCK` (not `> 0`) rejects a node that merely replays the preserved db.
  NEW_PID="$(ouro_node_pid)"
  [ -n "$NEW_PID" ] || ouro_emit_error 30 "upgrade_verify_failed" "no running node on $MACHINE after upgrade"
  if [ -n "$PRE_PID" ] && [ "$NEW_PID" = "$PRE_PID" ]; then
    ouro_emit_error 30 "upgrade_verify_failed" "node PID unchanged ($NEW_PID) — restart did not happen on $MACHINE"
  fi
  found=0
  for _ in $(seq 1 40); do
    blk=$(ouro_cardano_cli query tip $NET 2>/dev/null \
          | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1)
    if [ "${blk:--1}" -gt "${PRE_BLOCK:--1}" ] 2>/dev/null; then found=1; break; fi
    sleep 3
  done
  if [ "$found" = 1 ]; then
    ouro_emit_ok false "upgraded node verified: restarted (pid $PRE_PID->$NEW_PID) + forging past $PRE_BLOCK (block=$blk)"
  else
    ouro_emit_error 30 "upgrade_verify_failed" "node did not forge past pre-upgrade block $PRE_BLOCK on $MACHINE"
  fi
else
  ouro_emit_ok false "upgrade verification passed"
fi
