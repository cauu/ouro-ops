#!/usr/bin/env python3
"""S0017 p2-8 / TC-14 — static supervisor-adapter gate.

Every node/daemon lifecycle action must route through the supervisor adapter in
ouro-skills/lib/ouro-lib.sh (ouro_node_* / ouro_proc_* / ouro_daemon_spawn), so
a node started by the adapter can never be half-managed by a stray pkill/setsid
elsewhere (split-brain). This gate FORBIDS the raw supervision primitives
anywhere outside the adapter and asserts the adapter actually defines the API
and is actually used by the lifecycle scripts.

Zero-tolerance by design: a primitive appearing outside the lib — even in a
comment — trips the gate. Reword the comment or route the call through the
adapter. Runs standalone: `python3 tests/test_supervisor_gate.py`.
"""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKILLS = ROOT / "ouro-skills"
ADAPTER = SKILLS / "lib" / "ouro-lib.sh"

# Raw process-supervision primitives that must live only in the adapter.
FORBIDDEN = ("pgrep", "pkill", "setsid", "systemctl", "docker", "podman")
FORBIDDEN_RE = re.compile(r"\b(" + "|".join(FORBIDDEN) + r")\b")

# The adapter API every lifecycle script is expected to call instead.
ADAPTER_FUNCS = (
    "ouro_proc_running",
    "ouro_proc_pid",
    "ouro_proc_stop",
    "ouro_daemon_spawn",
    "ouro_node_running",
    "ouro_node_pid",
    "ouro_node_stop",
    "ouro_node_start",
    "ouro_node_restart",
)

# Scripts that start/stop/restart the cardano-node — they MUST use ouro_node_*.
NODE_LIFECYCLE = (
    "runtime/scripts/restart.sh",
    "runtime/scripts/topology-apply.sh",
    "upgrade/scripts/upgrade-one.sh",
    "kes-rotation/scripts/rotate.sh",
)


def main():
    adapter_text = ADAPTER.read_text()

    # 1. The adapter defines the whole API.
    for fn in ADAPTER_FUNCS:
        assert f"{fn}()" in adapter_text, f"adapter missing definition: {fn}()"

    # 2. No raw supervision primitive appears in any skill script except the adapter.
    offenders = []
    for script in sorted(SKILLS.rglob("*.sh")):
        if script.resolve() == ADAPTER.resolve():
            continue
        for lineno, line in enumerate(script.read_text().splitlines(), 1):
            m = FORBIDDEN_RE.search(line)
            if m:
                rel = script.relative_to(ROOT)
                offenders.append(f"{rel}:{lineno}: {m.group(1)} -> {line.strip()}")
    assert not offenders, "supervision primitives outside the adapter:\n" + "\n".join(offenders)

    # 3. Every node-lifecycle script actually routes through the adapter.
    for rel in NODE_LIFECYCLE:
        text = (SKILLS / rel).read_text()
        assert "ouro_node_" in text, f"{rel} does not use the ouro_node_* adapter"

    print("supervisor adapter gate passed")


if __name__ == "__main__":
    main()
