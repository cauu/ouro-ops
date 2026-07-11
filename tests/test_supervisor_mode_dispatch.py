#!/usr/bin/env python3
"""S0017 p2-5 — supervisor-mode lifecycle dispatch + fail-closed guard.

Drives the adapter's mode resolution and dispatch with an injected /proc and stubbed
supervision binaries, asserting:
  * the effective mode is detection-governed and cross-checked against the declaration;
  * the fail-closed guard TERMINATES the script (exit 40) on none/ambiguous/mismatch —
    NOT merely the command-substitution subshell (the bypass this guards against);
  * a restart dispatches to the correct binary per mode (systemctl/docker/podman/bare);
  * ouro_node_detect_mode agrees with the detect/runtime probe's mode field.

Standalone: `python3 tests/test_supervisor_mode_dispatch.py`.
"""
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / "ouro-skills/lib/ouro-lib.sh"
PROBE = ROOT / "ouro-skills/detect/scripts/runtime.sh"
CID64 = "c" * 64

CG = {
    "docker":  f"0::/system.slice/docker-{CID64}.scope\n",
    "podman":  f"0::/machine.slice/libpod-{CID64}.scope\n",
    "systemd": "0::/system.slice/cardano-node.service\n",
    "bare":    "0::/user.slice/user-1000.slice/session-3.scope\n",
}


def _exec(p: Path, body: str):
    p.write_text(body)
    p.chmod(0o755)


def bed(tmp, cgroup, pids=("4242",)):
    """Injected /proc + a stubbed PATH; every supervision binary logs its argv to record."""
    proc, binp = Path(tmp) / "proc", Path(tmp) / "bin"
    record = Path(tmp) / "calls.log"
    record.write_text("")
    first = pids[0] if pids else "4242"
    d = proc / first
    d.mkdir(parents=True, exist_ok=True)
    (d / "cgroup").write_text(cgroup)
    (d / "cmdline").write_bytes(b"cardano-node\x00run\x00")
    binp.mkdir(parents=True, exist_ok=True)
    lines = "\n".join(pids)
    if pids:
        _exec(binp / "pgrep", f'#!/usr/bin/env bash\nprintf "%s\\n" "{lines}"\nexit 0\n')
    else:
        _exec(binp / "pgrep", "#!/usr/bin/env bash\nexit 1\n")
    for tool in ("systemctl", "docker", "podman", "pkill", "setsid", "cardano-node"):
        _exec(binp / tool, f'#!/usr/bin/env bash\necho "{tool} $*" >> "{record}"\nexit 0\n')
    _exec(binp / "sleep", "#!/usr/bin/env bash\nexit 0\n")  # no real settle wait
    return proc, binp, record


def run(snippet, proc, binp):
    env = dict(os.environ)
    env["OURO_PROC_ROOT"] = str(proc)
    env["PATH"] = f"{binp}:{env['PATH']}"
    env["OURO_MACHINE"] = "bp1"
    env["OURO_TOOL_NAME"] = "test/mode"  # ouro_emit_error needs a tool name
    return subprocess.run(["bash", "-c", f"source {LIB}\n{snippet}"], env=env,
                         text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main():
    # --- fail-closed guard MUST terminate the script, not just the subshell ---
    # detected=bare but declared=systemd => mismatch. The line after the guard must
    # NOT run, and no restart binary may be called.
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["bare"])
        r = run('MODE="$(ouro_node_effective_mode systemd)"\n'
                'ouro_node_guard_mode "$MODE"\n'
                'echo "REACHED-$MODE"', proc, binp)
        assert r.returncode == 40, (r.returncode, r.stdout, r.stderr)
        assert "REACHED" not in r.stdout, "guard did NOT terminate the script (subshell bypass!)"
        assert json.loads(r.stdout)["error"]["code"] == "runtime_mode_mismatch", r.stdout
        assert record.read_text() == "", "no restart binary may run on a mismatch"

    # --- ambiguous (two node processes) => exit 40, no action ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["bare"], pids=("4242", "4243"))
        r = run('MODE="$(ouro_node_effective_mode "")"\nouro_node_guard_mode "$MODE"\necho REACHED',
                proc, binp)
        assert r.returncode == 40 and "REACHED" not in r.stdout, r.stdout
        assert json.loads(r.stdout)["error"]["code"] == "runtime_mode_ambiguous", r.stdout

    # --- none (no running node) => exit 40 ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, "0::/\n", pids=())
        r = run('MODE="$(ouro_node_effective_mode "")"\nouro_node_guard_mode "$MODE"\necho REACHED',
                proc, binp)
        assert r.returncode == 40 and "REACHED" not in r.stdout, r.stdout
        assert json.loads(r.stdout)["error"]["code"] == "node_not_running", r.stdout

    # --- dispatch: systemd restarts the detected unit ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["systemd"])
        r = run('MODE="$(ouro_node_effective_mode "")"\nouro_node_guard_mode "$MODE"\n'
                'ouro_node_restart_mode "$MODE"', proc, binp)
        assert r.returncode == 0, (r.stdout, r.stderr)
        assert "systemctl restart cardano-node.service" in record.read_text(), record.read_text()

    # --- dispatch: docker restarts the detected container id (12-hex) ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["docker"])
        r = run('MODE="$(ouro_node_effective_mode "")"\nouro_node_guard_mode "$MODE"\n'
                'ouro_node_restart_mode "$MODE"', proc, binp)
        assert r.returncode == 0, (r.stdout, r.stderr)
        assert f"docker restart {CID64[:12]}" in record.read_text(), record.read_text()

    # --- dispatch: bare uses the host-process path, never systemctl/docker ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["bare"])
        r = run('MODE="$(ouro_node_effective_mode "")"\nouro_node_guard_mode "$MODE"\n'
                'ouro_node_restart_mode "$MODE"', proc, binp)
        assert r.returncode == 0, (r.stdout, r.stderr)
        log = record.read_text()
        assert "systemctl" not in log and "docker" not in log and "podman" not in log, log

    # --- declared matches detected => acts (no mismatch) ---
    with tempfile.TemporaryDirectory() as tmp:
        proc, binp, record = bed(tmp, CG["systemd"])
        r = run('MODE="$(ouro_node_effective_mode systemd)"\nouro_node_guard_mode "$MODE"\n'
                'ouro_node_restart_mode "$MODE"', proc, binp)
        assert r.returncode == 0 and "systemctl restart" in record.read_text(), r.stdout

    # --- consistency: ouro_node_detect_mode == detect/runtime probe's mode field ---
    for mode, cg in CG.items():
        with tempfile.TemporaryDirectory() as tmp:
            proc, binp, record = bed(tmp, cg)
            lib_mode = run("ouro_node_detect_mode", proc, binp).stdout.strip()
            env = dict(os.environ, OURO_PROC_ROOT=str(proc), OURO_MACHINE="bp1",
                       PATH=f"{binp}:{os.environ['PATH']}")
            probe_out = subprocess.run(["bash", str(PROBE)], env=env, text=True,
                                       stdout=subprocess.PIPE, check=True).stdout
            probe_mode = json.loads(probe_out)["data"]["mode"]
            assert lib_mode == probe_mode == mode, (mode, lib_mode, probe_mode)

    print("supervisor mode dispatch + fail-closed guard passed")


if __name__ == "__main__":
    main()
