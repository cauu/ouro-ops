#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
SPEC="${OURO_SPEC:?OURO_SPEC required}"
python3 - "$SPEC" <<'PY' >/dev/null
import sys, yaml
data = yaml.safe_load(open(sys.argv[1]))
assert data["spec_version"] == 1
assert len(data["machines"]) >= 2
for machine in data["machines"]:
    assert machine["ssh"]["key_ref"].startswith("creds://")
PY
ouro_emit_ok false "spec and credential references are deploy-ready"
