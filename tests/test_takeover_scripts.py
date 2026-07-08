#!/usr/bin/env python3
import json
import os
import subprocess
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())


def run(script, env, check=True):
    result = subprocess.run(["bash", str(ROOT / "ouro-skills/deploy/scripts" / script)], env=env, text=True, stdout=subprocess.PIPE, check=check)
    if result.stdout:
        payload = json.loads(result.stdout)
        jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return result


def env_for(manifest):
    env = os.environ.copy()
    env.update({
        "OURO_AUDIT_ID": "audit-takeover-test",
        "OURO_TOOL_NAME": "deploy/takeover",
        "OURO_MACHINE": "bp1",
        "OURO_STATE_DIR": "/tmp/ouro-takeover-script-state",
        "OURO_LEGACY_MANIFEST": str(ROOT / manifest),
    })
    return env


def main():
    subprocess.run(["rm", "-rf", "/tmp/ouro-takeover-script-state"], check=True)
    good = env_for("tests/fixtures/deploy/legacy-manifest.json")
    first = json.loads(run("takeover.sh", good).stdout)
    second = json.loads(run("takeover.sh", good).stdout)
    assert first["changed"] is True
    assert second["changed"] is False
    good["OURO_TOOL_NAME"] = "deploy/takeover-verify"
    assert json.loads(run("takeover-verify.sh", good).stdout)["status"] == "ok"

    subprocess.run(["rm", "-rf", "/tmp/ouro-takeover-script-state"], check=True)
    bad = env_for("tests/fixtures/deploy/legacy-manifest-bad.json")
    failed = run("takeover.sh", bad, check=False)
    assert failed.returncode != 0
    assert not Path("/tmp/ouro-takeover-script-state/takeover-bp1").exists()
    print("takeover scripts passed")


if __name__ == "__main__":
    main()
