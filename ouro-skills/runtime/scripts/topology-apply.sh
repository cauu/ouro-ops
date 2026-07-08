#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-runtime-state}"
MARKER="$STATE_DIR/topology-$MACHINE"
ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && touch '$MARKER'"
