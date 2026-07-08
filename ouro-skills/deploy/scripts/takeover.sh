#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
MANIFEST="${OURO_LEGACY_MANIFEST:?OURO_LEGACY_MANIFEST required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-deploy-state}"
MARKER="$STATE_DIR/takeover-$MACHINE"
ROLLBACK="$STATE_DIR/takeover-$MACHINE.rollback"

if ! python3 - "$MANIFEST" "$MACHINE" <<'PY' >/dev/null 2>/dev/null
import json, sys
manifest = json.load(open(sys.argv[1]))
machine = sys.argv[2]
entry = manifest["machines"][machine]
assert entry["legacy_container_running"] is True
assert entry["keys_present"] is True
assert entry["node_socket_active"] is True
PY
then
  ouro_emit_error 20 "takeover_precondition_failed" "legacy container, keys, or node socket not ready"
fi

ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && cp '$MANIFEST' '$ROLLBACK' && touch '$MARKER'"
