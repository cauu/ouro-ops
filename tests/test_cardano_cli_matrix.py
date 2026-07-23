#!/usr/bin/env python3
"""The KES cold-signing script preserves cardano-cli era discipline.

The cold-sign / registration flows separate the cardano-cli VERSION from the ledger ERA:
  * KES opcert commands are era-NEUTRAL — `node issue-op-cert` / `node key-gen-KES` are NEVER
    prefixed with an era (the opcert format does not depend on the ledger era).
This gate freezes that discipline so a future edit cannot silently add an era to an opcert command.

    python3 tests/test_cardano_cli_matrix.py
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
KES_VKEY = ROOT / "tests/fixtures/kes/kes-vkey-public.json"

failures = []
def check(cond, msg):
    if not cond:
        failures.append(msg)


def ouro_bin():
    subprocess.run(["cargo", "build", "-q"], cwd=ROOT, check=True)
    return str(ROOT / "target" / "debug" / "ouro-ops")


# The era-neutral opcert commands: an era prefix in front of these is a bug.
ERA_NEUTRAL = ("node issue-op-cert", "node key-gen-KES")
ERAS = ("byron", "shelley", "allegra", "mary", "alonzo", "babbage", "conway")


def no_era_prefix(text, cmd):
    """Assert `cmd` never appears immediately after an era token (e.g. `conway node issue-op-cert`)."""
    for era in ERAS:
        if re.search(rf'\b{era}\s+{re.escape(cmd)}', text):
            return False
    return True


def main():
    binary = ouro_bin()

    # KES cold-sign script: era-neutral issue-op-cert.
    kes = subprocess.run([binary, "kes", "cold-sign-script", "--kes-vkey", str(KES_VKEY),
                          "--kes-period", "10"], cwd=ROOT, capture_output=True, text=True).stdout
    check("node issue-op-cert" in kes, "kes cold-sign must call `node issue-op-cert`")
    for cmd in ERA_NEUTRAL:
        check(no_era_prefix(kes, cmd), f"kes cold-sign wrongly era-prefixes `{cmd}`")

    if failures:
        print("FAIL — cardano-cli era/version gate:")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("PASS — cardano-cli era gate: opcert commands remain era-neutral")


if __name__ == "__main__":
    main()
