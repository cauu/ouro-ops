#!/usr/bin/env python3
import hashlib
import io
import json
import pathlib
import subprocess
import tarfile
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "packaging" / "verify-release-assets.py"
TARGETS = (
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fixtures(directory, runner_digest="a" * 64):
    for target in TARGETS:
        archive = directory / f"ouro-ops-v0.1.1-{target}.tar.gz"
        info = tarfile.TarInfo("ouro-ops")
        payload = f"native-{target}".encode()
        info.size = len(payload)
        info.mode = 0o755
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.addfile(info, io.BytesIO(payload))
        (directory / f"descriptor-{target}.json").write_text(
            json.dumps(
                {
                    "package": archive.name,
                    "package_sha256": sha256(archive),
                    "runner_platform": "linux/x86_64",
                    "runner_sha256": runner_digest,
                    "target": target,
                    "version": "0.1.1",
                }
            )
        )


def run(directory):
    return subprocess.run(
        [
            "python3",
            str(SCRIPT),
            "--dir",
            str(directory),
            "--version",
            "0.1.1",
            "--write-checksums",
        ],
        check=False,
        capture_output=True,
        text=True,
    )


def main():
    with tempfile.TemporaryDirectory() as tmp:
        directory = pathlib.Path(tmp)
        fixtures(directory)
        accepted = run(directory)
        assert accepted.returncode == 0, accepted
        checksums = (directory / "SHA256SUMS").read_text().splitlines()
        assert len(checksums) == 4
        assert [line.split("  ", 1)[1] for line in checksums] == [
            f"ouro-ops-v0.1.1-{target}.tar.gz" for target in TARGETS
        ]

        archive = directory / "ouro-ops-v0.1.1-x86_64-unknown-linux-musl.tar.gz"
        archive.write_bytes(archive.read_bytes() + b"tampered")
        assert run(directory).returncode != 0

    with tempfile.TemporaryDirectory() as tmp:
        directory = pathlib.Path(tmp)
        fixtures(directory)
        descriptor = directory / "descriptor-aarch64-apple-darwin.json"
        value = json.loads(descriptor.read_text())
        value["runner_sha256"] = "b" * 64
        descriptor.write_text(json.dumps(value))
        assert run(directory).returncode != 0

    workflow = (ROOT / ".github" / "workflows" / "release-publish.yml").read_text()
    for required in (
        "ubuntu-24.04-arm",
        "macos-15-intel",
        "macos-15",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "OURO_EMBED_LINUX_X86_64_RUNNER",
        "actions/attest@v4",
        "subject-checksums: release/SHA256SUMS",
        'gh release create "$TAG"',
        'gh release verify "$TAG"',
        'for attempt in 1 2 3 4 5 6 7 8 9 10 11 12',
        'test "$verified" = true',
    ):
        assert required in workflow, required
    assert "\n  pull_request:" not in workflow
    assert "\n  push:" not in workflow
    print("S0028 four-platform release asset contract passed")


if __name__ == "__main__":
    main()
