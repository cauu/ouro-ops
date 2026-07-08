#!/usr/bin/env python3
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
AUDIT = ROOT / "docs/parity/S0014-parity-audit.md"
REQUIRED = [
    "commands/kes.rs",
    "commands/pool.rs",
    "commands/staking.rs",
    "keychain.rs",
    "db/",
    "deploy orchestration",
    "upgrade orchestration",
    "runtime config/restart",
    "observability bootstrap/rollback",
    "React UI",
    "Python sidecar",
    "Ansible playbooks",
]


def main():
    text = AUDIT.read_text()
    assert "Status: pass" in text
    for item in REQUIRED:
        assert item in text, f"missing parity item {item}"
    print("parity audit passed")


if __name__ == "__main__":
    main()
