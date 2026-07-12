#!/usr/bin/env python3
"""S0017 p4-5 — cardano-cli version pin + era-discipline golden gate.

The cold-sign / registration flows separate the cardano-cli VERSION from the ledger ERA:
  * KES opcert commands are era-NEUTRAL — `node issue-op-cert` / `node key-gen-KES` are NEVER
    prefixed with an era (the opcert format does not depend on the ledger era).
  * deploy transaction commands are era-SCOPED — `<era> transaction witness/build/...` and the
    certificate builders always carry the era (the tx/cert format DOES depend on it).

This gate freezes that discipline so a future edit cannot silently add an era to an opcert command
(or drop it from a tx command) and desync from the validated cardano-cli line. It also asserts the
read-only version probe pins a supported floor + a validated reference version. Fast, standalone:

    python3 tests/test_cardano_cli_matrix.py
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SK = ROOT / "ouro-skills"
KES_VKEY = ROOT / "tests/fixtures/kes/kes-vkey-public.json"
TXBODY = ROOT / "tests/fixtures/deploy/tx-body-unsigned.json"

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

    # 1. version probe pins a supported floor + a validated reference version.
    probe = (SK / "detect/scripts/cardano-cli.sh").read_text()
    check("SUPPORTED_MAJOR_MIN=" in probe, "detect/cardano-cli must pin SUPPORTED_MAJOR_MIN")
    check(re.search(r'VALIDATED_VERSION="\d+\.\d+\.\d+', probe) is not None,
          "detect/cardano-cli must record a VALIDATED_VERSION")

    # 2. KES cold-sign script: era-NEUTRAL issue-op-cert.
    kes = subprocess.run([binary, "kes", "cold-sign-script", "--kes-vkey", str(KES_VKEY),
                          "--kes-period", "10"], cwd=ROOT, capture_output=True, text=True).stdout
    check("node issue-op-cert" in kes, "kes cold-sign must call `node issue-op-cert`")
    for cmd in ERA_NEUTRAL:
        check(no_era_prefix(kes, cmd), f"kes cold-sign wrongly era-prefixes `{cmd}`")

    # 3. deploy cold-sign script: era-SCOPED transaction witness.
    dep = subprocess.run([binary, "deploy", "cold-sign-script", "--tx-body", str(TXBODY),
                          "--cold-key", "cold", "--testnet-magic", "1"],
                         cwd=ROOT, capture_output=True, text=True).stdout
    check(re.search(r'\bconway\s+transaction\s+witness', dep) is not None,
          "deploy cold-sign must call era-scoped `<era> transaction witness`")
    check(re.search(r'\btransaction\s+witness', dep) and "conway transaction witness" in dep,
          "deploy cold-sign transaction witness must be era-scoped")

    # 4. committed KES L2 scripts keep the opcert commands era-neutral.
    for rel in ("kes-rotation/scripts/rotate.sh",
                "kes-rotation/scripts/generate-offline.sh",
                "kes-rotation/scripts/push-offline.sh"):
        t = (SK / rel).read_text()
        for cmd in ERA_NEUTRAL:
            check(no_era_prefix(t, cmd), f"{rel} wrongly era-prefixes `{cmd}`")

    # 5. the registration builder keeps tx/cert commands era-scoped (conway).
    rb = (SK / "deploy/scripts/register-build.sh").read_text()
    check('CLI=(cardano-cli conway)' in rb, "register-build must pin the era in its cardano-cli prefix")
    check("transaction build" in rb and "registration-certificate" in rb,
          "register-build must build the tx + registration certs")

    if failures:
        print("FAIL — cardano-cli era/version gate:")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("PASS — cardano-cli era/version gate: opcert era-neutral, tx era-scoped, version pinned")


if __name__ == "__main__":
    main()
