#!/usr/bin/env python3
"""S0027 TC-1: legacy Deploy surfaces stay absent."""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
LEGACY_OPERATIONS = (
    "deploy/preflight",
    "deploy/status",
    "deploy/register-build",
    "deploy/register-submit",
    "deploy/provision",
    "deploy/sync",
    "deploy/start",
    "deploy/takeover",
)


def invoke(binary: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(binary), *args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def test_legacy_deploy_namespace_and_operations_are_absent() -> None:
    subprocess.run(["cargo", "build", "-q", "-p", "ouro"], cwd=ROOT, check=True)
    binary = ROOT / "target" / "debug" / "ouro-ops"

    help_text = invoke(binary, "help").stdout
    assert "deploy cold-sign-script" not in help_text

    retired_command = invoke(binary, "deploy", "cold-sign-script")
    assert retired_command.returncode != 0
    command_error = json.loads(retired_command.stdout)
    assert command_error["status"] == "error"
    assert "unknown command deploy" in command_error["error"]["detail"]

    for operation in LEGACY_OPERATIONS:
        retired_operation = invoke(
            binary,
            "op",
            "run",
            "--op",
            operation,
            "--node",
            "bp1",
            "--param",
            "machine=bp1",
            "--observation",
            "{}",
            "--plan",
        )
        assert retired_operation.returncode != 0
        operation_error = json.loads(retired_operation.stdout)
        detail = operation_error["error"]["detail"]
        assert (
            "typed registry" in detail
            or "disabled" in detail
            or "unknown write operation" in detail
        )
