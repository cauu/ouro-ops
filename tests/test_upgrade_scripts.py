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
ONE_RELAY_DOWNTIME = str(ROOT / "tests/fixtures/pool-spec/valid-single-relay-downtime.yaml")


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
    # Failure injection is via an allowlisted state marker (env hooks are stripped).
    failing = "/tmp/ouro-upgrade-script-failing"
    subprocess.run(["rm", "-rf", failing], check=True)
    Path(failing).mkdir(parents=True)
    Path(failing, "__test_inject_fail__relay1").write_text("")
    failed = run(TWO_RELAY, failing, check=False)
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

    # An agent CANNOT loosen the invariant via caller env: OURO_QUORUM_MIN_RELAYS is
    # stripped by the tool-run env allowlist, so the single-relay upgrade still fails.
    env_bypass = "/tmp/ouro-upgrade-script-envbypass"
    subprocess.run(["rm", "-rf", env_bypass], check=True)
    blocked = run(ONE_RELAY, env_bypass, extra={"OURO_QUORUM_MIN_RELAYS": "0"}, check=False)
    assert blocked.returncode == 10, blocked.stdout
    assert validate(blocked.stdout)["error"]["code"] == "relay_quorum_violation"

    # The operator CAN accept single-relay downtime — but only via the human-authored
    # spec policy (upgrade.min_online_relays: 0), not via environment.
    override = "/tmp/ouro-upgrade-script-override"
    subprocess.run(["rm", "-rf", override], check=True)
    ok = validate(run(ONE_RELAY_DOWNTIME, override).stdout)
    assert ok["data"]["completed"] == ["relay1", "bp1"]
    print("upgrade scripts passed")


if __name__ == "__main__":
    main()
