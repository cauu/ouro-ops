#!/usr/bin/env python3
"""Site build: one static page plus exact copies of the public agent skills."""

from __future__ import annotations

import importlib.util
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / "web" / "onboarding"
SKILLS = ROOT / "ouro-skills"
BOOTSTRAP = ROOT / "packaging" / "install-bootstrap.sh"


def generator():
    spec = importlib.util.spec_from_file_location("ouro_site_generator", SITE / "generate.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def build_into(dist: Path, skills_root: Path = SKILLS) -> str:
    generator().build(SITE / "index.html", skills_root, dist, BOOTSTRAP)
    return (dist / "index.html").read_text(encoding="utf-8")


def copy_skills(destination: Path) -> Path:
    root = destination / "skills"
    for name in generator().PUBLIC_SKILLS:
        shutil.copytree(SKILLS / name, root / name)
    return root


def test_build_publishes_exact_skills_and_install_command() -> None:
    with tempfile.TemporaryDirectory() as td:
        dist = Path(td) / "dist"
        html = build_into(dist)
        published = sorted(p.relative_to(dist).as_posix() for p in dist.rglob("*") if p.is_file())
        assert published == [
            "index.html",
            "skills/kes-rotation/SKILL.md",
            "skills/pool-ops/SKILL.md",
        ]
        for name in ("pool-ops", "kes-rotation"):
            assert (dist / "skills" / name / "SKILL.md").read_text() == (
                SKILLS / name / "SKILL.md"
            ).read_text()
        names = json.loads(re.search(r"const SKILL_NAMES = (\[[^\n]*\]);", html).group(1))
        assert names == ["pool-ops", "kes-rotation"]
        command = json.loads(re.search(r'const INSTALL_COMMAND = (".*");', html).group(1))
        assert command == BOOTSTRAP.read_text()
        assert "__OURO_" not in html


def test_page_is_network_denied_and_links_repo() -> None:
    with tempfile.TemporaryDirectory() as td:
        html = build_into(Path(td) / "dist")
    assert "connect-src 'none'" in html
    assert 'href="https://github.com/cauu/ouro-ops"' in html
    assert 'href="skills/pool-ops/SKILL.md"' in html
    assert 'href="skills/kes-rotation/SKILL.md"' in html
    assert html.count("© 2026 Ouro Ops") == 1


def test_inline_javascript_parses() -> None:
    if not shutil.which("node"):
        pytest.skip("node is not installed")
    with tempfile.TemporaryDirectory() as td:
        html = build_into(Path(td) / "dist")
        scripts = re.findall(r"<script>(.*?)</script>", html, re.DOTALL)
        assert len(scripts) == 1
        script = Path(td) / "page.js"
        script.write_text(scripts[0], encoding="utf-8")
        checked = subprocess.run(["node", "--check", str(script)], text=True, capture_output=True)
    assert checked.returncode == 0, checked.stderr


def test_script_terminator_in_bootstrap_is_escaped() -> None:
    with tempfile.TemporaryDirectory() as td:
        bootstrap = Path(td) / "bootstrap.sh"
        bootstrap.write_text("echo '</script><script>alert(1)</script>'\n")
        dist = Path(td) / "dist"
        generator().build(SITE / "index.html", SKILLS, dist, bootstrap)
        html = (dist / "index.html").read_text()
    assert "alert(1)</script>" not in html
    assert "\\u003c/script\\u003e" in html


def test_missing_skill_fails_build() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = copy_skills(Path(td))
        (root / "kes-rotation" / "SKILL.md").unlink()
        with pytest.raises(ValueError, match="missing public skill"):
            build_into(Path(td) / "dist", root)


@pytest.mark.parametrize(
    ("old", "new", "message"),
    [
        ("name: pool-ops", "name: other", "name must be"),
        ("description:", "summary:", "description"),
    ],
)
def test_invalid_front_matter_fails_build(old: str, new: str, message: str) -> None:
    with tempfile.TemporaryDirectory() as td:
        root = copy_skills(Path(td))
        path = root / "pool-ops" / "SKILL.md"
        path.write_text(path.read_text().replace(old, new, 1))
        with pytest.raises(ValueError, match=message):
            build_into(Path(td) / "dist", root)


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-q"]))
