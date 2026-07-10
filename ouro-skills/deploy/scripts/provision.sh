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
OURO="${OURO_BIN:-ouro-ops}"

# The `node` user exists only on a real node host (container); on the dev host it is
# absent, so ownership enforcement/chown are conditional on it existing.
have_node=0
getent passwd node >/dev/null 2>&1 && have_node=1

# Real, idempotent provisioning of the node host's NON-node state. Convergence requires
# the layout, the rendered config, the keys dir mode (0700), and — on a real host — node
# ownership. (cardano-node itself is started in p2-1.)
converged=1
[[ -d "$CARDANO_ROOT/db" && -f "$RENDERED/config.json" && -f "$RENDERED/topology.json" ]] || converged=0
# Perms/owner convergence are real-node-host concerns (GNU stat + the `node` user); the
# dev host (BSD stat, no node user) verifies file layout only.
if (( have_node )); then
  [[ "$(stat -c %a "$CARDANO_ROOT/keys" 2>/dev/null)" == "700" ]] || converged=0
  [[ "$(stat -c %U "$CARDANO_ROOT" 2>/dev/null)" == "node" ]] || converged=0
fi

if (( converged )); then
  ouro_emit_ok false "node host already provisioned"
else
  mkdir -p "$CARDANO_ROOT/db" "$CARDANO_ROOT/keys" "$CONFIG_DIR"
  chmod 0700 "$CARDANO_ROOT/keys"
  if ! "$OURO" config render --spec "$SPEC" --machine "$MACHINE" --out "$CONFIG_DIR" >/dev/null; then
    ouro_emit_error 20 "config_render_failed" "could not render node config for $MACHINE"
  fi
  if (( have_node )) && ! chown -R node "$CARDANO_ROOT"; then
    ouro_emit_error 20 "chown_failed" "could not set node ownership on $CARDANO_ROOT"
  fi
  ouro_emit_ok true "provisioned dir layout + rendered node config"
fi
