#!/usr/bin/env python3
import json
import os
import subprocess
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())


def run(script, env, check=True):
    result = subprocess.run(["bash", str(ROOT / script)], env=env, text=True, stdout=subprocess.PIPE, check=check)
    payload = json.loads(result.stdout)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return result, payload


def env(state, tool, machine="bp1"):
    value = os.environ.copy()
    value.update({
        "OURO_AUDIT_ID": f"audit-{tool}",
        "OURO_TOOL_NAME": tool,
        "OURO_MACHINE": machine,
        "OURO_STATE_DIR": state,
    })
    return value


def main():
    runtime_state = "/tmp/ouro-runtime-script-state"
    subprocess.run(["rm", "-rf", runtime_state], check=True)
    runtime_env = env(runtime_state, "runtime/topology-apply")
    first = run("ouro-skills/runtime/scripts/topology-apply.sh", runtime_env)[1]
    second = run("ouro-skills/runtime/scripts/topology-apply.sh", runtime_env)[1]
    assert first["changed"] is True
    assert second["changed"] is False
    runtime_env["OURO_TOOL_NAME"] = "runtime/verify"
    assert run("ouro-skills/runtime/scripts/verify.sh", runtime_env)[1]["status"] == "ok"

    obs_state = "/tmp/ouro-observability-script-state"
    subprocess.run(["rm", "-rf", obs_state], check=True)
    obs_env = env(obs_state, "observability/install-gateway", "gateway")
    assert run("ouro-skills/observability/scripts/install-gateway.sh", obs_env)[1]["changed"] is True
    obs_env["OURO_TOOL_NAME"] = "observability/verify"
    assert run("ouro-skills/observability/scripts/verify.sh", obs_env)[1]["status"] == "ok"
    obs_env["OURO_TOOL_NAME"] = "observability/rollback"
    assert run("ouro-skills/observability/scripts/rollback.sh", obs_env)[1]["changed"] is True
    failed = run("ouro-skills/observability/scripts/verify.sh", obs_env, check=False)
    assert failed[0].returncode == 20
    print("runtime and observability scripts passed")


if __name__ == "__main__":
    main()
