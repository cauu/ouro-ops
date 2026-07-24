#!/usr/bin/env python3
import importlib.util
import json
import pathlib
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "packaging" / "release-version.py"
SPEC = importlib.util.spec_from_file_location("release_version", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(MODULE)


def fixture(directory: pathlib.Path):
    cargo = directory / "Cargo.toml"
    lock = directory / "Cargo.lock"
    cargo.write_text(
        '[package]\nname = "ouro"\nversion = "0.1.0"\n\n[dependencies]\nserde = "1"\n'
    )
    lock.write_text(
        'version = 4\n\n[[package]]\nname = "ouro"\nversion = "0.1.0"\n'
        'dependencies = [\n "serde",\n]\n\n[[package]]\nname = "serde"\nversion = "1.0.0"\n'
    )
    return cargo, lock


def test_bumps():
    assert MODULE.next_version("0.1.0", "patch") == "0.1.1"
    assert MODULE.next_version("0.1.0", "minor") == "0.2.0"
    assert MODULE.next_version("0.1.0", "major") == "1.0.0"
    for invalid in ("v0.1.0", "0.1.0-rc.1", "01.1.0", "0.1"):
        try:
            MODULE.parse_version(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError(f"accepted non-stable version {invalid}")


def test_only_cargo_versions_change():
    for kind, expected in (("patch", "0.1.1"), ("minor", "0.2.0"), ("major", "1.0.0")):
        with tempfile.TemporaryDirectory() as tmp:
            cargo, lock = fixture(pathlib.Path(tmp))
            completed = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "bump",
                    "--kind",
                    kind,
                    "--cargo-toml",
                    str(cargo),
                    "--cargo-lock",
                    str(lock),
                    "--write",
                ],
                check=False,
                capture_output=True,
                text=True,
            )
            assert completed.returncode == 0, completed
            result = json.loads(completed.stdout)
            assert result == {
                "changed": True,
                "current": "0.1.0",
                "files": ["Cargo.toml", "Cargo.lock"],
                "kind": kind,
                "next": expected,
            }
            assert f'version = "{expected}"' in cargo.read_text()
            ouro_block = lock.read_text().split('name = "ouro"', 1)[1].split("[[package]]", 1)[0]
            assert f'version = "{expected}"' in ouro_block
            assert 'name = "serde"\nversion = "1.0.0"' in lock.read_text()


def test_partial_state_recovery():
    assert MODULE.release_state(
        "0.1.1", "chore(release): v0.1.1", ["v0.1.1"], False
    ) == ("resume", "v0.1.1")
    assert MODULE.release_state(
        "0.1.1", "chore(release): v0.1.1", ["v0.1.1"], True
    ) == ("new", "v0.1.1")
    for subject, tags in (
        ("chore(release): v0.1.1", []),
        ("other", ["v0.1.1"]),
        ("chore(release): v0.1.1", ["v0.1.0"]),
    ):
        assert MODULE.release_state("0.1.1", subject, tags, False)[0] == "blocked"


def test_workflow_contract():
    workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text()
    for required in (
        "workflow_dispatch:",
        "group: cli-release",
        "cancel-in-progress: false",
        "github.ref == 'refs/heads/main'",
        "test \"$(git rev-parse origin/main)\" = \"$BASE_SHA\"",
        "test \"$(git diff --cached --name-only | sort)\"",
        'git commit -m "chore(release): $TAG"',
        'git push origin HEAD:main',
        'git push origin "refs/tags/$TAG"',
        'gh workflow run release-publish.yml --ref "$TAG"',
        "state_args+=(--release-exists)",
        "if test \"$state\" = resume",
    ):
        assert required in workflow, required
    assert "\n  pull_request:" not in workflow
    assert "\n  push:" not in workflow
    assert "force" not in workflow


if __name__ == "__main__":
    test_bumps()
    test_only_cargo_versions_change()
    test_partial_state_recovery()
    test_workflow_contract()
    print("S0028 deterministic release version preparation passed")
