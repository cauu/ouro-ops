#!/usr/bin/env python3
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SKILLS = [
    ROOT / "ouro-skills/deploy/SKILL.md",
    ROOT / "ouro-skills/upgrade/SKILL.md",
    ROOT / "ouro-skills/runtime/SKILL.md",
    ROOT / "ouro-skills/observability/SKILL.md",
    ROOT / "ouro-skills/kes-rotation/SKILL.md",
    ROOT / "ouro-skills/troubleshooting/SKILL.md",
]
FORBIDDEN = [" ssh ", " scp ", " docker ", " bash ", "sudo ", "rsync "]


def main():
    for path in SKILLS:
        text = path.read_text()
        assert "Decision Tree" in text
        assert "Stop Conditions" in text
        assert "Red Lines" in text
        lowered = f" {text.lower()} "
        for word in FORBIDDEN:
            assert word not in lowered, f"{path} contains forbidden primitive {word!r}"
        assert "ouro tool run" in text or path.name == "SKILL.md"
    print("skill docs passed")


if __name__ == "__main__":
    main()
