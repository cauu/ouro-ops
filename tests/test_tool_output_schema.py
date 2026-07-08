#!/usr/bin/env python3
import json
import os
import subprocess
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT / "schemas" / "tool-output.schema.json").read_text())


def validate_output(text):
    payload = json.loads(text)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return payload


def main():
    marker = Path("/tmp/ouro-l2-idempotent-marker")
    marker.unlink(missing_ok=True)

    missing = subprocess.run(
        ["bash", str(ROOT / "tests/l2/idempotent-sample.sh"), str(marker)],
        text=True,
        stdout=subprocess.PIPE,
        check=False,
    )
    assert missing.returncode == 10
    assert validate_output(missing.stdout)["error"]["code"] == "missing_audit_context"

    env = os.environ.copy()
    env.update({
        "OURO_AUDIT_ID": "audit-test",
        "OURO_TOOL_NAME": "tests/idempotent-sample",
        "OURO_MACHINE": "bp1",
    })
    first = subprocess.run(
        ["bash", str(ROOT / "tests/l2/idempotent-sample.sh"), str(marker)],
        text=True,
        stdout=subprocess.PIPE,
        check=True,
        env=env,
    )
    assert validate_output(first.stdout)["changed"] is True

    second = subprocess.run(
        ["bash", str(ROOT / "tests/l2/idempotent-sample.sh"), str(marker)],
        text=True,
        stdout=subprocess.PIPE,
        check=True,
        env=env,
    )
    assert validate_output(second.stdout)["changed"] is False
    print("tool output schema and idempotency passed")


if __name__ == "__main__":
    main()
