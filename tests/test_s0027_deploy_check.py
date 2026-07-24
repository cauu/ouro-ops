#!/usr/bin/env python3
"""S0027 TC-12/TC-13: one read-only Check separates static failure from startup pending."""

import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "ouro-ops"


def inspect_facts(**overrides: object) -> str:
    values: dict[str, object] = {
        "schema": "ouro-deploy-inspect-v1",
        "os_id": "ubuntu",
        "os_version": "22.04",
        "arch": "x86_64",
        "memory_bytes": 34359738368,
        "free_disk_bytes": 1099511627776,
        "privilege": "sudo_n",
        "docker_mode": "user",
        "compose_v2": "true",
        "chrony_installed": "true",
        "chrony_synced": "true",
        "chrony_offset": "0.001",
        "ufw_state": "active",
        "ssh_listener": "true",
        "aggregator_reachable": "false",
        "p2p_listener": "false",
        "metrics_listener": "false",
        "metrics_public_listener": "false",
        "ufw_p2p_allow": "false",
        "ufw_ssh_allow": "true",
        "ufw_metrics_allow": "false",
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


def check_facts(machine: str, artifacts: dict, *, role: str, **overrides: object) -> str:
    values: dict[str, object] = {
        "schema": "ouro-deploy-check-v1",
        "compose_file_sha256": artifacts["compose_sha256"],
        "topology_sha256": artifacts["topology_sha256"],
        "owned_count": 1,
        "container_running": "true",
        "container_status": "running",
        "restart_count": 0,
        "restart_policy": "unless-stopped",
        "privileged": "false",
        "pid_mode": "",
        "devices": "null",
        "cap_add": "null",
        "compose_project": f"ouro-{machine}",
        "compose_service": "cardano-node",
        "image_platform": "linux/amd64",
        "mount_db": "/opt/ouro/db|true",
        "mount_ipc": "/opt/ouro/ipc|true",
        "mount_topology": "/opt/ouro/topology.json|false",
        "mount_keys": "/opt/ouro/keys|true" if role == "bp" else "",
        "metrics_bindings": '[{"HostIp":"127.0.0.1","HostPort":"12798"}]',
        "p2p_bindings": (
            '[{"HostIp":"0.0.0.0","HostPort":"3001"}]' if role == "relay" else "null"
        ),
        "log_type": "json-file",
        "log_max_size": "50m",
        "log_max_file": "3",
        "env_network": "mainnet",
        "env_topology": "/ouro/topology.json",
        "env_database": "/data/db",
        "env_socket": "/ipc/node.socket",
        "env_block_producer": "false",
        "env_restore_snapshot": "true",
        "socket_ready": "true",
        "tip_ready": "true",
        "host_metrics_ready": "true",
        "container_metrics_ready": "true",
        "p2p_listening": "true",
        "established_peers": 1,
        "fatal_log_evidence": "false",
        "cold_key_artifact": "false",
    }
    values.update(overrides)
    return "".join(f"{key}={value}\n" for key, value in values.items())


def run(command: str, spec: Path, env: dict[str, str]):
    result = subprocess.run(
        [str(BIN), "deploy", command, "--spec", str(spec)],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
    )
    assert result.stdout, result.stderr
    return result, json.loads(result.stdout)


def main() -> None:
    subprocess.run(["cargo", "build", "-q", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0027-check-"))
    credentials = home / "credentials"
    credentials.mkdir()
    for machine in ("bp1", "relay1"):
        key = credentials / machine
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
    public_endpoint: {host: relay.example.com, port: 3001}
    ssh: {host: 192.0.2.2, port: 2222, user: relay-admin, key_ref: creds://relay1}
"""
    )
    host_key = home / "host-key"
    subprocess.run(
        ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(host_key)],
        check=True,
    )
    public = host_key.with_suffix(".pub").read_text().split()[1]
    (home / "known_hosts").write_text(
        f"192.0.2.1 ssh-ed25519 {public}\n"
        f"[192.0.2.2]:2222 ssh-ed25519 {public}\n"
    )
    facts_dir = home / "facts"
    facts_dir.mkdir()
    for machine in ("bp1", "relay1"):
        (facts_dir / f"{machine}-inspect.facts").write_text(inspect_facts())
        (facts_dir / f"{machine}-check.facts").write_text("")
    events = home / "events.log"
    fakebin = home / "fakebin"
    fakebin.mkdir()
    fake_ssh = fakebin / "ssh"
    fake_ssh.write_text(
        "#!/bin/sh\n"
        "target=relay1\n"
        "case \"$*\" in *bp-admin@192.0.2.1*) target=bp1 ;; esac\n"
        "case \"$*\" in\n"
        "  *ouro-deploy-check-v1*) event=check ;;\n"
        "  *ouro-deploy-inspect-v1*) event=inspect ;;\n"
        "  *) exit 80 ;;\n"
        "esac\n"
        "printf '%s|%s\\n' \"$target\" \"$event\" >> \"$OURO_TEST_EVENTS\"\n"
        "cat \"$OURO_TEST_FACTS/$target-$event.facts\"\n"
    )
    fake_ssh.chmod(0o700)
    env = dict(
        os.environ,
        OURO_HOME=str(home),
        OURO_RELEASES_FILE=str(ROOT / "data" / "releases.json"),
        OURO_JSON="1",
        OURO_TEST_EVENTS=str(events),
        OURO_TEST_FACTS=str(facts_dir),
        PATH=f"{fakebin}:{os.environ['PATH']}",
    )

    _, baseline = run("inspect", spec, env)
    baseline_data = baseline["data"]
    baseline_nodes = {node["machine"]: node for node in baseline_data["nodes"]}
    for machine, role, lifecycle in (
        ("bp1", "bp", "bootstrap"),
        ("relay1", "relay", "operational"),
    ):
        node = baseline_nodes[machine]
        selection = node["selection"]
        marker = json.dumps(
            {
                "schema_version": 1,
                "fleet_identity_digest": baseline_data["fleet_identity_digest"],
                "machine_id": machine,
                "role": role,
                "network": "mainnet",
                "genesis_identity": baseline_data["genesis_identity"],
                "repository": selection["repository"],
                "platform": selection["platform"],
                "platform_manifest_digest": selection["platform_manifest_digest"],
                "image_config_digest": selection["image_config_digest"],
            },
            separators=(",", ":"),
        )
        (facts_dir / f"{machine}-inspect.facts").write_text(
            inspect_facts(
                p2p_listener="true" if role == "relay" else "false",
                metrics_listener="true",
                ufw_p2p_allow="true" if role == "relay" else "false",
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
        (facts_dir / f"{machine}-check.facts").write_text(
            check_facts(machine, node["expected_artifacts"], role=role)
        )

    events.write_text("")
    ready_result, ready = run("check", spec, env)
    assert ready_result.returncode == 0
    assert ready["data"]["classification"] == "ready"
    assert ready["data"]["target_writes"] is False
    ready_nodes = {node["machine"]: node for node in ready["data"]["nodes"]}
    assert ready_nodes["bp1"]["lifecycle"] == "bootstrap"
    assert ready_nodes["bp1"]["forging_readiness"] == "not_applicable"
    assert ready_nodes["bp1"]["block_production"] == "disabled"
    assert events.read_text().splitlines() == [
        "bp1|inspect",
        "bp1|check",
        "relay1|inspect",
        "relay1|check",
    ]

    bp_complete = (facts_dir / "bp1-inspect.facts").read_text()
    (facts_dir / "bp1-inspect.facts").write_text(
        bp_complete.replace("owned_lifecycle=bootstrap", "owned_lifecycle=operational")
    )
    _, operational = run("check", spec, env)
    operational_bp = {node["machine"]: node for node in operational["data"]["nodes"]}[
        "bp1"
    ]
    assert operational_bp["status"] == "failed"
    assert operational_bp["forging_readiness"] == "failed"

    (facts_dir / "bp1-inspect.facts").write_text(
        bp_complete.replace("owned_lifecycle=bootstrap", "owned_lifecycle=")
    )
    _, unlabeled = run("check", spec, env)
    unlabeled_bp = {node["machine"]: node for node in unlabeled["data"]["nodes"]}[
        "bp1"
    ]
    assert unlabeled_bp["status"] == "failed"
    assert unlabeled_bp["forging_readiness"] == "failed"
    (facts_dir / "bp1-inspect.facts").write_text(bp_complete)

    (facts_dir / "bp1-check.facts").write_text(
        check_facts(
            "bp1",
            baseline_nodes["bp1"]["expected_artifacts"],
            role="bp",
            cold_key_artifact="true",
        )
    )
    _, cold_key = run("check", spec, env)
    cold_key_bp = {node["machine"]: node for node in cold_key["data"]["nodes"]}[
        "bp1"
    ]
    assert cold_key_bp["status"] == "failed"
    assert "cold_key_artifact_present" in cold_key_bp["static_failures"]

    (facts_dir / "bp1-check.facts").write_text(
        check_facts(
            "bp1",
            baseline_nodes["bp1"]["expected_artifacts"],
            role="bp",
            metrics_bindings='[{"HostIp":"0.0.0.0","HostPort":"12798"}]',
            p2p_bindings='[{"HostIp":"0.0.0.0","HostPort":"3001"}]',
        )
    )
    _, public_ports = run("check", spec, env)
    public_bp = {node["machine"]: node for node in public_ports["data"]["nodes"]}[
        "bp1"
    ]
    assert public_bp["status"] == "failed"
    assert "port_binding_mismatch" in public_bp["static_failures"]

    (facts_dir / "bp1-check.facts").write_text(
        check_facts("bp1", baseline_nodes["bp1"]["expected_artifacts"], role="bp")
    )
    (facts_dir / "relay1-check.facts").write_text(
        check_facts(
            "relay1",
            baseline_nodes["relay1"]["expected_artifacts"],
            role="relay",
            socket_ready="false",
            tip_ready="false",
            host_metrics_ready="false",
            container_metrics_ready="false",
            established_peers=0,
        )
    )
    _, pending = run("check", spec, env)
    assert pending["data"]["classification"] == "pending"
    pending_relay = {node["machine"]: node for node in pending["data"]["nodes"]}[
        "relay1"
    ]
    assert pending_relay["static_failures"] == []
    assert pending_relay["node_readiness"] == "pending"

    (facts_dir / "relay1-check.facts").write_text(
        check_facts(
            "relay1",
            baseline_nodes["relay1"]["expected_artifacts"],
            role="relay",
            compose_file_sha256="0" * 64,
        )
    )
    _, drifted = run("check", spec, env)
    drifted_relay = {node["machine"]: node for node in drifted["data"]["nodes"]}[
        "relay1"
    ]
    assert drifted_relay["status"] == "failed"
    assert "compose_file_mismatch" in drifted_relay["static_failures"]

    (facts_dir / "relay1-check.facts").write_text(
        check_facts(
            "relay1",
            baseline_nodes["relay1"]["expected_artifacts"],
            role="relay",
            container_running="false",
            container_status="exited",
        )
    )
    _, stopped = run("check", spec, env)
    stopped_relay = {node["machine"]: node for node in stopped["data"]["nodes"]}[
        "relay1"
    ]
    assert stopped_relay["status"] == "failed"
    assert stopped_relay["runtime"]["container_status"] == "exited"
    print("S0027 unified read-only Fleet Check passed")


if __name__ == "__main__":
    main()
