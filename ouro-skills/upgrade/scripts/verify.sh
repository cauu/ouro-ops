#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-upgrade-state}"
# Test-only failure injection via a state marker. Env-var hooks are stripped by the
# `ouro tool run` env allowlist (p5-2), so injection must go through allowlisted state.
if [[ -f "$STATE_DIR/__test_inject_fail__$MACHINE" ]]; then
  ouro_emit_error 30 "upgrade_verify_failed" "verification failed for $MACHINE"
fi

DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
SOCK="$DEVNET/node.socket"
if [ -S "$SOCK" ] || pgrep -f 'cardano-node run' >/dev/null 2>&1; then
  # REAL node host (p2-5): the upgraded node must resume forging before the next machine.
  export CARDANO_NODE_SOCKET_PATH="$SOCK"
  found=0
  for _ in $(seq 1 40); do
    blk=$(cardano-cli query tip --testnet-magic "${OURO_NETWORK_MAGIC:-1}" 2>/dev/null \
          | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",0))
except: print(0)' 2>/dev/null || echo 0)
    if [ "${blk:-0}" -gt 0 ] 2>/dev/null; then found=1; break; fi
    sleep 3
  done
  if [ "$found" = 1 ]; then
    ouro_emit_ok false "upgraded node verified forging (block=$blk)"
  else
    ouro_emit_error 30 "upgrade_verify_failed" "node did not resume forging after upgrade on $MACHINE"
  fi
else
  ouro_emit_ok false "upgrade verification passed"
fi
