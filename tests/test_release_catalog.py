#!/usr/bin/env python3
"""S0020 p4-15 — signed no-cache release selection and failure boundaries."""

import hashlib
import hmac
import json
import os
import subprocess
import tempfile
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/ouro-ops"
RELEASES = ROOT / "data/releases.json"


def run(home, source, *extra, test_key=None):
    env = dict(
        os.environ,
        OURO_HOME=str(home),
        OURO_RELEASES_FILE=str(source),
    )
    if test_key:
        env["OURO_ALLOWLIST_TEST_KEY"] = test_key
    return subprocess.run(
        [str(BIN), "release", "select", "--platform", "linux/amd64", *extra],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
    )


def main():
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    schema = json.loads((ROOT / "schemas/release-catalog.schema.json").read_text())
    jsonschema.Draft202012Validator.check_schema(schema)
    jsonschema.validate(json.loads(RELEASES.read_text()), schema)
    with tempfile.TemporaryDirectory(prefix="ouro-release-catalog-") as temporary:
        home = Path(temporary)

        deploy = run(home, RELEASES)
        assert deploy.returncode == 0, deploy.stderr
        deploy_value = json.loads(deploy.stdout)
        assert deploy_value["data"]["selection"] == "deploy_recommended"
        assert deploy_value["data"]["repository"] == "ghcr.io/blinklabs-io/cardano-node"
        assert deploy_value["data"]["image"]["release"] == "11.0.1-1"
        bootstrap = deploy_value["data"]["deploy_bootstrap"]
        assert bootstrap["database_marker"] == "/data/db/protocolMagicId"
        assert bootstrap["metrics"] == {
            "container_port": 12798,
            "host_ip": "127.0.0.1",
            "host_port": 12798,
        }
        assert set(bootstrap["networks"]) == {"mainnet", "preprod", "preview"}
        assert bootstrap["required_binaries"] == [
            "cardano-cli",
            "cardano-node",
            "mithril-client",
            "nview",
            "txtop",
        ]
        assert deploy_value["data"]["cache_written"] is False
        assert list(home.iterdir()) == [], "release selection must not create local state"

        production = json.loads(RELEASES.read_text())
        bp_1053 = [
            image
            for contract in production["contracts"]
            for image in contract["allowed"]
            if image["release"] == "10.5.3-1" and image["platform"] == "linux/amd64"
        ]
        assert bp_1053 == [
            {
                "release": "10.5.3-1",
                "platform": "linux/amd64",
                "oci_index_digest": "sha256:ec379c67d1ef2f0e4478bf3b28ac16db3a62535d6af8f92d6d1e53766a382afb",
                "platform_manifest_digest": "sha256:3f2aa6636cae566d89faf44b4a1640fd1619b715306664c0d3db0b27dcb31dd4",
                "image_config_digest": "sha256:ea53539f722c08ced4df221e329438e1f48ae80ef196687753c2583081421905",
            }
        ]

        historical = [
            "sha256:ea53539f722c08ced4df221e329438e1f48ae80ef196687753c2583081421905",
            "sha256:a3223d93539d28e4f54e0b20dfc644a55387d5522a3d85b3b981eacff23c0c7a",
            "sha256:0fb74b5921860a6547ce5b6c669d59b71169d1c48b014f2fafcec2e4d382f1b3",
            "sha256:5fe0bf791a0af8884386479555996bf4ad7621493889625a2886039bf8734e51",
        ]
        for current in historical:
            upgrade = run(home, RELEASES, "--from", current)
            assert upgrade.returncode == 0, upgrade.stderr
            upgrade_value = json.loads(upgrade.stdout)
            assert upgrade_value["data"]["selection"] == "upgrade_recommended"
            assert upgrade_value["data"]["repository"] == "ghcr.io/blinklabs-io/cardano-node"
            assert upgrade_value["data"]["image"]["release"] == "11.0.1-1"
            assert upgrade_value["data"]["deploy_bootstrap"] is None
            for field in (
                "oci_index_digest",
                "platform_manifest_digest",
                "image_config_digest",
            ):
                assert upgrade_value["data"]["image"][field].startswith("sha256:")
            if current == historical[-1]:
                assert upgrade_value["data"]["transition"]["from_image_config_digest"] == current
            else:
                assert upgrade_value["data"]["transition"] is None

        already_current = deploy_value["data"]["image"]["image_config_digest"]
        already = run(home, RELEASES, "--from", already_current)
        assert already.returncode != 0
        assert "already the signed recommended release" in (already.stdout + already.stderr)
        assert list(home.iterdir()) == []

        # A newly signed catalog changes selection without rebuilding this binary.
        dynamic = json.loads(RELEASES.read_text())
        future = "sha256:" + "9" * 64
        dynamic["allowlist_version"] = 99
        template = dict(dynamic["contracts"][0]["allowed"][0])
        template.update(
            release="future-1",
            oci_index_digest="sha256:" + "7" * 64,
            platform_manifest_digest="sha256:" + "8" * 64,
            image_config_digest=future,
        )
        dynamic["contracts"][0]["allowed"].append(template)
        dynamic["recommended"]["linux/amd64"] = future
        dynamic["signature"] = "pending"
        unsigned = dict(dynamic)
        unsigned.pop("signature")
        canonical = json.dumps(unsigned, sort_keys=True, separators=(",", ":")).encode()
        test_key = "release-catalog-fixture-key"
        dynamic["signature"] = "test-hmac-sha256:" + hmac.new(
            test_key.encode(), canonical, hashlib.sha256
        ).hexdigest()
        future_file = home / "future.json"
        future_file.write_text(json.dumps(dynamic, separators=(",", ":")))
        selected_future = run(home, future_file, test_key=test_key)
        assert selected_future.returncode == 0, selected_future.stderr
        assert json.loads(selected_future.stdout)["data"]["image"]["release"] == "future-1"

        wrong_repository_doc = dict(dynamic)
        wrong_repository_doc["repository"] = "docker.io/untrusted/cardano-node"
        wrong_repository_doc["signature"] = "pending"
        unsigned = dict(wrong_repository_doc)
        unsigned.pop("signature")
        canonical = json.dumps(unsigned, sort_keys=True, separators=(",", ":")).encode()
        wrong_repository_doc["signature"] = "test-hmac-sha256:" + hmac.new(
            test_key.encode(), canonical, hashlib.sha256
        ).hexdigest()
        wrong_repository = home / "wrong-repository.json"
        wrong_repository.write_text(json.dumps(wrong_repository_doc, separators=(",", ":")))
        refused_repository = run(home, wrong_repository, test_key=test_key)
        assert refused_repository.returncode != 0
        assert "repository must be exactly" in (
            refused_repository.stdout + refused_repository.stderr
        )

        tampered = home / "tampered.json"
        tampered.write_text(RELEASES.read_text().replace("10.6.4-1", "10.6.4-evil"))
        refused = run(home, tampered)
        assert refused.returncode != 0
        assert "signature is invalid" in (refused.stdout + refused.stderr)

        unavailable = run(home, home / "missing.json")
        assert unavailable.returncode != 0
        assert "cannot read OURO_RELEASES_FILE" in (unavailable.stdout + unavailable.stderr)

    print("signed no-cache release selection passed")


if __name__ == "__main__":
    main()
