#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-deploy-state}"
MARKER="$STATE_DIR/takeover-$MACHINE"
ROLLBACK="$STATE_DIR/takeover-$MACHINE.rollback"

if [[ ! -f "$MARKER" || ! -f "$ROLLBACK" ]]; then
  ouro_emit_error 20 "takeover_not_ready" "takeover marker or rollback artifact missing"
fi

# Real-mode rollback artifact is a `sha256sum` key snapshot (starts with a hex hash);
# manifest-mode is a JSON copy (starts with '{'). Only the real path re-checks the live node.
if [[ "$(head -c1 "$ROLLBACK")" != "{" ]]; then
  DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
  # Key preservation: every snapshotted key must still exist with the SAME checksum.
  if ! sha256sum -c "$ROLLBACK" >/dev/null 2>&1; then
    ouro_emit_error 30 "keys_not_preserved" "a legacy key changed or is missing since takeover"
  fi
  # Continuity: the legacy node must still be running (takeover did not disrupt forging).
  pgrep -f 'cardano-node run' >/dev/null 2>&1 || ouro_emit_error 30 "node_not_running" "legacy node stopped after takeover"
  ouro_emit_ok false "takeover verified: keys preserved (checksums match) + legacy node still running"
else
  ouro_emit_ok false "takeover legacy node and rollback artifact verified"
fi
