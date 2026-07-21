#!/usr/bin/env python3
"""S0020 p3-1 product seams: local artifact preview and declared-account diagnostics."""

import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"
GENESIS = "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81"


def run(env, *args):
    result = subprocess.run([str(BIN), *args], cwd=ROOT, env=env, text=True,
                            capture_output=True)
    value = json.loads(result.stdout or result.stderr)
    return result.returncode, value


def main():
    with tempfile.TemporaryDirectory(prefix="ouro-s0020-product-") as raw:
        base = Path(raw)
        home = base / "home"
        creds = home / "credentials"
        creds.mkdir(parents=True)
        (creds / "bp1").write_text("fixture-key")
        (creds / "relay1").write_text("fixture-key")
        (home / "known_hosts").write_text("fixture host key\n")

        spec = base / "pool-spec.yaml"
        spec.write_text(f"""spec_version: 1
pool:
  network: mainnet
  network_magic: 764824073
  genesis_hashes:
    shelley: "{GENESIS}"
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh: {{ host: 10.0.0.1, port: 22, user: bp-admin, key_ref: "creds://bp1" }}
  - id: relay1
    role: relay
    public_endpoint: {{ host: relay.example, port: 3001 }}
    ssh: {{ host: 10.0.0.2, port: 22, user: relay-ops, key_ref: "creds://relay1" }}
upgrade:
  min_online_relays: 0
""")

        bin_dir = base / "bin"
        bin_dir.mkdir()
        args_log = base / "ssh-args"
        ssh = bin_dir / "ssh"
        ssh.write_text("""#!/bin/sh
printf '%s\\n' "$@" > "$OURO_TEST_SSH_ARGS"
printf 'diagnostic-evidence\\n'
""")
        ssh.chmod(0o755)
        env = dict(os.environ, OURO_HOME=str(home), OURO_TEST_SSH_ARGS=str(args_log),
                   PATH=f"{bin_dir}:{os.environ['PATH']}")

        code, value = run(env, "diag", "exec", "--dispatch", "bp1", "--spec", str(spec),
                          "--", "printf", "diagnostic-evidence")
        assert code == 0 and value["status"] == "ok", value
        data = value["data"]
        assert data["principal"] == "bp-admin"
        assert data["assurance"] == "operator_ssh_diagnostic"
        assert data["read_only_enforced"] is False
        assert data["stdout"] == "diagnostic-evidence\n"
        argv = args_log.read_text()
        assert "bp-admin@10.0.0.1" in argv and "ouro-diag" not in argv
        assert "StrictHostKeyChecking=yes" in argv and "UserKnownHostsFile=" in argv
        assert "ouro-ops" not in argv and "/usr/local/" not in argv
        code, audit_value = run(env, "audit", "log", "--limit", "10")
        assert code == 0, audit_value
        invocation = data["audit_id"]
        diag_events = [event for event in audit_value["data"]["events"]
                       if event["invocation_id"] == invocation]
        assert {event["event"] for event in diag_events} == {"start", "finish"}, diag_events
        assert next(event for event in diag_events if event["event"] == "finish")["machine"] == "bp1"

        # Unknown and duplicate control flags refuse before SSH; command flags after `--` remain
        # opaque diagnostic argv rather than being parsed as Ouro options.
        args_log.unlink(missing_ok=True)
        for invalid in (
            ("--timout", "1"),
            ("--spec", str(spec)),
        ):
            prefix = ["diag", "exec", "--dispatch", "bp1", "--spec", str(spec)]
            code, value = run(env, *prefix, *invalid, "--", "uname", "-s")
            assert code != 0 and value["status"] == "error", value
            assert not args_log.exists(), "invalid diagnostic flags must refuse before SSH"

        # Each machine uses its own declared account; no fixed principal is inferred.
        args_log.unlink(missing_ok=True)
        code, value = run(env, "diag", "exec", "--dispatch", "relay1", "--spec", str(spec),
                          "--", "uname", "-s")
        assert code == 0 and value["data"]["principal"] == "relay-ops", value
        assert "relay-ops@10.0.0.2" in args_log.read_text()

        # A real SSH transport failure gets a crash terminal event instead of a false finish or a
        # permanently dangling start.
        ssh.write_text("#!/bin/sh\nexit 255\n")
        code, value = run(env, "diag", "exec", "--dispatch", "bp1", "--spec", str(spec),
                          "--", "uname", "-s")
        assert code != 0 and "diag transport" in value["error"]["detail"], value
        code, audit_value = run(env, "audit", "log", "--limit", "10")
        assert code == 0, audit_value
        diag_events = [event for event in audit_value["data"]["events"]
                       if event["tool"] == "diag/exec"]
        latest_id = diag_events[0]["invocation_id"]
        latest = [event for event in diag_events if event["invocation_id"] == latest_id]
        assert {event["event"] for event in latest} == {"start", "crash"}, latest

    print("S0020 product flow seams passed")


if __name__ == "__main__":
    main()
