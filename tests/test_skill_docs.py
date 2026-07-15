#!/usr/bin/env python3
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
# S0019 greenfield skills (judgment frameworks; writes via `ouro-ops op run`, adopt-first).
S0019_SKILLS = [
    ROOT / "ouro-skills/adopt/SKILL.md",
    ROOT / "ouro-skills/config/SKILL.md",
    ROOT / "ouro-skills/deploy/SKILL.md",
    ROOT / "ouro-skills/upgrade/SKILL.md",
    ROOT / "ouro-skills/runtime/SKILL.md",
    ROOT / "ouro-skills/observability/SKILL.md",
    ROOT / "ouro-skills/kes-rotation/SKILL.md",
    ROOT / "ouro-skills/troubleshooting/SKILL.md",
    ROOT / "ouro-skills/onboard/SKILL.md",
]
# S0017 skills kept for the legacy dispatch model (disabled at the mechanism by S0019 §2.8, but the
# decision docs remain readable).
LEGACY_SKILLS = [
    ROOT / "ouro-skills/detect/SKILL.md",
]
SKILLS = S0019_SKILLS + LEGACY_SKILLS
FORBIDDEN = [" ssh ", " scp ", " docker ", " bash ", "sudo ", "rsync "]
# Universal red lines every skill must carry.
REQUIRED_RED_LINES = [
    "no secret directory access",
    "cold, KES secret, or VRF",
]
# S0019 skills must show the data-not-instructions red line and use a greenfield command surface
# (write skills → `ouro-ops op run`; read skills → `ouro-ops diag exec`; adoption → `ouro-ops adopt`).
S0019_REQUIRED = ["DATA"]
S0019_COMMANDS = ["ouro-ops op run", "ouro-ops diag exec", "ouro-ops adopt"]
# Legacy skills keep the S0017 contract.
LEGACY_REQUIRED = [
    "ouro-ops tool run",
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
        # A decision layer (S0017 "Decision Tree" or S0019 "Decision guidance"), stops, red lines.
        assert "Decision Tree" in text or "Decision guidance" in text, f"{path} lacks a decision layer"
        assert "Stop Conditions" in text
        assert "Red Lines" in text
        lowered = f" {text.lower()} "
        for word in FORBIDDEN:
            assert word not in lowered, f"{path} contains forbidden primitive {word!r}"
        for phrase in REQUIRED_RED_LINES:
            assert phrase in text, f"{path} lacks red line {phrase!r}"
        if path in S0019_SKILLS:
            for phrase in S0019_REQUIRED:
                assert phrase in text, f"{path} lacks required phrase {phrase!r}"
            assert any(c in text for c in S0019_COMMANDS), \
                f"{path} references no greenfield command surface {S0019_COMMANDS}"
            assert "L3 diagnostics are read-only" not in text, \
                f"{path} contradicts the unprivileged-but-not-read-only diagnostic boundary"
        else:
            for phrase in LEGACY_REQUIRED:
                assert phrase in text, f"{path} lacks required phrase {phrase!r}"

    kes = (ROOT / "ouro-skills/kes-rotation/SKILL.md").read_text()
    assert "does NOT expose remaining KES periods" in kes
    assert "Use managed health to determine" not in kes, \
        "KES Skill must not infer KES lifetime from the fixed tip-only health read"
    onboard = (ROOT / "ouro-skills/onboard/SKILL.md").read_text()
    for phrase in [
        "data.ssh_access_policy",
        "bootstrap_user_preserved: true",
        "Never infer runtime-formatted values from static binary string fragments",
        "ouro-ops creds check --name <name>",
        "ouro-ops creds register --name <name>",
        "never key contents",
        "legacy_s0017_paths_retired",
        "effective_ssh_policy_verified: true",
    ]:
        assert phrase in onboard, f"onboard Skill lacks rendered-policy guard {phrase!r}"
    adopt = (ROOT / "ouro-skills/adopt/SKILL.md").read_text()
    assert "ouro-ops creds check --name <name>" in adopt
    assert "Never list credentials" in adopt
    print("skill docs passed")


if __name__ == "__main__":
    main()
