#!/usr/bin/env python3
import json
import subprocess
from pathlib import Path

import jsonschema

from _ctx import ROOT, tool_run

SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())
HOME = "/tmp/ouro-takeover-home"
STATE_DIR = "/tmp/ouro-takeover-script-state"


def run(tool, manifest, check=True):
    result = tool_run(
        tool,
        machine="bp1",
        env={"OURO_STATE_DIR": STATE_DIR, "OURO_LEGACY_MANIFEST": str(ROOT / manifest)},
        home=HOME,
        check=check,
    )
    if result.stdout:
        payload = json.loads(result.stdout)
        jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return result


def main():
    subprocess.run(["rm", "-rf", STATE_DIR, HOME], check=True)
    good = "tests/fixtures/deploy/legacy-manifest.json"
    first = json.loads(run("deploy/takeover", good).stdout)
    second = json.loads(run("deploy/takeover", good).stdout)
    assert first["changed"] is True
    assert second["changed"] is False
    assert json.loads(run("deploy/takeover-verify", good).stdout)["status"] == "ok"

    subprocess.run(["rm", "-rf", STATE_DIR], check=True)
    failed = run("deploy/takeover", "tests/fixtures/deploy/legacy-manifest-bad.json", check=False)
    assert failed.returncode != 0
    assert not Path(STATE_DIR, "takeover-bp1").exists()
    print("takeover scripts passed")


if __name__ == "__main__":
    main()
