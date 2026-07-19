#!/usr/bin/env python3
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
# Exact public Skill set: local control CLI + existing cardano SSH + ephemeral runner.
STATELESS_SKILLS = [
    ROOT / "ouro-skills/upgrade/SKILL.md",
    ROOT / "ouro-skills/runtime/SKILL.md",
    ROOT / "ouro-skills/observability/SKILL.md",
    ROOT / "ouro-skills/kes-rotation/SKILL.md",
    ROOT / "ouro-skills/troubleshooting/SKILL.md",
    ROOT / "ouro-skills/deploy/SKILL.md",
]
SKILLS = STATELESS_SKILLS
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
        # Website generator/CLI compatibility metadata.
        meta = _front_matter(text)
        assert str(meta.get("skill_version", "")).isdigit(), \
            f"{path} front matter needs integer skill_version, got {meta.get('skill_version')!r}"
        assert re.match(r"^(>=|>|=|\^|~)?\d+\.\d+\.\d+", meta.get("requires_ouro", "")), \
            f"{path} front matter needs semver requires_ouro, got {meta.get('requires_ouro')!r}"
        assert meta.get("requires_contract") == "1", \
            f"{path} front matter needs current requires_contract"
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
    assert "typed BP observation supplies the current KES period automatically" in " ".join(kes.split())
    assert "Use managed health to determine" not in kes, \
        "KES Skill must not infer KES lifetime from the fixed tip-only health read"
    for phrase in [
        "kes-rotation/stage-key",
        "kes-rotation/discard-stage",
        "pending_existing: true",
        "choose: continue this pending rotation, or discard it",
        "Do not silently choose",
        "ouro-ops kes airgap-bundle",
        "cardano_cli_version",
        "M-series Mac",
        "Intel/AMD Linux",
        "uname -s",
        "uname -m",
        "mac-apple-silicon",
        "linux-arm",
        "the five fixed files",
        "never restage",
        "air-gapped machine",
        "copy only that PUBLIC file back",
        "ouro-ops kes airgap-cleanup",
        "<pool-spec-dir>/ouro-kes-rotation/<bp>/pending/node.cert",
        "Never ask for a path, attachment, pasted certificate bytes",
        "--artifact-preflight",
        "changed: false",
        "executor_available: false",
        "signature/key/counter/window evidence",
        "three-file backup/promotion/restart plan",
        "This is the production workflow",
        "historical test that stopped after this preflight",
        "credentials/normalize-forging-permissions",
        "accepts no path, mode or owner parameter",
        "needs confirmation but no fleet permit",
        "rollback availability",
        "a real BP container restart",
        "actually activated",
        "node_state_counter_status: no_blocks_minted_yet",
        "cold_identity_bound: true",
        "ordinary BP readiness remains",
        "kes_rotation_repair_ready",
        "target_kes_rotation_repair_ready: true",
        "target_online: false",
        "every other disruptive operation",
        "keys_directory_safe",
        "kes_skey_private",
        "vrf_skey_private",
        "forging_key_owner_supported",
        "target_kes_rotation_permissions",
        "`forging_key_permissions_safe:false` alone is not a KES refusal",
        "takes no fleet permit",
        "already invalid/expired active",
        "not Phase-A success gates",
        "staging directory is absent",
        "rollback files were removed",
    ]:
        assert phrase in kes, f"KES Skill lacks Phase A/B contract {phrase!r}"
    for unnecessary_prompt in [
        "operator-named PUBLIC KES",
        "Require a current target KES period",
        "operator-named-cold-sign-script",
        "ouro-ops kes cold-sign-script",
    ]:
        assert unnecessary_prompt not in kes, f"KES Skill still asks for unnecessary input {unnecessary_prompt!r}"
    assert "do not ask for another file-write go-ahead or any output path" in " ".join(kes.split())
    assert "<operator-named-public-opcert>" not in kes
    assert "substitute a mock certificate" in kes
    assert "Do not treat null as zero" in kes
    assert ".discarded-*` copy after success" in kes
    assert "ouro-ops kes push" not in kes, "KES Skill still directs the agent to legacy kes push"
    upgrade = (ROOT / "ouro-skills/upgrade/SKILL.md").read_text()
    assert "ouro-ops inbox preview" not in " ".join(upgrade.split())
    assert "--artifact-file" not in " ".join(upgrade.split())
    assert "upgrade/preload-image" in upgrade
    assert "ouro-ops release select --platform linux/amd64" in " ".join(upgrade.split())
    assert "never has to maintain a local allowlist file" in " ".join(upgrade.split())
    assert "ghcr.io/blinklabs-io/cardano-node@sha256:<platform-manifest>" in upgrade
    assert "planning performed no pull or mutation" in " ".join(upgrade.split())
    assert "active-container invariance" in " ".join(upgrade.split())
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
