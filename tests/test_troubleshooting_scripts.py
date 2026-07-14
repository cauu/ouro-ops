#!/usr/bin/env python3
"""S0017 p5-18 gates for the troubleshooting surface.

1. logs classifier: taxonomy categories hit on fixture lines (and do not false-positive on
   clean lines); excerpts are bounded; output is schema-valid.
2. service probe: on a nodeless host it honestly reports not-running (schema-valid, exit 20).
3. read-only static gate: troubleshooting scripts contain no mutation verbs — read-only is
   proven, not claimed. (The diag channel's read-only-ness is proven in rust: ouro-diag has
   no sudoers entry and the ssh argv carries no sudo.)
"""
import json
import re
import subprocess
import tempfile
from pathlib import Path

import jsonschema

from _ctx import ROOT, tool_run

SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())
HOME = "/tmp/ouro-troubleshooting-home"

FIXTURE_DIRTY = """\
[relay1:cardano.node.ChainDB:Info] Chain extended, new tip
[relay1:cardano.node.DiskFull:Error] writeBlock: No space left on device
[relay1:cardano.node.Forge:Error] InvalidKesSignatureOCERT period check failed
[relay1:cardano.node.Handshake:Warning] HandshakeError refused version
[relay1:cardano.node.Clock:Warning] TraceBlockFromFuture slot ahead (clock skew?)
plain info line, nothing wrong here
"""

FIXTURE_CLEAN = """\
[relay1:cardano.node.ChainDB:Info] Chain extended, new tip
[relay1:cardano.node.Mempool:Info] tx added
"""


def run(tool, machine="bp1", env=None, check=True):
    result = tool_run(tool, machine=machine, env=env or {}, home=HOME, check=check)
    payload = json.loads(result.stdout)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return result, payload


def main():
    subprocess.run(["rm", "-rf", HOME], check=True)

    # 1a. dirty fixture: the four seeded categories are found, exit 20, excerpts bounded.
    with tempfile.NamedTemporaryFile("w", suffix=".log", delete=False) as f:
        f.write(FIXTURE_DIRTY)
        dirty = f.name
    result, payload = run("troubleshooting/logs", env={"OURO_LOGS_SOURCE": dirty}, check=False)
    assert result.returncode == 20, f"dirty logs should exit 20, got {result.returncode}"
    cats = set(payload["data"]["findings"])
    assert {"disk_full", "kes_invalid", "network_handshake", "clock_skew"} <= cats, cats
    for finding in payload["data"]["findings"].values():
        assert finding["count"] >= 1
        assert all(len(e) <= 200 for e in finding["excerpts"])
        assert len(finding["excerpts"]) <= 3

    # 1b. clean fixture: no findings, exit 0 — the classifier does not cry wolf.
    with tempfile.NamedTemporaryFile("w", suffix=".log", delete=False) as f:
        f.write(FIXTURE_CLEAN)
        clean = f.name
    result, payload = run("troubleshooting/logs", env={"OURO_LOGS_SOURCE": clean})
    assert result.returncode == 0
    assert payload["data"]["findings"] == {}

    # 2. service on a nodeless host: honest not-running, schema-valid, exit 20.
    result, payload = run("troubleshooting/service", check=False)
    assert result.returncode == 20, f"nodeless service should exit 20, got {result.returncode}"
    assert payload["data"]["running"] is False

    # 3. read-only static gate: no mutation verbs in troubleshooting scripts.
    forbidden = re.compile(r"\b(rm -rf|rm -f|mv |cp |chmod|chown|kill\b|restart\b|truncate\b|mkfs|dd )")
    for script in (ROOT / "ouro-skills/troubleshooting/scripts").glob("*.sh"):
        for i, line in enumerate(script.read_text().splitlines(), 1):
            code = line.split("#", 1)[0]
            assert not forbidden.search(code), f"{script.name}:{i} mutation verb in read-only script: {line.strip()}"

    print("troubleshooting scripts passed")


if __name__ == "__main__":
    main()
