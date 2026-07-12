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
  # REAL node host (p2-5/p2-fix1). Resolve the supervision mode FIRST (declaration
  # cross-checked; none/ambiguous/mismatch => exit 40): on a container-managed host,
  # cardano-cli lives INSIDE the container, so the host-side tip pre-check below would
  # spuriously fail — the container branch does its own convergence verification instead.
  if [ -f "$MARKER" ]; then
    ouro_emit_ok false "node already rolling-upgraded"; exit 0
  fi
  PRE_PID="$(ouro_node_pid)"
  [ -n "$PRE_PID" ] || ouro_emit_error 30 "node_not_running" "expected a running node on $MACHINE before upgrade"
  MODE="$(ouro_node_effective_mode "$(ouro_declared_mode "${OURO_SPEC:-}" "$MACHINE")")"
  ouro_node_guard_mode "$MODE"

  if [ "$MODE" = "docker" ] || [ "$MODE" = "podman" ]; then
    # Container upgrade = image re-pin + RECREATE from the spec-DECLARED image (a host-binary
    # swap under a container is a silent no-op). Compose-managed containers converge the
    # compose file (the deployment's source of truth — else the next `up` rolls the node
    # back); plain-run containers fail closed inside ouro_node_upgrade_container. This branch
    # is TERMINAL — it must exit, NOT fall through to the bare/systemd path below (whose
    # host-side cardano-cli precheck would spuriously fail on a container host).
    WANT="$(python3 - "$SPEC" "$MACHINE" <<'PY' 2>/dev/null || true
import yaml,sys
s=yaml.safe_load(open(sys.argv[1])); mid=sys.argv[2]
m=next((x for x in s.get("machines",[]) if x.get("id")==mid),{}) or {}
print((m.get("runtime") or {}).get("image",""), end="")
PY
)"
    [ -n "$WANT" ] || ouro_emit_error 40 "runtime_image_undeclared" \
      "container upgrade needs spec runtime.image for $MACHINE (the declared target version)"
    CID="$(ouro_supervisor_container_id "$PRE_PID")"
    [ -n "$CID" ] || ouro_emit_error 40 "container_unresolved" "could not resolve node container on $MACHINE"
    # Idempotency by ground truth: already running the declared image's content id => no-op.
    if [ -n "$(ouro_image_id_of "$MODE" "$WANT")" ] \
       && [ "$(ouro_container_image_id "$MODE" "$CID")" = "$(ouro_image_id_of "$MODE" "$WANT")" ]; then
      ouro_emit_ok false "node container already on declared image $WANT"; exit 0
    fi
    ouro_node_upgrade_container "$MODE" "$CID" "$WANT"
    sleep 2
    NEW_PID="$(ouro_node_pid)"
    [ -n "$NEW_PID" ] || ouro_emit_error 30 "node_not_running" "no node process after container recreate on $MACHINE"
    [ "$NEW_PID" != "$PRE_PID" ] || ouro_emit_error 30 "container_not_recreated" "node PID unchanged after container upgrade on $MACHINE"
    mkdir -p "$STATE_DIR"; touch "$MARKER"
    ouro_emit_ok true "node container recreated onto declared image $WANT (pid $PRE_PID->$NEW_PID)"
    exit 0
  fi

  # bare/systemd: the new binary is already staged on the host — record the pre-restart
  # PID + block so `verify` can prove a genuine restart (PID changed) and NEW forging
  # (block strictly advances). Keep the db (wiping it re-triggers the p2-0 cold-start).
  SOCK="$DEVNET/node.socket"
  export CARDANO_NODE_SOCKET_PATH="$SOCK"
  PRE_BLOCK="$(cardano-cli query tip --testnet-magic "$MAGIC" 2>/dev/null \
    | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1)"
  [ "${PRE_BLOCK:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "node_query_failed" "could not read tip before upgrade on $MACHINE"
  mkdir -p "$STATE_DIR"
  printf '%s' "$PRE_PID"   > "$STATE_DIR/pre-pid-$MACHINE"
  printf '%s' "$PRE_BLOCK" > "$STATE_DIR/pre-block-$MACHINE"
  ouro_node_restart_mode "$MODE"
  touch "$MARKER"
  ouro_emit_ok true "node rolling-restarted (pre pid=$PRE_PID block=$PRE_BLOCK recorded for verify)"
else
  # Marker mode: relay host without a managed node, or the deterministic unit tests.
  ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && touch '$MARKER'"
fi
