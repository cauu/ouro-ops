#!/usr/bin/env python3
"""S0025 p5-2 — current docs/site/CI describe one release-ready architecture."""

from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    release = (ROOT / ".github/workflows/release.yml").read_text()
    release_check = (ROOT / ".github/workflows/release-check.yml").read_text()
    release_publish = (ROOT / ".github/workflows/release-publish.yml").read_text()
    site = (ROOT / ".github/workflows/site.yml").read_text()
    yaml.safe_load(release)
    yaml.safe_load(release_check)
    yaml.safe_load(release_publish)
    yaml.safe_load(site)
    assert "tests/test_release_candidate.py" in release_check
    assert "workflow_dispatch:" in release and "workflow_dispatch:" in release_publish
    assert "actions/attest@v4" in release_publish
    assert 'gh release create "$TAG"' in release_publish
    assert "pull_request:" not in release_publish and "\n  push:" not in release_publish
    assert "./web/onboarding/build.sh" in site
    assert "tests/test_web_generator.py" in site
    assert "cloudflare/wrangler-action@v3" in site
    assert "environment: production" in site
    assert "github.ref == 'refs/heads/main'" in site
    for forbidden in ("pull_request_target", "versions upload", "github-script"):
        assert forbidden.lower() not in site.lower(), f"site workflow contains {forbidden}"

    current_docs = [
        ROOT / "README.md",
        ROOT / "docs/S0016-threat-model.md",
        ROOT / "docs/S0019-threat-model.md",
        ROOT / "docs/S0020-operations.md",
        ROOT / "packaging/RELEASE.md",
        ROOT / "web/onboarding/README.md",
        ROOT / "data/releases.json",
    ]
    joined = "\n".join(path.read_text() for path in current_docs)
    assert "ouro-ops skill show" not in joined
    assert "ouro-ops tool run" not in joined
    assert "GitHub Releases in `cauu/ouro-ops` are the only supported CLI distribution channel" in joined
    assert "packaging/verified-reinstall.sh" in joined
    assert "self-update" not in joined
    assert "release@ouro.example" not in joined
    assert "RWQPLACEHOLDER" not in joined
    assert "ghcr.io/blinklabs-io/cardano-node" in joined
    assert "image layers" in joined

    page = (ROOT / "web/onboarding/index.html").read_text()
    assert "connect-src 'none'" in page
    assert "fetch(" not in page
    assert "node_version:" not in page
    assert "release-paired ephemeral Linux runner" in page
    assert "./target/release-candidate-control/release/ouro-ops" in page
    assert "fall back to any" in page
    assert "brew install ouro/tap/ouro" not in page
    assert "ouro.example/install.sh" not in page
    print("current release/site/documentation boundaries passed")


if __name__ == "__main__":
    main()
