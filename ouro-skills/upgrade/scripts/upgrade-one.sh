#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-upgrade-state}"
MARKER="$STATE_DIR/upgraded-$MACHINE"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"

if pgrep -f 'cardano-node run' >/dev/null 2>&1; then
  # REAL node host (p2-5): a rolling restart onto the SAME config/db is the observable
  # upgrade delta. Keep the db (chain unchanged) — wiping it would re-trigger the p2-0
  # cold-start (NoLedgerView). Only the running process rotates.
  if [ -f "$MARKER" ]; then
    ouro_emit_ok false "node already rolling-upgraded"
  fi
  POOL="$DEVNET/pools-keys/pool1"
  SOCK="$DEVNET/node.socket"
  pkill -f 'cardano-node run' 2>/dev/null || true
  sleep 2
  setsid cardano-node run \
    --config "$DEVNET/config.json" --topology "$DEVNET/topology.json" \
    --database-path "$DEVNET/db" --socket-path "$SOCK" \
    --shelley-kes-key "$POOL/kes.skey" --shelley-vrf-key "$POOL/vrf.skey" \
    --shelley-operational-certificate "$POOL/opcert.cert" --port 3001 \
    >/var/log/cardano-node.log 2>&1 < /dev/null &
  mkdir -p "$STATE_DIR"; touch "$MARKER"
  ouro_emit_ok true "node rolling-restarted (real upgrade delta, db preserved)"
else
  # Marker mode: relay host without a managed node, or the deterministic unit tests.
  ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && touch '$MARKER'"
fi
