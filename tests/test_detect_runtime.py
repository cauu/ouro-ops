#!/usr/bin/env python3
"""S0017 p2-1 / p2-2 / p2-7 — detect/runtime probe: mode detection + no-leak (TC-6).

The probe is a read-only, confined supervision-mode detector that emits a CLOSED,
typed projection. This test drives it with injected /proc + a stubbed PATH (fake
pgrep/docker/systemctl) so every supervision mode — and the fail-closed ambiguity
paths — can be exercised without a real container, AND asserts that secret-shaped
canaries planted in the probe's raw input sources (cgroup, cmdline, `docker inspect`
JSON) never surface in the projection. Standalone: `python3 tests/test_detect_runtime.py`.
"""
import json
import os
import stat
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "ouro-skills/detect/scripts/runtime.sh"

CID64 = "a" * 64          # deterministic 64-hex container id
LIBPOD64 = "b" * 64

# Secret-shaped canaries planted across the probe's raw input sources. The S0015 corpus
# canary is included so this test shares the "must never leak" token with E2E-9.
CANARIES = [
    "OURO-CANARY-SECRET-DO-NOT-LEAK-v1",              # S0015 fingerprint corpus canary
    "AKIAIOSFODNN7EXAMPLE",                            # AWS-style access key id
    "password=hunter2",                               # generic credential
    "ed25519_sk1qqqzzcanarycanarycanarycanary",       # bech32-shaped
    "-----BEGIN OPENSSH PRIVATE KEY-----",            # PEM-shaped
]


def _write_exec(path: Path, body: str):
    path.write_text(body)
    path.chmod(path.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)


def run_probe(tmp, *, cgroup, cmdline="", pids=("4242",), docker=None, systemctl=None):
    """Run the probe with an injected /proc and a stubbed PATH."""
    proc = Path(tmp) / "proc"
    binp = Path(tmp) / "bin"
    proc.mkdir(parents=True, exist_ok=True)
    binp.mkdir(parents=True, exist_ok=True)
    first_pid = pids[0] if pids else "4242"
    d = proc / first_pid
    d.mkdir(parents=True, exist_ok=True)
    (d / "cgroup").write_text(cgroup)
    (d / "cmdline").write_bytes(cmdline.encode())

    pid_lines = "\n".join(pids)
    _write_exec(binp / "pgrep", (
        "#!/usr/bin/env bash\n"
        'for a in "$@"; do case "$a" in *cardano-node*) '
        f'printf "%s\\n" "{pid_lines}"; exit 0;; esac; done\nexit 1\n'
    ))
    # Fake docker/podman: `inspect --format` projects ONE field (keyed off the format string:
    # image digest vs. a compose label); a RAW `inspect` returns a canary-laden blob — proving
    # the probe only ever calls the --format projection, never the raw inspect JSON.
    raw_leak = ('{"Env":["' + CANARIES[1] + '","' + CANARIES[2] + '"],'
                '"Mounts":"' + CANARIES[0] + '"}')
    default_rt = (
        "#!/usr/bin/env bash\n"
        'if [ "$1" = inspect ] && [ "$2" = --format ]; then\n'
        '  case "$3" in\n'
        '    *compose.project*) echo "ouro-pool" ;;\n'
        '    *compose.service*) echo "cardano-node" ;;\n'
        '    *.Image*) echo "sha256:cafe1234" ;;\n'
        '    *) echo "<no value>" ;;\n'
        '  esac\n'
        '  exit 0\n'
        'fi\n'
        f"echo '{raw_leak}'; exit 0\n"
    )
    _write_exec(binp / "docker", docker or default_rt)
    _write_exec(binp / "podman", docker or default_rt)
    _write_exec(binp / "systemctl", systemctl or "#!/usr/bin/env bash\necho '" + raw_leak + "'\n")

    env = dict(os.environ)
    env["OURO_PROC_ROOT"] = str(proc)
    env["OURO_MACHINE"] = "bp1"
    env["PATH"] = f"{binp}:{env['PATH']}"
    out = subprocess.run(["bash", str(PROBE)], env=env, text=True,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
    return out.stdout


def assert_no_leak(raw_output):
    for c in CANARIES:
        assert c not in raw_output, f"CANARY LEAKED into projection: {c!r}"


def main():
    cmdline = "\x00".join([
        "--config", "/opt/devnet/config.json", "--port", "3001",
        "--shelley-kes-key", "/opt/cardano/keys/kes.skey",
        # a secret-shaped canary sitting in argv, next to the (public) key PATH:
        CANARIES[3], CANARIES[4],
    ]) + "\x00"

    with tempfile.TemporaryDirectory() as tmp:
        # --- docker mode ---
        cg = f"0::/system.slice/docker-{CID64}.scope\n# {CANARIES[1]} {CANARIES[2]}\n"
        out = run_probe(tmp, cgroup=cg, cmdline=cmdline)
        data = json.loads(out)["data"]
        assert data["mode"] == "docker", data
        assert data["evidence"]["container_id"] == CID64[:12], data
        assert data["evidence"]["image_digest"] == "sha256:cafe1234", data
        # compose-managed node: which project/service owns it (single-label projections).
        assert data["evidence"]["compose"] == {"project": "ouro-pool", "service": "cardano-node"}, data
        assert data["port"] == 3001, data
        assert data["node_running"] is True and data["node_count"] == 1, data
        # p2-5b: a stable target fingerprint the confirm gate binds to.
        assert data["evidence_hash"].startswith("fp_") and len(data["evidence_hash"]) == 35, data
        assert_no_leak(out)
        first_hash = data["evidence_hash"]
        again = json.loads(run_probe(tmp, cgroup=cg, cmdline=cmdline))["data"]["evidence_hash"]
        assert again == first_hash, "evidence_hash must be stable for the same target"

    with tempfile.TemporaryDirectory() as tmp:
        # --- podman mode ---
        cg = f"0::/machine.slice/libpod-{LIBPOD64}.scope\n"
        out = run_probe(tmp, cgroup=cg, cmdline=cmdline)
        data = json.loads(out)["data"]
        assert data["mode"] == "podman", data
        assert data["evidence"]["container_id"] == LIBPOD64[:12], data
        assert_no_leak(out)

    with tempfile.TemporaryDirectory() as tmp:
        # --- systemd mode (unit slice, no container) ---
        cg = "0::/system.slice/cardano-node.service\n"
        out = run_probe(tmp, cgroup=cg, cmdline=cmdline)
        data = json.loads(out)["data"]
        assert data["mode"] == "systemd", data
        assert data["evidence"]["unit"] == "cardano-node.service", data
        assert data["evidence"]["container_id"] is None, data
        assert_no_leak(out)

    with tempfile.TemporaryDirectory() as tmp:
        # --- bare mode (running process, no unit, no container) ---
        cg = "0::/user.slice/user-1000.slice/session-3.scope\n"
        out = run_probe(tmp, cgroup=cg, cmdline=cmdline)
        data = json.loads(out)["data"]
        assert data["mode"] == "bare", data
        assert_no_leak(out)

    with tempfile.TemporaryDirectory() as tmp:
        # --- ambiguous: two matching node processes (same-host double node) => fail closed ---
        cg = "0::/user.slice/session-3.scope\n"
        out = run_probe(tmp, cgroup=cg, cmdline=cmdline, pids=("4242", "4243"))
        data = json.loads(out)["data"]
        assert data["mode"] == "ambiguous", data
        assert "multiple_node_processes" in data["conflict"], data
        assert data["node_count"] == 2, data
        assert_no_leak(out)

    with tempfile.TemporaryDirectory() as tmp:
        # --- none: no running node ---
        out = run_probe(tmp, cgroup="0::/\n", pids=())
        data = json.loads(out)["data"]
        assert data["mode"] == "none" and data["node_running"] is False, data

    print("detect/runtime probe: mode detection + no-leak passed")


if __name__ == "__main__":
    main()
