#!/usr/bin/env python3
"""S0017 p4-6 — deterministic ROLLBACK test for kes-rotation/push-offline.

The happy path (a real rotation) is proven by make e2e-t2-offline-rotation. This test drives the
FAILURE path in isolation: when the node does not come up correctly after installing the new
opcert, push-offline must restore the PREVIOUS opcert + KES signing key and restart onto them,
so a bad hand-off never leaves the BP on a half-installed, non-forging key.

It runs the whole script directly (bypassing the audit gate via a stub $OURO_BIN) with a stubbed
cardano-cli / process table. The rollback is triggered by a restart that does not change the node
PID (a legitimate "restart failed" signal push-offline guards against) — the cheapest deterministic
trigger that still exercises rollback_and_die's file restoration + restart + error emission.

Standalone: `python3 tests/test_kes_offline_rollback.py`.
"""
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "ouro-skills/kes-rotation/scripts/push-offline.sh"
PID = "4242"

ORIG_OPCERT = "ORIGINAL-OPCERT-CONTENT\n"
ORIG_KES_SKEY = "ORIGINAL-KES-SKEY-CONTENT\n"
ORIG_KES_VKEY = "ORIGINAL-KES-VKEY-CONTENT\n"


def _exec(p: Path, body: str):
    p.write_text(body)
    p.chmod(0o755)


def setup(tmp: Path):
    devnet = tmp / "devnet"
    pool = devnet / "pools-keys" / "pool1"
    stage = pool / "offline-stage"
    stage.mkdir(parents=True)
    # the live pair (what rollback must restore) + the staged pair + the returned signed cert.
    (pool / "cold.skey").write_text("COLD-KEY\n")
    (pool / "opcert.cert").write_text(ORIG_OPCERT)
    (pool / "kes.skey").write_text(ORIG_KES_SKEY)
    (pool / "kes.vkey").write_text(ORIG_KES_VKEY)
    (stage / "kes.skey.staged").write_text("STAGED-KES-SKEY\n")
    (stage / "kes.vkey.staged").write_text("STAGED-KES-VKEY\n")
    (stage / "node.cert.signed").write_text("COLD-SIGNED-OPCERT\n")

    # injected /proc: a single node pid in a BARE cgroup → detect resolves 'bare'.
    proc = tmp / "proc"
    d = proc / PID
    d.mkdir(parents=True)
    (d / "cgroup").write_text("0::/user.slice/user-1000.slice/session-3.scope\n")
    (d / "cmdline").write_bytes(b"cardano-node\x00run\x00")

    binp = tmp / "bin"
    binp.mkdir()
    # cardano-cli: kes-period-info → a constant on-disk counter; tip → a fixed block. Enough for
    # the pre-install ground-truth reads; the PID-unchanged check fires before the forge loop.
    _exec(binp / "cardano-cli", r'''#!/usr/bin/env bash
if [ "$1" = query ] && [ "$2" = kes-period-info ]; then
  echo "✓ current KES period is in the operational certificate's valid range"
  echo '{"qKesOnDiskOperationalCertificateNumber": 5}'
  exit 0
fi
if [ "$1" = query ] && [ "$2" = tip ]; then
  echo '{"slot": 1000, "block": 100}'
  exit 0
fi
exit 0
''')
    # static pgrep → the node PID never changes across the restart → rollback trigger.
    _exec(binp / "pgrep", f'#!/usr/bin/env bash\nprintf "%s\\n" "{PID}"\nexit 0\n')
    for tool in ("pkill", "setsid", "cardano-node", "systemctl", "docker", "podman"):
        _exec(binp / tool, f'#!/usr/bin/env bash\nexit 0\n')
    _exec(binp / "sleep", "#!/usr/bin/env bash\nexit 0\n")  # no real settle wait
    # stub ouro-ops: only `tool verify-context` is called by the audit gate → succeed.
    _exec(binp / "ouro-ops", '#!/usr/bin/env bash\nexit 0\n')
    return devnet, pool, proc, binp


def main():
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        devnet, pool, proc, binp = setup(tmp)
        env = dict(os.environ)
        env.update({
            "PATH": f"{binp}:{env['PATH']}",
            "OURO_PROC_ROOT": str(proc),
            "OURO_DEVNET_DIR": str(devnet),
            "OURO_MACHINE": "bp1",
            "OURO_NETWORK_MAGIC": "1",
            "OURO_AUDIT_ID": "test-audit",
            "OURO_TOOL_NAME": "kes-rotation/push-offline",
            "OURO_INVOCATION_TOKEN": "test-token",
            "OURO_BIN": str(binp / "ouro-ops"),
        })
        # OURO_SPEC intentionally unset → declared mode empty → effective mode = detected (bare).
        env.pop("OURO_SPEC", None)
        r = subprocess.run(["bash", str(SCRIPT)], env=env, text=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)

        failures = []
        if r.returncode != 30:
            failures.append(f"expected exit 30 (rolled back), got {r.returncode}; stderr={r.stderr[-400:]}")
        # the emitted JSON must report an error, rolled-back.
        try:
            out = json.loads(r.stdout.strip().splitlines()[-1])
            if out.get("status") != "error":
                failures.append(f"expected status=error, got {out.get('status')}")
            if out.get("error", {}).get("code") != "kes_push_rolled_back":
                failures.append(f"expected error.code=kes_push_rolled_back, got {out.get('error')}")
        except Exception as e:
            failures.append(f"stdout not the expected single-line JSON: {e}; stdout={r.stdout[-400:]}")
        # THE invariant: the previous live pair is restored (not left on the staged/half-installed one).
        if (pool / "opcert.cert").read_text() != ORIG_OPCERT:
            failures.append(f"opcert.cert NOT restored: {(pool/'opcert.cert').read_text()!r}")
        if (pool / "kes.skey").read_text() != ORIG_KES_SKEY:
            failures.append(f"kes.skey NOT restored: {(pool/'kes.skey').read_text()!r}")

        if failures:
            print("FAIL — push-offline rollback:")
            for f in failures:
                print(f"  - {f}")
            raise SystemExit(1)
        print("PASS — push-offline rollback: previous opcert + KES skey restored on failed install (exit 30)")


if __name__ == "__main__":
    main()
