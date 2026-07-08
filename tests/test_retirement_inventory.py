#!/usr/bin/env python3
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
A_CLASS_REMOVED = [
    "src",
    "sidecar",
    "prototype",
    "package.json",
    "pnpm-lock.yaml",
    "vite.config.ts",
    "tailwind.config.js",
    "index.html",
    "src-tauri/src/commands/monitor.rs",
    "src-tauri/src/commands/task.rs",
    "src-tauri/src/lib.rs",
    "src-tauri/src/main.rs",
    "src-tauri/tauri.conf.json",
    "src-tauri/capabilities",
]


def main():
    for rel in A_CLASS_REMOVED:
        assert not (ROOT / rel).exists(), f"{rel} should be retired"
    parity = (ROOT / "docs/parity/S0014-parity-audit.md").read_text()
    assert "telemetry basic-auth" in parity
    print("retirement inventory passed")


if __name__ == "__main__":
    main()
