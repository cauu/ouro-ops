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

if [ -f "$DEVNET/config.json" ] && ouro_node_running; then
  # REAL node host: rolling-restart the running cardano-node (db preserved to avoid the p2-0
  # cold-start). Record the pre-restart PID so runtime/verify can prove a genuine restart.
  PRE_PID="$(ouro_node_pid)"
  printf '%s' "$PRE_PID" > "$STATE_DIR/pre-pid-$MACHINE"
  # p2-5: dispatch by detected mode, cross-checked against the spec declaration.
  # Resolve is pure; the guard emits exit 40 at TOP LEVEL on none/ambiguous/mismatch.
  MODE="$(ouro_node_effective_mode "$(ouro_declared_mode "${OURO_SPEC:-}" "$MACHINE")")"
  ouro_node_guard_mode "$MODE"
  ouro_node_restart_mode "$MODE"
  date +%s > "$MARKER"
  ouro_emit_ok true "node restarted (was pid $PRE_PID)"
else
  # Marker mode: non-node host / deterministic unit tests.
  date +%s > "$MARKER"
  ouro_emit_ok true "runtime restart recorded"
fi
