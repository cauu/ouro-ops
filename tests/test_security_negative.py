#!/usr/bin/env python3
import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def main():
    direct = subprocess.run(
        ["bash", str(ROOT / "ouro-skills/deploy/scripts/provision.sh")],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        check=False,
    )
    assert direct.returncode == 10
    assert json.loads(direct.stdout)["error"]["code"] == "missing_audit_context"

    mismatch_home = "/tmp/ouro-security-mismatch"
    subprocess.run(["rm", "-rf", mismatch_home], check=True)
    env = os.environ.copy()
    env["OURO_HOME"] = mismatch_home
    created = subprocess.run(
        ["cargo", "run", "-q", "--", "confirm", "create", "--action", "rollback", "--machine", "bp1", "--ttl", "60s"],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        check=True,
    )
    token = json.loads(created.stdout)["data"]["token"]
    refused = subprocess.run(
        [
            "cargo", "run", "-q", "--", "kes", "push",
            "--spec", "examples/pool-spec.minimal.yaml",
            "--machine", "bp1",
            "--cert", "tests/fixtures/kes/node-cert-valid.json",
            "--counter-state", "tests/fixtures/kes/counter-state.json",
            "--confirm-token", token,
        ],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        check=False,
    )
    assert refused.returncode == 10
    assert token not in refused.stdout

    secret_cert = subprocess.run(
        [
            "cargo", "run", "-q", "--", "kes", "push",
            "--spec", "examples/pool-spec.minimal.yaml",
            "--machine", "bp1",
            "--cert", "tests/fixtures/kes/node-cert-with-secret.json",
            "--counter-state", "tests/fixtures/kes/counter-state.json",
            "--confirm-token", "confirm:kes-push:bp1",
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        check=False,
    )
    assert secret_cert.returncode != 0
    assert "unknown field" in secret_cert.stdout
    print("security negative tests passed")


if __name__ == "__main__":
    main()
