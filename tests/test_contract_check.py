#!/usr/bin/env python3
"""S0025 p3-1: external Skill compatibility is the first, pure action."""

import json
import os
import pathlib
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "ouro-ops"
SKILLS = (
    "observability",
    "troubleshooting",
    "runtime",
    "upgrade",
    "kes-rotation",
    "deploy",
)


def run_check(temp: pathlib.Path, ouro: str, contract: str):
    env = os.environ.copy()
    for name in ("HOME", "OURO_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"):
        path = temp / name.lower()
        path.mkdir()
        env[name] = str(path)
    before = sorted(str(path.relative_to(temp)) for path in temp.rglob("*"))
    result = subprocess.run(
        [
            str(BIN),
            "contract",
            "check",
            "--requires-ouro",
            ouro,
            "--requires-contract",
            contract,
        ],
        cwd=temp,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    after = sorted(str(path.relative_to(temp)) for path in temp.rglob("*"))
    assert after == before, "contract check must not create or change local state"
    lines = result.stdout.splitlines()
    assert len(lines) == 1, result.stdout
    return result, json.loads(lines[0])


def test_contract_check_accepts_and_refuses_without_state():
    subprocess.run(["cargo", "build", "-q", "-p", "ouro"], cwd=ROOT, check=True)
    cases = (
        (">=0.1.0", "1", 0, "ok", None),
        (">=999.0.0", "1", 10, "error", "ouro_version_too_old"),
        (">=0.1.0", "2", 10, "error", "cli_contract_mismatch"),
        ("0.1.0", "1", 10, "error", "malformed_ouro_requirement"),
        (">=0.1.0", "one", 10, "error", "malformed_contract_requirement"),
    )
    for index, (ouro, contract, exit_code, status, error_code) in enumerate(cases):
        with tempfile.TemporaryDirectory(prefix=f"ouro-contract-{index}-") as raw:
            result, record = run_check(pathlib.Path(raw), ouro, contract)
        assert result.returncode == exit_code, result.stderr
        assert record["tool"] == "ouro.contract.check"
        assert record["status"] == status
        assert record["changed"] is False
        assert record["audit_id"] is None
        if error_code is None:
            assert record["data"]["cli_contract"] == 1
            assert record["data"]["requires_ouro"] == ouro
        else:
            assert record["error"]["code"] == error_code
            assert "install" in record["error"]["hint"]


def test_six_skills_run_exact_front_matter_check_first():
    for name in SKILLS:
        text = (ROOT / "ouro-skills" / name / "SKILL.md").read_text()
        command = (
            "ouro-ops contract check --requires-ouro '>=0.1.0' "
            "--requires-contract 1"
        )
        assert text.count(command) == 1, name
        assert text.find("ouro-ops") == text.find(command), name
        assert text.find(command) < text.find("## Purpose"), name
        prefix = text[: text.find(command)]
        assert "Before reading a pool spec" in prefix, name


def test_pure_contract_module_has_no_state_or_transport_dependencies():
    source = (ROOT / "crates" / "ouro" / "src" / "contract.rs").read_text()
    for forbidden in (
        "std::fs",
        "std::net",
        "std::process",
        "ConfigPaths",
        "CredentialRef",
        "AuditStore",
        "SshRunner",
    ):
        assert forbidden not in source


if __name__ == "__main__":
    test_contract_check_accepts_and_refuses_without_state()
    test_six_skills_run_exact_front_matter_check_first()
    test_pure_contract_module_has_no_state_or_transport_dependencies()
    print("pure external-Skill contract preflight passed")
