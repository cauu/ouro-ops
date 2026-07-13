#!/usr/bin/env bash
# S0017 p5-15 — read-only aggregated node health for ANY role (bp AND relay), dispatched per
# machine. This is the primary observability surface: the AGENT reads the typed facts below and
# reports CONCLUSIONS to the operator per the skill's interpretation table (KES due, node down,
# sync stalled, disk pressure). No public endpoint is involved — the BP stays closed; facts leave
# the machine only through this audited, confined dispatch.
#
# Closed typed projection: no raw command output, no paths beyond the reported db mount, no
# secrets (kes-period-info reads only the PUBLIC opcert).
set -euo pipefail  # query failures are guarded; emit helpers exit explicitly

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
SPEC="${OURO_SPEC:-}"
NET="$(ouro_network_args)"   # p5-14: network from the spec (mainnet-aware)

# Role from the spec (data for the interpretation table; tolerant when absent).
ROLE=""
if [ -n "$SPEC" ] && [ -f "$SPEC" ]; then
  ROLE="$(python3 - "$SPEC" "$MACHINE" <<'PY' 2>/dev/null || true
import yaml, sys
s = yaml.safe_load(open(sys.argv[1])) or {}
print(next((m.get("role", "") for m in s.get("machines", []) if m.get("id") == sys.argv[2]), ""))
PY
)"
fi

RUNNING=false; MODE=""
TIP1=""; TIP2=""
KES_JSON=""; OPCERT_PRESENT=false
DISK_PCT=""; DB_PATH=""

if ouro_node_running; then
  RUNNING=true
  MODE="$(ouro_node_detect_mode 2>/dev/null || echo "")"
  SOCK="$(ouro_node_socket)"
  export CARDANO_NODE_SOCKET_PATH="$SOCK"
  query_tip() { ouro_cardano_cli query tip $NET 2>/dev/null; }
  TIP1="$(query_tip)" || true
  sleep 3
  TIP2="$(query_tip)" || true
  # KES facts (bp): read from the live node via the PUBLIC opcert. Absent opcert (relay) is
  # normal, not an error.
  POOL="$(ouro_node_pool_dir)"
  if [ -f "$POOL/opcert.cert" ]; then
    OPCERT_PRESENT=true
    KES_JSON="$(ouro_cardano_cli query kes-period-info --op-cert-file "$POOL/opcert.cert" $NET 2>/dev/null \
      | python3 -c 'import sys; s=sys.stdin.read(); i=s.find("{"); print(s[i:] if i>=0 else "")' 2>/dev/null || echo "")"
  fi
  # Disk pressure on the chain-db filesystem (path from the running node's own argv, p5-3).
  DB_PATH="$(ouro_node_arg --database-path)"; DB_PATH="${DB_PATH:-${OURO_DEVNET_DIR:-/opt/devnet}/db}"
  DISK_PCT="$(df -P "$DB_PATH" 2>/dev/null | awk 'NR==2 {gsub(/%/,"",$5); print $5}' || echo "")"
fi

python3 - "$MACHINE" "$ROLE" "$RUNNING" "$MODE" "$OPCERT_PRESENT" "$DISK_PCT" "${OURO_AUDIT_ID:-}" \
  "$TIP1" "$TIP2" "$KES_JSON" <<'PY'
import json, sys
mid, role, running_s, mode, opcert_s, disk_s, audit_id, t1_raw, t2_raw, kes_raw = sys.argv[1:11]
running = running_s == "true"
opcert = opcert_s == "true"

def parse(raw, fallback=None):
    try:
        return json.loads(raw) if raw.strip() else fallback
    except Exception:
        return fallback

t1 = parse(t1_raw, {})
t2 = parse(t2_raw, t1)
kes = parse(kes_raw, {})

checks = []
def add(name, ok, sev="critical", ec=20, detail=""):
    checks.append({"name": name, "pass": bool(ok),
                   "severity": "info" if ok else sev, "exit_class": 0 if ok else ec,
                   "rollback_safe": True, "detail": detail or ("pass" if ok else "fail")})

add(f"{mid}.node_running", running, detail=f"running={running} mode={mode or 'n/a'}")

block = t1.get("block", -1); slot = t1.get("slot", -1); era = t1.get("era", "")
b2 = t2.get("block", block); s2 = t2.get("slot", slot)
try:
    sync = float(t1.get("syncProgress", "nan"))
except Exception:
    sync = float("nan")
advancing = (b2 > block) or (s2 > slot)
if running:
    add(f"{mid}.tip_readable", block >= 0 and era != "", detail=f"block={block} era={era}")
    add(f"{mid}.sync_100", sync >= 99.99, "warning", detail=f"sync={t1.get('syncProgress')}")
    add(f"{mid}.tip_advancing", advancing, "warning",
        detail=f"slot {slot}->{s2} block {block}->{b2}")

# KES urgency (bp with an opcert): remaining periods until the opcert stops validating.
kes_remaining = None
if opcert:
    cur = kes.get("qKesCurrentKesPeriod"); end = kes.get("qKesEndKesInterval")
    if isinstance(cur, int) and isinstance(end, int):
        kes_remaining = end - cur
        add(f"{mid}.kes_remaining", kes_remaining > 30, "warning",
            detail=f"remaining_periods={kes_remaining} (rotate at <=30)")
    else:
        add(f"{mid}.kes_readable", False, "warning", detail="kes-period-info unreadable")

disk_pct = int(disk_s) if disk_s.isdigit() else None
if disk_pct is not None:
    add(f"{mid}.disk_below_90", disk_pct < 90, "warning", detail=f"chain-db filesystem {disk_pct}% used")

exit_code = max([c["exit_class"] for c in checks if not c["pass"]], default=0)
payload = {
  "tool": "observability/health", "machine": mid,
  "status": "ok" if exit_code == 0 else "error", "changed": False,
  "checks": checks,
  "data": {"role": role or None, "node_running": running, "mode": mode or None,
           "tip": {"slot": slot, "block": block} if running else None,
           "era": era or None, "sync_progress": t1.get("syncProgress"),
           "tip_advancing": advancing if running else None,
           "kes": {"opcert_present": opcert, "remaining_periods": kes_remaining},
           "disk": {"chain_db_used_pct": disk_pct}},
  "duration_s": 3.0, "audit_id": (audit_id or None),
}
if exit_code:
    payload["error"] = {"code": f"exit_{exit_code}", "detail": "health checks reported findings",
                        "hint": "read data + failed checks and report conclusions to the operator (see skill)"}
print(json.dumps(payload, separators=(",", ":")))
sys.exit(exit_code)
PY
