#!/usr/bin/env python3
"""S0017 p5-1 — deterministic dispatch test for the cardano-cli managed-mode adapter.

`ouro_cardano_cli` must run cardano-cli in the node's supervision context: on the HOST for
bare/systemd, but `<runtime> exec <cid> cardano-cli` for a containerized node (where cardano-cli
lives inside the container). This drives that dispatch with an injected /proc + stubbed binaries
and asserts the exact command built for each mode — the same seam test_supervisor_mode_dispatch
uses. Standalone: `python3 tests/test_cardano_cli_adapter.py`.
"""
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / "ouro-skills/lib/ouro-lib.sh"
CID64 = "a" * 64

CG = {
    "docker": f"0::/system.slice/docker-{CID64}.scope\n",
    "bare": "0::/user.slice/user-1000.slice/session-3.scope\n",
}


def _exec(p: Path, body: str):
    p.write_text(body)
    p.chmod(0o755)


def bed(tmp, cgroup, pid="4242"):
    proc, binp = Path(tmp) / "proc", Path(tmp) / "bin"
    record = Path(tmp) / "calls.log"
    record.write_text("")
    d = proc / pid
    d.mkdir(parents=True, exist_ok=True)
    (d / "cgroup").write_text(cgroup)
    (d / "cmdline").write_bytes(b"cardano-node\x00run\x00")
    binp.mkdir(parents=True, exist_ok=True)
    _exec(binp / "pgrep", f'#!/usr/bin/env bash\nprintf "%s\\n" "{pid}"\nexit 0\n')
    # every stubbed binary logs "name <args>" so we can assert what the adapter invoked.
    for tool in ("docker", "podman", "cardano-cli"):
        _exec(binp / tool, f'#!/usr/bin/env bash\necho "{tool} $*" >> "{record}"\nexit 0\n')
    return proc, binp, record


def run(snippet, proc, binp, sock="/opt/devnet/node.socket"):
    env = dict(os.environ)
    env["OURO_PROC_ROOT"] = str(proc)
    env["PATH"] = f"{binp}:{env['PATH']}"
    env["CARDANO_NODE_SOCKET_PATH"] = sock
    return subprocess.run(["bash", "-c", f"source {LIB}\n{snippet}"], env=env,
                          text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main():
    failures = []
    def check(c, m):
        if not c:
            failures.append(m)

    # --- container (docker) mode: dispatch to `docker exec -e SOCK <cid[:12]> cardano-cli ...` ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["docker"])
        r = run("ouro_cardano_cli query tip --testnet-magic 1", proc, binp)
        log = record.read_text()
        check(r.returncode == 0, f"docker-mode adapter failed: {r.stderr}")
        check(f"docker exec -e CARDANO_NODE_SOCKET_PATH=/opt/devnet/node.socket {CID64[:12]} cardano-cli query tip --testnet-magic 1" in log,
              f"docker-mode did not dispatch through `docker exec <cid> cardano-cli`: {log!r}")
        # the HOST cardano-cli stub must NOT have been called directly.
        check("cardano-cli query tip" not in log.replace(f"{CID64[:12]} cardano-cli query tip", ""),
              f"docker-mode wrongly called host cardano-cli: {log!r}")

    # --- container availability check goes through the container too ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["docker"])
        r = run("ouro_cardano_cli_available && echo AVAIL", proc, binp)
        log = record.read_text()
        check("AVAIL" in r.stdout, f"container availability check failed: {r.stdout} {r.stderr}")
        check(f"docker exec {CID64[:12]} sh -c" in log,
              f"availability did not probe inside the container: {log!r}")

    # --- bare mode: run the HOST cardano-cli directly, never docker/podman ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["bare"])
        r = run("ouro_cardano_cli query tip --testnet-magic 1", proc, binp)
        log = record.read_text()
        check(r.returncode == 0, f"bare-mode adapter failed: {r.stderr}")
        check("cardano-cli query tip --testnet-magic 1" in log, f"bare-mode did not call host cardano-cli: {log!r}")
        check("docker exec" not in log and "podman exec" not in log,
              f"bare-mode wrongly went through a container: {log!r}")

    if failures:
        print("FAIL — cardano-cli adapter dispatch:")
        for f in failures:
            print(f"  - {f}")
        raise SystemExit(1)
    print("PASS — cardano-cli adapter dispatch: container -> `<rt> exec <cid> cardano-cli`, bare -> host cardano-cli")


if __name__ == "__main__":
    main()
