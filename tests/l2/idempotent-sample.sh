#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

MARKER="${1:?marker path required}"
MARKER_DIR="$(dirname "$MARKER")"
ouro_require_audit_context
ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$MARKER_DIR' && touch '$MARKER'"
