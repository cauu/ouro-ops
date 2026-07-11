#!/usr/bin/env bash
# S0017 automated acceptance — one command that validates the whole batch.
#
#   Phase 1 (fast): cargo tests + the python suite.
#   Phase 2 (docker): the real-container E2E suite that exercises everything this batch
#     changed — the supervisor adapter, supervision-mode detection, the target-bound
#     confirmation gate, container (compose) upgrade + rollback, systemd mode — plus core
#     regression (dispatch/audit/no-scp, KES, no-leak, rolling upgrade).
#
# The bed's `apt-get` occasionally fails to reach the mirrors (exit 100); that is transient
# infra, not a code fault, so docker checks auto-retry up to 3 times on that specific error.
# Prints a PASS/FAIL table and exits non-zero if any check failed. Requires a running docker.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
LOGDIR="$(pwd)/tmp/accept"; rm -rf "$LOGDIR"; mkdir -p "$LOGDIR"

RESULTS=()
record() { RESULTS+=("$1|$2|$3"); }   # name | PASS|FAIL | note

hr() { printf '%s\n' "------------------------------------------------------------"; }

# ---- Phase 1: fast tests -------------------------------------------------------------
hr; echo "[accept] Phase 1 — fast tests (cargo + python)"; hr

# Single-threaded avoids a pre-existing parallel-test race on a global env var (unrelated).
if cargo test -q -- --test-threads=1 > "$LOGDIR/cargo.log" 2>&1; then
  n=$(grep -oE '[0-9]+ passed' "$LOGDIR/cargo.log" | head -1)
  echo "  PASS  cargo ($n)"; record "cargo unit/integration" PASS "$n"
else
  echo "  FAIL  cargo (see $LOGDIR/cargo.log)"; record "cargo unit/integration" FAIL "see log"
fi

pyp=0; pyf=0
for t in tests/test_*.py; do
  if python3 "$t" > "$LOGDIR/py.log" 2>&1; then pyp=$((pyp+1)); else pyf=$((pyf+1)); echo "  FAIL  python: $t"; cp "$LOGDIR/py.log" "$LOGDIR/py-fail-$(basename "$t").log"; fi
done
if [ "$pyf" = 0 ]; then echo "  PASS  python ($pyp files)"; record "python suite" PASS "$pyp files"; \
  else echo "  FAIL  python ($pyf/$((pyp+pyf)) failed)"; record "python suite" FAIL "$pyf failed"; fi

# ---- Phase 2: docker E2E (auto-retry the transient apt flake) -------------------------
hr; echo "[accept] Phase 2 — docker E2E suite"; hr

run_e2e() {   # $1 = human name, $2 = make target
  local name="$1" target="$2" log="$LOGDIR/${2}.log" attempt
  for attempt in 1 2 3; do
    echo "  ...  $name ($target, attempt $attempt)"
    if make "$target" > "$log" 2>&1; then
      echo "  PASS  $name"; record "$name" PASS "attempt $attempt"; return
    fi
    if grep -qE 'exit code: 100|Temporary failure resolving|apt-get.*Retries' "$log"; then
      echo "        transient apt failure — retrying"; continue
    fi
    echo "  FAIL  $name (see $log)"; record "$name" FAIL "see log"; return
  done
  echo "  FAIL  $name (apt kept failing after 3 tries)"; record "$name" FAIL "apt flake x3"
}

run_e2e "T2 base (dispatch/audit/no-scp)"        e2e-t2
run_e2e "runtime + confirm gate (p2-5b)"         e2e-t2-runtime
run_e2e "supervision modes: systemd + compose"   e2e-t2-runtime-modes
run_e2e "KES rotation + confirm gate"            e2e-t2-kes
run_e2e "secret-leak scanner (no-leak)"          e2e-t2-secrets
run_e2e "rolling upgrade (rollout)"              e2e-t2-upgrade

# ---- Summary -------------------------------------------------------------------------
hr; echo "[accept] SUMMARY"; hr
fails=0
for row in "${RESULTS[@]}"; do
  IFS='|' read -r name status note <<< "$row"
  printf '  %-4s  %-42s %s\n' "$status" "$name" "$note"
  [ "$status" = FAIL ] && fails=$((fails+1))
done
hr
if [ "$fails" = 0 ]; then
  echo "  ACCEPTANCE: ALL PASSED (${#RESULTS[@]} checks)"; echo "  logs: $LOGDIR"; exit 0
else
  echo "  ACCEPTANCE: $fails/${#RESULTS[@]} FAILED — logs in $LOGDIR"; exit 1
fi
