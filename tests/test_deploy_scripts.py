#!/usr/bin/env python3
import json
import os
import subprocess
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())


def run_script(script, env, check=True):
    result = subprocess.run(
        ["bash", str(ROOT / "ouro-skills/deploy/scripts" / script)],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        check=check,
    )
    payload = json.loads(result.stdout)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return result, payload


def base_env():
    env = os.environ.copy()
    env.update({
        "OURO_AUDIT_ID": "audit-deploy-test",
        "OURO_TOOL_NAME": "deploy/test",
        "OURO_SPEC": str(ROOT / "examples/pool-spec.minimal.yaml"),
        "OURO_MACHINE": "bp1",
        "OURO_STATE_DIR": "/tmp/ouro-deploy-script-state",
        "OURO_STATUS_SNAPSHOT": str(ROOT / "tests/fixtures/deploy/verify-healthy.json"),
    })
    return env


def main():
    env = base_env()
    subprocess.run(["rm", "-rf", env["OURO_STATE_DIR"]], check=True)
    assert run_script("preflight.sh", env)[1]["changed"] is False

    first = run_script("provision.sh", env)[1]
    second = run_script("provision.sh", env)[1]
    assert first["changed"] is True
    assert second["changed"] is False

    assert run_script("sync.sh", env)[1]["changed"] is True
    assert run_script("start.sh", env)[1]["changed"] is True

    verify = run_script("verify.sh", env)[1]
    names = {check["name"] for check in verify["checks"]}
    assert "bp1.network_magic" in names
    assert "bp1.genesis_hash" in names
    assert "bp1.topology_p2p" in names
    assert "bp1.db_integrity" in names
    assert "bp1.kes_remaining" in names
    assert "pool.parameters" in names
    print("deploy scripts passed")


if __name__ == "__main__":
    main()
