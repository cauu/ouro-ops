#!/usr/bin/env python3
"""Deterministic S0028 Cargo version bump and release-state classifier."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys


STABLE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


def parse_version(value: str) -> tuple[int, int, int]:
    match = STABLE.fullmatch(value)
    if not match:
        raise ValueError(f"expected stable MAJOR.MINOR.PATCH, got {value!r}")
    return tuple(map(int, match.groups()))  # type: ignore[return-value]


def next_version(current: str, kind: str) -> str:
    major, minor, patch = parse_version(current)
    if kind == "patch":
        patch += 1
    elif kind == "minor":
        minor += 1
        patch = 0
    elif kind == "major":
        major += 1
        minor = patch = 0
    else:
        raise ValueError(f"unsupported bump kind {kind!r}")
    return f"{major}.{minor}.{patch}"


def cargo_version(cargo_toml: pathlib.Path) -> str:
    in_package = False
    for line in cargo_toml.read_text().splitlines():
        if line.startswith("["):
            in_package = line.strip() == "[package]"
        elif in_package:
            match = re.fullmatch(r'version\s*=\s*"([^"]+)"\s*', line)
            if match:
                parse_version(match.group(1))
                return match.group(1)
    raise ValueError(f"{cargo_toml} has no root [package] version")


def replace_one(text: str, pattern: re.Pattern[str], replacement: str, label: str) -> str:
    updated, count = pattern.subn(replacement, text, count=1)
    if count != 1:
        raise ValueError(f"expected exactly one {label}, found {count}")
    return updated


def write_versions(cargo_toml: pathlib.Path, cargo_lock: pathlib.Path, version: str) -> None:
    parse_version(version)
    cargo_text = cargo_toml.read_text()
    package_start = cargo_text.find("[package]")
    next_section = cargo_text.find("\n[", package_start + 1)
    if package_start < 0:
        raise ValueError("Cargo.toml has no [package] section")
    if next_section < 0:
        next_section = len(cargo_text)
    package = cargo_text[package_start:next_section]
    updated_package = replace_one(
        package,
        re.compile(r'(?m)^version\s*=\s*"[^"]+"\s*$'),
        f'version = "{version}"',
        "root Cargo.toml package version",
    )
    updated_cargo = (
        cargo_text[:package_start] + updated_package + cargo_text[next_section:]
    )

    lock_text = cargo_lock.read_text()
    ouro_block = re.compile(
        r'(?ms)(^\[\[package\]\]\nname = "ouro"\nversion = ")[^"]+(".*?)(?=^\[\[package\]\]|\Z)'
    )
    updated_lock, count = ouro_block.subn(rf"\g<1>{version}\g<2>", lock_text, count=1)
    if count != 1:
        raise ValueError("expected exactly one ouro package in Cargo.lock")

    cargo_toml.write_text(updated_cargo)
    cargo_lock.write_text(updated_lock)


def release_state(
    current: str, head_subject: str, head_tags: list[str], release_exists: bool
) -> tuple[str, str]:
    parse_version(current)
    expected_tag = f"v{current}"
    expected_subject = f"chore(release): {expected_tag}"
    version_tags = sorted(tag for tag in head_tags if STABLE.fullmatch(tag.removeprefix("v")))
    exact_commit = head_subject == expected_subject
    exact_tag = version_tags == [expected_tag]
    if exact_commit and exact_tag and not release_exists:
        return "resume", expected_tag
    if exact_commit or expected_tag in version_tags:
        if release_exists and exact_commit and exact_tag:
            return "new", expected_tag
        return "blocked", expected_tag
    if version_tags:
        return "blocked", expected_tag
    return "new", expected_tag


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    subcommands = result.add_subparsers(dest="command", required=True)

    current = subcommands.add_parser("current")
    current.add_argument("--cargo-toml", type=pathlib.Path, default=pathlib.Path("Cargo.toml"))

    bump = subcommands.add_parser("bump")
    bump.add_argument("--kind", choices=("patch", "minor", "major"), required=True)
    bump.add_argument("--cargo-toml", type=pathlib.Path, default=pathlib.Path("Cargo.toml"))
    bump.add_argument("--cargo-lock", type=pathlib.Path, default=pathlib.Path("Cargo.lock"))
    bump.add_argument("--write", action="store_true")

    state = subcommands.add_parser("state")
    state.add_argument("--current-version", required=True)
    state.add_argument("--head-subject", required=True)
    state.add_argument("--head-tag", action="append", default=[])
    state.add_argument("--release-exists", action="store_true")
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "current":
            print(cargo_version(args.cargo_toml))
            return 0
        if args.command == "bump":
            current = cargo_version(args.cargo_toml)
            version = next_version(current, args.kind)
            if args.write:
                write_versions(args.cargo_toml, args.cargo_lock, version)
            print(
                json.dumps(
                    {
                        "kind": args.kind,
                        "current": current,
                        "next": version,
                        "changed": args.write,
                        "files": ["Cargo.toml", "Cargo.lock"] if args.write else [],
                    },
                    sort_keys=True,
                )
            )
            return 0
        state, tag = release_state(
            args.current_version,
            args.head_subject,
            args.head_tag,
            args.release_exists,
        )
        print(json.dumps({"state": state, "tag": tag}, sort_keys=True))
        return 1 if state == "blocked" else 0
    except (OSError, ValueError) as error:
        print(json.dumps({"status": "error", "detail": str(error)}, sort_keys=True))
        return 2


if __name__ == "__main__":
    sys.exit(main())
