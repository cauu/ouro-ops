#!/usr/bin/env python3
"""S0019 p5-5 — `ouro-ops inbox stage` (content-addressed ingress) + audit event emission
(schema-valid, closed fields)."""
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"
SCHEMA = json.loads((ROOT / "schemas/audit-event.schema.json").read_text())


def validate_closed_audit_event(event):
    """Schema-specific stdlib validator so the acceptance test has no undeclared dependency."""
    assert isinstance(event, dict)
    assert set(SCHEMA["required"]) <= set(event)
    assert set(event) <= set(SCHEMA["properties"])
    for name, value in event.items():
        rule = SCHEMA["properties"][name]
        if rule.get("type") == "string":
            assert isinstance(value, str) and len(value) >= rule.get("minLength", 0)
        elif rule.get("type") == "integer":
            assert isinstance(value, int) and not isinstance(value, bool)
            assert value >= rule.get("minimum", value)
        if "enum" in rule:
            assert value in rule["enum"]


def run(home, *args, input_text=None):
    env = dict(os.environ, OURO_HOME=home)
    r = subprocess.run([str(BIN), *args], env=env, text=True, capture_output=True,
                       input=input_text)
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
    # The fixed target wrapper uses bounded stdin, not a control-local target path.
    _, streamed = run(home, "inbox", "stage", "--local", "--type", "opcert", "--stdin",
                      input_text=cert.read_text())
    assert streamed["data"]["artifact_ref"] == d["data"]["artifact_ref"], streamed
    creds = Path(home) / "credentials"
    creds.mkdir(exist_ok=True)
    (creds / "ouro-op").write_text("key")
    _, dispatch = run(home, "inbox", "stage", "--type", "opcert", "--file", str(cert),
                      "--dispatch", "10.0.0.9", "--plan")
    argv = " ".join(dispatch["data"]["ssh_argv"])
    assert "/usr/local/sbin/ouro-inbox-stage 'opcert'" in argv, dispatch
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
                 "mounts": [{"kind": "bind", "source_id": "8:1", "destination": "/data/db",
                             "read_only": False, "owner": "0:0", "mode": "0755", "no_symlink": True}],
                 "topology_hash": "t", "config_hash": "c",
                 "kes_opcert_id": "kes:5", "has_forging_keys": True, "host_key_sha256": "a" * 64,
                 "genesis_hash": "g", "network": "mainnet"}}))
    _, preview = run(home, "adopt", "--node", "bp1", "--role", "bp",
                     "--preview", "--observation", str(o))
    data = preview["data"]
    _, approval = run(home, "confirm", "adopt", "create", "--node", "bp1",
                      "--candidate-hash", data["candidate_hash"],
                      "--host-key", data["host_key_sha256"])
    run(home, "adopt", "--node", "bp1", "--role", "bp",
        "--approve-token", approval["data"]["approve_token"], "--observation", str(o))
    run(home, "op", "run", "--op", "observability/health", "--node", "bp1",
        "--param", "machine=bp1", "--observation", str(o), "--plan")
    run(home, "op", "run", "--op", "config/render", "--node", "bp1",
        "--param", "machine=bp1", "--observation", str(o), "--plan")
    audit = Path(home) / "s0019-audit.jsonl"
    assert audit.exists(), "audit log written"
    events = [json.loads(l) for l in audit.read_text().splitlines() if l.strip()]
    assert events, "at least one audit event"
    for ev in events:
        validate_closed_audit_event(ev)
        assert ev["at_epoch"] > 0, ev
    assert (audit.stat().st_mode & 0o777) == 0o600
    assert any(e["event"] == "adopt" for e in events), events
    assert any(e["event"] == "live_preflight" for e in events), events
    assert any(e["event"] == "refusal" and e.get("operation_id") == "config/render"
               for e in events), events

    # Audit append never follows an attacker-controlled symlink or mutates its target.
    unsafe_home = Path(tempfile.mkdtemp())
    victim = unsafe_home / "victim"
    victim.write_text("keep")
    (unsafe_home / "s0019-audit.jsonl").symlink_to(victim)
    _, refused = run(str(unsafe_home), "op", "run", "--op", "runtime/restart",
                     "--node", "bp1", "--param", "machine=bp1", "--plan")
    assert "audit path" in json.dumps(refused), refused
    assert victim.read_text() == "keep"

    print("inbox stage + audit emission passed")


if __name__ == "__main__":
    main()
