#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-upgrade-state}"
# Test-only failure injection via a state marker. Env-var hooks are stripped by the
# `ouro tool run` env allowlist (p5-2), so injection must go through allowlisted state.
if [[ -f "$STATE_DIR/__test_inject_fail__$MACHINE" ]]; then
  ouro_emit_error 30 "upgrade_verify_failed" "verification failed for $MACHINE"
fi
ouro_emit_ok false "upgrade verification passed"
