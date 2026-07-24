#!/usr/bin/env python3
"""Validate and checksum the canonical CLI archives and installer Release asset."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
import tarfile


TARGETS = (
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
)
INSTALLER_NAME = "ouro-install.sh"


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify(directory: pathlib.Path, version: str) -> list[tuple[str, pathlib.Path]]:
    expected_archives = {
        target: directory / f"ouro-ops-v{version}-{target}.tar.gz" for target in TARGETS
    }
    actual_archives = set(directory.glob("ouro-ops-v*.tar.gz"))
    if actual_archives != set(expected_archives.values()):
        raise ValueError(
            f"release archives differ from canonical four: {sorted(map(str, actual_archives))}"
        )

    runner_digests = set()
    checksums = []
    for target, archive in expected_archives.items():
        descriptor_path = directory / f"descriptor-{target}.json"
        descriptor = json.loads(descriptor_path.read_text())
        expected_descriptor = {
            "package": archive.name,
            "runner_platform": "linux/x86_64",
            "target": target,
            "version": version,
        }
        for key, value in expected_descriptor.items():
            if descriptor.get(key) != value:
                raise ValueError(
                    f"{descriptor_path.name} {key}={descriptor.get(key)!r}, expected {value!r}"
                )
        runner_digest = descriptor.get("runner_sha256")
        if not isinstance(runner_digest, str) or len(runner_digest) != 64:
            raise ValueError(f"{descriptor_path.name} has no valid runner SHA-256")
        int(runner_digest, 16)
        runner_digests.add(runner_digest)
        if descriptor.get("package_sha256") != sha256(archive):
            raise ValueError(f"{archive.name} digest does not match its native descriptor")
        with tarfile.open(archive, "r:gz") as bundle:
            members = bundle.getmembers()
            if len(members) != 1 or members[0].name != "ouro-ops":
                raise ValueError(f"{archive.name} must contain exactly one ouro-ops")
            member = members[0]
            if not member.isfile() or member.mode & 0o111 == 0:
                raise ValueError(f"{archive.name} ouro-ops is not an executable regular file")
        checksums.append((sha256(archive), archive))
    if len(runner_digests) != 1:
        raise ValueError(f"control descriptors disagree on runner digest: {runner_digests}")
    installer = directory / INSTALLER_NAME
    canonical_installer = pathlib.Path(__file__).with_name(INSTALLER_NAME)
    if not installer.is_file() or installer.is_symlink():
        raise ValueError(f"{INSTALLER_NAME} must be a regular file")
    if installer.read_bytes() != canonical_installer.read_bytes():
        raise ValueError(f"{INSTALLER_NAME} differs from the canonical repository source")
    if installer.stat().st_mode & 0o111 == 0:
        raise ValueError(f"{INSTALLER_NAME} is not executable")
    checksums.append((sha256(installer), installer))
    return checksums


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", type=pathlib.Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--write-checksums", action="store_true")
    args = parser.parse_args()
    try:
        checksums = verify(args.dir, args.version)
        if args.write_checksums:
            (args.dir / "SHA256SUMS").write_text(
                "".join(f"{digest}  {path.name}\n" for digest, path in checksums)
            )
    except (OSError, ValueError, json.JSONDecodeError, tarfile.TarError) as error:
        print(f"release asset verification failed: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "status": "ok",
                "version": args.version,
                "targets": list(TARGETS),
                "installer": INSTALLER_NAME,
                "runner_sha256": json.loads(
                    (args.dir / f"descriptor-{TARGETS[0]}.json").read_text()
                )["runner_sha256"],
                "checksums_written": args.write_checksums,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
