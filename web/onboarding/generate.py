#!/usr/bin/env python3
"""Build the release-form static site from the six canonical public Ouro Skills."""

from __future__ import annotations

import argparse
import json
import re
import shutil
from pathlib import Path


PUBLIC_SKILLS = {
    "observability": "observability/SKILL.md",
    "troubleshooting": "troubleshooting/SKILL.md",
    "runtime": "runtime/SKILL.md",
    "upgrade": "upgrade/SKILL.md",
    "kes-rotation": "kes-rotation/SKILL.md",
    "deploy": "deploy/SKILL.md",
}
EXPECTED_PUBLIC_SKILLS = frozenset(
    {"observability", "troubleshooting", "runtime", "upgrade", "kes-rotation", "deploy"}
)
PLACEHOLDER = "__OURO_PUBLIC_SKILLS_JSON__"
BOOTSTRAP_PLACEHOLDER = "__OURO_INSTALL_BOOTSTRAP_JSON__"
VERSION_REQUIREMENT = re.compile(r"^>=\d+\.\d+\.\d+$")


def parse_front_matter(path: Path, content: str) -> dict[str, object]:
    if not content.startswith("---\n"):
        raise ValueError(f"{path}: SKILL.md must start with YAML front matter")
    end = content.find("\n---\n", 4)
    if end < 0:
        raise ValueError(f"{path}: unterminated YAML front matter")
    fields: dict[str, object] = {}
    for raw_line in content[4:end].splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if ":" not in line:
            raise ValueError(f"{path}: malformed front-matter line {raw_line!r}")
        key, raw_value = line.split(":", 1)
        key = key.strip()
        value = raw_value.strip().strip('"\'')
        if key in fields:
            raise ValueError(f"{path}: duplicate front-matter field {key}")
        fields[key] = value

    expected = {"skill_version", "requires_ouro", "requires_contract"}
    missing = expected - fields.keys()
    unknown = fields.keys() - expected
    if missing:
        raise ValueError(f"{path}: missing front-matter fields: {', '.join(sorted(missing))}")
    if unknown:
        raise ValueError(f"{path}: unknown front-matter fields: {', '.join(sorted(unknown))}")
    try:
        skill_version = int(str(fields["skill_version"]))
        requires_contract = int(str(fields["requires_contract"]))
    except ValueError as error:
        raise ValueError(f"{path}: skill_version/requires_contract must be integers") from error
    if skill_version < 1 or requires_contract < 1:
        raise ValueError(f"{path}: skill_version/requires_contract must be positive")
    requires_ouro = str(fields["requires_ouro"])
    if not VERSION_REQUIREMENT.fullmatch(requires_ouro):
        raise ValueError(f"{path}: requires_ouro must be an exact >=x.y.z floor")
    return {
        "skill_version": skill_version,
        "requires_ouro": requires_ouro,
        "requires_contract": requires_contract,
    }


def safe_json(value: object) -> str:
    # This JSON is inserted as a JavaScript object literal. Escape every token that can terminate
    # a script element or create an HTML parse boundary before the JS parser sees the string.
    encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    return (
        encoded.replace("&", "\\u0026")
        .replace("<", "\\u003c")
        .replace(">", "\\u003e")
        .replace("\u2028", "\\u2028")
        .replace("\u2029", "\\u2029")
    )


def build(
    source: Path, skills_root: Path, dist: Path, install_bootstrap: Path | None = None
) -> Path:
    if frozenset(PUBLIC_SKILLS) != EXPECTED_PUBLIC_SKILLS:
        raise ValueError("public Skill mapping must contain exactly the six supported operations")
    paths = list(PUBLIC_SKILLS.values())
    if len(paths) != len(set(paths)):
        raise ValueError("public Skill mapping contains duplicate source files")

    template = source.read_text(encoding="utf-8")
    if template.count(PLACEHOLDER) != 1:
        raise ValueError(f"{source}: expected exactly one {PLACEHOLDER} placeholder")
    if template.count(BOOTSTRAP_PLACEHOLDER) != 1:
        raise ValueError(f"{source}: expected exactly one {BOOTSTRAP_PLACEHOLDER} placeholder")
    if install_bootstrap is None:
        install_bootstrap = (
            Path(__file__).resolve().parents[2] / "packaging/install-bootstrap.sh"
        )
    if not install_bootstrap.is_file():
        raise ValueError(f"missing canonical install bootstrap: {install_bootstrap}")
    install_commands = install_bootstrap.read_text(encoding="utf-8")

    payload: dict[str, object] = {}
    for operation, relative in sorted(PUBLIC_SKILLS.items()):
        path = skills_root / relative
        if not path.is_file():
            raise ValueError(f"missing canonical public Skill: {path}")
        content = path.read_text(encoding="utf-8")
        metadata = parse_front_matter(path, content)
        payload[operation] = {**metadata, "content": content}

    rendered = template.replace(PLACEHOLDER, safe_json(payload)).replace(
        BOOTSTRAP_PLACEHOLDER, safe_json(install_commands)
    )
    if PLACEHOLDER in rendered or BOOTSTRAP_PLACEHOLDER in rendered:
        raise ValueError("unresolved canonical source placeholder")
    if dist.exists():
        shutil.rmtree(dist)
    dist.mkdir(parents=True)
    output = dist / "index.html"
    output.write_text(rendered, encoding="utf-8")
    if [path.name for path in dist.iterdir()] != ["index.html"]:
        raise ValueError("site build must contain exactly index.html")
    return output


def main() -> int:
    here = Path(__file__).resolve().parent
    root = here.parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, default=here / "index.html")
    parser.add_argument("--skills-root", type=Path, default=root / "ouro-skills")
    parser.add_argument(
        "--install-bootstrap",
        type=Path,
        default=root / "packaging/install-bootstrap.sh",
    )
    parser.add_argument("--dist", type=Path, default=here / "dist")
    args = parser.parse_args()
    output = build(
        args.source.resolve(),
        args.skills_root.resolve(),
        args.dist.resolve(),
        args.install_bootstrap.resolve(),
    )
    print(f"built {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
