#!/usr/bin/env python3
"""S0019 p4-1 — the adopt + intent (`op`) pipeline via the CLI. Every gate must fire in order and
refuse before any mutation."""
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"


def cfg_digest():
    a = json.loads((ROOT / "data/allowlist.json").read_text())
    return a["contracts"][0]["allowed"][0]["image_config_digest"]


def obs(home, **over):
    live = {
        "image_config_digest": cfg_digest(), "platform": "linux/amd64", "container_id": "cid1",
        "container_creation_epoch": 1000, "entrypoint": ["cardano-node"], "args": ["run"],
        "mount_source_ids": ["8:1:1"], "topology_hash": "t0", "config_hash": "c0",
        "kes_opcert_id": "kes:5", "has_forging_keys": True, "host_key_sha256": "hk",
        "genesis_hash": "gh", "network": "mainnet",
    }
    live.update(over)
    doc = {"supervisor": {"runtime": "docker", "rootful": True, "rootless": False,
                          "node_container_count": 1, "uses_bind_mounts": True,
                          "daemon_socket": "/var/run/docker.sock", "restart_policy": "unless-stopped",
                          "orchestration": "run"}, "live": live}
    p = Path(home) / "obs.json"
    p.write_text(json.dumps(doc))
    return str(p)


def run(home, *args):
    env = dict(os.environ, OURO_HOME=home)
    r = subprocess.run([str(BIN), *args], env=env, text=True, capture_output=True)
    try:
        out = json.loads(r.stdout or r.stderr)
    except Exception:
        out = {"status": "error", "raw": (r.stdout + r.stderr)[:200]}
    return r.returncode, out


def main():
    home = tempfile.mkdtemp()
    o = obs(home)

    # 0. op on an UNadopted node → not_ouro_managed
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert "not_ouro_managed" in json.dumps(d), d

    # 1. adopt a conforming node
    _, d = run(home, "adopt", "--node", "bp1", "--role", "bp",
               "--approve-token", "op-tok", "--observation", o)
    assert d["status"] == "ok" and d["data"]["state_generation"] == 0, d

    # 2. dangerous write without a confirm-token → refuse
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and "dangerous write" in json.dumps(d), d

    # 3. hostile param → refuse (closed schema)
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1; rm -rf /", "--observation", o, "--plan")
    assert d["status"] == "error", d

    # 4. unregistered / legacy op → refuse
    _, d = run(home, "op", "run", "--op", "deploy/takeover", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and ("disabled" in json.dumps(d) or "registry" in json.dumps(d)), d

    # 5. mint a confirm-token bound to the exact intent, then op run passes all gates (plan)
    #    First discover the intent hash from the refusal message path: build it the same way by
    #    running confirm create with the hash printed by a dry validate — simpler: the op refusal
    #    text carries the confirm command with the hash. Extract it.
    _, ref = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
                 "--param", "machine=bp1", "--observation", o, "--plan")
    msg = json.dumps(ref)
    import re
    m = re.search(r"--intent-hash ([0-9a-f]+)", msg)
    assert m, f"no intent hash in refusal: {msg}"
    ihash = m.group(1)
    _, tok = run(home, "confirm", "create", "--op", "runtime/restart", "--node", "bp1",
                 "--intent-hash", ihash)
    token = tok["data"]["confirm_token"]
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--confirm-token", token, "--plan")
    assert d["status"] == "ok", f"confirmed dangerous op should pass gates: {d}"

    # 6. live drift (container recreated) → refuse before mutation
    o2 = obs(home + "/d2" if False else home, container_id="cid-swapped")
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o2, "--confirm-token", token, "--plan")
    assert d["status"] == "error" and "drift" in json.dumps(d), d

    print("S0019 pipeline (adopt + op gates) passed")


if __name__ == "__main__":
    main()
