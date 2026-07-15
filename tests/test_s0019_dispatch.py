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

    # --- p9-9: exact-name credential check/preview/register; no listing, bytes, or path output ---
    operator_key = Path(home) / "operator-id-ed25519"
    operator_key.write_text("test-only-private-key-bytes")
    operator_key.chmod(0o600)
    _, d = run(home, "creds", "check", "--name", "bp1")
    assert d["status"] == "ok" and d["data"]["registered"] is False, d
    _, preview_cred = run(
        home,
        "creds",
        "register",
        "--name",
        "bp1",
        "--path",
        str(operator_key),
        "--dry-run",
    )
    assert preview_cred["status"] == "ok" and preview_cred["changed"] is False, preview_cred
    assert preview_cred["data"]["planned"] is True, preview_cred
    assert preview_cred["data"]["credential_contents_read"] is False, preview_cred
    preview_text = json.dumps(preview_cred)
    assert str(operator_key) not in preview_text and "test-only-private-key-bytes" not in preview_text
    assert not (Path(home) / "credentials" / "bp1").exists(), preview_cred

    _, registered = run(
        home, "creds", "register", "--name", "bp1", "--path", str(operator_key)
    )
    assert registered["status"] == "ok" and registered["changed"] is True, registered
    assert registered["data"]["registered"] is True and registered["data"]["usable"] is True
    assert (Path(home) / "credentials" / "bp1").is_symlink()
    _, checked = run(home, "creds", "check", "--name", "bp1")
    assert checked["data"]["usable"] is True and checked["data"]["entry_kind"] == "symlink"
    _, idempotent = run(
        home, "creds", "register", "--name", "bp1", "--path", str(operator_key)
    )
    assert idempotent["status"] == "ok" and idempotent["changed"] is False, idempotent
    _, no_list = run(home, "creds", "list")
    assert no_list["status"] == "error" and "unsupported" in json.dumps(no_list), no_list
    _, ignored_flag = run(home, "creds", "check", "--name", "bp1", "--dry-run")
    assert ignored_flag["status"] == "error" and "unexpected" in json.dumps(ignored_flag)
    _, duplicate_name = run(
        home,
        "creds",
        "register",
        "--name",
        "bp1",
        "--name",
        "relay1",
        "--path",
        str(operator_key),
        "--dry-run",
    )
    assert duplicate_name["status"] == "error" and "unexpected" in json.dumps(duplicate_name)

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
    assert preview["convergence"] == "preview", preview
    assert preview["planned_state"] == "host-onboarded", preview
    assert preview["pinned_host_key"] is None, preview
    assert preview["host_key_status"] == "not_checked_in_dry_run", preview
    assert preview["expected_host_key_supplied"] is True, preview
    assert preview["effective_ssh_policy_verified"] is False, preview
    access = preview["ssh_access_policy"]
    assert access["drop_in"] == "/etc/ssh/sshd_config.d/20-ouro-s0019.conf", access
    assert access["allow_users"] == ["ouro-op", "ouro-diag", "cardano"], access
    assert access["bootstrap_user"] == "cardano", access
    assert access["bootstrap_user_preserved"] is True, access
    assert "AllowUsers ouro-op ouro-diag cardano" in access["rendered_config"], access
    assert access["legacy_s0017_paths_retired"] == [
        "/etc/ssh/sshd_config.d/10-ouro.conf",
        "/etc/sudoers.d/ouro-exec",
        "/usr/local/sbin/ouro-tool-run",
    ], access
    descs = [step["desc"] for step in preview["manifest"]["steps"]]
    assert "retire S0017 privilege path" in descs, descs
    assert "guarded install, validate and reload SSH policy" in descs, descs
    assert descs[0] == "read-only SSH policy and principal preflight", descs
    assert "arm SSH policy rollback" in descs, descs
    assert descs.index("stage hardened sshd policy") < descs.index("arm SSH policy rollback"), descs
    assert descs.index("arm SSH policy rollback") < descs.index(
        "guarded install, validate and reload SSH policy"
    ), descs
    assert descs[-5] == "guarded install, validate and reload SSH policy", descs
    assert descs[-4:-1] == [
        "fresh SSH login (bootstrap: cardano)",
        "fresh SSH login (write principal: ouro-op)",
        "fresh SSH login (diagnostic principal: ouro-diag)",
    ], descs
    assert descs[-1] == "disarm verified SSH policy rollback", descs
    assert "AAAA0123456789abcdef" not in json.dumps(access), access
    assert all(
        not step["changed"] and step["planned"] and not step["executed"]
        for step in preview["manifest"]["steps"]
    ), preview["manifest"]

    # A real-mode fresh-login verification failure emits exactly one typed JSON record. The fake
    # transport reports a converged read-only probe, then rejects only the ouro-op fresh session;
    # no provisioning write is simulated.
    failure_key = Path(home) / "failure-id-ed25519"
    subprocess.run(
        ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(failure_key)],
        check=True,
    )
    _, registered_failure = run(
        home,
        "creds",
        "register",
        "--name",
        "failure",
        "--path",
        str(failure_key),
    )
    assert registered_failure["status"] == "ok", registered_failure
    fakebin = Path(home) / "fakebin"
    fakebin.mkdir()
    fake_ssh = fakebin / "ssh"
    fake_ssh.write_text(
        "#!/bin/sh\n"
        "case \"$*\" in\n"
        "  *uname\\ -s*) printf 'os=Linux\\narch=x86_64\\nid=ubuntu\\nid_like=debian\\nsystemd=yes\\n'; exit 0;;\n"
        "  *ouro-op@*\\ true) exit 1;;\n"
        "  *) exit 0;;\n"
        "esac\n"
    )
    fake_ssh.chmod(0o700)
    linux_elf = Path(home) / "ouro-ops-linux"
    elf = bytearray(20)
    elf[:4] = b"\x7fELF"
    elf[18:20] = (0x3E).to_bytes(2, "little")
    linux_elf.write_bytes(elf)
    failed = subprocess.run(
        [
            str(BIN),
            "onboard",
            "--host",
            "192.0.2.1",
            "--bootstrap-user",
            "cardano",
            "--bootstrap-key",
            "creds://failure",
            "--control-pubkey",
            str(failure_key.with_suffix(".pub")),
            "--ouro-binary",
            str(linux_elf),
        ],
        env=dict(os.environ, OURO_HOME=home, PATH=f"{fakebin}:{os.environ['PATH']}"),
        text=True,
        capture_output=True,
    )
    lines = failed.stdout.splitlines()
    assert failed.returncode == 10 and len(lines) == 1, (failed.returncode, failed.stdout, failed.stderr)
    failure = json.loads(lines[0])
    assert failure["status"] == "error" and failure["changed"] is False, failure
    assert failure["data"]["convergence"] == "verification_failed", failure
    assert failure["data"]["effective_ssh_policy_verified"] is False, failure

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

    # A transport failure is one bounded typed record, not a silent exit 255. Remote stderr is
    # untrusted DATA but retaining a bounded excerpt makes host-key/auth failures diagnosable.
    transport_bin = Path(home) / "transport-bin"
    transport_bin.mkdir()
    fake_transport = transport_bin / "ssh"
    fake_transport.write_text(
        "#!/bin/sh\n"
        "printf 'Permission denied (publickey).\\n' >&2\n"
        "exit 255\n"
    )
    fake_transport.chmod(0o700)
    transport = subprocess.run(
        [
            str(BIN), "op", "run", "--op", "observability/health", "--node", "bp1",
            "--param", "machine=bp1", "--dispatch", "192.0.2.1", "--ssh-key", "creds://bp1",
        ],
        env=dict(os.environ, OURO_HOME=home, PATH=f"{transport_bin}:{os.environ['PATH']}"),
        text=True,
        capture_output=True,
    )
    assert transport.returncode == 255 and len(transport.stdout.splitlines()) == 1, transport
    transport_result = json.loads(transport.stdout)
    assert transport_result["status"] == "error", transport_result
    assert transport_result["error"]["code"] == "ssh_exit_255", transport_result
    assert "Permission denied" in transport_result["error"]["detail"], transport_result
    assert len(transport_result["error"]["detail"]) < 4096, transport_result

    # Standalone S0017 detection is retired before local execution or legacy dispatch can produce
    # control-host facts under a target machine id.
    for detect_args in (
        ["tool", "run", "detect/runtime", "--machine", "bp1"],
        ["tool", "run", "detect/runtime", "--dispatch", "bp1", "--spec", "pool-spec.yaml"],
    ):
        _, retired = run(home, *detect_args)
        assert retired["status"] == "error" and "retired in S0019" in json.dumps(retired), retired

    # Nested agent-facing help must be discoverable without supplying the operation's required
    # arguments first.
    for help_args, needle in (
        (["op", "run", "--help"], "--dispatch <host>"),
        (["fleet", "permit", "create", "--help"], "--online-relays"),
        (["inbox", "stage", "--help"], "--type <opcert|tx|image>"),
        (["adopt", "--help"], "--bootstrap-user <account>"),
        (["manifest", "verify", "--help"], "--against <bundle-manifest.json>"),
    ):
        helped = subprocess.run([str(BIN), *help_args], text=True, capture_output=True)
        assert helped.returncode == 0 and needle in helped.stdout, (help_args, helped)

    print("S0019 dispatch-level negatives passed")


if __name__ == "__main__":
    main()
