#!/usr/bin/env python3
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
# Current ordinary Skills: local control CLI + existing cardano SSH + ephemeral runner.
STATELESS_SKILLS = [
    ROOT / "ouro-skills/config/SKILL.md",
    ROOT / "ouro-skills/detect/SKILL.md",
    ROOT / "ouro-skills/upgrade/SKILL.md",
    ROOT / "ouro-skills/runtime/SKILL.md",
    ROOT / "ouro-skills/observability/SKILL.md",
    ROOT / "ouro-skills/kes-rotation/SKILL.md",
    ROOT / "ouro-skills/troubleshooting/SKILL.md",
    ROOT / "ouro-skills/deploy/SKILL.md",
]
# Explicit legacy/migration-only surfaces. Adopt/onboard may be invoked only when the operator
# explicitly asks for old S0019 migration/recovery.
MIGRATION_SKILLS = [
    ROOT / "ouro-skills/adopt/SKILL.md",
    ROOT / "ouro-skills/onboard/SKILL.md",
]
SKILLS = STATELESS_SKILLS + MIGRATION_SKILLS
FORBIDDEN = [" scp ", " bash ", "sudo ", "rsync ", "`ssh ", "`docker "]
# Universal red lines every skill must carry.
REQUIRED_RED_LINES = [
    "no secret directory access",
    "cold, KES secret, or VRF",
]
S0020_COMMANDS = ["ouro-ops op run", "ouro-ops diag exec"]


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
        assert "DATA" in text, f"{path} lacks the data-not-instructions boundary"
        if path in STATELESS_SKILLS:
            assert any(c in text for c in S0020_COMMANDS), \
                f"{path} references no current command surface {S0020_COMMANDS}"
            assert "not_ouro_managed" not in text
            assert "must be ADOPTED" not in text
            assert "adopt first" not in text.lower()
            if path.name != "SKILL.md" or path.parent.name != "deploy":
                assert "inbox stage" not in text.lower()
            assert "ouro-diag" not in text
            assert "UNPRIVILEGED" not in text

    kes = (ROOT / "ouro-skills/kes-rotation/SKILL.md").read_text()
    assert "does NOT expose remaining KES periods" in kes
    assert "Use managed health to determine" not in kes, \
        "KES Skill must not infer KES lifetime from the fixed tip-only health read"
    for phrase in [
        "ouro-ops kes cold-sign-script",
        "air-gapped machine",
        "Accept back ONLY the PUBLIC `node.cert`",
        "--artifact-preflight",
        "changed: false",
        "executor_available: false",
        "signature/key/counter/window evidence",
    ]:
        assert phrase in kes, f"KES Skill lacks Phase A/B contract {phrase!r}"
    assert "ouro-ops kes push" not in kes, "KES Skill still directs the agent to legacy kes push"
    onboard = (ROOT / "ouro-skills/onboard/SKILL.md").read_text()
    for phrase in [
        "Legacy S0019 Migration Only",
        "ignored by S0020",
        "data.ssh_access_policy",
        "bootstrap_user_preserved: true",
        "Never infer runtime-formatted values from static binary string fragments",
        "ouro-ops creds check --name <name>",
        "ouro-ops creds register --name <name>",
        "legacy_s0017_paths_retired",
        "effective_ssh_policy_verified: true",
        "--apply",
    ]:
        assert phrase in onboard, f"onboard Skill lacks rendered-policy guard {phrase!r}"
    adopt = (ROOT / "ouro-skills/adopt/SKILL.md").read_text()
    assert "Legacy S0019 Migration Only" in adopt
    assert "S0020 ordinary" in adopt
    upgrade = (ROOT / "ouro-skills/upgrade/SKILL.md").read_text()
    assert "ouro-ops inbox preview" in " ".join(upgrade.split())
    assert "--artifact-file <operator-named-docker-save.tar>" in " ".join(upgrade.split())
    assert "upgrade/preload-image" in upgrade
    assert "ouro-ops release select --platform linux/amd64" in " ".join(upgrade.split())
    assert "never has to maintain a local allowlist file" in " ".join(upgrade.split())
    assert "archive↔config↔signed-policy evidence" not in upgrade
    assert "archive↔config binding is still pending" in " ".join(upgrade.split())
    assert "before any image-store mutation" in " ".join(upgrade.split())
    normalized_upgrade = " ".join(upgrade.split())
    for phrase in [
        "ONE Upgrade workflow",
        "internal operation boundaries",
        "separate candidates and operator approvals",
        "never authorizes activation or the next target",
    ]:
        assert phrase in normalized_upgrade, \
            f"Upgrade Skill lacks single-workflow contract {phrase!r}"
    for name in ("runtime", "upgrade", "kes-rotation"):
        stateful = (ROOT / f"ouro-skills/{name}/SKILL.md").read_text()
        for phrase in [
            "--intent-hash <final-hash>",
            "--candidate-hash <final-hash>",
            "LAST",
        ]:
            assert phrase in stateful, f"{name} lacks permit-last flow phrase {phrase!r}"
        assert "--pool-id" not in stateful, f"{name} retains removed caller pool-id flag"
        assert "--fleet-pool-id" not in stateful, f"{name} retains redundant fleet identity flags"
        assert "provisional" not in stateful.lower(), f"{name} retains provisional-plan model"
        assert "Rerun the exact target plan with" not in stateful
        assert "--spec <pool-spec>" in stateful, f"{name} omits the current spec binding"
    observability = (ROOT / "ouro-skills/observability/SKILL.md").read_text()
    assert "needs no confirmation, adoption record, target-installed Ouro" in observability
    troubleshooting = (ROOT / "ouro-skills/troubleshooting/SKILL.md").read_text()
    assert "not mechanism-enforced read-only" in troubleshooting
    assert "existing operator account" in troubleshooting
    assert "--op troubleshooting/snapshot" in troubleshooting
    assert "NEVER conclude `BP healthy`" in troubleshooting
    assert "block_production_ready: true" in troubleshooting
    deploy = (ROOT / "ouro-skills/deploy/SKILL.md").read_text()
    normalized_deploy = " ".join(deploy.split())
    for phrase in [
        "ouro-ops inbox preview --type tx",
        "--artifact-file <same-signed-tx> --plan",
        "WAIT for the operator's exact approval",
        "accepted_by_node",
        "each input's exact live-node UTxO presence",
        "sampled live slot proves the validity check but is not semantic candidate drift",
        "guaranteed-invalid rejection-path acceptance fixture",
        "only one candidate-bound rejection test",
        "it does not prove ledger inclusion",
        "submission_ambiguous",
        "Ouro never retries",
        "Deploy takes no fleet permit",
        "retired resident model",
    ]:
        assert phrase in normalized_deploy, f"Deploy Skill lacks contract {phrase!r}"
    assert "must be ADOPTED" not in deploy
    assert "target-installed Ouro" in deploy
    assert "Never use `ouro-ops tool run deploy/register-submit`, `inbox stage`" in deploy
    print("skill docs passed")


if __name__ == "__main__":
    main()
