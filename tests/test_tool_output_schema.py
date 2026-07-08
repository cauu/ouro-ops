#!/usr/bin/env python3
import json
import subprocess
from pathlib import Path

import jsonschema

from _ctx import ROOT, ouro_bin, tool_run

SCHEMA = json.loads((ROOT / "schemas" / "tool-output.schema.json").read_text())
LIB = ROOT / "ouro-skills/lib/ouro-lib.sh"
SAMPLE = ROOT / "tests/l2/idempotent-sample.sh"


def validate_output(text):
    payload = json.loads(text)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return payload


def main():
    marker = Path("/tmp/ouro-l2-idempotent-marker")
    marker.unlink(missing_ok=True)

    # Direct call with no audit context is refused (exit 10).
    missing = subprocess.run(
        ["bash", str(SAMPLE), str(marker)], text=True, stdout=subprocess.PIPE, check=False
    )
    assert missing.returncode == 10
    assert validate_output(missing.stdout)["error"]["code"] == "missing_audit_context"

    # A FORGED context (env vars set, but no CLI-signed token) is also refused — the
    # gate is mechanism-level, not "env present". This is the S0014 P0/P1 fix.
    forged = subprocess.run(
        ["bash", str(SAMPLE), str(marker)],
        text=True,
        stdout=subprocess.PIPE,
        check=False,
        env={
            "PATH": subprocess.os.environ["PATH"],
            "OURO_HOME": "/tmp/ouro-forge-home",
            "OURO_AUDIT_ID": "fabricated",
            "OURO_TOOL_NAME": "deploy/provision",
            "OURO_INVOCATION_TOKEN": "inv_bogus",
            "OURO_BIN": ouro_bin(),
        },
    )
    assert forged.returncode == 10, forged.stdout
    assert validate_output(forged.stdout)["error"]["code"] == "invalid_audit_context"

    # Idempotency + schema through the audited path: changed=true then changed=false.
    subprocess.run(["rm", "-rf", "/tmp/ouro-idem-home", "/tmp/ouro-idem-state"], check=True)
    first = tool_run(
        "runtime/topology-apply",
        machine="bp1",
        env={"OURO_STATE_DIR": "/tmp/ouro-idem-state"},
        home="/tmp/ouro-idem-home",
    )
    assert validate_output(first.stdout)["changed"] is True
    second = tool_run(
        "runtime/topology-apply",
        machine="bp1",
        env={"OURO_STATE_DIR": "/tmp/ouro-idem-state"},
        home="/tmp/ouro-idem-home",
    )
    assert validate_output(second.stdout)["changed"] is False

    # Exit class 40 (unknown state) is a real, schema-valid emission.
    unknown = subprocess.run(
        ["bash", "-c", f"source '{LIB}'; ouro_emit_unknown ledger_unknown 'cannot determine state'"],
        text=True,
        stdout=subprocess.PIPE,
        check=False,
    )
    assert unknown.returncode == 40
    assert validate_output(unknown.stdout)["error"]["code"] == "ledger_unknown"

    print("tool output schema and idempotency passed")


if __name__ == "__main__":
    main()
