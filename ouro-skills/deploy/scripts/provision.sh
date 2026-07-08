#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
SPEC="${OURO_SPEC:?OURO_SPEC required}"
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
# Node-host root; container default is /opt/cardano, tests inject a temp dir.
CARDANO_ROOT="${OURO_CARDANO_ROOT:-/opt/cardano}"
CONFIG_DIR="$CARDANO_ROOT/config"
RENDERED="$CONFIG_DIR/$MACHINE"
OURO="${OURO_BIN:-ouro}"

# Real, idempotent provisioning of the node host's NON-node state: directory layout +
# rendered node config from the spec. (cardano-node itself is started in p2-1.)
# detect = layout + rendered config already present; act = create + render.
if [[ -d "$CARDANO_ROOT/db" && -f "$RENDERED/config.json" && -f "$RENDERED/topology.json" ]]; then
  ouro_emit_ok false "node host already provisioned"
else
  mkdir -p "$CARDANO_ROOT/db" "$CARDANO_ROOT/keys" "$CONFIG_DIR"
  chmod 0700 "$CARDANO_ROOT/keys"
  if ! "$OURO" config render --spec "$SPEC" --machine "$MACHINE" --out "$CONFIG_DIR" >/dev/null; then
    ouro_emit_error 20 "config_render_failed" "could not render node config for $MACHINE"
  fi
  chown -R node "$CARDANO_ROOT" 2>/dev/null || true
  ouro_emit_ok true "provisioned dir layout + rendered node config"
fi
