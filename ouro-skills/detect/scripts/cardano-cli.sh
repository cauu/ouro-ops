#!/usr/bin/env bash
# S0017 p4-5 — confined, read-only cardano-cli version probe (detect/cardano-cli).
#
# The cold-sign / registration flows call cardano-cli with a specific era discipline: the KES
# opcert commands (`node issue-op-cert`, `node key-gen-KES`) are era-NEUTRAL, while the deploy
# transaction commands (`<era> transaction witness/build/...`) are era-SCOPED. That discipline is
# validated against the pinned cardano-cli line below. This probe lets the agent PRECHECK the
# target's cardano-cli before an operation instead of failing cryptically mid-flow.
#
# Closed projection only: version string + numeric major/minor/patch + a `supported` boolean +
# the validated reference version. No raw help/output, no paths. Read-only → no audit-write gate.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

MACHINE="${OURO_MACHINE:-}"

# Pinned support policy: the era discipline above is validated on the cardano-cli 10.x line.
SUPPORTED_MAJOR_MIN=10
VALIDATED_VERSION="10.14.0.0"

RAW=""
if command -v cardano-cli >/dev/null 2>&1; then
  # `cardano-cli --version` → "cardano-cli 10.14.0.0 - linux-... - ghc-9.6"; take field 2 only.
  RAW="$(cardano-cli --version 2>/dev/null | head -1 | awk '{print $2}')"
fi

python3 - "$MACHINE" "$RAW" "$SUPPORTED_MAJOR_MIN" "$VALIDATED_VERSION" "${OURO_AUDIT_ID:-}" <<'PY'
import json, re, sys
machine, raw, major_min, validated, audit_id = sys.argv[1:6]
present = bool(raw)
m = re.match(r'^(\d+)\.(\d+)\.(\d+)', raw or "")
major = int(m.group(1)) if m else -1
minor = int(m.group(2)) if m else -1
patch = int(m.group(3)) if m else -1
supported = present and major >= int(major_min)
payload = {"tool": "detect/cardano-cli", "machine": (machine or None),
           "status": "ok", "changed": False,
           "checks": [{"name": "cardano_cli_probe", "pass": True, "severity": "info",
                       "exit_class": 0, "rollback_safe": True,
                       "detail": f"cardano-cli {'present '+raw if present else 'absent'}; supported={supported}"}],
           "data": {"present": present, "version": (raw or None),
                    "major": major, "minor": minor, "patch": patch,
                    "supported": supported, "supported_major_min": int(major_min),
                    "validated_version": validated},
           "duration_s": 0.0, "audit_id": (audit_id or None)}
print(json.dumps(payload, separators=(",", ":")))
PY
