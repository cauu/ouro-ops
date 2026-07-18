#!/usr/bin/env python3
"""S0025 gates proving decision Skills are external to the Rust CLI."""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def ouro(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["cargo", "run", "-q", "-p", "ouro", "--", *args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=check,
    )


def test_cli_has_no_decision_skill_route_or_assets() -> None:
    help_text = ouro("help").stdout
    assert "skill show" not in help_text
    assert "skill     show|list" not in help_text

    retired = ouro("skill", "list", check=False)
    assert retired.returncode != 0
    assert "unknown command skill" in retired.stdout

    manifest = json.loads(ouro("manifest", "show").stdout)
    assert "decision_hash" not in manifest
    assert all(not path.endswith("/SKILL.md") for path in manifest["assets"])
    assert "lib/ouro-probe.sh" in manifest["assets"]
    assert "schemas/pool-spec.schema.json" in manifest["assets"]

    build_script = (ROOT / "build.rs").read_text(encoding="utf-8")
    assert 'rel.ends_with("/SKILL.md")' not in build_script
    assert "cargo:rerun-if-changed=ouro-skills" not in build_script


if __name__ == "__main__":
    test_cli_has_no_decision_skill_route_or_assets()
    print("external Skill CLI boundary passed")
