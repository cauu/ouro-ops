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

AUTH_FILE="${OURO_TELEMETRY_AUTH_FILE:-/opt/ouro/telemetry-auth}"
PORT="${OURO_TELEMETRY_PORT:-12798}"
if [[ -f "$AUTH_FILE" ]]; then
  # REAL mode (p2-8): authenticated scrape must be 200; unauthenticated must be 401.
  if ! python3 - "$AUTH_FILE" "$PORT" <<'PYEOF'
import sys, base64, urllib.request, urllib.error
cred = open(sys.argv[1]).read().strip(); port = sys.argv[2]
def code(auth):
    req = urllib.request.Request(f"http://127.0.0.1:{port}/metrics")
    if auth:
        req.add_header("Authorization", "Basic " + base64.b64encode(cred.encode()).decode())
    try:
        return urllib.request.urlopen(req, timeout=3).status
    except urllib.error.HTTPError as e:
        return e.code
ca, cn = code(True), code(False)
sys.exit(0 if (ca == 200 and cn == 401) else 1)
PYEOF
  then
    ouro_emit_error 30 "telemetry_auth_failed" "authed scrape != 200 or unauth scrape != 401"
  fi
  # Password must NOT appear in the gateway log (request logging is disabled).
  PW="$(cut -d: -f2- "$AUTH_FILE")"
  if [[ -n "$PW" ]] && grep -qF -- "$PW" /var/log/telemetry-gateway.log 2>/dev/null; then
    ouro_emit_error 30 "telemetry_secret_leak" "basic-auth password found in gateway log"
  fi
  ouro_emit_ok false "telemetry basic-auth verified: authed 200, unauth 401, no password in logs"
else
  ouro_emit_ok false "observability gateway verification passed"
fi
