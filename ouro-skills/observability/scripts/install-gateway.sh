#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:-gateway}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-observability-state}"
MARKER="$STATE_DIR/gateway-$MACHINE"
AUTH_REF="${OURO_TELEMETRY_AUTH_REF:-creds://relay-telemetry-basic-auth}"
# Provisioned basic-auth credential on the target (resolved creds://relay-telemetry-basic-auth).
AUTH_FILE="${OURO_TELEMETRY_AUTH_FILE:-/opt/ouro/telemetry-auth}"
PORT="${OURO_TELEMETRY_PORT:-12798}"

if [[ -f "$AUTH_FILE" ]]; then
  # REAL mode (p2-8): stand up a basic-auth-protected metrics gateway. The credential is read
  # from the provisioned file at runtime; it is NEVER passed on argv, echoed, or logged.
  cat > /opt/ouro/telemetry-gateway.py <<'PYEOF'
import sys, os, base64
from http.server import BaseHTTPRequestHandler, HTTPServer
CRED = open(os.environ["OURO_TEL_AUTH_FILE"]).read().strip()   # "user:password"
EXPECT = "Basic " + base64.b64encode(CRED.encode()).decode()
class H(BaseHTTPRequestHandler):
    def log_message(self, *a):     # never log requests -> no Authorization header in logs
        return
    def do_GET(self):
        if self.headers.get("Authorization", "") != EXPECT:
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="telemetry"')
            self.end_headers(); self.wfile.write(b"unauthorized\n"); return
        self.send_response(200); self.send_header("Content-Type", "text/plain"); self.end_headers()
        self.wfile.write(b"cardano_node_metrics 1\n")
HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PYEOF
  if ouro_proc_running 'telemetry-gateway.py' && [ -f "$MARKER" ]; then
    ouro_emit_ok false "telemetry gateway already running"
  else
    # The daemon reads the (non-secret) auth-file PATH from the environment; export it so the
    # detached process inherits it. The credential VALUE is never on argv, echoed, or logged.
    export OURO_TEL_AUTH_FILE="$AUTH_FILE"
    ouro_daemon_spawn /var/log/telemetry-gateway.log python3 /opt/ouro/telemetry-gateway.py "$PORT"
    up=0
    for _ in $(seq 1 20); do
      if python3 -c "import socket;socket.create_connection(('127.0.0.1',$PORT),1).close()" 2>/dev/null; then up=1; break; fi
      sleep 0.5
    done
    # Do NOT report success on a gateway that never bound (port in use / python crash) — the
    # old code wrote the marker + ok regardless of whether it was listening.
    [ "$up" = 1 ] || ouro_emit_error 30 "gateway_not_listening" "telemetry gateway failed to bind 127.0.0.1:$PORT"
    # Prove basic-auth actually enforces (authed 200 / unauth 401) before claiming success.
    if ! python3 - "$AUTH_FILE" "$PORT" <<'PYEOF'
import sys, base64, urllib.request, urllib.error
cred = open(sys.argv[1]).read().strip(); port = sys.argv[2]
def code(auth):
    r = urllib.request.Request(f"http://127.0.0.1:{port}/metrics")
    if auth: r.add_header("Authorization", "Basic " + base64.b64encode(cred.encode()).decode())
    try: return urllib.request.urlopen(r, timeout=3).status
    except urllib.error.HTTPError as e: return e.code
sys.exit(0 if (code(True) == 200 and code(False) == 401) else 1)
PYEOF
    then
      ouro_emit_error 30 "gateway_auth_broken" "gateway is listening but basic-auth is not enforced (authed!=200 or unauth!=401)"
    fi
    mkdir -p "$STATE_DIR"; printf '%s\n' "$AUTH_REF" > "$MARKER"
    ouro_emit_ok true "telemetry basic-auth gateway installed + auth-verified on :$PORT"
  fi
else
  # Marker mode (deterministic unit tests): no provisioned credential present.
  ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && printf '%s\n' '$AUTH_REF' > '$MARKER'"
fi
