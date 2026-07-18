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
GENESIS = "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81"
HOST_KEY = "SHA256:" + "a" * 43


def cfg_digest():
    return json.loads((ROOT / "data/allowlist.json").read_text())["contracts"][0]["allowed"][0]["image_config_digest"]


def obs_doc(home, sup=None, **live_over):
    live = {"image_config_digest": cfg_digest(), "platform": "linux/amd64", "container_id": "cid1",
            "container_creation_epoch": 1000, "container_name": "cardano-node",
            "image_reference": "ghcr.io/blinklabs-io/cardano-node:test",
            "entrypoint": ["/usr/local/bin/entrypoint"], "args": ["run"],
            "image_entrypoint": ["/usr/local/bin/entrypoint"], "image_cmd": [],
            "mounts": [
                {"kind": "bind", "source_id": "8:1", "destination": "/data/db",
                 "read_only": False, "owner": "0:0", "mode": "0755", "no_symlink": True},
                {"kind": "bind", "source_id": "8:2", "destination": "/opt/cardano/config",
                 "read_only": True, "owner": "0:0", "mode": "0755", "no_symlink": True},
                {"kind": "bind", "source_id": "8:3", "destination": "/ipc",
                 "read_only": False, "owner": "0:0", "mode": "0755", "no_symlink": True},
            ],
            "topology_hash": "t0", "config_hash": "c0",
            "kes_opcert_id": "kes:5", "has_forging_keys": True,
            "forging_key_permissions_safe": True, "host_key_sha256": HOST_KEY,
            "genesis_hash": GENESIS, "network": "mainnet"}
    live.update(live_over)
    supervisor = {"runtime": "docker", "rootful": True, "rootless": False, "node_container_count": 1,
                  "uses_bind_mounts": True, "daemon_socket": "/var/run/docker.sock",
                  "restart_policy": "unless-stopped", "orchestration": "run"}
    if sup:
        supervisor.update(sup)
    p = Path(home) / f"obs-{abs(hash(json.dumps(live)+json.dumps(supervisor)))}.json"
    relay = not live["has_forging_keys"]
    p.write_text(json.dumps({"supervisor": supervisor, "live": live,
                             "readiness": {"node_running": True, "socket_answers": True,
                                           "tip_block": 100, "tip_block_next": 100,
                                           "tip_synced": True, "kes_opcert_valid": not relay,
                                           "forging_credentials_ready": not relay,
                                           "established_peers": 2}}))
    return str(p)


def run(home, *args, env_extra=None):
    env = dict(os.environ, OURO_HOME=home)
    if env_extra:
        env.update(env_extra)
    r = subprocess.run([str(BIN), *args], env=env, text=True, capture_output=True)
    try:
        return r.returncode, json.loads(r.stdout or r.stderr)
    except Exception:
        return r.returncode, {"status": "error", "raw": (r.stdout + r.stderr)[:200]}


def security_digest(home):
    _, out = run(home, "version")
    assert out["status"] == "ok", out
    return out["data"]["security_identity"]


def adoption_args(home, observation, role, node):
    return ("--node", node, "--role", role, "--observation", observation,
            "--expect-embedded", security_digest(home),
            "--expected-role", role, "--expected-network", "mainnet",
            "--expected-genesis", GENESIS, "--expected-host-key", HOST_KEY)


def adopt(home, o, role="bp", node="bp1"):
    args = adoption_args(home, o, role, node)
    code, preview = run(home, "adopt", *args, "--preview")
    if code != 0:
        return code, preview
    data = preview["data"]
    _, approval = run(home, "confirm", "adopt", "create", "--node", node,
                      "--candidate-hash", data["candidate_hash"],
                      "--host-key", data["host_key_sha256"])
    return run(home, "adopt", *args, "--approve-token", approval["data"]["approve_token"])


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
        "SHA256:" + "a" * 43,
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

    # A misspelled preview flag can never fall through into a real SSH/host mutation.
    _, typo_preview = run(
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
        "--dryrun",
    )
    assert typo_preview["status"] == "error" and "unexpected" in json.dumps(typo_preview)

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
            "--apply",
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
    relay_key_drift = obs_doc(home, has_forging_keys=True, container_id="rcid")
    _, d = run(home, "op", "run", "--op", "fleet/status", "--node", "relay1",
               "--param", "machine=relay1", "--observation", relay_key_drift)
    assert d["status"] == "error" and "forging keys" in json.dumps(d), d
    missing_ref = "opcert-x@sha256:" + "b" * 64
    _, d = run(home, "op", "run", "--op", "kes-rotation/install-opcert", "--node", "relay1",
               "--param", "machine=relay1", "--param", f"opcert={missing_ref}",
               "--fleet-spec-digest", "sha256:" + "a" * 64,
               "--fleet-pool-id", "pool-test", "--fleet-min-online-relays", "1",
               "--observation", obs_doc(home, has_forging_keys=False, container_id="rcid"), "--plan")
    assert d["status"] == "error" and "BP-only" in json.dumps(d), d

    # --- adopt a good bp for the rest ---
    o = obs_doc(home)
    _, d = adopt(home, o)
    assert d["status"] == "ok", d
    _, d = run(home, "op", "run", "--op", "fleet/status", "--node", "bp1",
               "--param", "machine=bp1", "--local", "--observation", o)
    assert d["status"] == "error" and "must use the embedded live probe" in json.dumps(d), d
    _, d = run(home, "op", "run", "--op", "fleet/status", "--node", "bp1",
               "--param", "machine=bp1",
               "--observation", obs_doc(home, sup={"node_container_count": 2}))
    assert d["status"] == "error" and "exactly 1" in json.dumps(d), d

    # --- write-seal refuses any op (TC-6) ---
    txn = Path(home) / "txn"
    txn.mkdir(exist_ok=True)
    (txn / "bp1.txn.json").write_text(json.dumps(
        {"audit_id": "a", "operation_id": "runtime/restart", "node_id": "bp1", "state": "sealed"}))
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and "sealed" in json.dumps(d).lower(), d

    # --- a plan reports pending recovery but remains pure read; it never seals/reconciles ---
    (txn / "bp1.txn.json").write_text(json.dumps(
        {"audit_id": "a", "operation_id": "config/render", "node_id": "bp1", "state": "committed"}))
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o, "--plan")
    assert d["status"] == "error" and "pending transaction" in json.dumps(d), d
    assert not (txn / "bp1.seal").exists(), "--plan must not reconcile or create a write seal"
    assert json.loads((txn / "bp1.txn.json").read_text())["state"] == "committed"
    # A REAL operation also refuses without auto-verify/rollback: ordinary invocations cannot use a
    # stale journal to bypass fresh fleet + human authorization.
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--observation", o)
    assert d["status"] == "error" and "never auto-verify or auto-rollback" in json.dumps(d), d
    assert not (txn / "bp1.seal").exists(), "ordinary op must not mutate recovery metadata"
    assert json.loads((txn / "bp1.txn.json").read_text())["state"] == "committed"
    # Test-only cleanup; production requires the explicit operator recovery path.
    (txn / "bp1.txn.json").unlink()

    # --- Explicit transport-only inspection shows the S0020 ephemeral argv without claiming target validation. ---
    creds = Path(home) / "credentials"
    creds.mkdir(exist_ok=True)
    (creds / "bp1").write_text("key")
    transport_spec = Path(home) / "s0020-transport-spec.yaml"
    transport_spec.write_text(f"""spec_version: 1
pool:
  network: mainnet
  network_magic: 764824073
  genesis_hashes:
    shelley: {GENESIS}
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh:
      host: 10.0.0.9
      port: 22
      user: cardano
      key_ref: creds://bp1
  - id: relay1
    role: relay
    public_endpoint:
      host: relay.example.com
      port: 3001
    ssh:
      host: 10.0.0.10
      port: 22
      user: cardano
      key_ref: creds://relay1
upgrade:
  min_online_relays: 0
""")
    runner = Path(home) / "s0020-runner"
    runner.write_bytes(b"s0020-runner")
    _, d = run(home, "op", "run", "--op", "runtime/restart", "--node", "bp1",
               "--param", "machine=bp1", "--dispatch", "10.0.0.9",
               "--ssh-key", "creds://bp1", "--spec", str(transport_spec),
               "--candidate-hash", "a" * 64, "--transport-plan",
               env_extra={"OURO_EPHEMERAL_RUNNER": str(runner)})
    assert d["status"] == "ok" and d["data"]["principal"] == "cardano", d
    assert d["tool"] == "ouro.op.apply.dispatch.transport_plan" and d["data"]["target_validated"] is False
    j = " ".join(d["data"]["ssh_argv"])
    assert "cardano@10.0.0.9" in j and "ouro-op@" not in j and "StrictHostKeyChecking=yes" in j, j
    assert "mktemp -d /tmp/ouro-run.XXXXXXXXXX" in j and "'target' 'apply'" in j, j
    assert "/usr/local/sbin/ouro-op-run" not in j and "/usr/local/bin/ouro-ops" not in j, j
    assert "-F /dev/null" in j and "IdentityAgent=none" in j and "IdentitiesOnly=yes" in j, j
    assert "'--plan'" not in j, "transport plan itself is not forwarded"

    # Every privileged adoption dispatch, not merely its separate identity preflight, carries the
    # exact control security identity once. This is a transport-only argv inspection: no SSH runs.
    host_key_parts = failure_key.with_suffix(".pub").read_text().split()
    (Path(home) / "known_hosts").write_text(
        f"192.0.2.1 {host_key_parts[0]} {host_key_parts[1]}\n"
    )
    _, adopt_plan = run(
        home,
        "adopt",
        "--dispatch",
        "192.0.2.1",
        "--bootstrap-user",
        "cardano",
        "--ssh-key",
        "creds://bp1",
        "--spec",
        str(ROOT / "examples/pool-spec.minimal.yaml"),
        "--node",
        "bp1",
        "--role",
        "bp",
        "--plan",
    )
    assert adopt_plan["status"] == "ok", adopt_plan
    adopt_argv = " ".join(adopt_plan["data"]["ssh_argv"])
    assert adopt_argv.count("--expect-embedded") == 1, adopt_argv
    assert security_digest(home) in adopt_argv, adopt_argv

    # A transport failure is one bounded typed record, not a silent exit 255. Remote stderr is
    # untrusted DATA but retaining a bounded excerpt makes host-key/auth failures diagnosable.
    transport_bin = Path(home) / "transport-bin"
    transport_bin.mkdir()
    fake_transport = transport_bin / "ssh"
    transport_log = Path(home) / "transport-argv.log"
    fake_transport.write_text(
        "#!/bin/sh\n"
        f"printf '%s\\n' \"$*\" > '{transport_log}'\n"
        "printf 'Permission denied (publickey).\\n' >&2\n"
        "exit 255\n"
    )
    fake_transport.chmod(0o700)
    # Preview modes reject an approval capability before transport construction/execution; the
    # token must never appear in an argv preview or reach the fake SSH process.
    preview_secret = "CONFIRM-SENTINEL-MUST-NOT-LEAK"
    for preview_mode in ("--plan", "--transport-plan"):
        transport_log.unlink(missing_ok=True)
        rejected_preview = subprocess.run(
            [
                str(BIN), "op", "run", "--op", "runtime/restart", "--node", "bp1",
                "--param", "machine=bp1", "--dispatch", "192.0.2.1",
                "--ssh-key", "creds://bp1", "--confirm-token", preview_secret, preview_mode,
            ],
            env=dict(os.environ, OURO_HOME=home, PATH=f"{transport_bin}:{os.environ['PATH']}"),
            text=True,
            capture_output=True,
        )
        assert rejected_preview.returncode == 10, rejected_preview
        assert not transport_log.exists(), "preview approval must be rejected before SSH"
        assert preview_secret not in rejected_preview.stdout + rejected_preview.stderr
    # The fleet authority rejects caller-supplied facts, and a failed target snapshot does not mint
    # a lease/permit before returning the refusal.
    _, nondisruptive = run(
        home, "fleet", "permit", "create", "--spec",
        str(ROOT / "examples/pool-spec.minimal.yaml"), "--node", "bp1",
        "--op", "observability/health", "--intent-hash", "a" * 64,
        "--min-online-relays", "1", "--holder", "testctl",
    )
    assert nondisruptive["status"] == "error" and "not disruptive" in json.dumps(nondisruptive)
    assert not (Path(home) / "fleet-authority").exists(), nondisruptive
    _, supplied_facts = run(
        home, "fleet", "permit", "create", "--spec",
        str(ROOT / "examples/pool-spec.minimal.yaml"),
        "--node", "bp1", "--op", "runtime/restart", "--min-online-relays", "1",
        "--intent-hash", "a" * 64, "--online-relays", "99", "--holder", "testctl",
    )
    assert supplied_facts["status"] == "error" and "not accepted" in json.dumps(supplied_facts)
    for extra in (
        ["--node", "relay1"],
        ["--online-relays=99"],
    ):
        _, closed_fleet = run(
            home, "fleet", "permit", "create", "--spec",
            str(ROOT / "examples/pool-spec.minimal.yaml"),
            "--node", "bp1", "--op", "runtime/restart", "--min-online-relays", "1",
            "--intent-hash", "a" * 64, "--holder", "testctl", *extra,
        )
        assert closed_fleet["status"] == "error" and (
            "duplicate" in json.dumps(closed_fleet) or "unexpected" in json.dumps(closed_fleet)
        ), closed_fleet
    failed_fleet = subprocess.run(
        [
            str(BIN), "fleet", "permit", "create", "--spec",
            str(ROOT / "examples/pool-spec.minimal.yaml"),
            "--node", "bp1", "--op", "runtime/restart", "--min-online-relays", "1",
            "--intent-hash", "a" * 64, "--holder", "testctl",
        ],
        env=dict(os.environ, OURO_HOME=home, PATH=f"{transport_bin}:{os.environ['PATH']}"),
        text=True,
        capture_output=True,
    )
    assert failed_fleet.returncode != 0, failed_fleet
    assert not list((Path(home) / "fleet-authority").glob("*.lease.json")) \
        if (Path(home) / "fleet-authority").exists() else True
    # S0020 derives every target-plan pool binding from the operator spec. A missing spec stops on
    # control before SSH rather than falling back to the target-installed S0019 CLI.
    target_plan = subprocess.run(
        [
            str(BIN), "op", "run", "--op", "runtime/restart", "--node", "bp1",
            "--param", "machine=bp1", "--dispatch", "192.0.2.1", "--ssh-key", "creds://bp1",
            "--plan",
        ],
        env=dict(os.environ, OURO_HOME=home, PATH=f"{transport_bin}:{os.environ['PATH']}"),
        text=True,
        capture_output=True,
    )
    assert target_plan.returncode == 10, target_plan
    assert "missing --spec" in target_plan.stdout, target_plan.stdout
    transport = subprocess.run(
        [
            str(BIN), "op", "run", "--op", "observability/health", "--node", "bp1",
            "--param", "machine=bp1", "--dispatch", "10.0.0.9", "--ssh-key", "creds://bp1",
            "--spec", str(transport_spec),
        ],
        env=dict(
            os.environ,
            OURO_HOME=home,
            OURO_EPHEMERAL_RUNNER=str(linux_elf),
            PATH=f"{transport_bin}:{os.environ['PATH']}",
        ),
        text=True,
        capture_output=True,
    )
    assert transport.returncode == 255 and len(transport.stdout.splitlines()) == 1, transport
    transport_result = json.loads(transport.stdout)
    assert transport_result["status"] == "error", transport_result
    assert transport_result["error"]["code"] == "ssh_exit_255", transport_result
    assert "Permission denied" in transport_result["error"]["detail"], transport_result
    assert len(transport_result["error"]["detail"]) < 4096, transport_result

    # The entire S0017 tool family is absent before local execution or legacy dispatch can produce
    # control-host facts under a target machine id.
    for detect_args in (
        ["tool", "run", "detect/runtime", "--machine", "bp1"],
        ["tool", "run", "detect/runtime", "--dispatch", "bp1", "--spec", "pool-spec.yaml"],
    ):
        _, retired = run(home, *detect_args)
        assert retired["status"] == "error" and "unknown command tool" in json.dumps(retired), retired

    # Nested agent-facing help must be discoverable without supplying the operation's required
    # arguments first.
    for help_args, needle in (
        (["op", "run", "--help"], "--dispatch <host>"),
        (["fleet", "permit", "create", "--help"], "upgrade.min_online_relays"),
        (["inbox", "stage", "--help"], "--type <opcert|tx>"),
        (["adopt", "--help"], "--bootstrap-user <account>"),
        (["contract", "--help"], "--requires-contract <integer>"),
    ):
        helped = subprocess.run([str(BIN), *help_args], text=True, capture_output=True)
        assert helped.returncode == 0 and needle in helped.stdout, (help_args, helped)

    print("S0019 dispatch-level negatives passed")


if __name__ == "__main__":
    main()
