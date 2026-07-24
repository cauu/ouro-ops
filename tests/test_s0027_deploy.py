#!/usr/bin/env python3
"""S0027 TC-15..TC-18 integration gate.

The deterministic suite runs on every developer machine.  The destructive, real-host
Ubuntu acceptance is intentionally a separate user-operated harness:
fixtures/e2e/s0027/run.sh.
"""

import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(*command: str) -> None:
    subprocess.run(command, cwd=ROOT, check=True)


def main() -> None:
    for test in (
        "tests/test_s0027_ssh_trust.py",
        "tests/test_s0027_deploy_inspect.py",
        "tests/test_s0027_deploy_apply.py",
        "tests/test_s0027_deploy_check.py",
        "tests/test_s0027_deploy_absence.py",
        "tests/test_s0020_upgrade_workflow.py",
        "tests/test_release_catalog.py",
    ):
        run("python3", test)

    deploy = (ROOT / "crates/ouro/src/deploy.rs").read_text()
    production = deploy.split("#[cfg(test)]", 1)[0]
    assert 'docker_run pull "$image_ref"' in production
    assert "image inspect --format '{{.Id}}'" in production
    assert "CARDANO_BLOCK_PRODUCER" in production
    assert '"lifecycle": lifecycle_for(machine.role)' in production
    assert "accept-new" not in production
    assert "StrictHostKeyChecking=no" not in production
    assert "apt-get upgrade" not in production
    assert "ufw reset" not in production
    assert "docker compose down" not in production
    print("S0027 deterministic integration/security/lifecycle gate passed")


if __name__ == "__main__":
    main()
