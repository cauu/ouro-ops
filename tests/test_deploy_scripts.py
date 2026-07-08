#!/usr/bin/env python3
import json
import subprocess
from pathlib import Path

import jsonschema

from _ctx import ROOT, tool_run

SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())
HOME = "/tmp/ouro-deploy-home"
SPEC = str(ROOT / "examples/pool-spec.minimal.yaml")
STATE_DIR = "/tmp/ouro-deploy-script-state"
CARDANO_ROOT = "/tmp/ouro-deploy-cardano"


def env_extra():
    return {
        "OURO_STATE_DIR": STATE_DIR,
        "OURO_STATUS_SNAPSHOT": str(ROOT / "tests/fixtures/deploy/verify-healthy.json"),
        "OURO_CARDANO_ROOT": CARDANO_ROOT,
    }


def run_deploy(script, check=True):
    result = tool_run(
        f"deploy/{script}", spec=SPEC, machine="bp1", env=env_extra(), home=HOME, check=check
    )
    payload = json.loads(result.stdout)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return result, payload


def main():
    subprocess.run(["rm", "-rf", STATE_DIR, HOME, CARDANO_ROOT], check=True)
    assert run_deploy("preflight")[1]["changed"] is False

    # provision does a REAL idempotent action: creates the node-host dir layout and
    # renders the node config; second run detects it and reports changed=false.
    first = run_deploy("provision")[1]
    second = run_deploy("provision")[1]
    assert first["changed"] is True
    assert second["changed"] is False
    # Real observable side effect (not a marker):
    assert Path(CARDANO_ROOT, "config", "bp1", "config.json").is_file()
    assert Path(CARDANO_ROOT, "db").is_dir()

    assert run_deploy("sync")[1]["changed"] is True
    assert run_deploy("start")[1]["changed"] is True

    verify = run_deploy("verify")[1]
    names = {check["name"] for check in verify["checks"]}
    expected = {
        "bp1.container_running",
        "bp1.restart_window",
        "bp1.node_version",
        "bp1.tip_lag",
        "bp1.metrics",
        "bp1.chrony",
        "bp1.network_magic",
        "bp1.genesis_hash",
        "bp1.topology_p2p",
        "bp1.db_integrity",
        "bp1.bp_port_private",
        "bp1.forging",
        "bp1.kes_remaining",
        "relay1.container_running",
        "relay1.topology_p2p",
        "relay1.db_integrity",
        "pool.id_query",
        "pool.parameters",
    }
    assert expected <= names
    assert "machine_inventory" in names
    for check in verify["checks"]:
        assert {"severity", "exit_class", "rollback_safe"} <= check.keys()

    # Machine-set integrity: dropping a spec-declared machine from the snapshot must
    # fail verify (it must not silently pass by only checking the machines present).
    healthy = json.loads((ROOT / "tests/fixtures/deploy/verify-healthy.json").read_text())
    healthy["machines"] = [m for m in healthy["machines"] if m["id"] != "relay1"]
    partial = Path("/tmp/ouro-deploy-missing-relay.json")
    partial.write_text(json.dumps(healthy))
    env = dict(env_extra())
    env["OURO_STATUS_SNAPSHOT"] = str(partial)
    result = tool_run("deploy/verify", spec=SPEC, machine="bp1", env=env, home=HOME, check=False)
    payload = json.loads(result.stdout)
    assert result.returncode != 0, result.stdout
    inventory = next(c for c in payload["checks"] if c["name"] == "machine_inventory")
    assert inventory["pass"] is False
    print("deploy scripts passed")


if __name__ == "__main__":
    main()
