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
    # docker stub: ps → one cardano-node container; inspect → fixed fields; exec → hashes/keys.
    (binp / "docker").write_text(
        '#!/usr/bin/env bash\n'
        'case "$1 $2" in\n'
        '  "ps --filter") echo "cid-xyz";;\n'
        '  "ps --format") echo "cid-xyz cardano-node run";;\n'
        '  "inspect --format")\n'
        '     case "$3" in\n'
        '       "{{.Image}}") echo "sha256:cfg";;\n'
        '       "{{.Created}}") echo "2026-07-15T10:00:00Z";;\n'
        '       "{{json .Config.Entrypoint}}") echo "[\\"cardano-node\\"]";;\n'
        '       "{{json .Args}}") echo "[\\"run\\"]";;\n'
        '       "{{range .Mounts}}{{.Source}};{{end}}") echo "/srv/cardano/data;/srv/cardano/config;";;\n'
        '       "{{.HostConfig.RestartPolicy.Name}}") echo "unless-stopped";;\n'
        '     esac;;\n'
        '  "exec cid-xyz") shift 2; # sh -c ...\n'
        '     if echo "$*" | grep -q "cardano-cli query tip"; then echo "{\\"block\\":10,\\"slot\\":10}";\n'
        '     elif echo "$*" | grep -q netstat; then echo 2;\n'
        '     elif echo "$*" | grep -q kes.skey; then echo true;\n'
        '     elif echo "$*" | grep -q node.cert; then echo "opcerthash  /x";\n'
        '     else echo "deadbeef  /x"; fi;;\n'
        'esac\n'
    )
    (binp / "docker").chmod(0o755)

    env = dict(os.environ, PATH=f"{binp}:{os.environ['PATH']}", OURO_READINESS_SAMPLE_DELAY="0")
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
    assert live["entrypoint"] == ["cardano-node"] and live["args"] == ["run"]
    assert len(live["mount_source_ids"]) == 2
    assert live["has_forging_keys"] is True
    assert live["container_creation_epoch"] > 0
    # every key the Rust ObsLive/SupervisorObservation expects is present
    for k in ["image_config_digest", "platform", "container_id", "container_creation_epoch",
              "entrypoint", "args", "mount_source_ids", "topology_hash", "config_hash",
              "kes_opcert_id", "has_forging_keys", "host_key_sha256", "genesis_hash", "network"]:
        assert k in live, f"observation missing {k}"
    readiness = obs["readiness"]
    for k in ["node_running", "socket_answers", "tip_block", "tip_block_next",
              "kes_opcert_valid", "credential_loaded", "established_peers"]:
        assert k in readiness, f"readiness missing {k}"

    print("probe observation JSON passed")


if __name__ == "__main__":
    main()
