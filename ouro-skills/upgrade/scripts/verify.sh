#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
if [[ "${OURO_FAIL_MACHINE:-}" == "$MACHINE" ]]; then
  ouro_emit_error 30 "upgrade_verify_failed" "verification failed for $MACHINE"
fi
ouro_emit_ok false "upgrade verification passed"
