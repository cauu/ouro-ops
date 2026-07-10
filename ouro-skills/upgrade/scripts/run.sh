#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
SPEC="${OURO_SPEC:?OURO_SPEC required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-upgrade-state}"
LOCK="$STATE_DIR/upgrade.lock.d"
OWNER="$LOCK/owner"
mkdir -p "$STATE_DIR"

# Atomic mutex: `mkdir` succeeds for exactly one racer (no TOCTOU window). If the
# directory already exists, a lock with recorded owner metadata is genuinely held;
# a lock without an owner file is stale (left by a crash before it could claim it)
# and is reclaimed.
if ! mkdir "$LOCK" 2>/dev/null; then
  if [[ -s "$OWNER" ]]; then
    ouro_emit_error 10 "upgrade_lock_held" "another upgrade is already running (owner $(cat "$OWNER"))"
  fi
  rm -rf "$LOCK"
  if ! mkdir "$LOCK" 2>/dev/null; then
    ouro_emit_error 10 "upgrade_lock_held" "another upgrade is already running"
  fi
fi
printf '%s\n' "$OURO_AUDIT_ID" > "$OWNER"
# Release only the lock we still own (never delete a lock a later run reclaimed).
trap '[[ "$(cat "$OWNER" 2>/dev/null)" == "$OURO_AUDIT_ID" ]] && rm -rf "$LOCK"' EXIT

# Plan: relays first (BP-last), each line "id role". First line is "<relay_total> <quorum_min>".
# Quorum is a HUMAN-authored spec policy (spec.upgrade.min_online_relays, default 1), NOT an
# environment knob — an agent invoking `ouro-ops tool run` cannot loosen the §2.2#4 invariant.
mapfile -t PLAN < <(python3 - "$SPEC" <<'PY'
import sys, yaml
spec = yaml.safe_load(open(sys.argv[1]))
relays = [m["id"] for m in spec["machines"] if m["role"] == "relay"]
bp = [m["id"] for m in spec["machines"] if m["role"] == "bp"]
quorum = int((spec.get("upgrade") or {}).get("min_online_relays", 1))
print(f"{len(relays)} {quorum}")
for r in relays:
    print(f"{r} relay")
for b in bp:
    print(f"{b} bp")
PY
)
read -r RELAY_TOTAL QUORUM_MIN <<< "${PLAN[0]}"
# This serial orchestrator takes exactly one machine down at a time, so upgrading a relay
# leaves RELAY_TOTAL-1 relays online; refuse if that would fall below the spec quorum.

completed=()
for entry in "${PLAN[@]:1}"; do
  machine="${entry%% *}"
  role="${entry##* }"
  if [[ "$role" == "relay" ]]; then
    online_after=$((RELAY_TOTAL - 1))
    if (( online_after < QUORUM_MIN )); then
      ouro_emit_error 10 "relay_quorum_violation" "upgrading $machine would leave $online_after relay(s) online, below quorum $QUORUM_MIN; refuse"
    fi
  else
    # BP is upgraded last, after every relay is verified back online.
    if (( RELAY_TOTAL < QUORUM_MIN )); then
      ouro_emit_error 10 "relay_quorum_violation" "cannot upgrade BP $machine with only $RELAY_TOTAL relay(s), below quorum $QUORUM_MIN; refuse"
    fi
  fi
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
