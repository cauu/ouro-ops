#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-runtime-state}"
if [[ ! -f "$STATE_DIR/topology-$MACHINE" ]]; then
  ouro_emit_error 20 "runtime_topology_missing" "topology has not been applied for $MACHINE"
fi
ouro_emit_ok false "runtime verification passed"
