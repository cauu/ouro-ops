#!/usr/bin/env python3
"""S0027 TC-2 acceptance: verify the signed recommended image against its bootstrap facts.

The exact platform manifest must already be present locally. This keeps the gate read-only with
respect to registries; the acceptance harness performs the signed `docker pull` explicitly.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RELEASES = ROOT / "data" / "releases.json"
REPOSITORY = "ghcr.io/blinklabs-io/cardano-node"


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )


def main() -> None:
    catalog = json.loads(RELEASES.read_text())
    daemon_arch = run("docker", "info", "--format", "{{.Architecture}}").stdout.strip()
    architecture = {
        "x86_64": "amd64",
        "amd64": "amd64",
        "arm64": "arm64",
        "aarch64": "arm64",
    }[daemon_arch]
    platform_name = f"linux/{architecture}"
    config_digest = catalog["recommended"][platform_name]
    contract = catalog["contracts"][0]
    image = next(
        value
        for value in contract["allowed"]
        if value["platform"] == platform_name
        and value["image_config_digest"] == config_digest
    )
    reference = f"{REPOSITORY}@{image['platform_manifest_digest']}"

    inspected = json.loads(run("docker", "image", "inspect", reference).stdout)[0]
    assert inspected["Id"] == config_digest
    assert reference in inspected["RepoDigests"]
    assert f"{inspected['Os']}/{inspected['Architecture']}" == platform_name
    config = inspected["Config"]
    assert config["Entrypoint"] == ["/usr/local/bin/entrypoint"]
    assert config["Cmd"] is None
    assert config["User"] == ""
    assert set(config["ExposedPorts"]) >= {"12798/tcp", "3001/tcp"}

    probe = r"""
set -eu
for tool in cardano-node cardano-cli mithril-client nview txtop; do
  command -v "$tool"
done
for network in mainnet preprod preview; do
  sha256sum "/opt/cardano/config/$network/config.json"
  cardano-cli hash genesis-file \
    --genesis "/opt/cardano/config/$network/shelley-genesis.json"
  test -s "/opt/cardano/config/$network/genesis.vkey"
  test -s "/opt/cardano/config/$network/ancillary.vkey"
done
cat /usr/local/bin/run-node
"""
    observed = run(
        "docker",
        "run",
        "--rm",
        "--platform",
        platform_name,
        "--entrypoint",
        "/bin/sh",
        reference,
        "-c",
        probe,
    ).stdout
    for binary in contract["deploy"]["required_binaries"]:
        assert f"/{binary}" in observed
    for network, facts in contract["deploy"]["networks"].items():
        assert facts["config_sha256"] in observed, network
        assert facts["genesis_hash"] in observed, network
    for marker in (
        "if ! test -e ${CARDANO_DATABASE_PATH}/protocolMagicId",
        "mithril-client cardano-db download",
        'Detected populated ${CARDANO_DATABASE_PATH}... skipping restore',
        "CARDANO_SOCKET_PATH=${CARDANO_SOCKET_PATH:-/ipc/node.socket}",
        "--topology ${CARDANO_TOPOLOGY}",
        "if [[ ${CARDANO_BLOCK_PRODUCER} == true ]]",
    ):
        assert marker in observed, marker

    print(
        f"S0027 signed image contract passed: {platform_name} "
        f"{image['release']} {config_digest}"
    )


if __name__ == "__main__":
    main()
