#!/usr/bin/env python3
"""S0019 p3-3 (§2.12/§2.13/§2.15) — completeness gates for the three tables.

1. Every operation in the deny-by-default registry (crates/ouro/src/intent.rs) appears in the
   supported-operations table (docs/S0019-operations.md) — no registered op is unclassified.
2. The threat-model table names an enforcing component (§) AND a test for each row.
3. The audit-event schema is a valid, closed (additionalProperties:false) JSON Schema.
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    # 1. registry ⊆ operations table
    intent_src = (ROOT / "crates/ouro/src/intent.rs").read_text()
    reg_ops = set(re.findall(r'operation_id:\s*"([^"]+)"', intent_src))
    # drop the ParamSpec/struct-field false positives: keep only skill/script-shaped ids
    reg_ops = {o for o in reg_ops if "/" in o}
    ops_doc = (ROOT / "docs/S0019-operations.md").read_text()
    for op in reg_ops:
        assert op in ops_doc, f"registered op {op} is missing from the operations table (§2.15)"
    assert reg_ops, "expected at least one registered operation"

    # 2. threat model: every table row maps a component (§) and a test
    tm = (ROOT / "docs/S0019-threat-model.md").read_text()
    rows = [l for l in tm.splitlines() if l.startswith("|") and "§" in l and "Adversary" not in l]
    assert len(rows) >= 12, f"threat matrix too thin ({len(rows)} rows)"
    for r in rows:
        cols = [c.strip() for c in r.strip("|").split("|")]
        assert len(cols) == 3, f"threat row malformed: {r}"
        assert "§" in cols[1], f"row names no enforcing component: {r}"
        assert cols[2], f"row names no negative test: {r}"
    assert "OUT OF SCOPE" in tm and "bootstrap credential" in tm, "TCB / P0-1 boundary must be stated"

    # 3. audit schema is valid + closed
    schema = json.loads((ROOT / "schemas/audit-event.schema.json").read_text())
    assert schema.get("additionalProperties") is False, "audit event schema must be closed"
    assert "adopt" in schema["properties"]["event"]["enum"]
    assert "sealed" in schema["properties"]["event"]["enum"]

    print("S0019 completeness gates passed")


if __name__ == "__main__":
    main()
