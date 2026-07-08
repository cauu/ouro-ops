#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
SPEC="${OURO_SPEC:?OURO_SPEC required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-upgrade-state}"
LOCK="$STATE_DIR/upgrade.lock"
mkdir -p "$STATE_DIR"

if [[ -f "$LOCK" ]]; then
  ouro_emit_error 10 "upgrade_lock_held" "another upgrade is already running"
fi
printf '%s\n' "$OURO_AUDIT_ID" > "$LOCK"
trap 'rm -f "$LOCK"' EXIT

ORDER="$(python3 - "$SPEC" <<'PY'
import sys, yaml
spec = yaml.safe_load(open(sys.argv[1]))
relays = [m["id"] for m in spec["machines"] if m["role"] == "relay"]
bp = [m["id"] for m in spec["machines"] if m["role"] == "bp"]
print(" ".join(relays + bp))
PY
)"

completed=()
for machine in $ORDER; do
  export OURO_MACHINE="$machine"
  export OURO_TOOL_NAME="upgrade/upgrade-one"
  if ! bash "$ROOT/ouro-skills/upgrade/scripts/upgrade-one.sh" >/tmp/ouro-upgrade-one.json; then
    bash "$ROOT/ouro-skills/upgrade/scripts/rollback.sh" >/tmp/ouro-upgrade-rollback.json || true
    ouro_emit_error 30 "upgrade_one_failed" "upgrade failed for $machine"
  fi
  export OURO_TOOL_NAME="upgrade/verify"
  if ! bash "$ROOT/ouro-skills/upgrade/scripts/verify.sh" >/tmp/ouro-upgrade-verify.json; then
    export OURO_TOOL_NAME="upgrade/rollback"
    bash "$ROOT/ouro-skills/upgrade/scripts/rollback.sh" >/tmp/ouro-upgrade-rollback.json || true
    ouro_emit_error 30 "upgrade_verify_failed" "verification failed for $machine; batch stopped"
  fi
  completed+=("$machine")
done

export OURO_TOOL_NAME="upgrade/run"
python3 - "$OURO_AUDIT_ID" "${completed[@]}" <<'PY'
import json, sys
audit_id = sys.argv[1]
completed = sys.argv[2:]
print(json.dumps({
  "tool": "upgrade/run",
  "machine": None,
  "status": "ok",
  "changed": True,
  "checks": [{
    "name": "bp_last",
    "pass": True,
    "severity": "info",
    "exit_class": 0,
    "rollback_safe": True,
    "detail": "relays upgraded before bp",
  }],
  "duration_s": 0.0,
  "audit_id": audit_id,
  "data": {
    "completed": completed,
    "lock": "released",
    "rollback_stop": True,
  }
}, separators=(",", ":")))
PY
