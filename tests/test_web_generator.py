#!/usr/bin/env python3
"""S0025 website source-fidelity, serialization, and local-service gates."""

from __future__ import annotations

import importlib.util
import json
import re
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / "web/onboarding"
SOURCE = SITE / "index.html"
DIST = SITE / "dist/index.html"
PUBLIC = {
    "observability": "observability/SKILL.md",
    "troubleshooting": "troubleshooting/SKILL.md",
    "runtime": "runtime/SKILL.md",
    "upgrade": "upgrade/SKILL.md",
    "kes-rotation": "kes-rotation/SKILL.md",
    "deploy": "deploy/SKILL.md",
}


def load_generator():
    spec = importlib.util.spec_from_file_location("ouro_site_generator", SITE / "generate.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GENERATOR = load_generator()


def build() -> str:
    subprocess.run([str(SITE / "build.sh")], cwd=ROOT, check=True)
    return DIST.read_text(encoding="utf-8")


def payload(html: str) -> dict[str, object]:
    match = re.search(r"const SKILL_PAYLOADS = (\{[^\n]+\});", html)
    assert match, "built page must contain one serialized Skill payload"
    return json.loads(match.group(1))


def verified_reinstall(html: str) -> str:
    match = re.search(r"const VERIFIED_REINSTALL = (\".*\");", html)
    assert match, "built page must contain one serialized verified reinstall source"
    return json.loads(match.group(1))


def section(text: str, heading: str) -> str:
    match = re.search(rf"^{re.escape(heading)}\n.*?(?=^## |\Z)", text, re.MULTILINE | re.DOTALL)
    assert match, f"embedded Skill lacks {heading}"
    return match.group(0).rstrip()


def test_release_form_build_has_exact_canonical_skills() -> None:
    html = build()
    data = payload(html)
    assert set(data) == set(PUBLIC)
    ssh_sections = []
    for operation, relative in PUBLIC.items():
        canonical = (ROOT / "ouro-skills" / relative).read_text(encoding="utf-8")
        item = data[operation]
        assert item["content"] == canonical
        ssh_sections.append(section(item["content"], "## SSH account discovery"))
        normalized = " ".join(item["content"].split())
        for phrase in [
            "ask whether every declared machine uses the same SSH username or different usernames",
            "ask for that username once",
            "machine-id → SSH-username mapping",
            "Stop if any machine remains unresolved",
            "Never ask for a password, private-key content",
        ]:
            assert phrase in normalized, f"{operation} embedded Skill lacks {phrase!r}"
        assert isinstance(item["skill_version"], int) and item["skill_version"] > 0
        assert re.fullmatch(r">=\d+\.\d+\.\d+", item["requires_ouro"])
        assert item["requires_contract"] == 1
        assert html.count(json.dumps(canonical, ensure_ascii=False)[1:-1]) <= 1
    assert len(set(ssh_sections)) == 1
    assert "__OURO_PUBLIC_SKILLS_JSON__" not in html
    assert "__OURO_VERIFIED_REINSTALL_JSON__" not in html
    assert verified_reinstall(html) == (
        ROOT / "packaging" / "verified-reinstall.sh"
    ).read_text(encoding="utf-8")
    assert "skill.content.trimEnd()" in html
    assert "BEGIN OURO-SKILL.MD" in html
    assert "END OURO-SKILL.MD" in html
    assert "never its machine id" in html
    assert "diag exec --dispatch" in html
    assert "$HOME/.local/bin/ouro-ops" in html
    assert "./target/release-candidate-control/release/ouro-ops" not in html
    assert "Replace the leading bare command name ouro-ops" in html
    assert "do not invoke or fall back to another ouro-ops on path" in html.lower()
    assert "already authorizes writing the enclosed local pool-spec.yaml" in html
    assert "do not ask again for their paths" in html
    assert "before any remote operational-state or chain write" in html
    assert "The Skill's mandatory first action is therefore exactly:" in html
    assert "${skill.requires_ouro}" in html
    assert "${skill.requires_contract}" in html
    assert "mac: VERIFIED_REINSTALL" in html and "linux: VERIFIED_REINSTALL" in html
    assert "ouro-ops kes airgap-bundle" in html
    assert "M-series Mac" in html and "Intel/AMD Linux" in html
    assert "uname -s" in html and "uname -m" in html
    kes_prompt = payload(html)["kes-rotation"]["content"]
    upgrade_prompt = payload(html)["upgrade"]["content"]
    deploy_prompt = payload(html)["deploy"]["content"]
    troubleshooting_prompt = payload(html)["troubleshooting"]["content"]
    normalized_upgrade_prompt = " ".join(upgrade_prompt.split())
    normalized_deploy_prompt = " ".join(deploy_prompt.split())
    assert "result.container.orchestration" in upgrade_prompt
    assert "Compose manual handoff" in upgrade_prompt
    assert "docker compose -p <project> -f <config-file> config" in upgrade_prompt
    assert "docker compose -p <project> -f <config-file> up -d --no-deps <service>" in upgrade_prompt
    assert "Agent 不得执行 raw docker/compose 写操作" in upgrade_prompt
    assert "Do not create or request a transaction" in upgrade_prompt
    assert "quote `orchestration_reason`" in upgrade_prompt
    assert "Upgrade does not walk an intermediate version chain" in upgrade_prompt
    assert "Exact transition metadata is optional and never blocks an upgrade" in upgrade_prompt
    assert "preserves an observed `json-file` log driver" in normalized_upgrade_prompt
    assert "`max-file` and `max-size` rotation options" in normalized_upgrade_prompt
    assert "N→N+1 transition must be present" not in upgrade_prompt
    for phrase in [
        "one non-producing bootstrap BP and one or more operational Relays",
        "ouro-ops deploy inspect --spec <pool-spec.yaml>",
        "Never run, answer or automate this command",
        "one explicit approval and WAIT",
        "ouro-ops deploy apply --spec <pool-spec.yaml>",
        "ouro-ops deploy check --spec <pool-spec.yaml>",
        "separate BP Bootstrap capability",
    ]:
        assert phrase in normalized_deploy_prompt
    assert "signed transaction submission" not in normalized_deploy_prompt
    assert "Deploy a fresh BP + Relay Fleet" in html
    assert "ticker:" not in deploy_prompt
    assert "Upgrade to signed recommended release" in html
    assert "included Skill's \"SSH account discovery\" section" in html
    assert "This wrapper adds no SSH-account decision" in html
    assert "__SSH_USER_${m.id.toUpperCase().replaceAll" in html
    assert "user: cardano" not in html
    assert "existing cardano account" not in html
    assert "Never ask for a password, private-key content" in upgrade_prompt
    assert "`counter_status: no_blocks_minted_yet`" in troubleshooting_prompt
    assert "An untyped/missing or `unavailable` counter remains insufficient evidence" in (
        troubleshooting_prompt
    )
    assert "pending_existing: true" in kes_prompt
    assert "choose: continue this pending rotation, or discard it" in kes_prompt
    assert "kes-rotation/discard-stage" in kes_prompt
    assert "Do not silently choose" in kes_prompt
    assert "staging directory is absent" in kes_prompt
    assert "ouro-ops kes airgap-cleanup" in kes_prompt
    assert "<pool-spec-dir>/ouro-kes-rotation/<bp>/pending/node.cert" in kes_prompt
    assert "Never ask for a path, attachment, pasted certificate bytes" in kes_prompt
    assert "This is the production workflow" in kes_prompt
    assert "historical test that stopped after this preflight" in kes_prompt
    assert "exactly one real BP container restart" in kes_prompt
    assert "activation_unverified" in kes_prompt
    assert "automatic_rollback_performed: false" in kes_prompt
    assert "activation_pending: true" in kes_prompt
    assert "activation_resumed: true" in kes_prompt
    assert "node_state_counter_status: no_blocks_minted_yet" in kes_prompt
    assert "cold_identity_bound: true" in kes_prompt
    assert "Do not treat null as zero" in kes_prompt
    assert "target_qualification: kes_rotation_repair_ready" in kes_prompt
    assert "target_kes_rotation_repair_ready: true" in kes_prompt
    assert "target_online: false" in kes_prompt
    assert "target_kes_rotation_permissions" in kes_prompt
    assert "keys_directory_safe" in kes_prompt
    assert "normalize-forging-permissions" not in kes_prompt
    assert "`forging_key_permissions_safe:false` alone is not a KES refusal" in kes_prompt
    assert "<operator-named-public-opcert>" not in kes_prompt
    assert "ouro-ops kes cold-sign-script" not in kes_prompt
    assert "ouro-ops skill show" not in html
    assert html.count('data-op="') == 6


def test_animated_decorations_cannot_expand_document_width() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    rendered = build()
    for html in (source, rendered):
        assert "html,body{overflow-x:clip}" in html
        assert "animation:aura-drift 10s ease-in-out infinite alternate" in html
        assert ".hero{position:relative;overflow:visible}" in html


def test_navigation_links_to_the_official_github_repository() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    rendered = build()
    for html in (source, rendered):
        assert html.count('href="https://github.com/cauu/ouro-ops"') == 1
        assert 'class="github-link"' in html
        assert 'target="_blank"' in html
        assert 'rel="noopener noreferrer"' in html
        assert 'aria-label="Ouro Ops on GitHub"' in html
        assert ".github-link svg{display:block;width:17px;height:17px}" in html
        assert html.index('id="lang"') < html.index('class="github-link"')
        assert "@media (max-width:640px)" in html
        assert ".mark span{display:none}" in html


def test_wrapper_delegates_ssh_policy_to_canonical_skill() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    for retired_registration_field in [
        'id="f-registration"',
        'id="f-ticker"',
        "pledge_lovelace",
        "metadata_url",
        "registration:true",
    ]:
        assert retired_registration_field not in source
    for duplicate_policy in [
        "ask me whether every machine uses the",
        "same SSH username or different usernames",
        "ask me for that username once",
        "ask me for the username of each",
        "Do not assume cardano",
        "Do not ask for passwords or private-key",
        "Until every placeholder is resolved",
        "agent first asks whether machines share one SSH username",
        "agent 会先确认所有机器共用一个 SSH 用户名还是各自不同",
        "agent 會先確認所有機器共用一個 SSH 使用者名稱或各自不同",
        "全マシン共通の SSH ユーザー名か個別アカウントかを先に確認",
    ]:
        assert duplicate_policy not in source, \
            f"website wrapper/UI duplicates canonical Skill policy: {duplicate_policy!r}"
    assert "Resolve every __SSH_USER_<MACHINE_ID>__ placeholder exactly as required by the" in source
    assert "included Skill's \"SSH account discovery\" section" in source
    assert "This wrapper adds no SSH-account decision" in source
    for delegated_ui_copy in [
        "canonical Skill is the sole source",
        "仅由生成 Prompt 中内嵌的 canonical Skill 规定",
        "僅由生成 Prompt 中內嵌的 canonical Skill 規定",
        "canonical Skill のみが規定します",
    ]:
        assert delegated_ui_copy in source


def test_page_keeps_payload_inert_and_network_bounded() -> None:
    html = build()
    assert "default-src 'none'" in html
    assert "connect-src 'none'" in html
    assert not re.search(r"<script[^>]+src=", html)
    assert not re.search(r"<link[^>]+href=", html)
    fetches = re.findall(r"fetch\((.*?)\)", html, re.DOTALL)
    assert fetches == [], "the prompt generator must not make ambient network requests"
    assert 'id="copy-anyway"' in html
    assert "await clip(pendingPrompt)" in html
    assert html.index('document.execCommand("copy")') < html.index("navigator.clipboard.writeText(text)")
    assert "observed=await navigator.clipboard.readText()" in html
    assert 'throw new Error("clipboard readback mismatch")' in html
    assert '$("disclose-dlg").close("ok")' in html
    assert 'dlg.addEventListener("close"' not in html
    assert '$("prompt-out").textContent = pr' in html
    assert "innerHTML = skill" not in html


def test_operation_tiles_have_visible_icon_backgrounds() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    themes = re.findall(r'<span class="tile (t-[a-z]+)" aria-hidden="true">', source)
    assert len(themes) == 6
    for theme in themes:
        assert re.search(
            rf"\.{re.escape(theme)}\s*\{{[^}}]*background\s*:", source
        ), f"{theme} has no visible background; its white SVG icon will disappear"
    deploy = re.search(
        r'<button[^>]+data-op="deploy".*?</button>', source, re.DOTALL
    )
    assert deploy
    assert 'class="tile t-coral"' in deploy.group(0)
    assert deploy.group(0).count("<path ") >= 1


def test_built_inline_javascript_parses() -> None:
    html = build()
    scripts = re.findall(r"<script>(.*?)</script>", html, re.DOTALL)
    assert len(scripts) == 1, "the static page must keep one auditable inline script"
    with tempfile.TemporaryDirectory() as td:
        script = Path(td) / "onboarding.js"
        script.write_text(scripts[0], encoding="utf-8")
        checked = subprocess.run(
            ["node", "--check", str(script)], text=True, capture_output=True
        )
    assert checked.returncode == 0, checked.stderr


def copy_public_skills(destination: Path) -> None:
    for relative in PUBLIC.values():
        source = ROOT / "ouro-skills" / relative
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)


def test_script_terminator_in_skill_is_safely_serialized() -> None:
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        skills = tmp / "skills"
        dist = tmp / "dist"
        copy_public_skills(skills)
        target = skills / PUBLIC["observability"]
        attack = "\n</script><script>globalThis.ouroPwned=true</script>\n"
        target.write_text(target.read_text(encoding="utf-8") + attack, encoding="utf-8")
        output = GENERATOR.build(SOURCE, skills, dist)
        html = output.read_text(encoding="utf-8")
        assert attack.strip() not in html
        assert "\\u003c/script\\u003e\\u003cscript\\u003e" in html
        assert payload(html)["observability"]["content"].endswith(attack)


@pytest.mark.parametrize(
    "mutation, expected",
    [
        (lambda text: text.replace("requires_contract: 1\n", "", 1), "requires_contract"),
        (
            lambda text: text.replace("requires_contract: 1\n", "requires_contract: 1\nextra: no\n", 1),
            "unknown front-matter",
        ),
        (
            lambda text: text.replace("requires_contract: 1\n", "requires_contract: nope\n", 1),
            "must be integers",
        ),
    ],
)
def test_invalid_front_matter_fails_build(mutation, expected: str) -> None:
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        skills = tmp / "skills"
        copy_public_skills(skills)
        target = skills / PUBLIC["runtime"]
        target.write_text(mutation(target.read_text(encoding="utf-8")), encoding="utf-8")
        with pytest.raises(ValueError, match=expected):
            GENERATOR.build(SOURCE, skills, tmp / "dist")


def test_missing_skill_fails_build() -> None:
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        skills = tmp / "skills"
        copy_public_skills(skills)
        (skills / PUBLIC["deploy"]).unlink()
        with pytest.raises(ValueError, match="missing canonical public Skill"):
            GENERATOR.build(SOURCE, skills, tmp / "dist")


def test_built_site_is_served_over_local_http() -> None:
    expected = build().encode()
    with socket.socket() as reserve:
        reserve.bind(("127.0.0.1", 0))
        port = reserve.getsockname()[1]
    process = subprocess.Popen(
        [str(SITE / "serve-local.sh"), str(port)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        for _ in range(40):
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=1) as response:
                    assert response.status == 200
                    assert response.read() == expected
                    break
            except OSError:
                time.sleep(0.05)
        else:
            raise AssertionError("local production-form site did not become reachable")
    finally:
        process.terminate()
        process.wait(timeout=5)
