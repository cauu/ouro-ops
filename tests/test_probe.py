#!/usr/bin/env python3
"""S0019 p5-2 — the target-side observation probe emits a well-formed observation JSON. Uses docker
stubs (no real container needed); real gathering is bed-level (p5-6)."""
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
        f'  "inspect cid-xyz") echo \'[{{"Id":"cid-xyz000000000000","Name":"/cardano-node","Mounts":[{{"Type":"bind","Source":"{data_mount}","Destination":"/data/db","RW":true,"Mode":"rw","Propagation":"rprivate"}},{{"Type":"bind","Source":"{config_mount}","Destination":"/opt/cardano/config","RW":false,"Mode":"ro","Propagation":"rprivate"}},{{"Type":"bind","Source":"{ipc_mount}","Destination":"/ipc","RW":true,"Mode":"rw","Propagation":"rprivate"}}],"HostConfig":{{"RestartPolicy":{{"Name":"unless-stopped","MaximumRetryCount":0}},"NetworkMode":"bridge","PortBindings":{{}}}},"NetworkSettings":{{"Networks":{{"bridge":{{"Aliases":null,"IPAMConfig":null}}}}}},"Config":{{"Hostname":"cid-xyz00000","Image":"ghcr.io/blinklabs-io/cardano-node:10.5.4-1","Env":["CARDANO_BLOCK_PRODUCER=true","CARDANO_NETWORK=mainnet"],"Labels":{{"org.opencontainers.image.title":"cardano-node"}},"Entrypoint":["/usr/local/bin/entrypoint"]}},"Path":"/usr/local/bin/entrypoint","Args":["run"]}}]\';;\n'
        '  "image inspect") echo \'[{"Os":"linux","Architecture":"amd64","Config":{"Env":[],"Entrypoint":["/usr/local/bin/entrypoint"],"Cmd":[],"Labels":{"org.opencontainers.image.title":"cardano-node"}}}]\';;\n'
        '  "exec cid-xyz") shift 2; # sh -c ...\n'
        '     if echo "$*" | grep -q "cardano-cli query tip"; then echo "{\\"block\\":9,\\"slot\\":10,\\"era\\":\\"Conway\\",\\"syncProgress\\":\\"100.00\\"}";\n'
        '     elif echo "$*" | grep -q "hash genesis-file"; then echo "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81";\n'
        '     elif echo "$*" | grep -q "kes-period-info"; then printf "✓ period is valid\\n✓ counter agrees\\n{\\"qKesCurrentKesPeriod\\":10,\\"qKesStartKesInterval\\":9,\\"qKesEndKesInterval\\":20,\\"qKesOnDiskOperationalCertificateNumber\\":5,\\"qKesNodeStateOperationalCertificateNumber\\":5}\\n";\n'
        '     elif echo "$*" | grep -q "ss -Htn state established"; then echo 2;\n'
        '     elif echo "$*" | grep -q netstat; then echo 0;\n'
        '     elif echo "$*" | grep -q kes.skey; then echo true;\n'
        '     elif echo "$*" | grep -q node.cert; then echo "opcerthash  /x";\n'
        '     else echo "deadbeef  /x"; fi;;\n'
        'esac\n'
    )
    (binp / "docker").chmod(0o755)

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
    live = obs["live"]
    assert live["image_config_digest"] == "sha256:cfg"
    assert live["container_id"] == "cid-xyz"
    assert live["entrypoint"] == ["/usr/local/bin/entrypoint"] and live["args"] == ["run"]
    assert live["image_entrypoint"] == ["/usr/local/bin/entrypoint"] and live["image_cmd"] == []
    assert live["platform"] == "linux/amd64"
    assert live["container_name"] == "cardano-node"
    assert live["image_reference"] == "ghcr.io/blinklabs-io/cardano-node:10.5.4-1"
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
              "host_key_sha256", "genesis_hash", "network"]:
        assert k in live, f"observation missing {k}"
    readiness = obs["readiness"]
    for k in ["node_running", "socket_answers", "tip_block", "tip_block_next",
              "tip_block_height", "tip_slot", "tip_era", "sync_progress", "tip_synced",
              "kes_opcert_valid", "forging_credentials_ready", "established_peers"]:
        assert k in readiness, f"readiness missing {k}"
    assert readiness["tip_block"] == readiness["tip_block_next"] == 10
    assert readiness["tip_block_height"] == 9 and readiness["tip_slot"] == 10
    assert readiness["tip_era"] == "Conway" and readiness["sync_progress"] == "100.00"
    assert readiness["tip_synced"] is True, "healthy no-new-block sample must remain ready"
    assert readiness["kes_opcert_valid"] is True and readiness["forging_credentials_ready"] is True
    assert readiness["established_peers"] == 2, "ss-only Blink Labs image peers must be detected"
    assert obs["recreate"] is not None, "inherited nonempty OCI labels remain a valid baseline"

    # An explicit container user is not modeled by the sealed recreate argv. The probe must return
    # recreate:null instead of silently upgrading it as root.
    docker_stub = (binp / "docker").read_text()
    (binp / "docker").write_text(docker_stub.replace(
        '"Config":{"Hostname"',
        '"Config":{"User":"1000","Hostname"',
        1,
    ))
    unmodeled = subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_observe linux/amd64"],
        env=env, text=True, capture_output=True,
    )
    assert unmodeled.returncode == 0, unmodeled.stderr
    assert json.loads(unmodeled.stdout)["recreate"] is None, unmodeled.stdout
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
