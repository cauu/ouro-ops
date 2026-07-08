#!/usr/bin/env python3
import json
import subprocess
from pathlib import Path

import jsonschema

from _ctx import ROOT, tool_run

SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())
HOME = "/tmp/ouro-upgrade-home"
TWO_RELAY = str(ROOT / "tests/fixtures/pool-spec/valid-two-relay.yaml")
ONE_RELAY = str(ROOT / "examples/pool-spec.minimal.yaml")


def run(spec, state, extra=None, check=True):
    env = {"OURO_STATE_DIR": state}
    if extra:
        env.update(extra)
    return tool_run("upgrade/run", spec=spec, env=env, home=HOME, check=check)


def validate(text):
    payload = json.loads(text)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return payload


def main():
    subprocess.run(["rm", "-rf", HOME], check=True)

    # Happy path: two relays upgrade before BP; each relay upgrade keeps quorum.
    state = "/tmp/ouro-upgrade-script-state"
    subprocess.run(["rm", "-rf", state], check=True)
    payload = validate(run(TWO_RELAY, state).stdout)
    assert payload["data"]["completed"] == ["relay1", "relay2", "bp1"]

    # Lock contention: a pre-existing lock blocks a second batch (exit 10).
    locked = "/tmp/ouro-upgrade-script-locked"
    subprocess.run(["rm", "-rf", locked], check=True)
    Path(locked).mkdir(parents=True)
    Path(locked, "upgrade.lock").write_text("held")
    lock_result = run(TWO_RELAY, locked, check=False)
    assert lock_result.returncode == 10
    assert validate(lock_result.stdout)["error"]["code"] == "upgrade_lock_held"

    # Verify failure stops the batch before BP and rolls back the failed machine.
    failing = "/tmp/ouro-upgrade-script-failing"
    subprocess.run(["rm", "-rf", failing], check=True)
    failed = run(TWO_RELAY, failing, extra={"OURO_FAIL_MACHINE": "relay1"}, check=False)
    assert failed.returncode == 30
    assert validate(failed.stdout)["error"]["code"] == "upgrade_verify_failed"
    assert not Path(failing, "upgraded-bp1").exists()
    assert Path(failing, "rollback-relay1").exists()

    # Quorum invariant: a single-relay topology cannot be rolling-upgraded because
    # taking the only relay down would break "BP + >=1 relay online" (exit 10).
    single = "/tmp/ouro-upgrade-script-single"
    subprocess.run(["rm", "-rf", single], check=True)
    quorum = run(ONE_RELAY, single, check=False)
    assert quorum.returncode == 10, quorum.stdout
    assert validate(quorum.stdout)["error"]["code"] == "relay_quorum_violation"
    assert not Path(single, "upgraded-relay1").exists()

    # Operator can explicitly accept single-relay downtime via the quorum override.
    override = "/tmp/ouro-upgrade-script-override"
    subprocess.run(["rm", "-rf", override], check=True)
    ok = validate(run(ONE_RELAY, override, extra={"OURO_QUORUM_MIN_RELAYS": "0"}).stdout)
    assert ok["data"]["completed"] == ["relay1", "bp1"]
    print("upgrade scripts passed")


if __name__ == "__main__":
    main()
