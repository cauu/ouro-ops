#!/usr/bin/env python3
"""S0017 p5-1 — static cardano-cli managed-mode adapter gate.

Every dispatched L2 script must call cardano-cli through the adapter `ouro_cardano_cli` /
`ouro_cardano_cli_available` in ouro-lib.sh, NOT the raw `cardano-cli` binary. Reason: in
docker/podman mode the node — and its cardano-cli — live INSIDE the container, so a raw host
`cardano-cli` would not exist. The adapter dispatches to `<runtime> exec <cid> cardano-cli` for a
containerized node and to the host binary otherwise. A raw invocation anywhere else silently breaks
container deployments.

This gate FORBIDS raw `cardano-cli` (and `command -v cardano-cli`) in any skill script except the
adapter itself, and asserts the adapter defines the API. Comments and quoted strings are stripped
first (so a `cardano-cli` mention in a message/heredoc is fine — only real invocations trip it).

Standalone: `python3 tests/test_cardano_cli_gate.py`.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKILLS = ROOT / "ouro-skills"
ADAPTER = SKILLS / "lib" / "ouro-lib.sh"
# `cardano-cli` as a real command word (not `ouro_cardano_cli`, which has underscores).
RAW_RE = re.compile(r"(?<![\w-])cardano-cli\b")


def strip_noncode(line):
    """Drop shell comments and quoted-string spans so only unquoted code remains."""
    out, i, n, quote = [], 0, len(line), None
    while i < n:
        c = line[i]
        if quote:
            if c == quote:
                quote = None
            i += 1
            continue
        if c == "#":
            break
        if c in ("'", '"'):
            quote = c
            i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def main():
    failures = []

    # 1. the adapter defines the API.
    adapter_text = ADAPTER.read_text()
    for fn in ("ouro_cardano_cli()", "ouro_cardano_cli_available()", "ouro_cardano_cli_resolve()"):
        if fn not in adapter_text:
            failures.append(f"adapter missing definition: {fn}")

    # 2. no raw cardano-cli command word in any skill script except the adapter.
    for path in sorted(SKILLS.rglob("*.sh")):
        if path.resolve() == ADAPTER.resolve():
            continue
        for lineno, raw in enumerate(path.read_text().splitlines(), 1):
            code = strip_noncode(raw)
            if RAW_RE.search(code):
                rel = path.relative_to(ROOT)
                failures.append(f"{rel}:{lineno}: raw cardano-cli — use ouro_cardano_cli: {raw.strip()}")

    if failures:
        print("FAIL — cardano-cli adapter gate:")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("PASS — cardano-cli adapter gate: all skill scripts route cardano-cli through the adapter")


if __name__ == "__main__":
    main()
