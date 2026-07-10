#!/usr/bin/env bash
# p2-5 REAL multi-machine rolling upgrade orchestrator. Runs on control and DISPATCHES
# upgrade-one + verify to each target (relays first, BP last), enforcing the §2.2#4
# invariants against the live bed:
#   - atomic lock: a concurrent rollout is refused (exit 10);
#   - relay quorum from spec.upgrade.min_online_relays (HUMAN policy, not an env knob):
#     taking a machine down below quorum is refused (exit 10) BEFORE any target is touched;
#   - BP-last: every relay is upgraded + verified before the BP;
#   - verify-before-next: a machine's verify must pass before the next machine; a failure
#     dispatches rollback to that machine and STOPS (exit 30) — the BP is never reached.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
SPEC="${OURO_SPEC:?OURO_SPEC required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-rollout-state}"
LOCK="$STATE_DIR/rollout.lock.d"
OWNER="$LOCK/owner"
mkdir -p "$STATE_DIR"

# Atomic mutex (mkdir has no TOCTOU window). A lock with an owner file is genuinely held;
# an owner-less lock is stale (crash before claim) and is reclaimed.
if ! mkdir "$LOCK" 2>/dev/null; then
  if [[ -s "$OWNER" ]]; then
    ouro_emit_error 10 "rollout_lock_held" "another rollout is already running (owner $(cat "$OWNER"))"
  fi
  rm -rf "$LOCK"; mkdir "$LOCK" 2>/dev/null || ouro_emit_error 10 "rollout_lock_held" "another rollout is already running"
fi
printf '%s\n' "$OURO_AUDIT_ID" > "$OWNER"
trap '[[ "$(cat "$OWNER" 2>/dev/null)" == "$OURO_AUDIT_ID" ]] && rm -rf "$LOCK"' EXIT

mapfile -t PLAN < <(python3 - "$SPEC" <<'PY'
import sys, yaml
spec = yaml.safe_load(open(sys.argv[1]))
relays = [m["id"] for m in spec["machines"] if m["role"] == "relay"]
bp = [m["id"] for m in spec["machines"] if m["role"] == "bp"]
quorum = int((spec.get("upgrade") or {}).get("min_online_relays", 1))
print(f"{len(relays)} {quorum}")
for r in relays: print(f"{r} relay")
for b in bp:     print(f"{b} bp")
PY
)
read -r RELAY_TOTAL QUORUM_MIN <<< "${PLAN[0]}"

completed=()
for entry in "${PLAN[@]:1}"; do
  machine="${entry%% *}"; role="${entry##* }"
  # Quorum check BEFORE touching the target (serial: one machine down at a time).
  if [[ "$role" == "relay" ]]; then
    online_after=$((RELAY_TOTAL - 1))
    (( online_after < QUORUM_MIN )) && ouro_emit_error 10 "relay_quorum_violation" \
      "upgrading $machine leaves $online_after relay(s), below quorum $QUORUM_MIN; refuse"
  else
    (( RELAY_TOTAL < QUORUM_MIN )) && ouro_emit_error 10 "relay_quorum_violation" \
      "cannot upgrade BP $machine with $RELAY_TOTAL relay(s), below quorum $QUORUM_MIN; refuse"
  fi
  # REAL per-machine step: dispatch upgrade-one, then verify, to the target itself.
  if ! ouro-ops tool run upgrade/upgrade-one --dispatch "$machine" --spec "$SPEC" >/tmp/ouro-rollout-one.json 2>&1; then
    ouro-ops tool run upgrade/rollback --dispatch "$machine" --spec "$SPEC" >/tmp/ouro-rollout-rb.json 2>&1 || true
    ouro_emit_error 30 "upgrade_one_failed" "upgrade-one failed for $machine; rolled back and stopped"
  fi
  if ! ouro-ops tool run upgrade/verify --dispatch "$machine" --spec "$SPEC" >/tmp/ouro-rollout-verify.json 2>&1; then
    ouro-ops tool run upgrade/rollback --dispatch "$machine" --spec "$SPEC" >/tmp/ouro-rollout-rb.json 2>&1 || true
    ouro_emit_error 30 "upgrade_verify_failed" "verify failed for $machine; rolled back and STOPPED (BP not reached)"
  fi
  completed+=("$machine")
done

python3 - "$OURO_AUDIT_ID" "${completed[@]}" <<'PY'
import json, sys
audit_id, completed = sys.argv[1], sys.argv[2:]
print(json.dumps({
  "tool": "upgrade/rollout", "machine": None, "status": "ok", "changed": True,
  "checks": [{"name": "bp_last", "pass": True, "severity": "info", "exit_class": 0,
              "rollback_safe": True, "detail": "relays dispatched+verified before bp"}],
  "duration_s": 0.0, "audit_id": audit_id,
  "data": {"completed": completed, "order": completed, "lock": "released", "verify_before_next": True},
}, separators=(",", ":")))
PY
