#!/usr/bin/env python3
"""S0020 p2-1 — target-validated plans need live state + pool bindings, not ownership state."""

import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"
GENESIS = "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81"
ACTIVE_KES_VKEY = {
    "type": "KesVerificationKey_ed25519_kes_2^6",
    "description": "active public KES key",
    "cborHex": "5820" + "11" * 32,
}
STAGED_KES_VKEY = {
    "type": "KesVerificationKey_ed25519_kes_2^6",
    "description": "staged public KES key",
    "cborHex": "5820" + "22" * 32,
}


def observation(container="cid-plan"):
    allowed = json.loads((ROOT / "data/allowlist.json").read_text())["contracts"][0]["allowed"][-1]
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
            "image_config_digest": allowed["image_config_digest"],
            "platform": allowed["platform"],
            "container_id": container,
            "container_name": "cardano-node",
            "image_reference": "ghcr.io/blinklabs-io/cardano-node:10.5.4-1",
            "container_creation_epoch": 1234,
            "entrypoint": ["/usr/local/bin/entrypoint"],
            "args": ["run"],
            "image_entrypoint": ["/usr/local/bin/entrypoint"],
            "image_cmd": [],
            "mounts": [
                {
                    "kind": "bind",
                    "source_id": "8:1",
                    "destination": "/data/db",
                    "read_only": False,
                    "owner": "1000:1000",
                    "mode": "0700",
                    "no_symlink": True,
                },
                {
                    "kind": "bind",
                    "source_id": "8:2",
                    "destination": "/opt/cardano/config",
                    "read_only": True,
                    "owner": "1000:1000",
                    "mode": "0700",
                    "no_symlink": True,
                },
                {
                    "kind": "bind",
                    "source_id": "8:3",
                    "destination": "/ipc",
                    "read_only": False,
                    "owner": "1000:1000",
                    "mode": "0700",
                    "no_symlink": True,
                },
            ],
            "topology_hash": "topology",
            "config_hash": "config",
            "kes_opcert_id": "opcert-public-digest",
            "has_forging_keys": True,
            "forging_key_permissions_safe": True,
            "host_key_sha256": "SHA256:" + "a" * 43,
            "genesis_hash": GENESIS,
            "network": "mainnet",
        },
        "readiness": {
            "node_running": True,
            "socket_answers": True,
            "tip_block": 10,
            "tip_block_next": 10,
            "tip_block_height": 9,
            "tip_slot": 10,
            "tip_era": "Conway",
            "sync_progress": "100.00",
            "tip_synced": True,
            "kes_opcert_valid": True,
            "kes": {
                "source": "cardano-cli query kes-period-info",
                "current_period": 100,
                "start_period": 90,
                "end_period": 152,
                "remaining_periods": 52,
                "opcert_counter_on_disk": 7,
                "opcert_counter_node_state": 7,
                "counter_consistent": True,
                "valid": True,
            },
            "forging_credentials_ready": True,
            "established_peers": 2,
        },
        "recreate": {
            "name": "cardano-node",
            "restart_policy": "unless-stopped",
            "network_mode": "bridge",
            "binds": [
                {"source": "/srv/data", "destination": "/data/db", "read_only": False},
                {
                    "source": "/srv/config",
                    "destination": "/opt/cardano/config",
                    "read_only": True,
                },
                {"source": "/srv/ipc", "destination": "/ipc", "read_only": False},
            ],
            "env": ["CARDANO_NETWORK=mainnet", "PRIVATE_VALUE=not-output"],
            "ports": [],
            "entrypoint": "/usr/local/bin/entrypoint",
            "args": ["run"],
        },
    }


def write_probe(path, value):
    payload = json.dumps(value, separators=(",", ":"))
    path.write_text(f"ouro_observe() {{ printf '%s\\n' '{payload}'; }}\n")


def invoke(home, *args, env_extra=None, path=None):
    env = dict(
        os.environ,
        OURO_HOME=str(home),
        OURO_RELEASES_FILE=str(ROOT / "data/releases.json"),
    )
    if env_extra:
        env.update(env_extra)
    if path:
        env["PATH"] = f"{path}:{env['PATH']}"
    completed = subprocess.run([str(BIN), *args], env=env, text=True, capture_output=True)
    raw = completed.stdout or completed.stderr
    return completed, json.loads(raw)


def target_args(operation, *params):
    return (
        "target",
        "plan",
        "--op",
        operation,
        "--node",
        "bp1",
        "--role",
        "bp",
        "--network",
        "mainnet",
        "--genesis",
        GENESIS,
        "--pool-id",
        "pool-0123456789abcdef01234567",
        "--pool-spec-digest",
        "sha256:" + "b" * 64,
        "--min-online-relays",
        "1",
        *params,
    )


def main():
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0020-plan-"))
    probe = home / "probe.sh"
    write_probe(probe, observation())
    probe_env = {"OURO_PROBE_LIB": str(probe)}

    fakebin = home / "fakebin"
    fakebin.mkdir()
    docker = fakebin / "docker"
    docker.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "case \"$*\" in\n"
        "  *'head -c 65537 /opt/cardano/config/keys/.ouro-kes-stage/kes.vkey'*) "
        f"printf '%s\\n' '{json.dumps(STAGED_KES_VKEY, separators=(',', ':'))}' ;;\n"
        "  *'head -c 65537 /opt/cardano/config/keys/kes.vkey'*) "
        f"printf '%s\\n' '{json.dumps(ACTIVE_KES_VKEY, separators=(',', ':'))}' ;;\n"
        "  *'stat -c %a /opt/cardano/config/keys/.ouro-kes-stage/kes.skey'*) printf '600\\n' ;;\n"
        "  *'test -s /opt/cardano/config/keys/.ouro-kes-stage/kes.skey'*) exit 0 ;;\n"
        "  *'test ! -e /opt/cardano/config/keys/.ouro-kes-stage'*) exit 0 ;;\n"
        "  *) exit 90 ;;\n"
        "esac\n"
    )
    docker.chmod(0o700)

    first, first_value = invoke(
        home,
        *target_args("runtime/restart", "--param", "machine=bp1"),
        env_extra=probe_env,
    )
    assert first.returncode == 0, first
    data = first_value["data"]
    assert first_value["tool"] == "ouro.op.plan" and first_value["changed"] is False
    assert data["management_state"] == "not_required"
    assert data["assurance"] == "live_target_validated"
    assert data["executor_plan"] == [["docker", "restart", "cid-plan"]]
    assert data["runtime_policy"]["running_image_config_digest"].startswith("sha256:")
    assert data["pool_binding"]["role"] == "bp" and data["pool_binding"]["network"] == "mainnet"
    assert data["fleet_permit_required"] is True and data["confirmation_required"] is True
    assert data["persistent_target_state_written"] is False
    assert "PRIVATE_VALUE=not-output" not in json.dumps(first_value)
    assert not (home / "attestations").exists() and not (home / "txn").exists()

    second, second_value = invoke(
        home,
        *target_args("runtime/restart", "--param", "machine=bp1"),
        env_extra=probe_env,
    )
    assert second.returncode == 0
    assert second_value["data"]["candidate_hash"] == data["candidate_hash"]

    # Runtime restart neither reads nor executes the upgrade-only recreate spec. Changes confined
    # to that spec must not invalidate the restart approval candidate.
    recreate_changed = observation()
    recreate_changed["recreate"]["env"].append("UNRELATED_RECREATE_VALUE=changed")
    write_probe(probe, recreate_changed)
    recreate_only, recreate_only_value = invoke(
        home,
        *target_args("runtime/restart", "--param", "machine=bp1"),
        env_extra=probe_env,
    )
    assert recreate_only.returncode == 0, recreate_only
    assert recreate_only_value["data"]["candidate_hash"] == data["candidate_hash"]
    write_probe(probe, observation())

    # Docker's inspect Mounts array has set semantics and may arrive in different orders. All typed
    # mount fields remain candidate-bound, but order alone cannot create a different candidate.
    mounts_reordered = observation()
    mounts_reordered["live"]["mounts"].reverse()
    write_probe(probe, mounts_reordered)
    reordered, reordered_value = invoke(
        home,
        *target_args("runtime/restart", "--param", "machine=bp1"),
        env_extra=probe_env,
    )
    assert reordered.returncode == 0, reordered
    assert reordered_value["data"]["candidate_hash"] == data["candidate_hash"]
    write_probe(probe, observation())

    # Phase A derives the live period and proposes generation only in the fixed private stage.
    staged, staged_value = invoke(
        home,
        *target_args("kes-rotation/stage-key", "--param", "machine=bp1"),
        env_extra=probe_env,
        path=fakebin,
    )
    assert staged.returncode == 0, (staged, staged_value)
    assert staged_value["data"]["kes_rotation"]["current_period"] == 100
    assert staged_value["data"]["kes_rotation"]["staged_vkey_sha256"] is None
    stage_plan = staged_value["data"]["executor_plan"]
    assert any("key-gen-KES" in argv for argv in stage_plan)
    assert any(".ouro-kes-stage/kes.skey.tmp" in arg for argv in stage_plan for arg in argv)
    assert all("cold.skey" not in arg for argv in stage_plan for arg in argv)
    assert staged_value["data"]["fleet_permit_required"] is False

    # Phase B binds the exact staged public key and shows backup/promotion/restart/cleanup for the
    # active KES pair plus public opcert. No key bytes or arbitrary target path enter the plan.
    opcert = "opcert@sha256:" + "c" * 64
    kes, kes_value = invoke(
        home,
        *target_args(
            "kes-rotation/install-opcert",
            "--param",
            "machine=bp1",
            "--param",
            f"opcert={opcert}",
        ),
        env_extra=probe_env,
        path=fakebin,
    )
    assert kes.returncode == 0, kes
    assert kes_value["data"]["kes_rotation"]["staged_vkey_sha256"]
    kes_plan = kes_value["data"]["executor_plan"]
    assert kes_plan[0] == [
        "docker",
        "exec",
        "cid-plan",
        "test",
        "!",
        "-e",
        "/opt/cardano/config/keys/kes.skey.ouro-prev",
    ]
    assert any("kes.skey.ouro-prev" in arg for argv in kes_plan for arg in argv)
    assert any("kes.vkey.ouro-prev" in arg for argv in kes_plan for arg in argv)
    assert any("node.cert.ouro-prev" in arg for argv in kes_plan for arg in argv)
    assert any(".ouro-kes-stage/kes.skey" in arg for argv in kes_plan for arg in argv)
    assert any(argv[0:2] == ["docker", "cp"] and opcert in argv[2] for argv in kes_plan)
    assert ["docker", "restart", "cid-plan"] in kes_plan
    assert any(any(".ouro-kes-stage" in arg for arg in argv) and "rm" in argv for argv in kes_plan)
    assert all("ouro-run." not in arg for argv in kes_plan for arg in argv)

    # Runtime and KES use the stable inspected layout, not membership in a changing release feed.
    # A future config digest therefore needs no CLI rebuild when the live layout still conforms.
    future = observation()
    future["live"]["image_config_digest"] = "sha256:" + "e" * 64
    write_probe(probe, future)
    for operation, params in (
        ("runtime/restart", ("--param", "machine=bp1")),
        (
            "kes-rotation/install-opcert",
            ("--param", "machine=bp1", "--param", f"opcert={opcert}"),
        ),
    ):
        future_plan, future_value = invoke(
            home, *target_args(operation, *params), env_extra=probe_env, path=fakebin
        )
        assert future_plan.returncode == 0, (future_plan, future_value)
        assert future_value["data"]["runtime_policy"]["release_feed_required"] is False
    write_probe(probe, observation())

    # Candidate drift is explicit: a recreated container changes both live-state and final hashes.
    write_probe(probe, observation(container="cid-new"))
    drifted, drifted_value = invoke(
        home,
        *target_args("runtime/restart", "--param", "machine=bp1"),
        env_extra=probe_env,
    )
    assert drifted.returncode == 0
    assert drifted_value["data"]["candidate_hash"] != data["candidate_hash"]
    write_probe(probe, observation())

    # Role/network and closed-grammar mismatches stop before a plan can be approved.
    for mutation, needle in (
        (("--role", "relay"), "relay"),
        (("--network", "preprod"), "pool binding mismatch"),
        (("--shell", "id"), "unexpected"),
    ):
        args = list(target_args("runtime/restart", "--param", "machine=bp1"))
        if mutation[0] in args:
            args[args.index(mutation[0]) + 1] = mutation[1]
        else:
            args.extend(mutation)
        refused, refused_value = invoke(home, *args, env_extra=probe_env)
        assert refused.returncode != 0 and needle in json.dumps(refused_value), refused_value

    # Control derives the remote binding from the operator spec; only the runner and closed target
    # plan argv cross SSH. The fake target returns the already-proven typed plan.
    pool_spec = home / "pool-spec.yaml"
    pool_spec.write_text(
        f"""spec_version: 1
pool:
  network: mainnet
  network_magic: 764824073
  genesis_hashes:
    shelley: {GENESIS}
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh:
      host: 192.0.2.1
      port: 22
      user: cardano
      key_ref: creds://bp1
  - id: relay1
    role: relay
    public_endpoint:
      host: 192.0.2.2
      port: 3001
    ssh:
      host: 192.0.2.2
      port: 22
      user: cardano
      key_ref: creds://relay1
upgrade:
  min_online_relays: 1
"""
    )
    credentials = home / "credentials"
    credentials.mkdir()
    (credentials / "bp1").write_text("test-key")
    (home / "known_hosts").write_text("192.0.2.1 ssh-ed25519 test\n")
    runner = home / "runner"
    runner_bytes = b"linux-runner-for-stateless-plan"
    runner.write_bytes(runner_bytes)
    transport_log = home / "transport.log"
    ssh = fakebin / "ssh"
    ssh.write_text(
        "#!/usr/bin/env bash\n"
        "set -eu\n"
        "dd of=/dev/null bs=65536 status=none\n"
        "printf '%s' \"$*\" >\"$OURO_TEST_PLAN_LOG\"\n"
        f"printf '%s\\n' '{json.dumps(first_value, separators=(',', ':'))}'\n"
    )
    ssh.chmod(0o700)
    dispatched, dispatched_value = invoke(
        home,
        "op",
        "run",
        "--op",
        "runtime/restart",
        "--node",
        "bp1",
        "--param",
        "machine=bp1",
        "--dispatch",
        "192.0.2.1",
        "--ssh-key",
        "creds://bp1",
        "--spec",
        str(pool_spec),
        "--plan",
        env_extra={
            "OURO_EPHEMERAL_RUNNER": str(runner),
            "OURO_TEST_PLAN_LOG": str(transport_log),
        },
        path=fakebin,
    )
    assert dispatched.returncode == 0 and dispatched_value["tool"] == "ouro.op.plan"
    remote = transport_log.read_text()
    for expected in [
        "cardano@192.0.2.1",
        "mktemp -d /tmp/ouro-run.XXXXXXXXXX",
        "'target' 'plan'",
        "'--role' 'bp'",
        "'--network' 'mainnet'",
        f"'--genesis' '{GENESIS}'",
        "'--param' 'machine=bp1'",
    ]:
        assert expected in remote, (expected, remote)
    assert str(pool_spec) not in remote and "/usr/local/bin/ouro-ops" not in remote
    assert hashlib.sha256(runner_bytes).hexdigest() in remote

    wrong_host, wrong_value = invoke(
        home,
        "op",
        "run",
        "--op",
        "runtime/restart",
        "--node",
        "bp1",
        "--param",
        "machine=bp1",
        "--dispatch",
        "192.0.2.99",
        "--spec",
        str(pool_spec),
        "--plan",
        env_extra={"OURO_EPHEMERAL_RUNNER": str(runner)},
        path=fakebin,
    )
    assert wrong_host.returncode != 0 and "does not match pool-spec host" in json.dumps(wrong_value)

    print("S0020 stateless target plans passed")


if __name__ == "__main__":
    main()
