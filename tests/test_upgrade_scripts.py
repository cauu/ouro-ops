#!/usr/bin/env python3
import json
import os
import subprocess
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())
RUN = ROOT / "ouro-skills/upgrade/scripts/run.sh"


def env_for(state):
    env = os.environ.copy()
    env.update({
        "OURO_AUDIT_ID": "audit-upgrade-test",
        "OURO_TOOL_NAME": "upgrade/run",
        "OURO_SPEC": str(ROOT / "examples/pool-spec.minimal.yaml"),
        "OURO_STATE_DIR": state,
    })
    return env


def validate(text):
    payload = json.loads(text)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return payload


def main():
    state = "/tmp/ouro-upgrade-script-state"
    subprocess.run(["rm", "-rf", state], check=True)
    success = subprocess.run(["bash", str(RUN)], env=env_for(state), text=True, stdout=subprocess.PIPE, check=True)
    payload = validate(success.stdout)
    assert payload["data"]["completed"] == ["relay1", "bp1"]

    locked = "/tmp/ouro-upgrade-script-locked"
    subprocess.run(["rm", "-rf", locked], check=True)
    Path(locked).mkdir(parents=True)
    Path(locked, "upgrade.lock").write_text("held")
    lock_result = subprocess.run(["bash", str(RUN)], env=env_for(locked), text=True, stdout=subprocess.PIPE, check=False)
    assert lock_result.returncode == 10
    assert validate(lock_result.stdout)["error"]["code"] == "upgrade_lock_held"

    failing = "/tmp/ouro-upgrade-script-failing"
    subprocess.run(["rm", "-rf", failing], check=True)
    fail_env = env_for(failing)
    fail_env["OURO_FAIL_MACHINE"] = "relay1"
    failed = subprocess.run(["bash", str(RUN)], env=fail_env, text=True, stdout=subprocess.PIPE, check=False)
    assert failed.returncode == 30
    assert validate(failed.stdout)["error"]["code"] == "upgrade_verify_failed"
    assert not Path(failing, "upgraded-bp1").exists()
    assert Path(failing, "rollback-relay1").exists()
    print("upgrade scripts passed")


if __name__ == "__main__":
    main()
