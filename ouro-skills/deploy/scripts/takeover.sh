#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-deploy-state}"
MARKER="$STATE_DIR/takeover-$MACHINE"
ROLLBACK="$STATE_DIR/takeover-$MACHINE.rollback"
MANIFEST="${OURO_LEGACY_MANIFEST:-}"

if [[ -n "$MANIFEST" ]]; then
  # Manifest mode (deterministic unit tests): preconditions from an injected manifest.
  if ! python3 - "$MANIFEST" "$MACHINE" <<'PY' >/dev/null 2>/dev/null
import json, sys
manifest = json.load(open(sys.argv[1]))
machine = sys.argv[2]
entry = manifest["machines"][machine]
assert entry["legacy_container_running"] is True
assert entry["keys_present"] is True
assert entry["node_socket_active"] is True
PY
  then
    ouro_emit_error 20 "takeover_precondition_failed" "legacy container, keys, or node socket not ready"
  fi
  ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && cp '$MANIFEST' '$ROLLBACK' && touch '$MARKER'"
else
  # REAL mode (p2-7, dispatched): preconditions come from the LIVE legacy node on the target;
  # the rollback artifact is a checksum snapshot of the preserved keys.
  DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
  SOCK="$DEVNET/node.socket"
  POOL="$DEVNET/pools-keys/pool1"
  ouro_node_running || ouro_emit_error 20 "takeover_precondition_failed" "no running legacy cardano-node on target"
  [ -S "$SOCK" ] || ouro_emit_error 20 "takeover_precondition_failed" "legacy node socket not active"
  for k in kes.skey vrf.skey cold.skey; do
    [ -f "$POOL/$k" ] || ouro_emit_error 20 "takeover_precondition_failed" "legacy key missing: $k"
  done
  if [ -f "$MARKER" ]; then
    ouro_emit_ok false "node already under ouro management"
  else
    mkdir -p "$STATE_DIR"
    # Snapshot key checksums BEFORE assuming management => rollback artifact + preservation proof.
    for k in kes.skey vrf.skey cold.skey; do sha256sum "$POOL/$k"; done > "$ROLLBACK"
    touch "$MARKER"
    ouro_emit_ok true "legacy node taken over; keys snapshotted for rollback"
  fi
fi
