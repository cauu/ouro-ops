#!/usr/bin/env python3
"""S0020 p1-2 — stateless observation through the ephemeral runner control surface."""

import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"


def observation():
    return {
        "supervisor": {
            "runtime": "docker",
            "rootful": True,
            "rootless": False,
            "node_container_count": 1,
            "uses_bind_mounts": True,
            "daemon_socket": "/var/run/docker.sock",
            "restart_policy": "unless-stopped",
            "orchestration": "run",
        },
        "live": {
            "image_config_digest": "sha256:" + "9" * 64,
            "platform": "linux/amd64",
            "container_id": "cid-live",
            "container_name": "cardano-node",
            "image_reference": "ghcr.io/blinklabs-io/cardano-node:future",
            "container_creation_epoch": 1,
            "entrypoint": ["/usr/local/bin/entrypoint"],
            "args": ["run"],
            "image_entrypoint": ["/usr/local/bin/entrypoint"],
            "image_cmd": [],
            "mounts": [],
            "topology_hash": "",
            "config_hash": "",
            "kes_opcert_id": "",
            "has_forging_keys": False,
            "forging_key_permissions_safe": False,
            "host_key_sha256": "",
            "genesis_hash": "genesis",
            "network": "mainnet",
        },
        "readiness": {
            "node_running": True,
            "socket_answers": True,
            "tip_block": 123456,
            "tip_block_next": 123456,
            "tip_block_height": 9876,
            "tip_slot": 123456,
            "tip_era": "Conway",
            "sync_progress": "100.00",
            "tip_synced": True,
            "kes_opcert_valid": False,
            "forging_credentials_ready": False,
            "established_peers": 3,
        },
        "recreate": None,
    }


def invoke(home, *args, extra_env=None, path=None):
    env = dict(os.environ, OURO_HOME=str(home))
    if extra_env:
        env.update(extra_env)
    if path:
        env["PATH"] = f"{path}:{env['PATH']}"
    completed = subprocess.run(
        [str(BIN), *args], env=env, text=True, capture_output=True
    )
    raw = completed.stdout or completed.stderr
    return completed, json.loads(raw)


def assert_live_result(value):
    assert value["status"] == "ok" and value["changed"] is False, value
    assert value["tool"] == "ouro.observe", value
    data = value["data"]
    assert data["assurance"] == "live_observation", data
    assert data["management_state"] == "not_required", data
    assert data["result"]["tip"] == {
        "block": 9876,
        "slot": 123456,
        "era": "Conway",
        "sync_progress": "100.00",
    }, data
    assert data["result"]["runtime_policy"]["supported"] is False, data
    assert data["result"]["runtime_policy"]["effect"].startswith("informational"), data


def main():
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0020-observe-"))
    fixture = home / "observation.json"
    fixture.write_text(json.dumps(observation()))

    # A local/debug read has no adoption metadata and an unsupported image, but still returns the
    # live evidence. Image policy is informational for reads and remains a write gate.
    local, local_value = invoke(
        home,
        "op",
        "run",
        "--op",
        "observability/health",
        "--node",
        "bp1",
        "--observation",
        str(fixture),
    )
    assert local.returncode == 0, local
    assert_live_result(local_value)
    assert not (home / "attestations").exists(), "read must not create ownership state"

    # The closed target command invokes the same live probe path. A test-only probe fixture avoids
    # requiring Docker while proving that target observe itself has no attestation prerequisite.
    probe = home / "probe.sh"
    payload = json.dumps(observation(), separators=(",", ":"))
    probe.write_text(f"ouro_observe() {{ printf '%s\\n' '{payload}'; }}\n")
    target, target_value = invoke(
        home,
        "target",
        "observe",
        "--node",
        "bp1",
        extra_env={"OURO_PROBE_LIB": str(probe)},
    )
    assert target.returncode == 0, target
    assert_live_result(target_value)

    credentials = home / "credentials"
    credentials.mkdir()
    (credentials / "bp1").write_text("test-only-ssh-key")
    (home / "known_hosts").write_text("192.0.2.1 ssh-ed25519 test\n")
    runner = home / "ouro-ops-linux-x86_64"
    runner_bytes = b"repository-built-linux-runner-fixture\x00\xff"
    runner.write_bytes(runner_bytes)
    runner_sha = hashlib.sha256(runner_bytes).hexdigest()
    runner_env = {"OURO_EPHEMERAL_RUNNER": str(runner)}

    preview, preview_value = invoke(
        home,
        "op",
        "run",
        "--op",
        "observability/health",
        "--node",
        "bp1",
        "--dispatch",
        "192.0.2.1",
        "--ssh-key",
        "creds://bp1",
        "--transport-plan",
        extra_env=runner_env,
    )
    assert preview.returncode == 0, preview
    data = preview_value["data"]
    assert preview_value["tool"] == "ouro.observe.dispatch.transport_plan", preview_value
    assert data["principal"] == "cardano" and data["persistent_target_install"] is False
    assert data["runner"]["sha256"] == runner_sha
    remote = " ".join(data["ssh_argv"])
    for expected in [
        "cardano@192.0.2.1",
        "StrictHostKeyChecking=yes",
        "mktemp -d /tmp/ouro-run.XXXXXXXXXX",
        "trap cleanup EXIT",
        f"'{runner_sha}'",
        '"$runner" \'target\' \'observe\' \'--node\' \'bp1\'',
    ]:
        assert expected in remote, (expected, remote)
    assert "/usr/local/bin/ouro-ops" not in remote and "ouro-op-run" not in remote
    assert "not_ouro_managed" not in json.dumps(preview_value)

    # A fake SSH transport consumes stdin and returns one target ToolOutput. This proves the public
    # command streams the exact selected bytes and transparently forwards the typed result.
    fakebin = home / "fakebin"
    fakebin.mkdir()
    log = home / "transport.log"
    remote_output = dict(target_value)
    ssh = fakebin / "ssh"
    ssh.write_text(
        "#!/usr/bin/env bash\n"
        "set -eu\n"
        "payload=$(mktemp)\n"
        "trap 'rm -f \"$payload\"' EXIT\n"
        "dd of=\"$payload\" bs=65536 status=none\n"
        "sha256sum \"$payload\" | awk '{print $1}' >\"$OURO_TEST_TRANSPORT_LOG\"\n"
        f"printf '%s\\n' '{json.dumps(remote_output, separators=(',', ':'))}'\n"
    )
    ssh.chmod(0o700)
    dispatched, dispatched_value = invoke(
        home,
        "op",
        "run",
        "--op",
        "observability/health",
        "--node",
        "bp1",
        "--dispatch",
        "192.0.2.1",
        "--ssh-key",
        "creds://bp1",
        extra_env={**runner_env, "OURO_TEST_TRANSPORT_LOG": str(log)},
        path=fakebin,
    )
    assert dispatched.returncode == 0, dispatched
    assert_live_result(dispatched_value)
    assert log.read_text().strip() == runner_sha, log.read_text()

    print("S0020 stateless ephemeral observability passed")


if __name__ == "__main__":
    main()
