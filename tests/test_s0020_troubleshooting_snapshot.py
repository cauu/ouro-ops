#!/usr/bin/env python3
"""S0020 p4-7 — role-aware troubleshooting baseline and BP KES conclusion gate."""

import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"


def observation(kes, *, peers=3, configured=True):
    return {
        "supervisor": {
            "runtime": "docker", "rootful": True, "rootless": False,
            "node_container_count": 1, "uses_bind_mounts": True,
            "daemon_socket": "/var/run/docker.sock", "restart_policy": "unless-stopped",
            "orchestration": "run",
        },
        "live": {
            "image_config_digest": "sha256:" + "9" * 64,
            "platform": "linux/amd64", "container_id": "cid-live",
            "container_name": "cardano-node",
            "image_reference": "ghcr.io/blinklabs-io/cardano-node:10.5.4-1",
            "container_creation_epoch": 1,
            "entrypoint": ["/usr/local/bin/entrypoint"], "args": ["run"],
            "image_entrypoint": ["/usr/local/bin/entrypoint"], "image_cmd": [],
            "mounts": [], "topology_hash": "", "config_hash": "",
            "kes_opcert_id": "opcert-digest", "has_forging_keys": True,
            "forging_key_permissions_safe": True, "host_key_sha256": "",
            "genesis_hash": "genesis", "network": "mainnet",
        },
        "readiness": {
            "node_running": True, "socket_answers": True,
            "tip_block": 192686230, "tip_block_next": 192686230,
            "tip_block_height": 13688009, "tip_slot": 192686230,
            "tip_era": "Conway", "sync_progress": "100.00", "tip_synced": True,
            "kes_opcert_valid": bool(kes and kes["valid"]), "kes": kes,
            "block_producer_configured": configured,
            "forging_credentials_ready": bool(kes and kes["valid"] and configured),
            "established_peers": peers,
        },
        "recreate": None,
    }


def kes(current, end, *, valid):
    return {
        "current_period": current, "start_period": 1342, "end_period": end,
        "remaining_periods": end - current, "opcert_counter_on_disk": 7,
        "opcert_counter_node_state": 7, "counter_consistent": True, "valid": valid,
    }


def invoke(home, *args, extra_env=None):
    env = dict(os.environ, OURO_HOME=str(home))
    if extra_env:
        env.update(extra_env)
    completed = subprocess.run([str(BIN), *args], env=env, text=True, capture_output=True)
    raw = completed.stdout or completed.stderr
    return completed, json.loads(raw)


def main():
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0020-troubleshooting-"))
    pool_spec = home / "pool-spec.yaml"
    pool_spec.write_text(
        """spec_version: 1
pool:
  network: mainnet
  network_magic: 764824073
  genesis_hashes:
    shelley: 1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh: {host: 192.0.2.1, port: 2222, user: cardano, key_ref: creds://bp1}
  - id: relay1
    role: relay
    public_endpoint: {host: 192.0.2.2, port: 3001}
    ssh: {host: 192.0.2.2, port: 22, user: cardano, key_ref: creds://relay1}
upgrade: {min_online_relays: 0}
"""
    )

    def snapshot(node, payload):
        fixture = home / f"{node}-observation.json"
        fixture.write_text(json.dumps(payload))
        result, value = invoke(
            home, "op", "run", "--op", "troubleshooting/snapshot", "--node", node,
            "--spec", str(pool_spec), "--observation", str(fixture),
        )
        assert result.returncode == 0, result
        assert value["tool"] == "ouro.troubleshooting.snapshot", value
        return value["data"]

    expired = snapshot("bp1", observation(kes(1486, 1404, valid=False)))
    assert expired["role_readiness"]["status"] == "not_ready", expired
    assert expired["role_readiness"]["overall_health_claimed"] is False, expired
    assert expired["result"]["liveness"]["tip_synced"] is True, expired
    assert expired["result"]["forging"]["status"] == "opcert_expired", expired
    assert expired["result"]["forging"]["block_production_ready"] is False, expired
    assert expired["result"]["forging"]["kes"]["remaining_periods"] == -82, expired

    missing = snapshot("bp1", observation(None))
    assert missing["role_readiness"]["status"] == "insufficient_evidence", missing
    assert missing["result"]["forging"]["status"] == "kes_evidence_unavailable", missing

    healthy = snapshot("bp1", observation(kes(1380, 1404, valid=True)))
    assert healthy["role_readiness"]["status"] == "ready", healthy
    assert healthy["result"]["forging"]["block_production_ready"] is True, healthy

    relay = snapshot("relay1", observation(None, peers=4, configured=False))
    assert relay["role_readiness"]["status"] == "ready", relay
    assert relay["result"]["forging"]["status"] == "not_applicable", relay
    assert relay["result"]["network"]["established_peers"] == 4, relay

    # The control derives the target role from the same spec that binds host, port and credential.
    credentials = home / "credentials"
    credentials.mkdir()
    (credentials / "bp1").write_text("test-only-ssh-key")
    (home / "known_hosts").write_text("[192.0.2.1]:2222 ssh-ed25519 test\n")
    runner = home / "ouro-ops-linux-x86_64"
    runner.write_bytes(b"troubleshooting-runner-fixture\x00\xff")
    digest = hashlib.sha256(runner.read_bytes()).hexdigest()
    preview, preview_value = invoke(
        home, "op", "run", "--op", "troubleshooting/snapshot", "--node", "bp1",
        "--dispatch", "192.0.2.1", "--ssh-key", "creds://bp1", "--spec", str(pool_spec),
        "--param", "machine=bp1", "--transport-plan",
        extra_env={"OURO_EPHEMERAL_RUNNER": str(runner)},
    )
    assert preview.returncode == 0, preview
    data = preview_value["data"]
    assert data["op"] == "troubleshooting/snapshot", data
    assert data["persistent_target_install"] is False, data
    assert data["runner"]["sha256"] == digest, data
    remote = " ".join(data["ssh_argv"])
    assert "'--op' 'troubleshooting/snapshot' '--role' 'bp'" in remote, remote
    assert "/usr/local/bin/ouro-ops" not in remote and "ouro-op-run" not in remote, remote

    print("S0020 role-aware troubleshooting snapshot passed")


if __name__ == "__main__":
    main()
