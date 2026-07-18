#!/usr/bin/env python3
"""S0025 p5-2 — current docs/site/CI describe one release-ready architecture."""

from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    release = (ROOT / ".github/workflows/release.yml").read_text()
    site = (ROOT / ".github/workflows/site.yml").read_text()
    yaml.safe_load(release)
    yaml.safe_load(site)
    assert "make release-candidate" in release
    assert "tests/test_release_candidate.py" in release
    assert "release-standard-not-published" in release
    for forbidden in ("tauri-action", "upload-artifact", "GITHUB_TOKEN", "releaseDraft"):
        assert forbidden not in release, f"CLI candidate workflow still publishes via {forbidden}"
    assert "./web/onboarding/build.sh" in site
    assert "tests/test_web_generator.py" in site
    for forbidden in ("wrangler", "upload-artifact", "CLOUDFLARE_API_TOKEN"):
        assert forbidden.lower() not in site.lower(), f"site workflow still deploys via {forbidden}"

    current_docs = [
        ROOT / "README.md",
        ROOT / "docs/S0016-threat-model.md",
        ROOT / "docs/S0019-threat-model.md",
        ROOT / "docs/S0020-operations.md",
        ROOT / "packaging/RELEASE.md",
        ROOT / "web/onboarding/README.md",
    ]
    joined = "\n".join(path.read_text() for path in current_docs)
    assert "ouro-ops skill show" not in joined
    assert "ouro-ops tool run" not in joined
    assert "release-standard-not-published" in joined
    assert "ghcr.io/blinklabs-io/cardano-node" in joined
    assert "image layers" in joined

    page = (ROOT / "web/onboarding/index.html").read_text()
    assert "connect-src 'none'" in page
    assert "fetch(" not in page
    assert "node_version:" not in page
    assert "release-paired ephemeral Linux runner" in page
    assert "brew install ouro/tap/ouro" not in page
    assert "ouro.example/install.sh" not in page
    print("current release/site/documentation boundaries passed")


if __name__ == "__main__":
    main()
