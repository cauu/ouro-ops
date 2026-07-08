#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:-gateway}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-observability-state}"
MARKER="$STATE_DIR/gateway-$MACHINE"
if [[ -f "$MARKER" ]]; then
  rm -f "$MARKER"
  ouro_emit_ok true "observability gateway rolled back"
else
  ouro_emit_ok false "observability gateway already absent"
fi
