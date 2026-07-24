#!/usr/bin/env python3
import importlib.util
import pathlib
import shutil
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "packaging" / "check-site-cli-floor.py"
SPEC = importlib.util.spec_from_file_location("site_cli_floor", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(MODULE)


def run(skills, released):
    return subprocess.run(
        [
            "python3",
            str(SCRIPT),
            "--skills-root",
            str(skills),
            "--released",
            released,
        ],
        check=False,
        capture_output=True,
        text=True,
    )


def main():
    accepted = run(ROOT / "ouro-skills", "v0.1.0")
    assert accepted.returncode == 0, accepted
    refused = run(ROOT / "ouro-skills", "v0.0.9")
    assert refused.returncode == 1 and '"cloudflare_write_allowed": false' in refused.stdout
    prerelease = run(ROOT / "ouro-skills", "v0.1.1-rc.1")
    assert prerelease.returncode == 2

    with tempfile.TemporaryDirectory() as tmp:
        skills = pathlib.Path(tmp) / "skills"
        shutil.copytree(ROOT / "ouro-skills", skills)
        deploy = skills / "deploy" / "SKILL.md"
        deploy.write_text(deploy.read_text().replace('requires_ouro: ">=0.1.0"', 'requires_ouro: ">=0.2.0"'))
        blocked = run(skills, "v0.1.9")
        assert blocked.returncode == 1
        ready = run(skills, "v0.2.0")
        assert ready.returncode == 0

    site = (ROOT / ".github" / "workflows" / "site.yml").read_text()
    floor = site.index("Enforce CLI floor before any Cloudflare write")
    cloudflare = site.index("cloudflare/wrangler-action@v3")
    assert floor < cloudflare
    assert 'gh release view --repo cauu/ouro-ops' in site
    publish = (ROOT / ".github" / "workflows" / "release-publish.yml").read_text()
    release = publish.index('gh release create "$TAG"')
    dispatch = publish.index("gh workflow run site.yml --ref main")
    assert release < dispatch
    print("S0028 Site CLI floor ordering passed")


if __name__ == "__main__":
    main()
