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

    retired_tool = ouro("tool", "run", "deploy/status", check=False)
    assert retired_tool.returncode != 0
    assert "unknown command tool" in retired_tool.stdout

    descriptor = json.loads(ouro("contract").stdout)["data"]
    assert set(descriptor) == {
        "ouro_version",
        "cli_contract",
        "runner_platform",
        "runner_sha256",
    }
    assert descriptor["cli_contract"] == 1
    assert descriptor["runner_platform"] == "linux/x86_64"

    retired_manifest = ouro("manifest", "show", check=False)
    assert retired_manifest.returncode != 0
    assert "unknown command manifest" in retired_manifest.stdout

    build_script = (ROOT / "build.rs").read_text(encoding="utf-8")
    assert 'rel.ends_with("/SKILL.md")' not in build_script
    assert "cargo:rerun-if-changed=ouro-skills" not in build_script
    assert 'rel.ends_with(".sh")' not in build_script

    assert not (ROOT / "packaging" / "bundle-manifest.json").exists()

    cli = (ROOT / "crates/ouro/src/cli.rs").read_text(encoding="utf-8")
    assert "fn run_tool" not in cli
    assert '"tool" => run_tool' not in cli
    provision = (ROOT / "crates" / "ouro" / "src" / "provision.rs").read_text()
    assert "ouro-tool-run" not in provision
    assert "ouro-ops tool run" not in provision

    version = (ROOT / "crates" / "ouro" / "src" / "version.rs").read_text()
    assert "version-floor" not in version
    assert "load_floor" not in version
    assert "required_ouro" not in (ROOT / "crates" / "ouro" / "src" / "assets.rs").read_text()


if __name__ == "__main__":
    test_cli_has_no_decision_skill_route_or_assets()
    print("external Skill CLI boundary passed")
