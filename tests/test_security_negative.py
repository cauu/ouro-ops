#!/usr/bin/env python3
import json
import os
import subprocess
from pathlib import Path

from _ctx import ROOT


def ouro(args, env, check=False):
    return subprocess.run(
        ["cargo", "run", "-q", "--", *args],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        check=check,
    )


def env_with_home(home):
    subprocess.run(["rm", "-rf", home], check=True)
    env = os.environ.copy()
    env["OURO_HOME"] = home
    return env


def main():
    # 1) Direct L2 script call with no audit context is refused.
    direct = subprocess.run(
        ["bash", str(ROOT / "ouro-skills/deploy/scripts/provision.sh")],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        check=False,
    )
    assert direct.returncode == 10
    assert json.loads(direct.stdout)["error"]["code"] == "missing_audit_context"

    push_args = [
        "kes", "push",
        "--spec", "examples/pool-spec.minimal.yaml",
        "--machine", "bp1",
        "--cert", "tests/fixtures/kes/node-cert-valid.json",
        "--counter-state", "tests/fixtures/kes/counter-state.json",
    ]

    # 2) The forgeable static `confirm:<action>:<machine>` token is REJECTED on the
    #    production path (S0014 P0 fix) — an agent cannot self-issue confirmation.
    static = ouro([*push_args, "--confirm-token", "confirm:kes-push:bp1"], env_with_home("/tmp/ouro-sec-static"))
    assert static.returncode == 10, static.stdout
    assert json.loads(static.stdout)["status"] == "error"
    assert "confirm:kes-push:bp1" not in static.stdout

    # 3) A real token bound to a DIFFERENT action does not satisfy kes-push.
    mismatch_env = env_with_home("/tmp/ouro-security-mismatch")
    created = ouro(
        ["confirm", "create", "--action", "rollback", "--machine", "bp1", "--ttl", "60s"],
        mismatch_env,
        check=True,
    )
    token = json.loads(created.stdout)["data"]["token"]
    refused = ouro([*push_args, "--confirm-token", token], mismatch_env)
    assert refused.returncode == 10
    assert token not in refused.stdout

    # 4) With a valid token, a cert carrying a stray secret field is rejected before push.
    secret_env = env_with_home("/tmp/ouro-security-secret")
    good = ouro(
        ["confirm", "create", "--action", "kes-push", "--machine", "bp1", "--ttl", "60s"],
        secret_env,
        check=True,
    )
    good_token = json.loads(good.stdout)["data"]["token"]
    secret_cert = ouro(
        [
            "kes", "push",
            "--spec", "examples/pool-spec.minimal.yaml",
            "--machine", "bp1",
            "--cert", "tests/fixtures/kes/node-cert-with-secret.json",
            "--counter-state", "tests/fixtures/kes/counter-state.json",
            "--confirm-token", good_token,
        ],
        secret_env,
    )
    assert secret_cert.returncode != 0
    assert "unknown field" in secret_cert.stdout
    print("security negative tests passed")


if __name__ == "__main__":
    main()
