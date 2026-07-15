#!/usr/bin/env python3
"""S0019 p5-5 — `ouro-ops inbox stage` (content-addressed ingress) + audit event emission
(schema-valid, closed fields)."""
import json
import os
import subprocess
import tempfile
from pathlib import Path

import jsonschema

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"
SCHEMA = json.loads((ROOT / "schemas/audit-event.schema.json").read_text())


def run(home, *args):
    env = dict(os.environ, OURO_HOME=home)
    r = subprocess.run([str(BIN), *args], env=env, text=True, capture_output=True)
    try:
        return r.returncode, json.loads(r.stdout or r.stderr)
    except Exception:
        return r.returncode, {"status": "error", "raw": (r.stdout + r.stderr)[:200]}


def cfg_digest():
    return json.loads((ROOT / "data/allowlist.json").read_text())["contracts"][0]["allowed"][0]["image_config_digest"]


def main():
    home = tempfile.mkdtemp()

    # --- inbox stage: an opcert artifact → content-addressed reference ---
    cert = Path(home) / "opcert.json"
    cert.write_text('{"type":"NodeOperationalCertificate","cborHex":"aa"}')
    _, d = run(home, "inbox", "stage", "--type", "opcert", "--file", str(cert))
    assert d["status"] == "ok" and "@sha256:" in d["data"]["artifact_ref"], d
    # a junk artifact of the wrong shape is refused
    bad = Path(home) / "bad.bin"
    bad.write_text("not json")
    _, d = run(home, "inbox", "stage", "--type", "opcert", "--file", str(bad))
    assert d["status"] == "error", d

    # --- audit event emission on an op run, schema-valid + closed ---
    o = Path(home) / "o.json"
    o.write_text(json.dumps({
        "supervisor": {"runtime": "docker", "rootful": True, "rootless": False,
                       "node_container_count": 1, "uses_bind_mounts": True,
                       "daemon_socket": "/var/run/docker.sock", "restart_policy": "unless-stopped",
                       "orchestration": "run"},
        "live": {"image_config_digest": cfg_digest(), "platform": "linux/amd64", "container_id": "cid",
                 "container_creation_epoch": 1, "entrypoint": ["cardano-node"], "args": ["run"],
                 "mount_source_ids": ["8:1:1"], "topology_hash": "t", "config_hash": "c",
                 "kes_opcert_id": "kes:5", "has_forging_keys": True, "host_key_sha256": "hk",
                 "genesis_hash": "g", "network": "mainnet"}}))
    run(home, "adopt", "--node", "bp1", "--role", "bp", "--approve-token", "x", "--observation", str(o))
    run(home, "op", "run", "--op", "observability/health", "--node", "bp1",
        "--param", "machine=bp1", "--observation", str(o))
    audit = Path(home) / "s0019-audit.jsonl"
    assert audit.exists(), "audit log written"
    events = [json.loads(l) for l in audit.read_text().splitlines() if l.strip()]
    assert events, "at least one audit event"
    for ev in events:
        jsonschema.Draft202012Validator(SCHEMA).validate(ev)  # schema-valid + closed
        assert set(ev) <= set(SCHEMA["properties"]), "only closed fields"
    assert any(e["event"] == "live_preflight" for e in events), events

    print("inbox stage + audit emission passed")


if __name__ == "__main__":
    main()
