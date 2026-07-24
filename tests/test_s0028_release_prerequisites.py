import json
import pathlib
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "packaging" / "release-prerequisites.py"
WRANGLER = ROOT / "web" / "onboarding" / "wrangler.jsonc"


def configured_snapshot():
    return {
        "immutable_releases": {"enabled": True, "enforced_by_owner": False},
        "actions_workflow_permissions": {
            "default_workflow_permissions": "read",
            "can_approve_pull_request_reviews": False,
        },
        "rulesets": [],
        "environment": {
            "name": "production",
            "protection_rules": [{"type": "branch_policy"}],
            "deployment_branch_policy": {
                "protected_branches": True,
                "custom_branch_policies": False,
            },
        },
        "environment_secrets": {
            "secrets": [
                {"name": "CLOUDFLARE_ACCOUNT_ID"},
                {"name": "CLOUDFLARE_API_TOKEN"},
            ]
        },
        "deployment_branch_policies": {"branch_policies": []},
    }


class ReleasePrerequisitesTests(unittest.TestCase):
    def run_snapshot(self, snapshot, *extra):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "snapshot.json"
            path.write_text(json.dumps(snapshot))
            completed = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--snapshot",
                    str(path),
                    "--wrangler-config",
                    str(WRANGLER),
                    *extra,
                ],
                check=False,
                capture_output=True,
                text=True,
            )
        return completed, json.loads(completed.stdout)

    def test_configured_snapshot_is_ready_without_secret_values(self):
        completed, result = self.run_snapshot(configured_snapshot(), "--require-ready")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(result["status"], "ready")
        self.assertFalse(result["changed"])
        self.assertFalse(result["secret_values_read"])
        self.assertEqual(result["missing"], [])
        self.assertEqual(result["worker"], "ouro-ops-site")

    def test_missing_snapshot_is_typed_and_non_mutating(self):
        snapshot = configured_snapshot()
        snapshot["immutable_releases"] = None
        snapshot["environment"]["protection_rules"].append(
            {"type": "required_reviewers"}
        )
        snapshot["environment_secrets"]["secrets"] = []
        completed, result = self.run_snapshot(snapshot)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(result["status"], "prerequisites_missing")
        self.assertFalse(result["changed"])
        self.assertFalse(result["secret_values_read"])
        self.assertEqual(
            result["missing"],
            [
                "immutable_releases",
                "production_no_manual_gate",
                "production_secret_names",
            ],
        )
        actions = {
            item["key"]: item["action"]
            for item in result["facts"]
            if not item["configured"]
        }
        self.assertTrue(all(actions.values()))

    def test_require_ready_fails_only_after_reporting_facts(self):
        snapshot = configured_snapshot()
        snapshot["environment"] = None
        snapshot["environment_secrets"] = {"secrets": []}
        completed, result = self.run_snapshot(snapshot, "--require-ready")
        self.assertEqual(completed.returncode, 1)
        self.assertEqual(result["status"], "prerequisites_missing")
        self.assertIn("production_environment", result["missing"])


if __name__ == "__main__":
    unittest.main()
