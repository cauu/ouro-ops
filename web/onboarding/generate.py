#!/usr/bin/env python3
"""Build the static site: one page plus the published agent skills.

dist/
  index.html                      template with the install command and skill names injected
  skills/<name>/SKILL.md          exact copies of ouro-skills/<name>/SKILL.md
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
from pathlib import Path


PUBLIC_SKILLS = ("pool-ops", "kes-rotation")
BOOTSTRAP_PLACEHOLDER = "__OURO_INSTALL_BOOTSTRAP_JSON__"
NAMES_PLACEHOLDER = "__OURO_SKILL_NAMES_JSON__"
_FRONT_MATTER = re.compile(r"\A---\n(.*?)\n---\n", re.DOTALL)


def front_matter(path: Path, content: str) -> dict[str, str]:
    """Top-level `key: value` pairs of the YAML front matter (nested blocks are skipped)."""
    match = _FRONT_MATTER.match(content)
    if not match:
        raise ValueError(f"{path}: SKILL.md must start with YAML front matter")
    fields: dict[str, str] = {}
    for line in match.group(1).splitlines():
        if not line.strip() or line.startswith((" ", "\t", "#")):
            continue
        key, sep, value = line.partition(":")
        if not sep:
            raise ValueError(f"{path}: malformed front-matter line {line!r}")
        fields[key.strip()] = value.strip().strip("\"'")
    return fields


def validate_skill(path: Path, expected_name: str) -> str:
    content = path.read_text(encoding="utf-8")
    fields = front_matter(path, content)
    if fields.get("name") != expected_name:
        raise ValueError(f"{path}: front-matter name must be {expected_name!r}")
    if not fields.get("description"):
        raise ValueError(f"{path}: front matter needs a description")
    return content


def safe_json(value: object) -> str:
    # Inserted into a <script> element: escape anything that could end it or open markup.
    encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    return (
        encoded.replace("&", "\\u0026")
        .replace("<", "\\u003c")
        .replace(">", "\\u003e")
        .replace(" ", "\\u2028")
        .replace(" ", "\\u2029")
    )


def build(source: Path, skills_root: Path, dist: Path, install_bootstrap: Path) -> Path:
    template = source.read_text(encoding="utf-8")
    for placeholder in (BOOTSTRAP_PLACEHOLDER, NAMES_PLACEHOLDER):
        if template.count(placeholder) != 1:
            raise ValueError(f"{source}: expected exactly one {placeholder} placeholder")
    if not install_bootstrap.is_file():
        raise ValueError(f"missing install bootstrap: {install_bootstrap}")

    skills = {}
    for name in PUBLIC_SKILLS:
        path = skills_root / name / "SKILL.md"
        if not path.is_file():
            raise ValueError(f"missing public skill: {path}")
        skills[name] = validate_skill(path, name)

    rendered = template.replace(
        BOOTSTRAP_PLACEHOLDER, safe_json(install_bootstrap.read_text(encoding="utf-8"))
    ).replace(NAMES_PLACEHOLDER, safe_json(list(PUBLIC_SKILLS)))

    if dist.exists():
        shutil.rmtree(dist)
    dist.mkdir(parents=True)
    output = dist / "index.html"
    output.write_text(rendered, encoding="utf-8")
    for name, content in skills.items():
        target = dist / "skills" / name / "SKILL.md"
        target.parent.mkdir(parents=True)
        target.write_text(content, encoding="utf-8")
    return output


def main() -> int:
    here = Path(__file__).resolve().parent
    root = here.parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, default=here / "index.html")
    parser.add_argument("--skills-root", type=Path, default=root / "ouro-skills")
    parser.add_argument(
        "--install-bootstrap", type=Path, default=root / "packaging/install-bootstrap.sh"
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
