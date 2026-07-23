#!/usr/bin/env python3
"""S0027 TC-4/TC-5: SSH trust is interactive, exact and user-owned."""

import errno
import json
import os
import pty
import select
import stat
import subprocess
import tempfile
import time
from pathlib import Path
from types import SimpleNamespace


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "ouro-ops"


def generate_key(path: Path) -> tuple[str, str]:
    subprocess.run(
        ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(path)],
        check=True,
    )
    public = path.with_suffix(".pub").read_text().split()
    fingerprint = subprocess.run(
        ["ssh-keygen", "-l", "-f", str(path.with_suffix(".pub"))],
        check=True,
        text=True,
        capture_output=True,
    ).stdout.split()[1]
    return public[1], fingerprint


def interactive(command: list[str], env: dict[str, str], confirmation: str):
    pid, descriptor = pty.fork()
    if pid == 0:
        os.chdir(ROOT)
        os.execve(command[0], command, env)
    output = bytearray()
    confirmed = False
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        ready, _, _ = select.select([descriptor], [], [], 0.1)
        if descriptor in ready:
            try:
                chunk = os.read(descriptor, 4096)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            output.extend(chunk)
            if not confirmed and b"to trust this exact key:" in output:
                os.write(descriptor, (confirmation + "\n").encode())
                confirmed = True
        waited, status = os.waitpid(pid, os.WNOHANG)
        if waited:
            return SimpleNamespace(
                returncode=os.waitstatus_to_exitcode(status),
                stdout=output.decode(errors="replace"),
            )
    waited, status = os.waitpid(pid, 0)
    return SimpleNamespace(
        returncode=os.waitstatus_to_exitcode(status),
        stdout=output.decode(errors="replace"),
    )


def main() -> None:
    subprocess.run(["cargo", "build", "-q", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0027-ssh-trust-"))
    credentials = home / "credentials"
    credentials.mkdir()
    for name in ("bp1", "relay1"):
        key = credentials / name
        key.write_text("test-only-private-key-placeholder")
        key.chmod(0o600)

    spec = home / "pool-spec.yaml"
    spec.write_text(
        """spec_version: 1
pool:
  network: mainnet
  network_magic: 764824073
  genesis_hashes:
    shelley: 1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh: {host: 192.0.2.1, port: 22, user: bp-admin, key_ref: creds://bp1}
  - id: relay1
    role: relay
    public_endpoint: {host: 192.0.2.2, port: 3001}
    ssh: {host: 192.0.2.2, port: 2222, user: relay-admin, key_ref: creds://relay1}
"""
    )

    fakebin = home / "fakebin"
    fakebin.mkdir()
    relay_key, relay_fingerprint = generate_key(home / "relay-host")
    bp_key, bp_fingerprint = generate_key(home / "bp-host")
    changed_key, changed_fingerprint = generate_key(home / "changed-host")
    scan_marker = home / "scan-called"
    ssh_log = home / "ssh.log"
    keyscan = fakebin / "ssh-keyscan"
    keyscan.write_text(
        "#!/bin/sh\n"
        f"touch '{scan_marker}'\n"
        "case \"$*\" in\n"
        f"  *192.0.2.1*) printf '%s\\n' '192.0.2.1 ssh-ed25519 {bp_key}' ;;\n"
        f"  *) if test -n \"$OURO_TEST_CHANGED_HOSTKEY\"; then "
        f"printf '%s\\n' '[192.0.2.2]:2222 ssh-ed25519 {changed_key}'; else "
        f"printf '%s\\n' '[192.0.2.2]:2222 ssh-ed25519 {relay_key}'; fi ;;\n"
        "esac\n"
    )
    keyscan.chmod(0o700)
    fake_ssh = fakebin / "ssh"
    fake_ssh.write_text(
        "#!/bin/sh\n"
        f"printf '%s\\n' \"$*\" >> '{ssh_log}'\n"
        "test -z \"$OURO_TEST_SSH_FAIL\"\n"
    )
    fake_ssh.chmod(0o700)
    env = dict(os.environ, OURO_HOME=str(home), PATH=f"{fakebin}:{os.environ['PATH']}")
    command = [
        str(BIN),
        "ssh",
        "trust",
        "--spec",
        str(spec),
        "--node",
        "relay1",
        "--expected-host-key",
        relay_fingerprint,
    ]

    inspect = subprocess.run(
        [str(BIN), "deploy", "inspect", "--spec", str(spec)],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
    )
    assert inspect.returncode != 0
    inspect_value = json.loads(inspect.stdout)
    assert inspect_value["error"]["code"] == "ssh_host_key_untrusted"
    assert inspect_value["data"]["target_contacted"] is False
    assert {target["machine"] for target in inspect_value["data"]["targets"]} == {
        "bp1",
        "relay1",
    }
    assert not scan_marker.exists(), "Inspect must not scan or contact an untrusted target"

    noninteractive = subprocess.run(
        command, cwd=ROOT, env=env, text=True, capture_output=True
    )
    assert noninteractive.returncode != 0
    assert "user-only and interactive" in noninteractive.stdout
    assert not scan_marker.exists(), "non-interactive caller must not contact the target"

    failed_auth = interactive(command, dict(env, OURO_TEST_SSH_FAIL="1"), "relay1")
    assert failed_auth.returncode != 0
    assert "account/credential verification failed" in failed_auth.stdout
    assert not (home / "known_hosts").exists(), "failed SSH auth must not persist trust"

    trusted = interactive(command, env, "relay1")
    assert trusted.returncode == 0, trusted.stdout
    assert relay_fingerprint in trusted.stdout
    assert "out-of-band fingerprint" in trusted.stdout
    known_hosts = home / "known_hosts"
    first = known_hosts.read_text()
    assert relay_key in first
    assert stat.S_IMODE(known_hosts.stat().st_mode) == 0o600
    strict_argv = ssh_log.read_text()
    for required in (
        "relay-admin@192.0.2.2",
        "StrictHostKeyChecking=yes",
        "IdentitiesOnly=yes",
        f"UserKnownHostsFile={home}",
    ):
        assert required in strict_argv, strict_argv

    repeated = interactive(command, env, "relay1")
    assert repeated.returncode == 0, repeated.stdout
    assert known_hosts.read_text() == first

    tofu_command = [
        str(BIN),
        "ssh",
        "trust",
        "--spec",
        str(spec),
        "--node",
        "bp1",
    ]
    tofu = interactive(tofu_command, env, "bp1")
    assert tofu.returncode == 0, tofu.stdout
    assert bp_fingerprint in tofu.stdout
    assert "user-accepted TOFU" in tofu.stdout

    changed_env = dict(env, OURO_TEST_CHANGED_HOSTKEY="1")
    changed_command = command[:-1] + [changed_fingerprint]
    changed = interactive(changed_command, changed_env, "relay1")
    assert changed.returncode == 0, changed.stdout
    assert "previously pinned host key differs" in changed.stdout
    assert changed_key in known_hosts.read_text()
    assert relay_key not in known_hosts.read_text()

    print("S0027 interactive SSH trust passed")


if __name__ == "__main__":
    main()
