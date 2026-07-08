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
ouro_emit_ok false "takeover legacy node and rollback artifact verified"
