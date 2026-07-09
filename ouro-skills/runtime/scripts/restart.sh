#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-runtime-state}"
MARKER="$STATE_DIR/restarted-$MACHINE"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
mkdir -p "$STATE_DIR"

if [ -f "$DEVNET/config.json" ] && pgrep -f 'cardano-node run' >/dev/null 2>&1; then
  # REAL node host: rolling-restart the running cardano-node (db preserved to avoid the p2-0
  # cold-start). Record the pre-restart PID so runtime/verify can prove a genuine restart.
  POOL="$DEVNET/pools-keys/pool1"; SOCK="$DEVNET/node.socket"
  PRE_PID="$(pgrep -f 'cardano-node run' | head -1)"
  printf '%s' "$PRE_PID" > "$STATE_DIR/pre-pid-$MACHINE"
  pkill -f 'cardano-node run' 2>/dev/null || true
  sleep 2
  setsid cardano-node run \
    --config "$DEVNET/config.json" --topology "$DEVNET/topology.json" \
    --database-path "$DEVNET/db" --socket-path "$SOCK" \
    --shelley-kes-key "$POOL/kes.skey" --shelley-vrf-key "$POOL/vrf.skey" \
    --shelley-operational-certificate "$POOL/opcert.cert" --port 3001 \
    >/var/log/cardano-node.log 2>&1 < /dev/null &
  date +%s > "$MARKER"
  ouro_emit_ok true "node restarted (was pid $PRE_PID)"
else
  # Marker mode: non-node host / deterministic unit tests.
  date +%s > "$MARKER"
  ouro_emit_ok true "runtime restart recorded"
fi
