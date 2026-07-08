#!/usr/bin/env bash
# S0015 p2-8 — REAL observability basic-auth E2E (E2E-18). Provisions a telemetry
# credential (creds://relay-telemetry-basic-auth) onto relay1, dispatches
# observability/install-gateway to stand up a basic-auth-protected metrics endpoint, then
# observability/verify to assert: authenticated scrape => 200, unauthenticated => 401, and
# the password appears in NO log/transcript/audit. Rollback removes the gateway.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-obs-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }
jqpy() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT
SPEC=/opt/ouro/pool-spec.yaml
PW="s3cr3t-telemetry-pw-DO-NOT-LEAK"
WORK="$(pwd)/tmp/obs-scan"; rm -rf "$WORK"; mkdir -p "$WORK"

echo "[bed] rebuild base + up + provision"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

echo "[obs] provision telemetry credential (creds://relay-telemetry-basic-auth) onto relay1"
dc exec -T relay1 bash -lc "umask 077; printf 'ourotel:%s' '$PW' > /opt/ouro/telemetry-auth && chmod 0400 /opt/ouro/telemetry-auth"
pass "telemetry basic-auth credential provisioned (0400) on relay1"

echo "[obs] install gateway (dispatch) — real basic-auth metrics endpoint"
outi=$(ctl ouro tool run observability/install-gateway --dispatch relay1 --spec "$SPEC"); echo "$outi" > "$WORK/install.json"
echo "$outi" | jqpy "d['status']" | grep -qx ok || fail "install-gateway not ok: $outi"
echo "$outi" | jqpy "d['changed']" | grep -qx True || fail "install-gateway not changed=true: $outi"
pass "gateway installed (changed=true, basic-auth server listening)"

echo "[obs] verify (dispatch) — authed 200, unauth 401, no password in logs"
outv=$(ctl ouro tool run observability/verify --dispatch relay1 --spec "$SPEC"); echo "$outv" > "$WORK/verify.json"
echo "$outv" | jqpy "d['status']" | grep -qx ok || fail "observability verify failed (auth/401/leak): $outv"
echo "$outv" | jqpy "d['checks'][0]['detail']" | grep -qi "200" || fail "verify did not confirm authed 200 / unauth 401: $outv"
pass "verify: authenticated scrape 200, unauthenticated 401 (real basic-auth enforced)"

echo "[obs] independent 401 proof — unauthenticated scrape from relay1 shell"
code=$(dc exec -T relay1 python3 -c 'import urllib.request,urllib.error
try: print(urllib.request.urlopen("http://127.0.0.1:12798/metrics",timeout=3).status)
except urllib.error.HTTPError as e: print(e.code)')
[ "$code" = 401 ] || fail "unauthenticated scrape returned $code (expected 401)"
pass "independent: unauthenticated GET /metrics => 401"

echo "[obs] password must not leak into transcript / audit / gateway log"
{
  cat "$WORK"/install.json "$WORK"/verify.json
  ctl ouro audit log --limit 50 2>/dev/null
  dc exec -T relay1 ouro audit log --limit 50 2>/dev/null
  dc exec -T relay1 bash -lc 'cat /var/log/telemetry-gateway.log 2>/dev/null || true'
} > "$WORK/corpus.txt" 2>&1
grep -qF -- "$PW" "$WORK/corpus.txt" && fail "telemetry password LEAKED into a log/transcript/audit" \
  || pass "no password in transcript/audit/gateway log ($(wc -c < "$WORK/corpus.txt" | tr -d ' ') bytes scanned)"

echo "[obs] rollback removes the gateway marker"
outr=$(ctl ouro tool run observability/rollback --dispatch relay1 --spec "$SPEC")
echo "$outr" | jqpy "d['status']" | grep -qx ok || fail "rollback not ok: $outr"
pass "rollback: gateway removed"

rm -rf "$WORK"
echo "p2-8 observability basic-auth E2E: ALL PASSED"
