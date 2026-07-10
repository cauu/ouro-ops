#!/usr/bin/env python3
import json
import subprocess
from pathlib import Path

from _ctx import ROOT, ouro_bin

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

    # A capability may not be marked `pass` while its replacement is only "planned":
    # every claimed-migrated command must actually run.
    assert "planned `ouro-ops pool overview`" not in text, "parity claims a planned (unbuilt) overview"

    # Executable parity for the retired Delegators/staking point-in-time view: the
    # replacement `ouro-ops pool overview` must run and return structured pool facts.
    result = subprocess.run(
        [ouro_bin(), "pool", "overview", "--spec", "examples/pool-spec.minimal.yaml"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        check=True,
    )
    payload = json.loads(result.stdout)
    overview = payload["data"]
    assert overview["pool"]["ticker"] == "OURO"
    assert overview["pool"]["network"] == "preprod"
    assert isinstance(overview["relays"], list) and overview["relays"]
    assert "creds://" not in result.stdout

    print("parity audit passed")


if __name__ == "__main__":
    main()
