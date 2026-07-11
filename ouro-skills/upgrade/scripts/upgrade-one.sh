#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-upgrade-state}"
MARKER="$STATE_DIR/upgraded-$MACHINE"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
SPEC="${OURO_SPEC:-}"

# Role from the SPEC, not a runtime process scan — a role=bp host with a DEAD node must still take the
# REAL path (a dead node cannot be silently treated as a relay and marker-passed). Magic also
# from the spec (OURO_NETWORK_MAGIC is not in the ouro-ops tool run env allowlist).
ROLE=""; MAGIC=1
if [ -n "$SPEC" ]; then
  read -r ROLE MAGIC < <(python3 - "$SPEC" "$MACHINE" <<'PY'
import yaml,sys
s=yaml.safe_load(open(sys.argv[1])); mid=sys.argv[2]
role=next((m["role"] for m in s["machines"] if m["id"]==mid),"")
print(role or "-", s["pool"]["network_magic"])
PY
) || true
fi
# A host is a real node host if a node is running, OR the spec says role=bp AND this host is
# provisioned as a node ($DEVNET/config.json exists — a container-only path, absent on the dev
# host so unit tests stay in marker mode). The role branch means a role=bp host with a DEAD node
# still takes the real path and FAILS there, rather than being silently marker-passed.
NODE_HOST=0
if ouro_node_running || { [ "$ROLE" = "bp" ] && [ -f "$DEVNET/config.json" ]; }; then
  NODE_HOST=1
fi

if [ "$NODE_HOST" = 1 ]; then
  # REAL node host (p2-5/p2-fix1). Record the pre-restart PID + block so `verify` can prove a
  # genuine restart (PID changed) and NEW forging (block strictly advances) — not a stale
  # `block>0` from the preserved db. Keep the db (wiping it re-triggers the p2-0 cold-start).
  if [ -f "$MARKER" ]; then
    ouro_emit_ok false "node already rolling-upgraded"
  fi
  SOCK="$DEVNET/node.socket"
  export CARDANO_NODE_SOCKET_PATH="$SOCK"
  PRE_PID="$(ouro_node_pid)"
  [ -n "$PRE_PID" ] || ouro_emit_error 30 "node_not_running" "expected a running node on $MACHINE before upgrade"
  PRE_BLOCK="$(cardano-cli query tip --testnet-magic "$MAGIC" 2>/dev/null \
    | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1)"
  [ "${PRE_BLOCK:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "node_query_failed" "could not read tip before upgrade on $MACHINE"
  mkdir -p "$STATE_DIR"
  printf '%s' "$PRE_PID"   > "$STATE_DIR/pre-pid-$MACHINE"
  printf '%s' "$PRE_BLOCK" > "$STATE_DIR/pre-block-$MACHINE"
  # p2-5: upgrade dispatch by detected mode + declaration cross-check. bare/systemd = the new
  # binary is already staged, so restart onto it. container = image digest re-pin + recreate
  # (a host-binary swap under a container is a silent no-op) — not yet modeled, so fail closed
  # rather than falsely report success. Real container upgrade lands with p2-9 fixtures.
  MODE="$(ouro_node_effective_mode "$(ouro_declared_mode "${OURO_SPEC:-}" "$MACHINE")")"
  ouro_node_guard_mode "$MODE"
  if [ "$MODE" = "docker" ] || [ "$MODE" = "podman" ]; then
    ouro_emit_error 40 "container_upgrade_unsupported" \
      "container upgrade needs image re-pin + recreate (pending p2-9); refusing host-binary swap on $MACHINE"
  fi
  ouro_node_restart_mode "$MODE"
  touch "$MARKER"
  ouro_emit_ok true "node rolling-restarted (pre pid=$PRE_PID block=$PRE_BLOCK recorded for verify)"
else
  # Marker mode: relay host without a managed node, or the deterministic unit tests.
  ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && touch '$MARKER'"
fi
