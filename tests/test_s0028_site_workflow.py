#!/usr/bin/env python3
from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]


def main():
    workflow_path = ROOT / ".github" / "workflows" / "site.yml"
    workflow = workflow_path.read_text()
    assert yaml.safe_load(workflow)
    for required in (
        "workflow_dispatch:",
        "pull_request:",
        "branches:",
        "- main",
        "environment: production",
        "github.ref == 'refs/heads/main'",
        "cloudflare/wrangler-action@v3",
        "CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_ACCOUNT_ID",
        "workingDirectory: web/onboarding",
        'wranglerVersion: "4.107.0"',
        "command: deploy",
        "id: deploy",
        "steps.deploy.outputs.deployment-url",
        "packaging/verify-production-site.py --url",
    ):
        assert required in workflow, required
    for forbidden in (
        "pull_request_target",
        "preview",
        "versions upload",
        "pull-requests: write",
        "github-script",
        "upload-artifact",
        "required-reviewers",
        "wait-timer",
        "- next",
    ):
        assert forbidden.lower() not in workflow.lower(), forbidden

    deploy = workflow.split("\n  deploy:\n", 1)[1]
    assert "github.event_name == 'pull_request'" not in deploy
    assert deploy.index("if:") < deploy.index("CLOUDFLARE_API_TOKEN")
    assert "secrets." not in workflow.split("\n  deploy:\n", 1)[0]

    wrangler = (ROOT / "web" / "onboarding" / "wrangler.jsonc").read_text()
    assert '"name": "ouro-ops-site"' in wrangler
    assert '"directory": "./dist"' in wrangler
    assert "main" not in wrangler and "compatibility_flags" not in wrangler
    print("S0028 PR-safe Site production workflow passed")


if __name__ == "__main__":
    main()
