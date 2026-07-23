#!/usr/bin/env python3
"""S0019 p5-2 — the target-side observation probe emits a well-formed observation JSON. Uses docker
stubs (no real container needed); real gathering is bed-level (p5-6)."""
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / "ouro-skills/lib/ouro-probe.sh"


def main():
    tmp = tempfile.mkdtemp()
    binp = Path(tmp) / "bin"
    binp.mkdir()
    data_mount = Path(tmp) / "data"
    config_mount = Path(tmp) / "config"
    ipc_mount = Path(tmp) / "ipc"
    data_mount.mkdir()
    config_mount.mkdir()
    ipc_mount.mkdir()
    mainnet = config_mount / "mainnet"
    config_keys = config_mount / "keys"
    mainnet.mkdir()
    config_keys.mkdir()
    (mainnet / "topology.json").write_text('{"Producers":[]}')
    (mainnet / "config.json").write_text('{"Protocol":"Cardano"}')
    (mainnet / "shelley-genesis.json").write_text('{"slotsPerKESPeriod":2}')
    (config_keys / "node.cert").write_text('{"type":"NodeOperationalCertificate"}')
    (config_keys / "kes.skey").write_text("metadata-only fixture")
    (config_keys / "vrf.skey").write_text("metadata-only fixture")
    os.chmod(config_keys, 0o755)
    os.chmod(config_keys / "kes.skey", 0o600)
    os.chmod(config_keys / "vrf.skey", 0o600)
    # docker stub: ps → the production Blink Labs image shape whose entrypoint command does not
    # contain `cardano-node`; inspect → fixed fields; exec → hashes/keys.
    (binp / "docker").write_text(
        '#!/usr/bin/env bash\n'
        'case "$1 $2" in\n'
        '  "ps --no-trunc")\n'
        '     if test -n "$OURO_TEST_NO_NODE"; then\n'
        '       echo "other-id|postgres:16-alpine|db|postgres";\n'
        '     else\n'
        '       echo "cid-xyz|ghcr.io/blinklabs-io/cardano-node:10.5.4-1|cardano-node|/usr/local/bin/entrypoint run";\n'
        '     fi;;\n'
        '  "inspect --format")\n'
        '     case "$3" in\n'
        '       "{{.Image}}") echo "sha256:cfg";;\n'
        '       "{{.Created}}") echo "2026-07-15T10:00:00.123456789Z";;\n'
        '       "{{json .Config.Entrypoint}}") echo "[\\"/usr/local/bin/entrypoint\\"]";;\n'
        '       "{{json .Args}}") echo "[\\"run\\"]";;\n'
        '       "{{range .Config.Env}}{{println .}}{{end}}") echo "CARDANO_NETWORK=mainnet";;\n'
        '       "{{range .Mounts}}{{.Source}};{{end}}") echo "/srv/cardano/data;/srv/cardano/config;";;\n'
        '       "{{.HostConfig.RestartPolicy.Name}}") echo "unless-stopped";;\n'
        '     esac;;\n'
        f'  "inspect cid-xyz") record=\'[{{"Id":"cid-xyz000000000000","Name":"/cardano-node","State":{{"Running":true,"Restarting":false,"Status":"running"}},"Mounts":[{{"Type":"bind","Source":"{data_mount}","Destination":"/data/db","RW":true,"Mode":"rw","Propagation":"rprivate"}},{{"Type":"bind","Source":"{config_mount}","Destination":"/opt/cardano/config","RW":false,"Mode":"ro","Propagation":"rprivate"}},{{"Type":"bind","Source":"{ipc_mount}","Destination":"/ipc","RW":true,"Mode":"rw","Propagation":"rprivate"}}],"HostConfig":{{"RestartPolicy":{{"Name":"unless-stopped","MaximumRetryCount":0}},"NetworkMode":"bridge","PortBindings":{{}}}},"NetworkSettings":{{"Networks":{{"bridge":{{"Aliases":null,"IPAMConfig":null}}}}}},"Config":{{"Hostname":"cid-xyz00000","Image":"ghcr.io/blinklabs-io/cardano-node:10.5.4-1","Env":["CARDANO_BLOCK_PRODUCER=true","CARDANO_NETWORK=mainnet"],"Labels":{{"org.opencontainers.image.title":"cardano-node"}},"Entrypoint":["/usr/local/bin/entrypoint"]}},"Path":"/usr/local/bin/entrypoint","Args":["run"]}}]\'; if test -n "$OURO_TEST_RESTARTING"; then printf "%s\\n" "$record" | sed \'s/"Running":true,"Restarting":false,"Status":"running"/"Running":false,"Restarting":true,"Status":"restarting"/\'; else printf "%s\\n" "$record"; fi;;\n'
        '  "image inspect") echo \'[{"Os":"linux","Architecture":"amd64","Config":{"Env":[],"Entrypoint":["/usr/local/bin/entrypoint"],"Cmd":[],"Labels":{"org.opencontainers.image.title":"cardano-node"}}}]\';;\n'
        '  "exec cid-xyz") shift 2; test -z "$OURO_TEST_RESTARTING" || exit 1; # sh -c ...\n'
        '     if echo "$*" | grep -q "cardano-cli query tip"; then echo "{\\"block\\":9,\\"slot\\":10,\\"era\\":\\"Conway\\",\\"syncProgress\\":\\"100.00\\"}";\n'
        '     elif echo "$*" | grep -q "hash genesis-file"; then echo "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81";\n'
        '     elif echo "$*" | grep -q "kes-period-info"; then if test -z "$OURO_TEST_KES_EMPTY"; then if test -n "$OURO_TEST_KES_NULL"; then node_state=null; else node_state=5; fi; printf "✓ period is valid\\n✓ counter agrees\\n{\\"qKesCurrentKesPeriod\\":10,\\"qKesStartKesInterval\\":9,\\"qKesEndKesInterval\\":20,\\"qKesOnDiskOperationalCertificateNumber\\":5,\\"qKesNodeStateOperationalCertificateNumber\\":%s}\\n" "$node_state"; fi;\n'
        '     elif echo "$*" | grep -q "cat /opt/cardano/config/mainnet/shelley-genesis.json"; then echo \'{"slotsPerKESPeriod":2}\';\n'
        '     elif echo "$*" | grep -q "ss -Htn state established"; then echo 2;\n'
        '     elif echo "$*" | grep -q netstat; then echo 0;\n'
        '     elif echo "$*" | grep -q "keys_directory_safe=%s"; then printf "keys_directory_safe=true\\nkes_skey_private=true\\nvrf_skey_private=true\\n";\n'
        '     elif echo "$*" | grep -q kes.skey; then echo true;\n'
        '     elif echo "$*" | grep -q node.cert; then echo "opcerthash  /x";\n'
        '     else echo "deadbeef  /x"; fi;;\n'
        'esac\n'
    )
    (binp / "docker").chmod(0o755)
    (binp / "curl").write_text(
        "#!/usr/bin/env bash\n"
        "printf '%s\\n' "
        "'cardano_node_metrics_operationalCertificateStartKESPeriod_int 3' "
        "'cardano_node_metrics_operationalCertificateExpiryKESPeriod_int 4' "
        "'cardano_node_metrics_currentKESPeriod_int 0'\n"
    )
    (binp / "curl").chmod(0o755)
    (binp / "stat").write_text(
        "#!/usr/bin/env python3\n"
        "import os,sys\n"
        "value=os.stat(sys.argv[3])\n"
        "print(format(value.st_mode & 0o777, 'o') if sys.argv[2]=='%a' else value.st_uid)\n"
    )
    (binp / "stat").chmod(0o755)

    env = dict(os.environ, PATH=f"{binp}:{os.environ['PATH']}", OURO_READINESS_SAMPLE_DELAY="0",
               OURO_HOST_KEY_SHA256="SHA256:" + "a" * 43)
    r = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=env, text=True, capture_output=True,
    )
    assert r.returncode == 0, r.stderr
    obs = json.loads(r.stdout)
    assert obs["supervisor"]["runtime"] == "docker"
    assert obs["supervisor"]["node_container_count"] == 1
    assert obs["supervisor"]["uses_bind_mounts"] is True
    assert obs["supervisor"]["orchestration"] == "run"
    assert obs["supervisor"]["orchestration_reason"] is None
    assert obs["supervisor"]["compose"] is None
    live = obs["live"]
    assert live["image_config_digest"] == "sha256:cfg"
    assert live["container_id"] == "cid-xyz"
    assert live["entrypoint"] == ["/usr/local/bin/entrypoint"] and live["args"] == ["run"]
    assert live["image_entrypoint"] == ["/usr/local/bin/entrypoint"] and live["image_cmd"] == []
    assert live["platform"] == "linux/amd64"
    assert live["container_name"] == "cardano-node"
    assert live["image_reference"] == "ghcr.io/blinklabs-io/cardano-node:10.5.4-1"
    assert live["container_running"] is True and live["container_restarting"] is False
    assert live["lifecycle"] is None
    assert len(live["mounts"]) == 3
    assert all(m["kind"] == "bind" and m["source_id"].count(":") == 1 for m in live["mounts"])
    assert {m["destination"] for m in live["mounts"]} == {"/data/db", "/opt/cardano/config", "/ipc"}
    assert live["mounts"][1]["read_only"] is True
    assert live["has_forging_keys"] is True
    assert live["genesis_hash"] == "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81"
    assert live["container_creation_epoch"] > 0
    # every key the Rust ObsLive/SupervisorObservation expects is present
    for k in ["image_config_digest", "platform", "container_id", "container_creation_epoch",
              "entrypoint", "args", "mounts", "topology_hash", "config_hash",
              "kes_opcert_id", "has_forging_keys", "forging_key_permissions_safe",
              "keys_directory_safe", "kes_skey_private", "vrf_skey_private",
              "host_key_sha256", "genesis_hash", "network"]:
        assert k in live, f"observation missing {k}"
    assert live["keys_directory_safe"] is True
    assert live["kes_skey_private"] is True and live["vrf_skey_private"] is True
    readiness = obs["readiness"]
    for k in ["node_running", "socket_answers", "tip_block", "tip_block_next",
              "tip_block_height", "tip_slot", "tip_era", "sync_progress", "tip_synced",
              "kes_opcert_valid", "kes", "block_producer_configured",
              "forging_credentials_ready", "established_peers"]:
        assert k in readiness, f"readiness missing {k}"
    assert readiness["tip_block"] == readiness["tip_block_next"] == 10
    assert readiness["tip_block_height"] == 9 and readiness["tip_slot"] == 10
    assert readiness["tip_era"] == "Conway" and readiness["sync_progress"] == "100.00"
    assert readiness["tip_synced"] is True, "healthy no-new-block sample must remain ready"
    assert readiness["kes_opcert_valid"] is True and readiness["forging_credentials_ready"] is True
    assert readiness["block_producer_configured"] is True
    assert readiness["kes"] == {
        "source": "cardano_cli", "current_period": 10, "start_period": 9, "end_period": 20,
        "remaining_periods": 10, "opcert_counter_on_disk": 5,
        "opcert_counter_node_state": 5, "counter_consistent": True,
        "counter_status": "present", "period_valid": True, "valid": True,
    }
    assert readiness["established_peers"] == 2, "ss-only Blink Labs image peers must be detected"
    assert obs["recreate"] is not None, "inherited nonempty OCI labels remain a valid baseline"
    assert obs["recreate"]["user"] == ""
    assert obs["recreate"]["group_add"] == []
    assert obs["recreate"]["labels"] == {"org.opencontainers.image.title": "cardano-node"}
    assert obs["recreate"]["log_driver"] == ""
    assert obs["recreate"]["log_options"] == {}
    assert [bind["destination"] for bind in obs["recreate"]["binds"]] == [
        "/data/db",
        "/ipc",
        "/opt/cardano/config",
    ]
    assert obs["recreate"]["env"] == [
        "CARDANO_BLOCK_PRODUCER=true",
        "CARDANO_NETWORK=mainnet",
    ]

    docker_stub = (binp / "docker").read_text()
    inherited_label = '"Labels":{"org.opencontainers.image.title":"cardano-node"}'
    compose_labels = (
        '"Labels":{"org.opencontainers.image.title":"cardano-node",'
        '"io.ouro.lifecycle":"operational",'
        '"com.docker.compose.project":"cardano",'
        '"com.docker.compose.service":"cardano-node",'
        '"com.docker.compose.project.working_dir":"/opt/cardano",'
        '"com.docker.compose.project.config_files":"/opt/cardano/compose.yaml",'
        '"com.docker.compose.config-hash":"cfg-hash"}'
    )
    (binp / "docker").write_text(docker_stub.replace(inherited_label, compose_labels, 1))
    compose_result = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=env, text=True, capture_output=True,
    )
    assert compose_result.returncode == 0, compose_result.stderr
    compose_value = json.loads(compose_result.stdout)
    compose_obs = compose_value["supervisor"]
    assert compose_obs["orchestration"] == "compose", compose_obs
    assert compose_obs["orchestration_reason"] is None, compose_obs
    assert compose_obs["compose"] == {
        "project": "cardano",
        "service": "cardano-node",
        "working_dir": "/opt/cardano",
        "config_files": ["/opt/cardano/compose.yaml"],
        "config_hash": "cfg-hash",
    }, compose_obs
    assert compose_value["live"]["lifecycle"] == "operational"

    portainer_labels = (
        '"Labels":{"org.opencontainers.image.title":"cardano-node",'
        '"io.portainer.accesscontrol.public":"true"}'
    )
    (binp / "docker").write_text(docker_stub.replace(inherited_label, portainer_labels, 1))
    unsupported_result = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=env, text=True, capture_output=True,
    )
    assert unsupported_result.returncode == 0, unsupported_result.stderr
    unsupported_obs = json.loads(unsupported_result.stdout)["supervisor"]
    assert unsupported_obs["orchestration"] == "unsupported", unsupported_obs
    assert unsupported_obs["orchestration_reason"] == "unsupported_orchestration:portainer"
    assert unsupported_obs["compose"] is None

    conflicting_labels = (
        '"Labels":{"com.docker.compose.project":"cardano",'
        '"io.portainer.accesscontrol.public":"true"}'
    )
    (binp / "docker").write_text(docker_stub.replace(inherited_label, conflicting_labels, 1))
    conflicting_result = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=env, text=True, capture_output=True,
    )
    assert conflicting_result.returncode == 0, conflicting_result.stderr
    conflicting_obs = json.loads(conflicting_result.stdout)["supervisor"]
    assert conflicting_obs["orchestration"] == "unsupported", conflicting_obs
    assert conflicting_obs["orchestration_reason"] == "conflicting_orchestration_labels:compose,portainer"
    (binp / "docker").write_text(docker_stub)

    # The direct-run recreate contract preserves the common json-file rotation pair observed on
    # production relay1. Any other driver, option or malformed value remains fail-closed.
    empty_port_bindings = '"PortBindings":{}'
    supported_logging = (
        '"PortBindings":{},"LogConfig":{"Type":"json-file",'
        '"Config":{"max-file":"3","max-size":"50m"}}'
    )
    (binp / "docker").write_text(docker_stub.replace(empty_port_bindings, supported_logging, 1))
    logging_result = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=env, text=True, capture_output=True,
    )
    assert logging_result.returncode == 0, logging_result.stderr
    logging_recreate = json.loads(logging_result.stdout)["recreate"]
    assert logging_recreate["log_driver"] == "json-file", logging_recreate
    assert logging_recreate["log_options"] == {
        "max-file": "3",
        "max-size": "50m",
    }, logging_recreate

    refused_logging = [
        supported_logging.replace('"json-file"', '"journald"', 1),
        supported_logging.replace('"max-file":"3"', '"compress":"true"', 1),
        supported_logging.replace('"max-file":"3"', '"max-file":"0"', 1),
        supported_logging.replace('"max-size":"50m"', '"max-size":"50mb"', 1),
        supported_logging.replace('"max-file":"3"', '"max-file":3', 1),
    ]
    for refused_log_config in refused_logging:
        (binp / "docker").write_text(
            docker_stub.replace(empty_port_bindings, refused_log_config, 1)
        )
        refused = subprocess.run(
            ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
            env=env, text=True, capture_output=True,
        )
        assert refused.returncode == 0, refused.stderr
        assert json.loads(refused.stdout)["recreate"] is None, refused.stdout
    (binp / "docker").write_text(docker_stub)

    def permission_facts(root: Path) -> dict[str, bool]:
        checked = subprocess.run(
            [
                "bash",
                "-c",
                f"source {LIB}\nouro_kes_rotation_permission_fixture_facts '{root}'",
            ],
            text=True,
            capture_output=True,
            env=env,
        )
        assert checked.returncode == 0, checked.stderr
        return {
            key: value == "true"
            for key, value in (line.split("=", 1) for line in checked.stdout.splitlines())
        }

    permission_root = Path(tempfile.mkdtemp(prefix="ouro-kes-permission-fixture-"))
    keys = permission_root / "opt/cardano/config/keys"
    keys.mkdir(parents=True)
    kes_key = keys / "kes.skey"
    vrf_key = keys / "vrf.skey"
    kes_key.write_text("fixture metadata only")
    vrf_key.write_text("fixture metadata only")
    os.chmod(keys, 0o755)
    os.chmod(kes_key, 0o600)
    os.chmod(vrf_key, 0o600)
    facts = permission_facts(permission_root)
    assert all(facts.values()), facts

    os.chmod(keys, 0o700)
    os.chmod(kes_key, 0o400)
    os.chmod(vrf_key, 0o400)
    assert all(permission_facts(permission_root).values())

    os.chmod(keys, 0o770)
    assert all(permission_facts(permission_root).values())
    os.chmod(keys, 0o777)
    assert permission_facts(permission_root)["keys_directory_safe"] is False
    os.chmod(keys, 0o755)
    for path, mode, fact in [
        (kes_key, 0o640, "kes_skey_private"),
        (vrf_key, 0o644, "vrf_skey_private"),
    ]:
        os.chmod(kes_key, 0o600)
        os.chmod(vrf_key, 0o600)
        os.chmod(path, mode)
        refused = permission_facts(permission_root)
        assert refused[fact] is False

    os.chmod(kes_key, 0o600)
    os.chmod(vrf_key, 0o600)
    kes_key.unlink()
    symlink_target = permission_root / "outside.skey"
    symlink_target.write_text("fixture metadata only")
    os.chmod(symlink_target, 0o600)
    kes_key.symlink_to(symlink_target)
    symlink_facts = permission_facts(permission_root)
    assert symlink_facts["kes_skey_private"] is False
    kes_key.unlink()
    kes_key.mkdir()
    non_regular = permission_facts(permission_root)
    assert non_regular["kes_skey_private"] is False
    kes_key.rmdir()
    kes_key.write_text("fixture metadata only")
    os.chmod(kes_key, 0o600)
    assert all(permission_facts(permission_root).values())

    permission_script = subprocess.run(
        ["bash", "-c", f"source {LIB}\nprintf '%s' \"$OURO_KES_ROTATION_PERMISSION_CHECK\""],
        text=True,
        capture_output=True,
        check=True,
    ).stdout
    assert "diag_uid" not in permission_script and "ouro-diag" not in permission_script
    assert "stat -c %u" not in permission_script

    # Expired cardano-cli queries can return no JSON. The fallback derives current KES from tip slot
    # and the public genesis value, never from the known-unreliable currentKESPeriod metric.
    fallback = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=dict(env, OURO_TEST_KES_EMPTY="1"), text=True, capture_output=True,
    )
    assert fallback.returncode == 0, fallback.stderr
    fallback_kes = json.loads(fallback.stdout)["readiness"]["kes"]
    assert fallback_kes["source"] == "prometheus_tip_and_genesis", fallback_kes
    assert fallback_kes["current_period"] == 5 and fallback_kes["end_period"] == 4, fallback_kes
    assert fallback_kes["remaining_periods"] == -1 and fallback_kes["valid"] is False, fallback_kes
    assert fallback_kes["counter_consistent"] is None, fallback_kes
    assert fallback_kes["counter_status"] == "unavailable", fallback_kes

    # cardano-cli represents OpCertNoBlocksMintedYet as a null node-state counter. Preserve that
    # typed fact and treat the active, in-period credentials as ready to produce their first block.
    # Candidate activation remains protected by the separate cold-identity-bound KES transaction.
    no_blocks = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=dict(env, OURO_TEST_KES_NULL="1"), text=True, capture_output=True,
    )
    assert no_blocks.returncode == 0, no_blocks.stderr
    no_blocks_readiness = json.loads(no_blocks.stdout)["readiness"]
    assert no_blocks_readiness["kes"] == {
        "source": "cardano_cli", "current_period": 10, "start_period": 9, "end_period": 20,
        "remaining_periods": 10, "opcert_counter_on_disk": 5,
        "opcert_counter_node_state": None, "counter_consistent": None,
        "counter_status": "no_blocks_minted_yet", "period_valid": True, "valid": True,
    }
    assert no_blocks_readiness["kes_opcert_valid"] is True
    assert no_blocks_readiness["forging_credentials_ready"] is True

    # A restart-looping container cannot answer docker exec, but the signed fixed bind layout still
    # provides enough public/metadata-only identity evidence for Phase B recovery planning. It must
    # not be misreported as a network/genesis mismatch or as ordinary node readiness.
    restarting = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=dict(env, OURO_TEST_RESTARTING="1"), text=True, capture_output=True,
    )
    assert restarting.returncode == 0, restarting.stderr
    restart_obs = json.loads(restarting.stdout)
    restart_live = restart_obs["live"]
    assert restart_live["container_running"] is False
    assert restart_live["container_restarting"] is True
    assert restart_live["container_status"] == "restarting"
    assert restart_live["network"] == "mainnet"
    assert restart_live["genesis_hash"] == hashlib.blake2b(
        (mainnet / "shelley-genesis.json").read_bytes(), digest_size=32
    ).hexdigest()
    assert restart_live["topology_hash"] == hashlib.sha256(
        (mainnet / "topology.json").read_bytes()
    ).hexdigest()
    assert restart_live["config_hash"] == hashlib.sha256(
        (mainnet / "config.json").read_bytes()
    ).hexdigest()
    assert restart_live["kes_opcert_id"] == hashlib.sha256(
        (config_keys / "node.cert").read_bytes()
    ).hexdigest()
    assert restart_live["has_forging_keys"] is True
    assert restart_live["keys_directory_safe"] is True
    assert restart_live["kes_skey_private"] is True
    assert restart_live["vrf_skey_private"] is True
    assert restart_obs["readiness"]["node_running"] is False
    assert restart_obs["readiness"]["socket_answers"] is False
    assert restart_obs["readiness"]["kes"] is None

    # Explicit user, supplementary groups and labels are closed recreate fields.
    modeled_stub = docker_stub.replace(
        '"Config":{"Hostname"',
        '"Config":{"User":"1000:1000","Hostname"',
        1,
    ).replace(
        '"PortBindings":{}',
        '"PortBindings":{"3001/tcp":[{"HostIp":"","HostPort":"3001"}],'
        '"12798/tcp":[{"HostIp":"127.0.0.1","HostPort":"12798"}]},'
        '"GroupAdd":["cardano","44"]',
        1,
    ).replace(
        '"Labels":{"org.opencontainers.image.title":"cardano-node"}',
        '"Labels":{"org.opencontainers.image.title":"cardano-node","io.ouro.role":"bp"}',
        1,
    )
    (binp / "docker").write_text(modeled_stub)
    modeled = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=env, text=True, capture_output=True,
    )
    assert modeled.returncode == 0, modeled.stderr
    modeled_recreate = json.loads(modeled.stdout)["recreate"]
    assert modeled_recreate["user"] == "1000:1000", modeled_recreate
    assert modeled_recreate["group_add"] == ["44", "cardano"], modeled_recreate
    assert modeled_recreate["ports"] == [
        {
            "container": "12798/tcp",
            "host_ip": "127.0.0.1",
            "host_port": "12798",
        },
        {"container": "3001/tcp", "host_ip": "", "host_port": "3001"},
    ], modeled_recreate
    assert modeled_recreate["labels"]["io.ouro.role"] == "bp", modeled_recreate
    (binp / "docker").write_text(docker_stub)

    # An unrelated running container is not a node candidate and still produces a numeric zero.
    no_node = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=dict(env, OURO_TEST_NO_NODE="1"), text=True, capture_output=True,
    )
    assert no_node.returncode == 0, no_node.stderr
    missing = json.loads(no_node.stdout)
    assert missing["supervisor"]["node_container_count"] == 0, missing
    assert missing["readiness"]["node_running"] is False, missing

    print("probe observation JSON passed")


if __name__ == "__main__":
    main()
