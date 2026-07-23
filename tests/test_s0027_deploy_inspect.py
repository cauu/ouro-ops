#!/usr/bin/env python3
"""S0027 TC-6/TC-7: Fleet Inspect is read-only, closed-schema and identity-aware."""

import json
import os
import stat
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "ouro-ops"
RELEASES = ROOT / "data" / "releases.json"


def facts(**overrides: object) -> str:
    values: dict[str, object] = {
        "schema": "ouro-deploy-inspect-v1",
        "os_id": "ubuntu",
        "os_version": "22.04",
        "arch": "x86_64",
        "memory_bytes": 34359738368,
        "free_disk_bytes": 1099511627776,
        "privilege": "sudo_n",
        "docker_mode": "missing",
        "compose_v2": "false",
        "chrony_installed": "false",
        "chrony_synced": "false",
        "chrony_offset": "",
        "ufw_state": "inactive",
        "ssh_listener": "true",
        "aggregator_reachable": "true",
        "ouro_state": "absent",
        "legacy_cardano_state": "absent",
        "legacy_home_config_state": "absent",
        "db_state": "absent",
        "keys_state": "absent",
        "keys_mode": "",
        "identity_marker": "",
        "node_container_count": 0,
        "owned_count": 0,
        "owned_running": "false",
        "owned_image": "",
        "owned_role": "",
        "owned_lifecycle": "",
        "owned_network": "",
        "owned_desired_digest": "",
    }
    values.update(overrides)
    return "".join(f"{key}={value}\n" for key, value in values.items())


def run_inspect(spec: Path, env: dict[str, str]) -> tuple[subprocess.CompletedProcess[str], dict]:
    result = subprocess.run(
        [str(BIN), "deploy", "inspect", "--spec", str(spec)],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
    )
    assert result.stdout, result.stderr
    return result, json.loads(result.stdout)


def main() -> None:
    subprocess.run(["cargo", "build", "-q", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0027-inspect-"))
    credentials = home / "credentials"
    credentials.mkdir()
    for name in ("bp1", "relay1"):
        key = credentials / name
        key.write_text("test-only-private-key-placeholder")
        key.chmod(0o600)

    spec = home / "pool-spec.yaml"
    spec.write_text(
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
    ssh: {host: 192.0.2.1, port: 22, user: bp-admin, key_ref: creds://bp1}
  - id: relay1
    role: relay
    public_endpoint: {host: 192.0.2.2, port: 3001}
    ssh: {host: 192.0.2.2, port: 2222, user: relay-ops, key_ref: creds://relay1}
upgrade: {min_online_relays: 1}
sync: {mode: genesis, mithril: null}
node_version: 99.0.0
"""
    )

    host_key = home / "host-key"
    subprocess.run(
        ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(host_key)],
        check=True,
    )
    public_key = host_key.with_suffix(".pub").read_text().split()[1]
    known_hosts = home / "known_hosts"
    known_hosts.write_text(
        f"192.0.2.1 ssh-ed25519 {public_key}\n"
        f"[192.0.2.2]:2222 ssh-ed25519 {public_key}\n"
    )
    known_hosts.chmod(0o600)

    fact_dir = home / "facts"
    fact_dir.mkdir()
    (fact_dir / "clean-bp.facts").write_text(facts())
    (fact_dir / "clean-relay.facts").write_text(facts())
    (fact_dir / "blocked-bp.facts").write_text(facts())
    (fact_dir / "blocked-relay.facts").write_text(
        facts(
            os_id="debian",
            free_disk_bytes=1024,
            legacy_cardano_state="nonempty_dir",
            node_container_count=1,
        )
    )
    (fact_dir / "injected-bp.facts").write_text(facts())
    (fact_dir / "injected-relay.facts").write_text(facts() + "run_this=sudo rm -rf /opt\n")

    fakebin = home / "fakebin"
    fakebin.mkdir()
    ssh_log = home / "ssh.log"
    fake_ssh = fakebin / "ssh"
    fake_ssh.write_text(
        "#!/bin/sh\n"
        f"printf '%s\\n' \"$*\" >> '{ssh_log}'\n"
        "role=relay\n"
        "case \"$*\" in *bp-admin@192.0.2.1*) role=bp ;; esac\n"
        f"cat '{fact_dir}/'\"${{OURO_TEST_MODE:-clean}}\"-\"$role\".facts\n"
    )
    fake_ssh.chmod(0o700)
    env = dict(
        os.environ,
        OURO_HOME=str(home),
        OURO_RELEASES_FILE=str(RELEASES),
        OURO_JSON="1",
        PATH=f"{fakebin}:{os.environ['PATH']}",
    )

    clean_result, clean = run_inspect(spec, env)
    assert clean_result.returncode == 0, clean_result.stderr
    assert clean["changed"] is False
    assert clean["data"]["classification"] == "applicable"
    assert clean["data"]["target_writes"] is False
    nodes = {node["machine"]: node for node in clean["data"]["nodes"]}
    assert nodes["bp1"]["ssh"]["user"] == "bp-admin"
    assert nodes["relay1"]["ssh"]["user"] == "relay-ops"
    assert nodes["bp1"]["change_set"][0] == "install_docker_engine"
    assert nodes["bp1"]["mithril"]["restore_expected"] is True
    assert nodes["bp1"]["selection"]["release"] == "11.0.1-1"
    irrelevant = home / "pool-spec-irrelevant-fields.yaml"
    irrelevant.write_text(spec.read_text().replace("node_version: 99.0.0", "node_version: 1.2.3"))
    irrelevant_result, irrelevant_value = run_inspect(irrelevant, env)
    assert irrelevant_result.returncode == 0
    assert irrelevant_value["data"]["fleet_identity_digest"] == clean["data"]["fleet_identity_digest"]
    irrelevant_nodes = {
        node["machine"]: node for node in irrelevant_value["data"]["nodes"]
    }
    assert irrelevant_nodes["bp1"]["desired_digest"] == nodes["bp1"]["desired_digest"]
    argv = ssh_log.read_text()
    assert "bp-admin@192.0.2.1" in argv
    assert "relay-ops@192.0.2.2" in argv
    assert "StrictHostKeyChecking=yes" in argv
    assert "ouro-deploy-inspect" in argv
    assert "'\\''22'\\'' '\\''bp1'\\''" in argv
    assert "'\\''2222'\\'' '\\''relay1'\\''" in argv

    blocked_result, blocked = run_inspect(spec, dict(env, OURO_TEST_MODE="blocked"))
    assert blocked_result.returncode == 0
    assert blocked["data"]["classification"] == "blocked"
    relay_reasons = {
        node["machine"]: node["reasons"] for node in blocked["data"]["nodes"]
    }["relay1"]
    assert {
        "unsupported_ubuntu_release",
        "insufficient_free_disk",
        "legacy_or_unknown_deployment_present",
        "unowned_cardano_node_container",
    }.issubset(relay_reasons)

    injected_result, injected = run_inspect(spec, dict(env, OURO_TEST_MODE="injected"))
    assert injected_result.returncode == 0
    assert injected["data"]["classification"] == "blocked"
    injected_relay = {
        node["machine"]: node for node in injected["data"]["nodes"]
    }["relay1"]
    assert injected_relay["reasons"] == ["inspect_failed"]
    assert "unknown fact run_this" in injected_relay["detail"]

    fleet_digest = clean["data"]["fleet_identity_digest"]
    genesis_identity = clean["data"]["genesis_identity"]
    for machine, role, lifecycle in (
        ("bp1", "bp", "bootstrap"),
        ("relay1", "relay", "operational"),
    ):
        node = nodes[machine]
        selection = node["selection"]
        marker = json.dumps(
            {
                "schema_version": 1,
                "fleet_identity_digest": fleet_digest,
                "machine_id": machine,
                "role": role,
                "network": "mainnet",
                "genesis_identity": genesis_identity,
                "repository": selection["repository"],
                "platform": selection["platform"],
                "platform_manifest_digest": selection["platform_manifest_digest"],
                "image_config_digest": selection["image_config_digest"],
            },
            separators=(",", ":"),
        )
        (fact_dir / f"complete-{role if role == 'bp' else 'relay'}.facts").write_text(
            facts(
                docker_mode="user",
                compose_v2="true",
                chrony_installed="true",
                chrony_synced="true",
                chrony_offset="0.001",
                ufw_state="active",
                aggregator_reachable="false",
                ouro_state="nonempty_dir",
                db_state="populated",
                keys_state="empty_dir" if role == "bp" else "absent",
                keys_mode="700" if role == "bp" else "",
                identity_marker=marker,
                node_container_count=1,
                owned_count=1,
                owned_running="true",
                owned_image=selection["image_config_digest"],
                owned_role=role,
                owned_lifecycle=lifecycle,
                owned_network="mainnet",
                owned_desired_digest=node["desired_digest"],
            )
        )
    complete_result, complete = run_inspect(spec, dict(env, OURO_TEST_MODE="complete"))
    assert complete_result.returncode == 0, complete_result.stderr
    assert complete["data"]["classification"] == "already_deployed"
    assert all(node["deployment_complete"] for node in complete["data"]["nodes"])

    assert stat.S_IMODE(known_hosts.stat().st_mode) == 0o600
    assert not any(path.name.startswith("fleet-identity") for path in home.iterdir())
    print("S0027 read-only Fleet Inspect passed")


if __name__ == "__main__":
    main()
