#!/usr/bin/env python3
"""S0025 p5-1 — paired release-candidate build/check contract."""

import hashlib
import json
import platform
import subprocess
import tarfile
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "packaging/release-candidate.sh"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    assert SCRIPT.stat().st_mode & 0o111, "release-candidate builder must be executable"
    subprocess.run(["bash", "-n", str(SCRIPT)], check=True)
    source = SCRIPT.read_text()
    for required in (
        "cargo zigbuild --locked --release",
        "x86_64-unknown-linux-musl",
        "OURO_EMBED_LINUX_X86_64_RUNNER",
        "contract check",
        "release select --platform linux/amd64",
        "release-upgrade-select.json",
        '"upgrade_recommended"',
        "shasum -a 256 -c SHA256SUMS",
        "SKILL.md",
        "formal_cli_publication",
    ):
        assert required in source, f"release-candidate builder lacks {required!r}"

    if platform.system() != "Darwin":
        print("paired release-candidate source contract passed (non-macOS host)")
        return
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    rustc = subprocess.run(["rustc", "-vV"], check=True, text=True, capture_output=True).stdout
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    candidate = ROOT / "dist/release-candidate" / f"v{version}-{host}"
    if not candidate.is_dir():
        print("paired release-candidate source contract passed (candidate not built in this job)")
        return

    manifest = json.loads((candidate / "candidate.json").read_text())
    descriptor = manifest["descriptor"]
    assert manifest["status"] == "release-standard-not-published"
    assert manifest["formal_cli_publication"] == "deferred"
    assert descriptor["ouro_version"] == version
    assert descriptor["cli_contract"] == 1
    assert descriptor["runner_platform"] == "linux/x86_64"
    runner = candidate / manifest["embedded_runner_evidence"]["file"]
    package = candidate / manifest["control"]["package"]
    assert sha256(runner) == descriptor["runner_sha256"]
    assert sha256(package) == manifest["control"]["sha256"]
    runner_strings = subprocess.run(
        ["strings", str(runner)], check=True, text=True, capture_output=True
    ).stdout
    for marker in (
        "upgrade: unsupported Docker log driver",
        "upgrade: unsupported json-file log option",
        'if key == "max-file"',
        'elif key == "max-size"',
    ):
        assert marker in runner_strings, f"paired Linux runner lacks logging marker {marker!r}"
    assert manifest["release_catalog_smoke"]["repository"] == (
        "ghcr.io/blinklabs-io/cardano-node"
    )
    assert manifest["release_catalog_smoke"]["historical_direct_upgrade_selection"] == (
        "upgrade_recommended"
    )
    assert manifest["release_catalog_smoke"]["historical_direct_upgrade_transition"] is None
    with tarfile.open(package, "r:gz") as archive:
        assert archive.getnames() == ["ouro-ops"]
    names = [path.relative_to(candidate).as_posix() for path in candidate.rglob("*")]
    assert not any("SKILL.md" in name or "ouro-skills" in name for name in names)
    assert not any(name.endswith((".oci", ".img", ".docker.tar")) for name in names)
    print("paired release-candidate build/check passed")


if __name__ == "__main__":
    main()
