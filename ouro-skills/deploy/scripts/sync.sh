#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
SPEC="${OURO_SPEC:?OURO_SPEC required}"
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
STATE_DIR="${OURO_STATE_DIR:-/tmp/ouro-deploy-state}"
MARKER="$STATE_DIR/synced-$MACHINE"

MODE="$(python3 - "$SPEC" <<'PY'
import sys, yaml
print((yaml.safe_load(open(sys.argv[1])).get("sync") or {}).get("mode") or "")
PY
)"
# p5-12: sync is operation-scoped/optional in the spec — this script is its consumer, so an
# absent block fails closed HERE with an actionable message instead of a KeyError.
[ -n "$MODE" ] || ouro_emit_error 10 "spec_sync_missing" "deploy/sync requires sync.mode in the spec (it is optional for other operations) — add sync: {mode: genesis|mithril}"

if [[ "$MODE" == "mithril" ]]; then
  DIGEST="${OURO_MITHRIL_DIGEST:-}"
  CERT_CHAIN="${OURO_MITHRIL_CERT_CHAIN:-}"
  if [[ -z "$DIGEST" || -z "$CERT_CHAIN" ]]; then
    ouro_emit_error 20 "mithril_evidence_missing" "Mithril digest and certificate chain evidence are required"
  fi
  ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && printf '%s\n%s\n' '$DIGEST' '$CERT_CHAIN' > '$MARKER'"
else
  ouro_check_then_act "test -f '$MARKER'" "mkdir -p '$STATE_DIR' && printf 'genesis\n' > '$MARKER'"
fi
