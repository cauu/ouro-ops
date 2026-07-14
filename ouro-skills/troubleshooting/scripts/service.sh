#!/usr/bin/env bash
# S0017 p5-18 — supervision-layer facts (privileged read): detected mode, restart counter,
# uptime, kernel OOM evidence. Answers "why is the node down / is it flapping". Closed typed
# projection; all supervisor queries go through the lib primitives (gate-confined). Free-form
# exploration belongs to `ouro-ops diag exec` (unprivileged ouro-diag).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"

FACTS="$(ouro_node_service_facts)"

python3 - "$MACHINE" "${OURO_AUDIT_ID:-}" "$FACTS" <<'PY'
import json, sys
mid, audit_id, raw = sys.argv[1], sys.argv[2], sys.argv[3]
facts = dict(line.split("=", 1) for line in raw.splitlines() if "=" in line)
running = facts.get("running") == "true"
restarts = int(facts.get("restarts") or -1)
oom = int(facts.get("oom_hits") or 0)
uptime = int(facts.get("uptime_s") or 0) if facts.get("uptime_s", "").isdigit() else None

checks = []
def add(name, ok, detail):
    checks.append({"name": name, "pass": bool(ok),
                   "severity": "info" if ok else "warning",
                   "exit_class": 0 if ok else 20, "rollback_safe": True, "detail": detail})

add(f"{mid}.node_running", running, f"running={running} mode={facts.get('mode')}")
# Flapping: a supervisor restart counter above 0 with a short uptime is the signature of a
# crash loop; bare mode has no counter (restarts=-1 → unknown, not a failure).
if restarts >= 0:
    add(f"{mid}.not_flapping", not (restarts > 0 and (uptime or 0) < 600),
        f"restarts={restarts} uptime_s={uptime}")
add(f"{mid}.no_oom_evidence", oom == 0, f"kernel oom hits(bounded scan)={oom}")

exit_code = max([c["exit_class"] for c in checks if not c["pass"]], default=0)
payload = {
    "tool": "troubleshooting/service", "machine": mid,
    "status": "ok" if exit_code == 0 else "error", "changed": False,
    "checks": checks,
    "data": {"mode": facts.get("mode"), "running": running, "pid": facts.get("pid") or None,
             "uptime_s": uptime, "restarts": restarts if restarts >= 0 else None,
             "oom_hits": oom},
    "duration_s": 0.0, "audit_id": (audit_id or None),
}
if exit_code:
    payload["error"] = {"code": "exit_20", "detail": "supervision-layer findings",
                        "hint": "read data + failed checks and report conclusions (see skill)"}
print(json.dumps(payload, separators=(",", ":")))
sys.exit(exit_code)
PY
