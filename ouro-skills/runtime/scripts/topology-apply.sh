#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-runtime-state}"
MARKER="$STATE_DIR/topology-$MACHINE"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"

if [ -f "$DEVNET/config.json" ]; then
  # REAL node host: render the node's peer topology from the spec's relay endpoints and apply it
  # (write + restart the node) — idempotent: unchanged topology => changed=false, no restart.
  SPEC="${OURO_SPEC:?OURO_SPEC required for real topology apply}"
  mkdir -p "$STATE_DIR"
  DESIRED="$(python3 - "$SPEC" <<'PY'
import json, sys, yaml
spec = yaml.safe_load(open(sys.argv[1]))
producers = [{"addr": m["public_endpoint"]["host"], "port": m["public_endpoint"]["port"], "valency": 1}
             for m in spec["machines"] if m["role"] == "relay" and m.get("public_endpoint")]
print(json.dumps({"Producers": producers}, separators=(",", ":"), sort_keys=True))
PY
)"
  CURRENT="$(python3 -c 'import json,sys;print(json.dumps(json.load(open(sys.argv[1])),separators=(",",":"),sort_keys=True))' "$DEVNET/topology.json" 2>/dev/null || echo "")"
  if [ "$DESIRED" = "$CURRENT" ]; then
    ouro_emit_ok false "topology already applied (${#DESIRED} bytes, unchanged)"
  else
    printf '%s' "$DESIRED" > "$DEVNET/topology.json"
    # Apply the new topology by restarting the node onto it (only if one is running).
    if ouro_node_running; then
      ouro_node_restart
    fi
    touch "$MARKER"
    ouro_emit_ok true "topology applied + node restarted (producers=$(printf '%s' "$DESIRED" | grep -o addr | wc -l | tr -d ' '))"
  fi
else
  # Marker mode: non-node host / deterministic unit tests.
  ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && touch '$MARKER'"
fi
