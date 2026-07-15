#!/usr/bin/env python3
"""S0019 p4-1 — the adopt + intent (`op`) pipeline via the CLI. Every gate must fire in order and
refuse before any mutation."""
import json
import os
import stat
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
        "mounts": [{"kind": "bind", "source_id": "8:1", "destination": "/data/db",
                    "read_only": False, "owner": "0:0", "mode": "0755", "no_symlink": True}],
        "topology_hash": "t0", "config_hash": "c0",
        "kes_opcert_id": "kes:5", "has_forging_keys": True, "host_key_sha256": "a" * 64,
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


def fleet_permit(home, operation):
    _, out = run(home, "fleet", "permit", "create", "--pool-id", "testpool",
                 "--node", "bp1", "--op", operation, "--role", "bp",
                 "--online-relays", "1", "--min-online-relays", "1",
                 "--relays-remaining", "0", "--holder", "testctl")
    assert out["status"] == "ok", out
    return out["data"]["fleet_permit"]


def adopt(home, observation, role="bp", node="bp1"):
    code, preview = run(home, "adopt", "--node", node, "--role", role,
                        "--preview", "--observation", observation)
    if code != 0:
        return code, preview
    data = preview["data"]
    _, approval = run(home, "confirm", "adopt", "create", "--node", node,
                      "--candidate-hash", data["candidate_hash"],
                      "--host-key", data["host_key_sha256"])
    return run(home, "adopt", "--node", node, "--role", role,
               "--approve-token", approval["data"]["approve_token"],
               "--observation", observation)


def main():
    home = tempfile.mkdtemp()
    o = obs(home)

    # 0. op on an UNadopted node → not_ouro_managed
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert "not_ouro_managed" in json.dumps(d), d

    # 1. adoption accepts only a signed, preview-bound approval; an arbitrary string is refused.
    _, preview = run(home, "adopt", "--node", "bp1", "--role", "bp",
                     "--preview", "--observation", o)
    assert preview["status"] == "ok" and len(preview["data"]["candidate_hash"]) == 64, preview
    _, forged = run(home, "adopt", "--node", "bp1", "--role", "bp",
                    "--approve-token", "arbitrary", "--observation", o)
    assert forged["status"] == "error" and "confirmation" in json.dumps(forged), forged

    # A valid single-use approval adopts the conforming node.
    _, d = adopt(home, o)
    assert d["status"] == "ok" and d["data"]["state_generation"] == 0, d
    attestation = Path(home) / "attestations" / "bp1.json"
    assert stat.S_IMODE(attestation.stat().st_mode) == 0o640
    attested = json.loads(attestation.read_text())["immutable"]
    assert attested["allowlist_version"] == 1 and len(attested["allowlist_digest"]) == 71
    assert len({attested["oci_index_digest"], attested["platform_manifest_digest"],
                attested["image_config_digest"]}) == 3, attested
    fp = fleet_permit(home, "runtime/restart")

    # Disruptive operations fail closed without a signed pool-wide step permit.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and "fleet-permit" in json.dumps(d), d

    # 2. dangerous write without a confirm-token → refuse
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o,
               "--fleet-permit", fp, "--plan")
    assert d["status"] == "error" and "dangerous write" in json.dumps(d), d

    # 3. hostile param → refuse (closed schema)
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1; rm -rf /", "--observation", o, "--plan")
    assert d["status"] == "error", d

    # 3a. every target selector is bound to the adopted machine; a valid-but-different id refuses.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=relay1", "--observation", o, "--plan")
    assert d["status"] == "error" and "target binding mismatch" in json.dumps(d), d
    # Node ids are validated before they can become attestation/journal/lock paths.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "../bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and "machine id" in json.dumps(d), d

    # 4. unregistered / legacy op → refuse
    _, d = run(home, "op", "run", "--op", "deploy/takeover", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and ("disabled" in json.dumps(d) or "registry" in json.dumps(d)), d
    # Misleading names that used to execute only a restart are retired, not silently aliased.
    for retired in ("config/render", "runtime/topology-apply", "kes-rotation/rotate"):
        _, d = run(home, "op", "run", "--op", retired, "--node", "bp1",
                   "--param", "machine=bp1", "--observation", o, "--plan")
        assert d["status"] == "error" and ("disabled" in json.dumps(d) or "registry" in json.dumps(d)), d

    # The approved deploy network must be the same network the attested executor will use.
    tx_ref = "tx-1@sha256:" + "b" * 64
    _, d = run(home, "op", "run", "--op", "deploy/register-submit", "--node", "bp1",
               "--param", "machine=bp1", "--param", f"tx={tx_ref}",
               "--param", "network=preview", "--observation", o, "--plan")
    assert d["status"] == "error" and "payload network" in json.dumps(d), d

    # 5. mint a confirm-token bound to the exact intent, then op run passes all gates (plan)
    #    First discover the intent hash from the refusal message path: build it the same way by
    #    running confirm create with the hash printed by a dry validate — simpler: the op refusal
    #    text carries the confirm command with the hash. Extract it.
    _, ref = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
                 "--param", "machine=bp1", "--observation", o,
                 "--fleet-permit", fp, "--plan")
    msg = json.dumps(ref)
    import re
    m = re.search(r"--intent-hash ([0-9a-f]+)", msg)
    assert m, f"no intent hash in refusal: {msg}"
    ihash = m.group(1)
    assert len(ihash) == 64, ihash
    _, tok = run(home, "confirm", "create", "--op", "runtime/restart", "--node", "bp1",
                 "--intent-hash", ihash)
    token = tok["data"]["confirm_token"]
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--fleet-permit", fp,
               "--confirm-token", token, "--plan")
    assert d["status"] == "ok", f"confirmed dangerous op should pass gates: {d}"

    # 5a. a stale control security identity is rejected on the target-side path.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--fleet-permit", fp,
               "--confirm-token", token,
               "--expect-embedded", "stale-control", "--plan")
    assert d["status"] == "error" and "security identity mismatch" in json.dumps(d), d

    # 6. typed-mount drift (same source id, changed ownership/mode) is identity drift too.
    changed_mount = [{"kind": "bind", "source_id": "8:1", "destination": "/data/db",
                      "read_only": False, "owner": "0:0", "mode": "0777", "no_symlink": True}]
    om = obs(home, mounts=changed_mount)
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", om, "--fleet-permit", fp,
               "--confirm-token", token, "--plan")
    assert d["status"] == "error" and "mount" in json.dumps(d), d

    # 7. live drift (container recreated) → refuse before mutation
    o2 = obs(home + "/d2" if False else home, container_id="cid-swapped")
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o2, "--fleet-permit", fp,
               "--confirm-token", token, "--plan")
    assert d["status"] == "error" and "drift" in json.dumps(d), d

    # 8. erasing the allowlist floor on an adopted node fails closed; it does not reset to v1.
    (Path(home) / "allowlist-floor.json").unlink()
    _, d = run(home, "op", "run", "--op", "observability/health", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o2)
    assert d["status"] == "error" and "anti-rollback floor is missing" in json.dumps(d), d

    print("S0019 pipeline (adopt + op gates) passed")


if __name__ == "__main__":
    main()
