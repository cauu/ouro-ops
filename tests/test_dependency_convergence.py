#!/usr/bin/env python3
import json
import sqlite3
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REMOVED = [
    "ansible",
    "src-tauri",
    "src",
    "sidecar",
    "package.json",
    "pnpm-lock.yaml",
    "skills-lock.json",
]
FORBIDDEN_REFS = [
    "tauri",
    "@tauri-apps",
    "ansible-playbook",
    "pnpm",
    "vite",
]


def main():
    for rel in REMOVED:
        assert not (ROOT / rel).exists(), f"{rel} should not remain"

    searchable = [
        ROOT / "Cargo.toml",
        ROOT / "Makefile",
        ROOT / "ci",
        ROOT / "crates",
        ROOT / "ouro-skills",
    ]
    text = "\n".join(
        path.read_text(errors="ignore")
        for base in searchable
        for path in ([base] if base.is_file() else base.rglob("*"))
        if path.is_file()
    ).lower()
    for token in FORBIDDEN_REFS:
        assert token not in text, f"forbidden old dependency reference remains: {token}"

    db = Path("/tmp/ouro-legacy-inspect.sqlite3")
    db.unlink(missing_ok=True)
    conn = sqlite3.connect(db)
    conn.execute("create table audit_events (id text)")
    conn.execute("create table tasks (id text)")
    conn.execute("create table machines (id text)")
    conn.commit()
    conn.close()

    result = subprocess.run(
        ["cargo", "run", "-q", "--", "legacy", "inspect", "--db", str(db)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        check=True,
    )
    payload = json.loads(result.stdout)
    assert set(payload["data"]["migrated_tables"]) == {"audit_events", "tasks"}
    assert "machines" in payload["data"]["skipped_tables"]
    print("dependency convergence passed")


if __name__ == "__main__":
    main()
