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
GENESIS = "162d29c4e1cf6b8a84f2d692e67a3ac6bc7851bc3e6e4afe64d15778bed8bd86"
HOST_KEY = "SHA256:" + "a" * 43


def cfg_digest():
    a = json.loads((ROOT / "data/allowlist.json").read_text())
    return a["contracts"][0]["allowed"][0]["image_config_digest"]


def obs(home, **over):
    live = {
        "image_config_digest": cfg_digest(), "platform": "linux/amd64", "container_id": "cid1",
        "container_creation_epoch": 1000, "container_name": "cardano-node",
        "image_reference": "ghcr.io/blinklabs-io/cardano-node:test",
        "entrypoint": ["/usr/local/bin/entrypoint"], "args": ["run"],
        "image_entrypoint": ["/usr/local/bin/entrypoint"], "image_cmd": [],
        "mounts": [
            {"kind": "bind", "source_id": "8:1", "destination": "/data/db",
             "read_only": False, "owner": "0:0", "mode": "0755", "no_symlink": True},
            {"kind": "bind", "source_id": "8:2", "destination": "/opt/cardano/config",
             "read_only": True, "owner": "0:0", "mode": "0755", "no_symlink": True},
            {"kind": "bind", "source_id": "8:3", "destination": "/ipc",
             "read_only": False, "owner": "0:0", "mode": "0755", "no_symlink": True},
        ],
        "topology_hash": "t0", "config_hash": "c0",
        "kes_opcert_id": "kes:5", "has_forging_keys": True,
        "forging_key_permissions_safe": True, "host_key_sha256": HOST_KEY,
        "genesis_hash": GENESIS,
        "network": "preprod",
    }
    live.update(over)
    relay = not live["has_forging_keys"]
    doc = {"supervisor": {"runtime": "docker", "rootful": True, "rootless": False,
                          "node_container_count": 1, "uses_bind_mounts": True,
                          "daemon_socket": "/var/run/docker.sock", "restart_policy": "unless-stopped",
                          "orchestration": "run"}, "live": live,
           "readiness": {"node_running": True, "socket_answers": True,
                         "tip_block": 100, "tip_block_next": 100, "tip_synced": True,
                         "kes_opcert_valid": not relay, "forging_credentials_ready": not relay,
                         "established_peers": 2}}
    p = Path(home) / "obs.json"
    p.write_text(json.dumps(doc))
    return str(p)


def run(home, *args, path=None, env_extra=None):
    env = dict(os.environ, OURO_HOME=home)
    if env_extra:
        env.update(env_extra)
    if path:
        env["PATH"] = f"{path}:{env['PATH']}"
    r = subprocess.run([str(BIN), *args], env=env, text=True, capture_output=True)
    try:
        out = json.loads(r.stdout or r.stderr)
    except Exception:
        out = {"status": "error", "raw": (r.stdout + r.stderr)[:200]}
    return r.returncode, out


def security_digest(home):
    _, out = run(home, "version")
    assert out["status"] == "ok", out
    return out["data"]["security_identity"]


def adoption_args(home, observation, role, node):
    return ("--node", node, "--role", role, "--observation", observation,
            "--expect-embedded", security_digest(home),
            "--expected-role", role, "--expected-network", "preprod",
            "--expected-genesis", GENESIS, "--expected-host-key", HOST_KEY)


def fleet_identity(home):
    _, out = run(home, "fleet", "spec", "identity", "--spec",
                 str(ROOT / "examples/pool-spec.minimal.yaml"))
    assert out["status"] == "ok", out
    return out["data"]


def fleet_permit(home, operation, intent_hash):
    credentials = Path(home) / "credentials"
    credentials.mkdir(exist_ok=True)
    for name in ("bp1", "relay1"):
        (credentials / name).write_text("test-key")
    fakebin = Path(home) / "fleet-fakebin"
    fakebin.mkdir(exist_ok=True)
    status = {
        "tool": "ouro.fleet.status", "machine": None, "status": "ok", "changed": False,
        "checks": [], "duration_s": 0.0, "audit_id": None,
        "data": {"node": "NODE", "role": "ROLE", "online": True,
                 "network": "preprod", "genesis_hash": GENESIS,
                 "host_key_sha256": HOST_KEY,
                 "image_config_digest": cfg_digest(), "state_generation": 0},
    }
    bp = json.dumps(status).replace('"NODE"', '"bp1"').replace('"ROLE"', '"bp"')
    relay = json.dumps(status).replace('"NODE"', '"relay1"').replace('"ROLE"', '"relay"')
    fake_ssh = fakebin / "ssh"
    fake_ssh.write_text(
        "#!/bin/sh\n"
        "dd of=/dev/null bs=65536 2>/dev/null\n"
        f"case \"$*\" in *ouro-exec@10.0.0.10*) printf '%s\\n' '{bp}';; "
        f"*) printf '%s\\n' '{relay}';; esac\n"
    )
    fake_ssh.chmod(0o700)
    runner = Path(home) / "fleet-runner"
    runner.write_bytes(b"fleet-runner")
    _, out = run(home, "fleet", "permit", "create", "--spec",
                 str(ROOT / "examples/pool-spec.minimal.yaml"),
                 "--node", "bp1", "--op", operation,
                 "--intent-hash", intent_hash, "--min-online-relays", "1",
                 "--holder", "testctl", path=fakebin,
                 env_extra={"OURO_EPHEMERAL_RUNNER": str(runner)})
    assert out["status"] == "ok", out
    assert out["data"]["facts"]["source"] == "target-validated-live-snapshot", out
    return out["data"]["fleet_permit"]


def adopt(home, observation, role="bp", node="bp1"):
    args = adoption_args(home, observation, role, node)
    code, preview = run(home, "adopt", *args, "--preview")
    if code != 0:
        return code, preview
    data = preview["data"]
    _, approval = run(home, "confirm", "adopt", "create", "--node", node,
                      "--candidate-hash", data["candidate_hash"],
                      "--host-key", data["host_key_sha256"])
    return run(home, "adopt", *args, "--approve-token", approval["data"]["approve_token"])


def main():
    home = tempfile.mkdtemp()
    o = obs(home)

    # 0. op on an UNadopted node → not_ouro_managed
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert "not_ouro_managed" in json.dumps(d), d

    # 1. adoption accepts only a signed, preview-bound approval; an arbitrary string is refused.
    adopt_bp_args = adoption_args(home, o, "bp", "bp1")
    _, preview = run(home, "adopt", *adopt_bp_args, "--preview")
    assert preview["status"] == "ok" and len(preview["data"]["candidate_hash"]) == 64, preview
    _, forged = run(home, "adopt", *adopt_bp_args, "--approve-token", "arbitrary")
    assert forged["status"] == "error" and "confirmation" in json.dumps(forged), forged

    # A valid single-use approval adopts the conforming node.
    _, d = adopt(home, o)
    assert d["status"] == "ok" and d["data"]["state_generation"] == 0, d
    attestation = Path(home) / "attestations" / "bp1.json"
    assert stat.S_IMODE(attestation.stat().st_mode) == 0o640
    attested = json.loads(attestation.read_text())["immutable"]
    expected_allowlist_version = json.loads(
        (ROOT / "data/allowlist.json").read_text()
    )["allowlist_version"]
    assert attested["allowlist_version"] == expected_allowlist_version
    assert len(attested["allowlist_digest"]) == 71
    assert len({attested["oci_index_digest"], attested["platform_manifest_digest"],
                attested["image_config_digest"]}) == 3, attested
    identity = fleet_identity(home)
    revised_spec = Path(home) / "pool-revised.yaml"
    revised_spec.write_text((ROOT / "examples/pool-spec.minimal.yaml").read_text().replace(
        'node_version: "10.5.4"', 'node_version: "10.5.5"'
    ))
    _, revised_identity = run(home, "fleet", "spec", "identity", "--spec", str(revised_spec))
    assert revised_identity["data"]["pool_id"] == identity["pool_id"]
    assert revised_identity["data"]["pool_spec_digest"] != identity["pool_spec_digest"]
    policy = ("--fleet-spec-digest", identity["pool_spec_digest"],
              "--fleet-pool-id", identity["pool_id"],
              "--fleet-min-online-relays", "1")

    # A disruptive plan is already FINAL and binds the stable pool identity/quorum policy. A live
    # permit is deliberately minted only after this hash is approved.
    locks = Path(home) / "txn" / "locks"
    before_locks = sorted(locks.glob("*")) if locks.exists() else []
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, *policy, "--plan")
    assert d["status"] == "ok" and d["data"]["intent_hash_final"] is True, d
    assert d["data"]["fleet_permit_required"] is True
    assert d["data"]["fleet_policy"]["pool_spec_digest"] == identity["pool_spec_digest"]
    after_locks = sorted(locks.glob("*")) if locks.exists() else []
    assert after_locks == before_locks, "--plan must not create a persistent node lock"

    ihash = d["data"]["intent_hash"]
    _, tok = run(home, "confirm", "create", "--op", "runtime/restart", "--node", "bp1",
                 "--intent-hash", ihash)
    token = tok["data"]["confirm_token"]
    fp = fleet_permit(home, "runtime/restart", ihash)

    # 2. A real dangerous write with a final live permit but no confirm-token still refuses.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o,
               *policy, "--fleet-permit", fp)
    assert d["status"] == "error" and "dangerous write" in json.dumps(d), d

    # 3. hostile param → refuse (closed schema)
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1; rm -rf /", "--observation", o, *policy, "--plan")
    assert d["status"] == "error", d

    # 3a. every target selector is bound to the adopted machine; a valid-but-different id refuses.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=relay1", "--observation", o, *policy, "--plan")
    assert d["status"] == "error" and "target binding mismatch" in json.dumps(d), d
    # Node ids are validated before they can become attestation/journal/lock paths.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "../bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and "machine id" in json.dumps(d), d

    # 4. unregistered / legacy op → refuse
    _, d = run(home, "op", "run", "--op", "retired/write", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and (
        "disabled" in json.dumps(d)
        or "registry" in json.dumps(d)
        or "unknown write operation" in json.dumps(d)
    ), d
    # Misleading names that used to execute only a restart are retired, not silently aliased.
    for retired in ("config/render", "runtime/topology-apply", "kes-rotation/rotate"):
        _, d = run(home, "op", "run", "--op", retired, "--node", "bp1",
                   "--param", "machine=bp1", "--observation", o, "--plan")
        assert d["status"] == "error" and ("disabled" in json.dumps(d) or "registry" in json.dumps(d)), d

    # A KES plan binds real staged PUBLIC bytes; a syntactically valid but absent artifact is not
    # displayed as if it were executable.
    missing_opcert = "opcert-1@sha256:" + "c" * 64
    _, d = run(home, "op", "run", "--op", "kes-rotation/install-opcert", "--node", "bp1",
               "--param", "machine=bp1", "--param", f"opcert={missing_opcert}",
               "--observation", o, *policy, "--plan")
    assert d["status"] == "error" and "artifact" in json.dumps(d), d

    # 5. Permit/confirmation capabilities are rejected by plan and never change the final hash.
    _, ref = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
                 "--param", "machine=bp1", "--observation", o,
                 *policy, "--fleet-permit", fp, "--plan")
    assert ref["status"] == "error" and "do not pass a fleet permit" in json.dumps(ref), ref
    # Approval is minted only AFTER plan review and is not accepted by plan mode. We intentionally
    # stop here: executing without --plan would restart the container.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--fleet-permit", fp,
               *policy, "--confirm-token", token, "--plan")
    assert d["status"] == "error" and "do not pass a confirm-token" in json.dumps(d), d

    # 5a. a stale control security identity is rejected on the target-side path.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o,
               *policy, "--expect-embedded", "stale-control", "--plan")
    assert d["status"] == "error" and "security identity mismatch" in json.dumps(d), d

    # 6. typed-mount drift (same source id, changed ownership/mode) is identity drift too.
    changed_mount = [{"kind": "bind", "source_id": "8:1", "destination": "/data/db",
                      "read_only": False, "owner": "0:0", "mode": "0777", "no_symlink": True}]
    om = obs(home, mounts=changed_mount)
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", om,
               *policy, "--plan")
    assert d["status"] == "error" and "mount" in json.dumps(d), d

    # 7. live drift (container recreated) → refuse before mutation
    o2 = obs(home + "/d2" if False else home, container_id="cid-swapped")
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o2,
               *policy, "--plan")
    assert d["status"] == "error" and "drift" in json.dumps(d), d

    # 8. S0020 reads do not consult the persistent allowlist floor or attestation. The active
    # signed image policy remains visible in the result and remains authoritative for writes.
    (Path(home) / "allowlist-floor.json").unlink()
    _, d = run(home, "op", "run", "--op", "observability/health", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o2)
    assert d["status"] == "ok" and d["data"]["management_state"] == "not_required", d
    assert d["data"]["assurance"] == "live_observation", d

    print("S0019 pipeline (adopt + op gates) passed")


if __name__ == "__main__":
    main()
