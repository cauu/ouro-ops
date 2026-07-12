#!/usr/bin/env python3
"""S0017 p1-7 — the P0-1 convenience-mode honest label must not silently disappear.

P0-1 (user decision): the bootstrap credential is NOT mechanism-isolated from the agent; the only
retained, non-defensive obligation is to say so honestly in every operator-facing place — the
`ouro-ops init` output, the spec, and the packaging docs — and to NEVER claim it IS isolated.

This gate freezes that: the honest note is present in the init output + packaging, and no doc
makes the false positive claim. Fast, standalone: `python3 tests/test_honest_labeling.py`.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HONEST = "NOT mechanism-isolated from the agent"

failures = []
def check(cond, msg):
    if not cond:
        failures.append(msg)


def main():
    cli = (ROOT / "crates/ouro/src/cli.rs").read_text()
    rel = (ROOT / "packaging/RELEASE.md").read_text()

    # 1. present in the init output (security_note) and the packaging docs.
    check(HONEST in cli, "ouro-ops init security_note must carry the honest convenience-mode label")
    check(HONEST in rel, "packaging/RELEASE.md must carry the honest convenience-mode label")

    # 2. no FALSE positive claim of isolation. Check at SENTENCE granularity: any sentence that
    #    mentions both "credential" and "isolated" must carry a negation/prohibition cue
    #    (not / never / no / false) — a bare positive claim of isolation fails.
    NEG = ("not", "never", " no ", "false")
    for name, text in (("cli.rs", cli), ("RELEASE.md", rel)):
        for sentence in re.split(r'(?<=[.;])\s+|\n', text):
            low = sentence.lower()
            if "credential" in low and "isolated" in low:
                check(any(n in low for n in NEG),
                      f"{name} appears to falsely claim the credential IS isolated: {sentence.strip()!r}")

    if failures:
        print("FAIL — honest-labeling gate:")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("PASS — honest-labeling gate: convenience-mode note present (init + packaging), no false isolation claim")


if __name__ == "__main__":
    main()
