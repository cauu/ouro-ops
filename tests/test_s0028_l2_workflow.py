#!/usr/bin/env python3
from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]


def main():
    workflow = (ROOT / ".github" / "workflows" / "l2-integration.yml").read_text()
    assert yaml.safe_load(workflow)
    assert "python3 -m venv /opt/ouro-ci-venv" in workflow
    assert (
        "/opt/ouro-ci-venv/bin/python -m pip install -r requirements-dev.txt"
        in workflow
    )
    assert 'export PATH="/opt/ouro-ci-venv/bin:$PATH"' in workflow
    dnf = workflow.split("dnf install -y ", 1)[1].splitlines()[0]
    assert "curl" not in dnf
    assert "nodejs" in dnf
    assert "python3-pip" in dnf
    apt = workflow.split("apt-get install -y ", 1)[1].splitlines()[0]
    assert "nodejs" in apt
    requirements = (ROOT / "requirements-dev.txt").read_text().lower()
    for dependency in ("jsonschema", "pyyaml", "pytest"):
        assert dependency in requirements
    print("S0028 L2 isolated dependency workflow passed")


if __name__ == "__main__":
    main()
