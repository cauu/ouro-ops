#!/usr/bin/env python3
"""S0017 p5-3 — node filesystem layout discovery from the running node's argv.

The ops scripts must find the node's real socket / pool-keys / genesis WITHOUT hand-declared paths:
they read them from the running `cardano-node run` process's own command line (any layout, zero
config), falling back to the /opt/devnet bed layout only when no node is running. This drives the
adapter's discovery helpers with an injected /proc + a stubbed pgrep.

Standalone: `python3 tests/test_node_layout_discovery.py`.
"""
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / "ouro-skills/lib/ouro-lib.sh"
PID = "4242"


def _exec(p: Path, body: str):
    p.write_text(body)
    p.chmod(0o755)


def bed(tmp: Path, running=True):
    proc, binp = tmp / "proc", tmp / "bin"
    binp.mkdir(parents=True)
    if running:
        d = proc / PID
        d.mkdir(parents=True)
        cfg = tmp / "node" / "config.json"
        cfg.parent.mkdir(parents=True)
        # relative genesis path → resolved against the config dir.
        cfg.write_text(json.dumps({"ShelleyGenesisFile": "genesis/shelley.json"}))
        argv = ["cardano-node", "run",
                "--config", str(cfg),
                "--topology", str(tmp / "node/topology.json"),
                "--database-path", str(tmp / "node/db"),
                "--socket-path", "/run/cardano/node.socket",
                "--shelley-kes-key", "/keys/pool/kes.skey",
                "--shelley-vrf-key", "/keys/pool/vrf.skey"]
        (d / "cmdline").write_bytes(b"\x00".join(a.encode() for a in argv) + b"\x00")
        _exec(binp / "pgrep", f'#!/usr/bin/env bash\nprintf "%s\\n" "{PID}"\nexit 0\n')
        return proc, binp, str(cfg.parent / "genesis/shelley.json")
    else:
        proc.mkdir(parents=True)
        _exec(binp / "pgrep", "#!/usr/bin/env bash\nexit 1\n")  # no node running
        return proc, binp, None


def run(expr, proc, binp):
    env = dict(os.environ)
    env["OURO_PROC_ROOT"] = str(proc)
    pydir = os.path.dirname(subprocess.run(["bash", "-lc", "command -v python3"], capture_output=True, text=True).stdout.strip())
    env["PATH"] = f"{binp}:{pydir}:/usr/bin:/bin"
    r = subprocess.run(["bash", "-c", f"source {LIB}\nprintf '%s' \"$({expr})\""],
                       env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return r.stdout.strip()


def main():
    failures = []
    def check(got, want, label):
        if got != want:
            failures.append(f"{label}: got {got!r}, want {want!r}")

    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        proc, binp, genesis = bed(tmp, running=True)
        # discovered from the live node's argv — NOT /opt/devnet.
        check(run("ouro_node_socket", proc, binp), "/run/cardano/node.socket", "socket discovery")
        check(run("ouro_node_pool_dir", proc, binp), "/keys/pool", "pool-dir discovery")
        check(run("ouro_node_genesis_shelley", proc, binp), genesis, "genesis discovery (from config)")

    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        proc, binp, _ = bed(tmp, running=False)
        # no running node → fall back to the /opt/devnet bed layout (keeps the E2E bed green).
        check(run("ouro_node_socket", proc, binp), "/opt/devnet/node.socket", "socket fallback")
        check(run("ouro_node_pool_dir", proc, binp), "/opt/devnet/pools-keys/pool1", "pool-dir fallback")

    if failures:
        print("FAIL — node layout discovery:")
        for f in failures:
            print(f"  - {f}")
        raise SystemExit(1)
    print("PASS — node layout discovery: socket/pool/genesis discovered from the live node argv; /opt/devnet fallback")


if __name__ == "__main__":
    main()
