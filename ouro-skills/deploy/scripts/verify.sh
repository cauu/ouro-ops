#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
SPEC="${OURO_SPEC:?OURO_SPEC required}"
SNAPSHOT="${OURO_STATUS_SNAPSHOT:?OURO_STATUS_SNAPSHOT required}"

python3 - "$SPEC" "$SNAPSHOT" "${OURO_AUDIT_ID:-}" <<'PY'
import json, sys, time, yaml

spec = yaml.safe_load(open(sys.argv[1]))
snapshot = json.load(open(sys.argv[2]))
audit_id = sys.argv[3] or None
checks = []

def add(name, passed, severity="critical", exit_class=30, rollback_safe=False, detail=""):
    checks.append({
        "name": name,
        "pass": bool(passed),
        "severity": "info" if passed else severity,
        "exit_class": 0 if passed else exit_class,
        "rollback_safe": bool(rollback_safe if not passed else True),
        "detail": detail or ("pass" if passed else "fail"),
    })

expected_magic = spec["pool"]["network_magic"]
expected_genesis = spec["pool"]["genesis_hashes"]["shelley"]
# p5-12: node_version/sync are operation-scoped — deploy verify consumes them, so a deploy
# spec that omits them fails closed here (they are optional only for non-deploy operations).
expected_version = spec.get("node_version")
if not expected_version:
    print(json.dumps({"status": "error", "exit_class": 10, "error": "spec_node_version_missing",
                      "detail": "deploy/verify requires node_version in the spec"}))
    sys.exit(10)
topology_mode = spec["topology_mode"]
machines = snapshot["machines"]

# Machine-set integrity: the snapshot must cover exactly the spec's declared machines.
# Otherwise a deploy could be judged healthy while a spec machine (e.g. a relay) is
# absent from — and therefore never checked by — the snapshot.
expected_ids = {m["id"] for m in spec["machines"]}
actual_ids = {m["id"] for m in machines}
missing = sorted(expected_ids - actual_ids)
unexpected = sorted(actual_ids - expected_ids)
add(
    "machine_inventory",
    not missing and not unexpected,
    detail=(
        "snapshot covers exactly the spec machines"
        if not missing and not unexpected
        else f"missing={missing} unexpected={unexpected}"
    ),
)

for machine in machines:
    mid = machine["id"]
    add(f"{mid}.container_running", machine.get("container_running") is True, detail="container is running")
    add(f"{mid}.restart_window", machine.get("restarts_10m", 99) == 0, "warning", 20, True, "no restarts in first 10m")
    add(f"{mid}.node_version", machine.get("node_version") == expected_version, detail="node version matches spec")
    add(f"{mid}.tip_lag", machine.get("tip_lag_s", 9999) < 60 and machine.get("slot_advancing") is True, "warning", 20, True, "tip lag <60s and slot advances")
    add(f"{mid}.metrics", machine.get("metrics_12798") is True, "warning", 20, True, "12798 metrics reachable")
    add(f"{mid}.chrony", abs(machine.get("chrony_offset_ms", 9999)) < 50, "warning", 20, True, "chrony offset <50ms")
    add(f"{mid}.network_magic", machine.get("network_magic") == expected_magic, detail="network magic matches spec")
    add(f"{mid}.genesis_hash", machine.get("genesis_hash") == expected_genesis, detail="genesis hash matches spec")
    add(f"{mid}.db_integrity", machine.get("db", {}).get("ledger_sanity") is True and machine.get("db", {}).get("era_mismatch") is False, detail="ledger sanity and era match")
    if topology_mode == "p2p":
        topology = machine.get("topology", {})
        add(f"{mid}.topology_p2p", topology.get("mode") == "p2p" and topology.get("local_roots_ok") is True and topology.get("peer_sharing_ok") is True, detail="P2P roots and peer sharing valid")
    else:
        peers = machine.get("peers", {})
        add(f"{mid}.topology_legacy", peers.get("inbound", 0) > 0 and peers.get("outbound", 0) > 0, detail="legacy inbound/outbound peers valid")
    if machine.get("role") == "bp":
        kes = machine.get("kes", {})
        add(f"{mid}.bp_port_private", machine.get("bp_public_port_reachable") is False, detail="BP node port is not public")
        add(f"{mid}.forging", machine.get("forging_enabled") is True, detail="forging enabled")
        add(f"{mid}.kes_remaining", kes.get("remaining_periods", 0) > 30, "warning", 20, True, "KES remaining periods above threshold")

if (spec.get("sync") or {}).get("mode") == "mithril":
    mithril = snapshot.get("mithril", {})
    add("mithril.digest", bool(mithril.get("snapshot_digest")), detail="Mithril snapshot digest recorded")
    add("mithril.certificate_chain", mithril.get("certificate_chain_valid") is True, detail="Mithril certificate chain valid")
    add("mithril.ledger_sanity", mithril.get("ledger_sanity") is True, detail="Mithril restored ledger sanity")

pool = snapshot.get("pool", {})
add("pool.id_query", bool(pool.get("pool_id")), detail="pool id query returned")
add("pool.parameters", pool.get("pledge_lovelace") == spec["pool"]["pledge_lovelace"] and pool.get("margin") == spec["pool"]["margin"], detail="pool pledge and margin match spec")

failed = [check for check in checks if not check["pass"]]
exit_code = max([check["exit_class"] for check in failed], default=0)
payload = {
    "tool": "deploy/verify",
    "machine": None,
    "status": "ok" if exit_code == 0 else "error",
    "changed": False,
    "checks": checks,
    "duration_s": 0.0,
    "audit_id": audit_id,
}
if exit_code:
    payload["error"] = {"code": f"exit_{exit_code}", "detail": "deploy verification failed", "hint": "inspect failed checks"}
print(json.dumps(payload, separators=(",", ":")))
sys.exit(exit_code)
PY
