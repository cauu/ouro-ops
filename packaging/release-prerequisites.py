#!/usr/bin/env python3
"""Read-only verifier for S0028 GitHub and Cloudflare release wiring."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
from typing import Any


REQUIRED_ENVIRONMENT = "production"
REQUIRED_SECRETS = {"CLOUDFLARE_ACCOUNT_ID", "CLOUDFLARE_API_TOKEN"}
REQUIRED_WORKER = "ouro-ops-site"


class ProbeError(RuntimeError):
    pass


def gh_json(endpoint: str, *, allow_missing: bool = False) -> Any:
    result = subprocess.run(
        ["gh", "api", "-H", "X-GitHub-Api-Version: 2026-03-10", endpoint],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        if allow_missing and ("HTTP 404" in result.stderr or "Not Found" in result.stderr):
            return None
        raise ProbeError(f"GitHub API probe failed for {endpoint}: {result.stderr.strip()}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ProbeError(f"GitHub API returned invalid JSON for {endpoint}") from error


def live_snapshot(repo: str) -> dict[str, Any]:
    base = f"repos/{repo}"
    environment = gh_json(
        f"{base}/environments/{REQUIRED_ENVIRONMENT}", allow_missing=True
    )
    secrets = (
        gh_json(f"{base}/environments/{REQUIRED_ENVIRONMENT}/secrets")
        if environment is not None
        else {"secrets": []}
    )
    branch_policies: dict[str, Any] = {"branch_policies": []}
    if (
        environment is not None
        and environment.get("deployment_branch_policy", {}).get(
            "custom_branch_policies"
        )
    ):
        branch_policies = gh_json(
            f"{base}/environments/{REQUIRED_ENVIRONMENT}/deployment-branch-policies"
        )
    return {
        "immutable_releases": gh_json(
            f"{base}/immutable-releases", allow_missing=True
        ),
        "actions_workflow_permissions": gh_json(
            f"{base}/actions/permissions/workflow"
        ),
        "rulesets": gh_json(f"{base}/rulesets?includes_parents=true"),
        "environment": environment,
        "environment_secrets": secrets,
        "deployment_branch_policies": branch_policies,
    }


def worker_name(path: pathlib.Path) -> str | None:
    text = path.read_text()
    match = re.search(r'^\s*"name"\s*:\s*"([^"]+)"', text, re.MULTILINE)
    return match.group(1) if match else None


def fact(
    key: str, configured: bool, observed: Any, required: Any, action: str
) -> dict[str, Any]:
    return {
        "key": key,
        "configured": configured,
        "observed": observed,
        "required": required,
        "action": None if configured else action,
    }


def evaluate(snapshot: dict[str, Any], wrangler: pathlib.Path) -> dict[str, Any]:
    immutable = snapshot.get("immutable_releases") or {}
    environment = snapshot.get("environment")
    protection_rules = environment.get("protection_rules", []) if environment else []
    forbidden_protection = sorted(
        {
            rule.get("type", "unknown")
            for rule in protection_rules
            if rule.get("type") in {"required_reviewers", "wait_timer"}
        }
    )
    branch_policy = (
        environment.get("deployment_branch_policy") if environment else None
    )
    custom_names = {
        policy.get("name")
        for policy in snapshot.get("deployment_branch_policies", {}).get(
            "branch_policies", []
        )
    }
    branch_restricted = bool(
        branch_policy
        and (
            branch_policy.get("protected_branches")
            or (
                branch_policy.get("custom_branch_policies")
                and "main" in custom_names
            )
        )
    )
    secret_names = {
        secret.get("name")
        for secret in snapshot.get("environment_secrets", {}).get("secrets", [])
    }
    permissions = snapshot.get("actions_workflow_permissions") or {}
    rulesets = snapshot.get("rulesets")
    local_worker = worker_name(wrangler)

    facts = [
        fact(
            "immutable_releases",
            immutable.get("enabled") is True,
            immutable.get("enabled", False),
            True,
            "Enable immutable releases in repository settings.",
        ),
        fact(
            "actions_settings_readable",
            permissions.get("default_workflow_permissions") in {"read", "write"},
            permissions.get("default_workflow_permissions"),
            "readable Actions workflow permissions",
            "Grant the verifier Actions read access and confirm repository Actions settings.",
        ),
        fact(
            "repository_rules_readable",
            isinstance(rulesets, list),
            len(rulesets) if isinstance(rulesets, list) else None,
            "rulesets API readable",
            "Grant repository metadata read access and review rules affecting release writes.",
        ),
        fact(
            "production_environment",
            environment is not None,
            environment.get("name") if environment else None,
            REQUIRED_ENVIRONMENT,
            "Create the GitHub production environment.",
        ),
        fact(
            "production_no_manual_gate",
            environment is not None and not forbidden_protection,
            forbidden_protection,
            [],
            "Remove production required reviewers and wait timers.",
        ),
        fact(
            "production_branch_restriction",
            environment is not None and branch_restricted,
            branch_policy,
            "protected branch or custom main-only policy",
            "Restrict production deployment to protected branches or a custom main policy.",
        ),
        fact(
            "production_secret_names",
            REQUIRED_SECRETS <= secret_names,
            sorted(secret_names & REQUIRED_SECRETS),
            sorted(REQUIRED_SECRETS),
            "Add the missing Cloudflare secrets to the production environment.",
        ),
        fact(
            "worker_identity",
            local_worker == REQUIRED_WORKER,
            local_worker,
            REQUIRED_WORKER,
            f'Set wrangler.jsonc name to "{REQUIRED_WORKER}".',
        ),
    ]
    missing = [item["key"] for item in facts if not item["configured"]]
    return {
        "schema_version": 1,
        "status": "ready" if not missing else "prerequisites_missing",
        "changed": False,
        "secret_values_read": False,
        "worker": REQUIRED_WORKER,
        "facts": facts,
        "missing": missing,
        "release_write_proof": "deferred_to_p5_real_release",
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="cauu/ouro-ops")
    parser.add_argument("--snapshot", type=pathlib.Path)
    parser.add_argument(
        "--wrangler-config",
        type=pathlib.Path,
        default=pathlib.Path("web/onboarding/wrangler.jsonc"),
    )
    parser.add_argument(
        "--require-ready",
        action="store_true",
        help="exit non-zero when external prerequisites are missing",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        snapshot = (
            json.loads(args.snapshot.read_text())
            if args.snapshot
            else live_snapshot(args.repo)
        )
        result = evaluate(snapshot, args.wrangler_config)
    except (OSError, json.JSONDecodeError, ProbeError) as error:
        result = {
            "schema_version": 1,
            "status": "probe_failed",
            "changed": False,
            "secret_values_read": False,
            "error": str(error),
        }
        print(json.dumps(result, sort_keys=True))
        return 2
    print(json.dumps(result, sort_keys=True))
    return 1 if args.require_ready and result["status"] != "ready" else 0


if __name__ == "__main__":
    sys.exit(main())
