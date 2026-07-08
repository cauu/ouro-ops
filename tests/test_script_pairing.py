#!/usr/bin/env python3
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_ROOT = ROOT / "ouro-skills"


def main():
    script_dirs = [
        SCRIPT_ROOT / "deploy/scripts",
        SCRIPT_ROOT / "upgrade/scripts",
        SCRIPT_ROOT / "runtime/scripts",
        SCRIPT_ROOT / "observability/scripts",
    ]
    for directory in script_dirs:
        assert (directory / "verify.sh").exists(), f"{directory} lacks verify.sh"
        scripts = {path.name for path in directory.glob("*.sh")}
        change_scripts = scripts - {"verify.sh"}
        assert change_scripts, f"{directory} has no change scripts"
        for script in change_scripts:
            text = (directory / script).read_text()
            assert "ouro_require_audit_context" in text, f"{script} lacks audit gate"
            assert "set -euo pipefail" in text, f"{script} lacks strict shell mode"

    lib = (SCRIPT_ROOT / "lib/ouro-lib.sh").read_text()
    assert "ouro_detect_package_manager" in lib
    assert "apt-get" in lib and "dnf" in lib
    assert "ouro_detect_firewall" in lib
    assert "ufw" in lib and "firewall-cmd" in lib
    print("script pairing and environment detection passed")


if __name__ == "__main__":
    main()
