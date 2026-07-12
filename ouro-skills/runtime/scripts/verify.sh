#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-runtime-state}"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"

if [ -f "$DEVNET/config.json" ]; then
  # REAL node host: after a restart / topology-apply the node must be running AND forging.
  SOCK="$DEVNET/node.socket"
  MAGIC=1
  [ -n "${OURO_SPEC:-}" ] && MAGIC="$(python3 -c 'import yaml,sys;print(yaml.safe_load(open(sys.argv[1]))["pool"]["network_magic"])' "$OURO_SPEC" 2>/dev/null || echo 1)"
  export CARDANO_NODE_SOCKET_PATH="$SOCK"
  ouro_node_running || ouro_emit_error 30 "node_not_running" "no running node on $MACHINE"
  tip_block() { ouro_cardano_cli query tip --testnet-magic "$MAGIC" 2>/dev/null \
    | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1; }
  # Wait for the (possibly just-restarted) node's socket to answer, then require the tip to
  # ADVANCE — a single tight sample can catch the node mid-warmup after a restart.
  B0=-1
  for _ in $(seq 1 20); do B0="$(tip_block)"; [ "${B0:--1}" -ge 0 ] 2>/dev/null && break; sleep 2; done
  [ "${B0:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "node_not_running" "node socket did not answer on $MACHINE"
  for _ in $(seq 1 30); do
    sleep 3
    B="$(tip_block)"
    if [ "${B:--1}" -gt "${B0:--1}" ] 2>/dev/null; then
      ouro_emit_ok false "runtime verified: node forging (block $B0 -> $B)"
      exit 0
    fi
  done
  ouro_emit_error 30 "node_not_forging" "node tip did not advance past $B0 on $MACHINE"
else
  # Marker mode: non-node host / deterministic unit tests.
  if [[ ! -f "$STATE_DIR/topology-$MACHINE" ]]; then
    ouro_emit_error 20 "runtime_topology_missing" "topology has not been applied for $MACHINE"
  fi
  ouro_emit_ok false "runtime verification passed"
fi
