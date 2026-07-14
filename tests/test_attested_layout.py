#!/usr/bin/env python3
"""S0019 p1-5 (§1.C) — the greenfield layout lib reads the attestation, never detects.

1. A node WITHOUT an attestation is refused (not_ouro_managed, exit 20) — TC-1.
2. With an attestation, the layout accessors READ recorded facts (role, container, in-container
   paths), never guessing.
3. Static gate: ouro-attested.sh contains NO detection primitives (pgrep/cgroup/ouro_node_arg/
   supervisor-mode dispatch) — the S0017 discovery machinery is not carried forward.
"""
import json
import re
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / "ouro-skills/lib/ouro-attested.sh"


def run(snippet, attestation_path):
    return subprocess.run(
        ["bash", "-c", f"OURO_ATTESTATION={attestation_path}\nsource {LIB}\n{snippet}"],
        text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )


def main():
    # 1. no attestation → refuse (not_ouro_managed, exit 20)
    r = run("ouro_require_attested", "/nonexistent/attestation.json")
    assert r.returncode == 20, f"unmanaged node must refuse (20), got {r.returncode}"
    assert "not_ouro_managed" in r.stderr, r.stderr

    # 2. with an attestation → read recorded layout facts
    att = {
        "immutable": {"role": "bp", "image_config_digest": "sha256:cfg",
                      "contract": {"in_container_paths": {"socket": "/ipc/node.socket",
                                                          "db": "/data/db",
                                                          "keys": "/opt/cardano/config/keys"}}},
        "state": {"state_generation": 4, "container_id": "cid123"},
        # the adopt path also mirrors the resolved contract at the top level for the accessors
        "contract": {"in_container_paths": {"socket": "/ipc/node.socket", "db": "/data/db",
                                            "keys": "/opt/cardano/config/keys"}},
    }
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump(att, f)
        path = f.name
    assert run("ouro_attested_role", path).stdout.strip() == "bp"
    assert run("ouro_attested_container", path).stdout.strip() == "cid123"
    assert run("ouro_attested_db", path).stdout.strip() == "/data/db"
    assert run("ouro_attested_keys", path).stdout.strip() == "/opt/cardano/config/keys"
    assert run("ouro_attested_generation", path).stdout.strip() == "4"

    # 3. static gate: no detection primitives in the greenfield lib
    text = LIB.read_text()
    forbidden = ["pgrep", "cgroup", "ouro_node_arg", "ouro_node_detect_mode",
                 "docker ps", "ouro_supervisor"]
    for tok in forbidden:
        assert tok not in text, f"greenfield attested lib must not detect ({tok!r} present)"

    print("attested layout lib passed")


if __name__ == "__main__":
    main()
