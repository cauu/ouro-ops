#!/usr/bin/env bash
# S0015 p3 — T3 agent-harness E2E (E2E-8/10/13). Brings up the bed, then N>=3 times: runs the
# agent scenarios (scripted reference driver) and MECHANICALLY asserts the §2.2 five behavioural
# invariants on the resulting audit + transcript + secret corpus. ANY invariant violation in ANY
# run is a hard FAIL (the N repeats expose harness flake, never average a violation away).
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-t3-$$}"
RUNS="${OURO_T3_RUNS:-3}"
CF=fixtures/e2e/compose.yaml
HARNESS=tests/e2e/agent-harness
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

echo "[bed] rebuild base + up (bp1 forging) + provision + push quorum2 spec"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null
docker cp fixtures/e2e/pool-spec.bed-quorum2.yaml "$(dc ps -q control)":/opt/ouro/pool-spec-q2.yaml >/dev/null

reset_state() {
  for m in control bp1 relay1 relay2; do
    dc exec -T "$m" bash -c 'rm -rf /tmp/ouro-upgrade-state /tmp/ouro-rollout-state' 2>/dev/null || true
  done
}

viol=0
for run in $(seq 1 "$RUNS"); do
  echo "[t3] run $run/$RUNS — execute scenarios + assert invariants"
  reset_state
  OUT="tmp/e2e-t3/run-$run"; rm -rf "$OUT"; mkdir -p "$OUT"
  OURO_E2E_PROJECT="$OURO_E2E_PROJECT" python3 "$HARNESS/run-scenarios.py" \
    --compose "$CF" --scenarios "$HARNESS/scenarios.yaml" --out "$OUT" || fail "run $run: driver crashed"
  if python3 "$HARNESS/assert-invariants.py" \
       --transcript "$OUT/transcript.jsonl" --audit "$OUT/audit.json" \
       --corpus "$OUT/corpus.txt" --fingerprints "$OUT/fingerprints.txt"; then
    pass "run $run: all 5 invariants hold"
  else
    echo "  (violation artefacts in $OUT/)"; viol=$((viol + 1))
  fi
done

[ "$viol" = 0 ] || fail "$viol/$RUNS runs had invariant violations (T3 hard FAIL)"
echo "p3 T3 agent-harness E2E: ALL PASSED ($RUNS runs, 0 invariant violations)"
