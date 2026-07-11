#!/usr/bin/env python3
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SKILLS = [
    ROOT / "ouro-skills/deploy/SKILL.md",
    ROOT / "ouro-skills/upgrade/SKILL.md",
    ROOT / "ouro-skills/runtime/SKILL.md",
    ROOT / "ouro-skills/observability/SKILL.md",
    ROOT / "ouro-skills/kes-rotation/SKILL.md",
    ROOT / "ouro-skills/troubleshooting/SKILL.md",
    ROOT / "ouro-skills/detect/SKILL.md",
]
FORBIDDEN = [" ssh ", " scp ", " docker ", " bash ", "sudo ", "rsync "]
REQUIRED_RED_LINES = [
    "ouro-ops tool run",
    "no secret directory access",
    "cold, KES secret, or VRF",
    "exit 30",
    "exit 40",
]


_FRONT_MATTER = re.compile(r"\A---\n(.*?)\n---\n", re.DOTALL)


def _front_matter(text):
    """S0016 p3-1 — parse the leading YAML front matter into a dict (simple key: value)."""
    m = _FRONT_MATTER.match(text)
    assert m, "SKILL.md must open with YAML front matter (--- ... ---)"
    meta = {}
    for line in m.group(1).splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        key, _, value = line.partition(":")
        meta[key.strip()] = value.strip().strip('"').strip("'")
    return meta


def main():
    for path in SKILLS:
        text = path.read_text()
        # S0016 p3-1: machine-readable version header (feeds embedded floor p3-2 + manifest p2-6).
        meta = _front_matter(text)
        assert str(meta.get("skill_version", "")).isdigit(), \
            f"{path} front matter needs integer skill_version, got {meta.get('skill_version')!r}"
        assert re.match(r"^(>=|>|=|\^|~)?\d+\.\d+\.\d+", meta.get("requires_ouro", "")), \
            f"{path} front matter needs semver requires_ouro, got {meta.get('requires_ouro')!r}"
        assert "Decision Tree" in text
        assert "Stop Conditions" in text
        assert "Red Lines" in text
        lowered = f" {text.lower()} "
        for word in FORBIDDEN:
            assert word not in lowered, f"{path} contains forbidden primitive {word!r}"
        for phrase in REQUIRED_RED_LINES:
            assert phrase in text, f"{path} lacks red line {phrase!r}"
    print("skill docs passed")


if __name__ == "__main__":
    main()
