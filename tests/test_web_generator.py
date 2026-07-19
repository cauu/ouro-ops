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


def test_release_form_build_has_exact_canonical_skills() -> None:
    html = build()
    data = payload(html)
    assert set(data) == set(PUBLIC)
    for operation, relative in PUBLIC.items():
        canonical = (ROOT / "ouro-skills" / relative).read_text(encoding="utf-8")
        item = data[operation]
        assert item["content"] == canonical
        assert isinstance(item["skill_version"], int) and item["skill_version"] > 0
        assert re.fullmatch(r">=\d+\.\d+\.\d+", item["requires_ouro"])
        assert item["requires_contract"] == 1
        assert html.count(json.dumps(canonical, ensure_ascii=False)[1:-1]) <= 1
    assert "__OURO_PUBLIC_SKILLS_JSON__" not in html
    assert "skill.content.trimEnd()" in html
    assert "BEGIN OURO-SKILL.MD" in html
    assert "END OURO-SKILL.MD" in html
    assert "never its machine id" in html
    assert "diag exec --dispatch" in html
    assert "./target/release-candidate-control/release/ouro-ops" in html
    assert "Replace the leading bare command name ouro-ops" in html
    assert "inspect, invoke, install, overwrite, or fall back" in html
    assert "already authorizes writing the enclosed local pool-spec.yaml" in html
    assert "do not ask again for their paths" in html
    assert "before any remote operational-state or chain write" in html
    assert "The Skill's mandatory first action is therefore exactly:" in html
    assert "ouro-ops kes airgap-bundle" in html
    assert "M-series Mac" in html and "Intel/AMD Linux" in html
    assert "uname -s" in html and "uname -m" in html
    kes_prompt = payload(html)["kes-rotation"]["content"]
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
