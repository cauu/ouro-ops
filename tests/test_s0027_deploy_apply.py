#!/usr/bin/env python3
"""S0027 TC-8..TC-11: fixed Fleet Apply shape and Relay→BP command ordering."""

import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "ouro-ops"
CONFIG_DIGEST = "sha256:0bb21e45159327c4e6109704df256c3c297c725a4b2cdf6d0e1899e3a9df468f"


def inspect_facts(**overrides: object) -> str:
    values = {
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
        "p2p_listener": "false",
        "metrics_listener": "false",
        "metrics_public_listener": "false",
        "ufw_p2p_allow": "false",
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


def apply(spec: Path, env: dict[str, str], mode: str):
    events = Path(env["OURO_TEST_EVENTS"])
    events.write_text("")
    result = subprocess.run(
        [str(BIN), "deploy", "apply", "--spec", str(spec)],
        cwd=ROOT,
        env=dict(env, OURO_TEST_MODE=mode),
        text=True,
        capture_output=True,
    )
    assert result.stdout, result.stderr
    return result, json.loads(result.stdout), events.read_text().splitlines()


def main() -> None:
    subprocess.run(["cargo", "build", "-q", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0027-apply-"))
    credentials = home / "credentials"
    credentials.mkdir()
    for name in ("bp1", "relay1", "relay2"):
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
    public_endpoint: {host: relay1.example.com, port: 3001}
    ssh: {host: 192.0.2.2, port: 2222, user: relay-one, key_ref: creds://relay1}
  - id: relay2
    role: relay
    public_endpoint: {host: relay2.example.com, port: 3002}
    ssh: {host: 192.0.2.3, port: 2200, user: relay-two, key_ref: creds://relay2}
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
        f"[192.0.2.3]:2200 ssh-ed25519 {public}\n"
    )
    fact_dir = home / "inspect-facts"
    fact_dir.mkdir()
    for machine in ("bp1", "relay1", "relay2"):
        (fact_dir / f"{machine}.facts").write_text(inspect_facts())
    events = home / "events.log"
    argv_dir = home / "argv"
    argv_dir.mkdir()
    fakebin = home / "fakebin"
    fakebin.mkdir()
    fake_ssh = fakebin / "ssh"
    fake_ssh.write_text(
        "#!/bin/sh\n"
        "target=relay2\n"
        "case \"$*\" in *bp-admin@192.0.2.1*) target=bp1 ;; "
        "*relay-one@192.0.2.2*) target=relay1 ;; esac\n"
        "event=fresh_ssh\n"
        "case \"$*\" in\n"
        "  *ouro-deploy-inspect-v1*) event=inspect ;;\n"
        "  *ouro-deploy-host-prepare-v1*) event=host_prepare ;;\n"
        "  *ouro-deploy-ufw-rollback-v1*) event=ufw_rollback ;;\n"
        "  *ouro-deploy-ufw-v1*) event=ufw_apply ;;\n"
        "  *ouro-deploy-artifacts-v1*) event=artifacts ;;\n"
        "  *ouro-deploy-compose-up-v1*) event=compose_up ;;\n"
        "esac\n"
        "printf '%s|%s\\n' \"$target\" \"$event\" >> \"$OURO_TEST_EVENTS\"\n"
        "printf '%s\\n' \"$*\" >> \"$OURO_TEST_ARGV_DIR/$target.log\"\n"
        "case \"$event\" in\n"
        "  inspect) cat \"$OURO_TEST_INSPECT_DIR/$target.facts\" ;;\n"
        "  host_prepare)\n"
        "    if test \"$OURO_TEST_MODE\" = chrony_fail && test \"$target\" = relay1; then exit 70; fi\n"
        "    printf '%s\\n' 'schema=ouro-deploy-host-prepare-v1' "
        "'packages_changed=true' 'docker_mode=sudo_n' 'chrony_synced=true' "
        "'chrony_offset=0.001' 'marker_installed=true' ;;\n"
        "  ufw_apply) printf '%s\\n' 'schema=ouro-deploy-ufw-v1' "
        "'added_ssh=true' \"added_p2p=$(test \"$target\" = bp1 && printf false || printf true)\" "
        "'enabled=true' ;;\n"
        "  fresh_ssh)\n"
        "    if test \"$OURO_TEST_MODE\" = fresh_ssh_fail && test \"$target\" = relay1; then exit 71; fi ;;\n"
        "  ufw_rollback) printf '%s\\n' 'schema=ouro-deploy-ufw-rollback-v1' 'restored=true' ;;\n"
        f"  artifacts) printf '%s\\n' 'schema=ouro-deploy-artifacts-v1' "
        f"'compose_valid=true' 'topology_installed=true' 'image_config={CONFIG_DIGEST}' ;;\n"
        "  compose_up)\n"
        "    if test \"$OURO_TEST_MODE\" = relay1_fail && test \"$target\" = relay1; then exit 72; fi\n"
        "    if test \"$OURO_TEST_MODE\" = all_relays_fail && test \"$target\" != bp1; then exit 73; fi\n"
        "    printf '%s\\n' 'schema=ouro-deploy-compose-up-v1' 'started=true' ;;\n"
        "esac\n"
    )
    fake_ssh.chmod(0o700)
    env = dict(
        os.environ,
        OURO_HOME=str(home),
        OURO_RELEASES_FILE=str(ROOT / "data" / "releases.json"),
        OURO_JSON="1",
        OURO_TEST_EVENTS=str(events),
        OURO_TEST_ARGV_DIR=str(argv_dir),
        OURO_TEST_INSPECT_DIR=str(fact_dir),
        PATH=f"{fakebin}:{os.environ['PATH']}",
    )

    success_result, success, success_events = apply(spec, env, "success")
    assert success_result.returncode == 0, success_result.stderr
    assert success["data"]["classification"] == "command_success"
    first_up = success_events.index("relay1|compose_up")
    artifact_positions = [
        index for index, event in enumerate(success_events) if event.endswith("|artifacts")
    ]
    assert len(artifact_positions) == 3 and max(artifact_positions) < first_up
    assert success_events[-3:] == [
        "relay1|compose_up",
        "relay2|compose_up",
        "bp1|compose_up",
    ]
    assert not any(
        token in "\n".join(success_events)
        for token in ("socket", "query", "metrics", "health", "sleep", "poll")
    )

    bp_argv = (argv_dir / "bp1.log").read_text()
    relay_argv = (argv_dir / "relay1.log").read_text()
    for required in (
        "ghcr.io/blinklabs-io/cardano-node@sha256:",
        "io.ouro.desired-digest",
        "CARDANO_BLOCK_PRODUCER",
        "max-size",
        "50m",
        "127.0.0.1",
        "/opt/ouro/db",
        "/opt/ouro/ipc",
        "/opt/ouro/topology.json",
        "unless-stopped",
        "--pull never",
    ):
        assert required in bp_argv + relay_argv, required
    assert "/opt/cardano/config/keys" in bp_argv
    assert "/opt/cardano/config/keys" not in relay_argv
    assert "/opt/cardano/config:" not in bp_argv + relay_argv
    assert "lifecycle: bootstrap" in bp_argv
    assert "lifecycle: operational" in relay_argv
    assert "relay1.example.com" in bp_argv and "relay2.example.com" in bp_argv
    assert "backbone.mainnet.emurgornd.com" in relay_argv

    relay_result, relay_value, relay_events = apply(spec, env, "relay1_fail")
    assert relay_result.returncode != 0
    assert relay_value["error"]["code"] == "deploy_apply_partial_failure"
    assert relay_events[-3:] == [
        "relay1|compose_up",
        "relay2|compose_up",
        "bp1|compose_up",
    ]

    all_failed_result, all_failed, all_failed_events = apply(
        spec, env, "all_relays_fail"
    )
    assert all_failed_result.returncode != 0
    assert "bp1|compose_up" not in all_failed_events
    bp_state = {
        node["machine"]: node for node in all_failed["data"]["nodes"]
    }["bp1"]
    assert bp_state["compose_up"] == "skipped_no_relay_command_succeeded"

    fresh_result, fresh_value, fresh_events = apply(spec, env, "fresh_ssh_fail")
    assert fresh_result.returncode != 0
    assert fresh_value["error"]["code"] == "deploy_apply_partial_failure"
    assert "relay1|ufw_rollback" in fresh_events
    assert "relay2|compose_up" in fresh_events and "bp1|compose_up" in fresh_events

    chrony_result, _, chrony_events = apply(spec, env, "chrony_fail")
    assert chrony_result.returncode != 0
    assert "relay1|ufw_apply" not in chrony_events
    assert "relay2|compose_up" in chrony_events and "bp1|compose_up" in chrony_events

    inspected_result = subprocess.run(
        [str(BIN), "deploy", "inspect", "--spec", str(spec)],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=True,
    )
    inspected = json.loads(inspected_result.stdout)["data"]
    inspected_nodes = {node["machine"]: node for node in inspected["nodes"]}
    roles = {"bp1": ("bp", "bootstrap"), "relay1": ("relay", "operational"), "relay2": ("relay", "operational")}
    for machine, (role, lifecycle) in roles.items():
        node = inspected_nodes[machine]
        selection = node["selection"]
        marker = json.dumps(
            {
                "schema_version": 1,
                "fleet_identity_digest": inspected["fleet_identity_digest"],
                "machine_id": machine,
                "role": role,
                "network": "mainnet",
                "genesis_identity": inspected["genesis_identity"],
                "repository": selection["repository"],
                "platform": selection["platform"],
                "platform_manifest_digest": selection["platform_manifest_digest"],
                "image_config_digest": selection["image_config_digest"],
            },
            separators=(",", ":"),
        )
        (fact_dir / f"{machine}.facts").write_text(
            inspect_facts(
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
    already_result, already, already_events = apply(spec, env, "success")
    assert already_result.returncode != 0
    assert already["error"]["code"] == "already_deployed"
    assert already["data"]["target_writes"] is False
    assert already_events == ["bp1|inspect", "relay1|inspect", "relay2|inspect"]
    print("S0027 fixed Compose Fleet Apply passed")


if __name__ == "__main__":
    main()
