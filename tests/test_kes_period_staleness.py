#!/usr/bin/env python3
"""S0017 p4-7 — deterministic staleness guard for kes-rotation/push-offline.

generate-offline stamps a freshness bundle (the period + the chain snapshot it was computed
against). Before installing, push-offline re-queries the chain and REFUSES a cert whose target
period has gone stale — a cert issued for a period too far in the past will not let the node forge,
so it must be caught BEFORE the disruptive install, not after.

This drives that guard in isolation: a bundle with an old period + a stubbed chain tip far ahead
=> push-offline must exit 30 (kes_period_stale) and leave the live opcert untouched (no install).
Fast, no docker. Standalone: `python3 tests/test_kes_period_staleness.py`.
"""
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "ouro-skills/kes-rotation/scripts/push-offline.sh"
ORIG_OPCERT = "ORIGINAL-OPCERT\n"


def _exec(p: Path, body: str):
    p.write_text(body)
    p.chmod(0o755)


def main():
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        pool = tmp / "devnet" / "pools-keys" / "pool1"
        stage = pool / "offline-stage"
        stage.mkdir(parents=True)
        (pool / "opcert.cert").write_text(ORIG_OPCERT)
        (pool / "kes.skey").write_text("LIVE-KES-SKEY\n")
        (stage / "kes.skey.staged").write_text("STAGED\n")
        (stage / "kes.vkey.staged").write_text("STAGEDV\n")
        (stage / "node.cert.signed").write_text("SIGNED\n")
        # bundle: issued for period 0; window 2. current tip (below) => period 100 => stale.
        json.dump({"period": 0, "tip_slot": 0, "slots_per_kes_period": 100,
                   "genesis_fingerprint": "x", "collected_at": "T", "max_age_periods": 2},
                  open(stage / "kes.bundle.json", "w"))

        binp = tmp / "bin"; binp.mkdir()
        _exec(binp / "cardano-cli", '''#!/usr/bin/env bash
if [ "$1" = query ] && [ "$2" = tip ]; then echo '{"slot": 10000, "block": 100}'; exit 0; fi
echo '{"qKesOnDiskOperationalCertificateNumber": 5}'; exit 0
''')
        _exec(binp / "ouro-ops", "#!/usr/bin/env bash\nexit 0\n")
        _exec(binp / "pgrep", "#!/usr/bin/env bash\necho 4242\n")

        env = dict(os.environ)
        env.update({
            "PATH": f"{binp}:{env['PATH']}",
            "OURO_DEVNET_DIR": str(tmp / "devnet"),
            "OURO_MACHINE": "bp1", "OURO_NETWORK_MAGIC": "1",
            "OURO_AUDIT_ID": "a", "OURO_TOOL_NAME": "kes-rotation/push-offline",
            "OURO_INVOCATION_TOKEN": "t", "OURO_BIN": str(binp / "ouro-ops"),
        })
        env.pop("OURO_SPEC", None)
        r = subprocess.run(["bash", str(SCRIPT)], env=env, text=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)

        failures = []
        if r.returncode != 30:
            failures.append(f"expected exit 30 (stale), got {r.returncode}; stderr={r.stderr[-300:]}")
        try:
            out = json.loads(r.stdout.strip().splitlines()[-1])
            if out.get("error", {}).get("code") != "kes_period_stale":
                failures.append(f"expected error.code=kes_period_stale, got {out.get('error')}")
        except Exception as e:
            failures.append(f"stdout not the expected JSON: {e}; stdout={r.stdout[-300:]}")
        # THE invariant: nothing was installed — the live opcert is untouched.
        if (pool / "opcert.cert").read_text() != ORIG_OPCERT:
            failures.append("opcert.cert was modified despite a stale-period refusal")

        if failures:
            print("FAIL — push-offline staleness guard:")
            for f in failures:
                print(f"  - {f}")
            raise SystemExit(1)
        print("PASS — push-offline staleness guard: stale-period cert refused before install (exit 30, opcert untouched)")


if __name__ == "__main__":
    main()
