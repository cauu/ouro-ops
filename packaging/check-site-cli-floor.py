#!/usr/bin/env python3
"""Fail before Cloudflare writes when canonical Skills require an unreleased CLI."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys


STABLE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
REQUIRED_SKILLS = (
    "observability",
    "troubleshooting",
    "runtime",
    "upgrade",
    "kes-rotation",
    "deploy",
)


def version(value: str) -> tuple[int, int, int]:
    value = value.removeprefix("v")
    match = STABLE.fullmatch(value)
    if not match:
        raise ValueError(f"expected stable MAJOR.MINOR.PATCH, got {value!r}")
    return tuple(map(int, match.groups()))  # type: ignore[return-value]


def skill_floor(path: pathlib.Path) -> tuple[int, int, int]:
    text = path.read_text()
    front = text.split("\n---\n", 1)[0]
    match = re.search(r'^requires_ouro:\s*["\']?>=(\d+\.\d+\.\d+)["\']?\s*$', front, re.M)
    if not match:
        raise ValueError(f"{path} has no exact >= stable requires_ouro floor")
    return version(match.group(1))


def evaluate(root: pathlib.Path, released: str) -> dict[str, object]:
    released_version = version(released)
    floors = {
        name: skill_floor(root / name / "SKILL.md") for name in REQUIRED_SKILLS
    }
    required = max(floors.values())
    ready = released_version >= required
    return {
        "status": "ready" if ready else "cli_release_required",
        "changed": False,
        "released": ".".join(map(str, released_version)),
        "required": ".".join(map(str, required)),
        "skill_floors": {
            name: ".".join(map(str, floor)) for name, floor in sorted(floors.items())
        },
        "cloudflare_write_allowed": ready,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--skills-root", type=pathlib.Path, default=pathlib.Path("ouro-skills"))
    parser.add_argument("--released", required=True)
    args = parser.parse_args()
    try:
        result = evaluate(args.skills_root, args.released)
    except (OSError, ValueError) as error:
        print(
            json.dumps(
                {
                    "status": "invalid_floor_evidence",
                    "changed": False,
                    "cloudflare_write_allowed": False,
                    "detail": str(error),
                },
                sort_keys=True,
            )
        )
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0 if result["cloudflare_write_allowed"] else 1


if __name__ == "__main__":
    sys.exit(main())
