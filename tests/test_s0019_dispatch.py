#!/usr/bin/env python3
"""S0019 p4-4 — dispatch/CLI-level negative tests beyond the p4-1 pipeline: adopt refuse paths,
crash recovery before a new write, and the write-seal. (Real container-bed docker execution and
crash injection mid-docker are the target-side seam; here every GATE and refuse path is exercised
through the CLI.)"""
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"


def cfg_digest():
    return json.loads((ROOT / "data/allowlist.json").read_text())["contracts"][0]["allowed"][0]["image_config_digest"]


def obs_doc(home, sup=None, **live_over):
    live = {"image_config_digest": cfg_digest(), "platform": "linux/amd64", "container_id": "cid1",
            "container_creation_epoch": 1000, "entrypoint": ["cardano-node"], "args": ["run"],
            "mounts": [{"kind": "bind", "source_id": "8:1", "destination": "/data/db",
                        "read_only": False, "owner": "0:0", "mode": "0755", "no_symlink": True}],
            "topology_hash": "t0", "config_hash": "c0",
            "kes_opcert_id": "kes:5", "has_forging_keys": True, "host_key_sha256": "a" * 64,
            "genesis_hash": "gh", "network": "mainnet"}
    live.update(live_over)
    supervisor = {"runtime": "docker", "rootful": True, "rootless": False, "node_container_count": 1,
                  "uses_bind_mounts": True, "daemon_socket": "/var/run/docker.sock",
                  "restart_policy": "unless-stopped", "orchestration": "run"}
    if sup:
        supervisor.update(sup)
    p = Path(home) / f"obs-{abs(hash(json.dumps(live)+json.dumps(supervisor)))}.json"
    p.write_text(json.dumps({"supervisor": supervisor, "live": live}))
    return str(p)


def run(home, *args):
    env = dict(os.environ, OURO_HOME=home)
    r = subprocess.run([str(BIN), *args], env=env, text=True, capture_output=True)
    try:
        return r.returncode, json.loads(r.stdout or r.stderr)
    except Exception:
        return r.returncode, {"status": "error", "raw": (r.stdout + r.stderr)[:200]}


def adopt(home, o, role="bp", node="bp1"):
    code, preview = run(home, "adopt", "--node", node, "--role", role,
                        "--preview", "--observation", o)
    if code != 0:
        return code, preview
    data = preview["data"]
    _, approval = run(home, "confirm", "adopt", "create", "--node", node,
                      "--candidate-hash", data["candidate_hash"],
                      "--host-key", data["host_key_sha256"])
    return run(home, "adopt", "--node", node, "--role", role,
               "--approve-token", approval["data"]["approve_token"], "--observation", o)


def main():
    home = tempfile.mkdtemp()

    # --- p9-7: onboard preview is a plan, never evidence of an attained remote state ---
    control_pubkey = Path(home) / "control.pub"
    control_pubkey.write_text("ssh-ed25519 AAAA0123456789abcdef operator@control\n")
    _, d = run(
        home,
        "onboard",
        "--host",
        "192.0.2.1",
        "--bootstrap-user",
        "cardano",
        "--bootstrap-key",
        "creds://bootstrap",
        "--control-pubkey",
        str(control_pubkey),
        "--ouro-binary",
        "/operator/supplied/ouro-ops-linux-x86_64",
        "--expected-host-key",
        "SHA256:operator-verified",
        "--dry-run",
    )
    assert d["status"] == "ok" and d["changed"] is False, d
    preview = d["data"]
    assert preview["dry_run"] is True and preview["state"] == "preview", preview
    assert preview["planned_state"] == "host-onboarded", preview
    assert preview["pinned_host_key"] is None, preview
    assert preview["host_key_status"] == "not_checked_in_dry_run", preview
    assert preview["expected_host_key_supplied"] is True, preview
    access = preview["ssh_access_policy"]
    assert access["drop_in"] == "/etc/ssh/sshd_config.d/20-ouro-s0019.conf", access
    assert access["allow_users"] == ["ouro-op", "ouro-diag", "cardano"], access
    assert access["bootstrap_user"] == "cardano", access
    assert access["bootstrap_user_preserved"] is True, access
    assert "AllowUsers ouro-op ouro-diag cardano" in access["rendered_config"], access
    assert "AAAA0123456789abcdef" not in json.dumps(access), access
    assert all(
        not step["changed"] and step["planned"] and not step["executed"]
        for step in preview["manifest"]["steps"]
    ), preview["manifest"]

    # --- adopt refuse paths (TC-2) ---
    # non-conforming supervisor (rootless)
    _, d = adopt(home, obs_doc(home, sup={"rootless": True}))
    assert d["status"] == "error" and "conform" in json.dumps(d), d
    # non-allowlisted image digest
    _, d = adopt(home, obs_doc(home, image_config_digest="sha256:" + "e" * 64))
    assert d["status"] == "error" and ("allowlist" in json.dumps(d) or "not on" in json.dumps(d)), d
    # relay bearing forging keys
    _, d = adopt(home, obs_doc(home, has_forging_keys=True), role="relay", node="relay1")
    assert d["status"] == "error" and "forging" in json.dumps(d), d
    # a conforming relay WITHOUT forging keys adopts fine
    _, d = adopt(home, obs_doc(home, has_forging_keys=False, container_id="rcid"), role="relay", node="relay1")
    assert d["status"] == "ok", d

    # --- adopt a good bp for the rest ---
    o = obs_doc(home)
    _, d = adopt(home, o)
    assert d["status"] == "ok", d

    # --- write-seal refuses any op (TC-6) ---
    txn = Path(home) / "txn"
    txn.mkdir(exist_ok=True)
    (txn / "bp1.txn.json").write_text(json.dumps(
        {"audit_id": "a", "operation_id": "runtime/restart", "node_id": "bp1", "state": "sealed"}))
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and "sealed" in json.dumps(d), d

    # --- a legacy committed journal lacks intent/pre-state/plans: never clear it as false success ---
    (txn / "bp1.txn.json").write_text(json.dumps(
        {"audit_id": "a", "operation_id": "config/render", "node_id": "bp1", "state": "committed"}))
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and "durable recovery context" in json.dumps(d), d
    assert (txn / "bp1.seal").exists(), "uncertain legacy write must seal target writes"
    assert json.loads((txn / "bp1.txn.json").read_text())["state"] == "sealed"
    # Test-only cleanup; production requires the explicit operator recovery path.
    (txn / "bp1.txn.json").unlink()
    (txn / "bp1.seal").unlink()

    # --- p5-1 SSH dispatch plan: op --dispatch runs on the target as the confined principal ---
    creds = Path(home) / "credentials"
    creds.mkdir(exist_ok=True)
    # p7-1: the op channel logs in as ouro-op (the write principal onboard installs), not ouro-exec.
    (creds / "ouro-op").write_text("key")
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--dispatch", "10.0.0.9", "--plan")
    assert d["status"] == "ok" and d["data"]["principal"] == "ouro-op", d
    j = " ".join(d["data"]["ssh_argv"])
    assert "ouro-op@10.0.0.9" in j and "ouro-exec@" not in j and "StrictHostKeyChecking=yes" in j, j
    assert "/usr/local/sbin/ouro-op-run" in j and "'--local'" in j, j

    print("S0019 dispatch-level negatives passed")


if __name__ == "__main__":
    main()
