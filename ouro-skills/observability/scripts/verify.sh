#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:-gateway}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-observability-state}"
if [[ ! -f "$STATE_DIR/gateway-$MACHINE" ]]; then
  ouro_emit_error 20 "observability_gateway_missing" "gateway has not been installed"
fi
ouro_emit_ok false "observability gateway verification passed"
