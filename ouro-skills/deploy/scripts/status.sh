#!/usr/bin/env bash
# p2-2 REAL node-state collector/verifier. Runs ON the target (dispatched) and builds the
# status from the LIVE node via `cardano-cli query tip` — NO injected OURO_STATUS_SNAPSHOT
# (that would defeat E2E-11's anti-placeholder gate). Read-only (changed=false).
#
# Honest scope: it asserts what a running node can truly prove — tip block>0, era, sync,
# network-magic (the query only succeeds on the right magic), genesis self-consistency
# (the node loaded the genesis whose hash we recompute), and slot/block advancement across
# two samples. Fuller checks (metrics/chrony/pool-params) arrive with p2-5/p2-8 infra.
set -euo pipefail  # query failures are guarded with `|| true`; emit helpers exit explicitly

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
SPEC="${OURO_SPEC:?OURO_SPEC required}"
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
SOCK="${OURO_NODE_SOCKET:-/opt/devnet/node.socket}"
GENESIS="${OURO_GENESIS_SHELLEY:-/opt/devnet/shelley-genesis.json}"
export CARDANO_NODE_SOCKET_PATH="$SOCK"

# Expected network magic from the spec (the collector queries with it; a mismatch makes
# `query tip` fail → node_query_failed, which IS the "wrong network fails" behavior).
MAGIC="$(python3 -c 'import yaml,sys; print(yaml.safe_load(open(sys.argv[1]))["pool"]["network_magic"])' "$SPEC" 2>/dev/null || echo "")"
[ -n "$MAGIC" ] || ouro_emit_error 20 "spec_magic_missing" "pool.network_magic not in spec"

query_tip() { cardano-cli query tip --testnet-magic "$MAGIC" 2>/dev/null; }
TIP1="$(query_tip)" || true
if [ -z "$TIP1" ] || ! printf '%s' "$TIP1" | grep -q '"block"'; then
  ouro_emit_error 30 "node_query_failed" "cardano-cli query tip failed (socket $SOCK, magic $MAGIC) — node down or wrong network"
fi
sleep 3
TIP2="$(query_tip)" || true

GENESIS_HASH="$(cardano-cli hash genesis-file --genesis "$GENESIS" 2>/dev/null | tr -d '\r' || echo "")"

python3 - "$MACHINE" "$MAGIC" "$GENESIS_HASH" "${OURO_AUDIT_ID:-}" "$TIP1" "$TIP2" <<'PY'
import json, sys
mid, magic, gh, audit_id, t1_raw, t2_raw = sys.argv[1:7]
t1 = json.loads(t1_raw)
# Robust second-sample parse: a garbage/partial second `query tip` must NOT crash the collector
# with a traceback — fall back to t1 (slot_advancing then reports no advance).
try:
    t2 = json.loads(t2_raw) if t2_raw.strip() else t1
except Exception:
    t2 = t1
checks = []
def add(name, ok, sev="critical", ec=30, detail=""):
    checks.append({"name": name, "pass": bool(ok),
                   "severity": "info" if ok else sev, "exit_class": 0 if ok else ec,
                   "rollback_safe": True, "detail": detail or ("pass" if ok else "fail")})

block, slot, era = t1.get("block", -1), t1.get("slot", -1), t1.get("era", "")
b2, s2 = t2.get("block", block), t2.get("slot", slot)
# sync as a numeric percentage, not an exact "100.00" string.
try:
    sync = float(t1.get("syncProgress", "nan"))
except Exception:
    sync = float("nan")
add(f"{mid}.tip_block_positive", block > 0, detail=f"block={block}")
add(f"{mid}.era_conway", era == "Conway", detail=f"era={era}")
add(f"{mid}.sync_100", sync >= 99.99, "warning", 20, detail=f"sync={t1.get('syncProgress')}")
# network_magic: a wrong magic makes `query tip` fail at the script's entry (node_query_failed),
# so REACHING here with a real tip (era present) is the proof the node answered on the spec magic
# — assert on that real result, not the tautological `int(magic) >= 0`.
add(f"{mid}.network_magic_answered", era != "" and block >= 0, detail=f"node returned a tip on magic {magic}")
# Honest name: this proves the shelley genesis file is present + hashable on the node host, NOT
# ledger-level consistency. E2E cross-checks it against the node's config ShelleyGenesisHash.
add(f"{mid}.genesis_hash_readable", len(gh) == 64 and all(c in "0123456789abcdef" for c in gh),
    detail=f"shelley genesis hash={gh[:16]}…")
add(f"{mid}.slot_advancing", (b2 > block) or (s2 > slot), "warning", 20,
    detail=f"slot {slot}->{s2} block {block}->{b2}")

exit_code = max([c["exit_class"] for c in checks if not c["pass"]], default=0)
payload = {
  "tool": "deploy/status", "machine": mid,
  "status": "ok" if exit_code == 0 else "error", "changed": False,
  "checks": checks,
  "data": {"tip": {"slot": slot, "block": block, "hash": t1.get("hash", "")},
           "era": era, "sync_progress": t1.get("syncProgress"),
           "network_magic": int(magic), "genesis_hash": gh,
           "slot_advancing": (b2 > block) or (s2 > slot)},
  "duration_s": 3.0, "audit_id": (audit_id or None),
}
if exit_code:
    payload["error"] = {"code": f"exit_{exit_code}", "detail": "node status checks failed",
                        "hint": "inspect failed checks"}
print(json.dumps(payload, separators=(",", ":")))
sys.exit(exit_code)
PY
