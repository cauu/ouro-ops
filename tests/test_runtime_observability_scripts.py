#!/usr/bin/env python3
import json
import subprocess

import jsonschema

from _ctx import ROOT, tool_run

SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())
HOME = "/tmp/ouro-runtime-home"


def run(tool, state, machine="bp1", check=True):
    result = tool_run(
        tool, machine=machine, env={"OURO_STATE_DIR": state}, home=HOME, check=check
    )
    payload = json.loads(result.stdout)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return result, payload


def main():
    subprocess.run(["rm", "-rf", HOME], check=True)
    runtime_state = "/tmp/ouro-runtime-script-state"
    subprocess.run(["rm", "-rf", runtime_state], check=True)
    first = run("runtime/topology-apply", runtime_state)[1]
    second = run("runtime/topology-apply", runtime_state)[1]
    assert first["changed"] is True
    assert second["changed"] is False
    assert run("runtime/verify", runtime_state)[1]["status"] == "ok"

    obs_state = "/tmp/ouro-observability-script-state"
    subprocess.run(["rm", "-rf", obs_state], check=True)
    assert run("observability/install-gateway", obs_state, machine="gateway")[1]["changed"] is True
    assert run("observability/verify", obs_state, machine="gateway")[1]["status"] == "ok"
    assert run("observability/rollback", obs_state, machine="gateway")[1]["changed"] is True
    failed = run("observability/verify", obs_state, machine="gateway", check=False)
    assert failed[0].returncode == 20
    print("runtime and observability scripts passed")


if __name__ == "__main__":
    main()
