#!/usr/bin/env bash
# S0017 p2-1 — confined, read-only supervision-mode probe (detect/runtime).
#
# L3 diagnostic posture: READ-ONLY, no writes, no key-directory access. It emits a
# CLOSED, typed projection of how the cardano-node is supervised — booleans, a mode
# enum, opaque immutable ids (container id / systemd unit basename), a content hash
# (image digest), and the listening port. It NEVER serializes raw env/argv/mounts/
# labels or full `inspect`/`systemctl cat` output, so operator secrets that happen to
# live in those sources cannot cross into the agent transcript (p2-2 no-leak; TC-6).
#
# The LLM reads this projection to ADVISE a mode; the mechanism re-verifies before any
# destructive action (p2-5). Multiple supervisor signals => mode="ambiguous" so the
# caller fails closed (p2-6, exit 40) instead of guessing.
#
# Being read-only it does NOT take the L2 audit-write gate. The target-side principal
# that runs it (read-only ouro-diag, no secret dir access) is established by
# provisioning (p1); this script is the mechanism + projection, independent of it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

MACHINE="${OURO_MACHINE:-}"

PID="$(ouro_node_pid)"
COUNT="$(ouro_node_count)"
RUNNING=false; [ -n "$PID" ] && RUNNING=true

CID="$(ouro_supervisor_container_id "$PID")"
RUNTIME="$(ouro_supervisor_container_runtime "$PID")"
UNIT="$(ouro_supervisor_systemd_unit "$PID")"
PORT="$(ouro_node_port "$PID")"
DIGEST=""
if [ -n "$RUNTIME" ] && [ -n "$CID" ]; then
  DIGEST="$(ouro_supervisor_image_digest "$RUNTIME" "$CID")"
fi

# Signals: a container id + runtime => container mode; a *.service slice (and NOT a
# container) => systemd; a running node with neither => bare.
SIG_DOCKER=false; SIG_PODMAN=false; SIG_SYSTEMD=false; SIG_BARE=false
if [ -n "$CID" ] && [ "$RUNTIME" = "docker" ]; then SIG_DOCKER=true; fi
if [ -n "$CID" ] && [ "$RUNTIME" = "podman" ]; then SIG_PODMAN=true; fi
if [ -z "$CID" ] && [ -n "$UNIT" ]; then SIG_SYSTEMD=true; fi
if [ "$RUNNING" = true ] && [ -z "$CID" ] && [ -z "$UNIT" ]; then SIG_BARE=true; fi

# Emit the closed projection. Mode + conflict resolution is done in python for clean
# JSON; python receives only already-projected scalars, never raw source text.
python3 - "$MACHINE" "$RUNNING" "$SIG_BARE" "$SIG_SYSTEMD" "$SIG_DOCKER" "$SIG_PODMAN" \
  "$UNIT" "$CID" "$DIGEST" "$PORT" "$COUNT" "${OURO_AUDIT_ID:-}" <<'PY'
import json, sys
(machine, running, s_bare, s_systemd, s_docker, s_podman,
 unit, cid, digest, port, count, audit_id) = sys.argv[1:13]

def b(x): return x == "true"
signals = {"bare": b(s_bare), "systemd": b(s_systemd),
           "docker": b(s_docker), "podman": b(s_podman)}
active = [name for name, on in signals.items() if on]
try:
    node_count = int(count)
except ValueError:
    node_count = 0

# p2-6 fail-closed ambiguity: more than one supervisor signal, more than one matching
# node process (same-host double node), or a running-but-unclassifiable process.
conflict = []
if not b(running):
    mode = "none"
elif node_count > 1:
    mode, conflict = "ambiguous", ["multiple_node_processes"] + sorted(active)
elif len(active) > 1:
    mode, conflict = "ambiguous", sorted(active)
elif len(active) == 1:
    mode = active[0]
else:
    mode, conflict = "ambiguous", ["unclassified"]

data = {
    "node_running": b(running),
    "node_count": node_count,
    "mode": mode,
    "signals": signals,
    "evidence": {
        "unit": unit or None,
        "container_id": cid or None,
        "image_digest": digest or None,
    },
    "port": int(port) if port else None,
    "conflict": conflict,
}
print(json.dumps({
    "tool": "detect/runtime",
    "machine": machine or None,
    "status": "ok",
    "changed": False,
    "checks": [],
    "duration_s": 0.0,
    "audit_id": audit_id or None,
    "data": data,
}, separators=(",", ":")))
PY
