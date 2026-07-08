#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"
ouro_require_audit_context
MACHINE="${OURO_MACHINE:-gateway}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-observability-state}"
MARKER="$STATE_DIR/gateway-$MACHINE"
AUTH_REF="${OURO_TELEMETRY_AUTH_REF:-creds://relay-telemetry-basic-auth}"
ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && printf '%s\n' '$AUTH_REF' > '$MARKER'"
