#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-deploy-state}"
MARKER="$STATE_DIR/takeover-$MACHINE"
ROLLBACK="$STATE_DIR/takeover-$MACHINE.rollback"

if [[ ! -f "$MARKER" || ! -f "$ROLLBACK" ]]; then
  ouro_emit_error 20 "takeover_not_ready" "takeover marker or rollback artifact missing"
fi

# Real-mode rollback artifact is a `sha256sum` key snapshot (starts with a hex hash);
# manifest-mode is a JSON copy (starts with '{'). Only the real path re-checks the live node.
if [[ "$(head -c1 "$ROLLBACK")" != "{" ]]; then
  DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
  SOCK="$DEVNET/node.socket"
  MAGIC=1
  [ -n "${OURO_SPEC:-}" ] && MAGIC="$(python3 -c 'import yaml,sys;print(yaml.safe_load(open(sys.argv[1]))["pool"]["network_magic"])' "$OURO_SPEC" 2>/dev/null || echo 1)"
  export CARDANO_NODE_SOCKET_PATH="$SOCK"
  # Key preservation: every snapshotted key must still exist with the SAME checksum.
  if ! sha256sum -c "$ROLLBACK" >/dev/null 2>&1; then
    ouro_emit_error 30 "keys_not_preserved" "a legacy key changed or is missing since takeover"
  fi
  # Continuity: the legacy node must still be running AND genuinely FORGING (tip advances across
  # two samples) — a hung/stuck process that only `pgrep`-matches is not a healthy takeover.
  pgrep -f 'cardano-node run' >/dev/null 2>&1 || ouro_emit_error 30 "node_not_running" "legacy node stopped after takeover"
  tip_block() { cardano-cli query tip --testnet-magic "$MAGIC" 2>/dev/null \
    | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",-1))
except: print(-1)' 2>/dev/null || echo -1; }
  B1="$(tip_block)"; sleep 5; B2="$(tip_block)"
  [ "${B1:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "node_query_failed" "legacy node socket unresponsive after takeover"
  [ "${B2:--1}" -gt "${B1:--1}" ] 2>/dev/null || ouro_emit_error 30 "node_not_forging" "legacy node tip did not advance ($B1 -> $B2) after takeover"
  ouro_emit_ok false "takeover verified: keys preserved + legacy node forging (block $B1 -> $B2)"
else
  ouro_emit_ok false "takeover legacy node and rollback artifact verified"
fi
