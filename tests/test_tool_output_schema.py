#!/usr/bin/env python3
"""Validate current typed/meta CLI envelopes against the public output schema."""

import json
import subprocess
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT / "schemas/tool-output.schema.json").read_text())


def invoke(*args: str, check: bool = True):
    return subprocess.run(
        ["cargo", "run", "-q", "-p", "ouro", "--", *args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=check,
    )


def validate(text: str):
    payload = json.loads(text)
    jsonschema.Draft202012Validator(SCHEMA).validate(payload)
    return payload


def main():
    paths = validate(invoke("paths").stdout)
    assert paths["status"] == "ok" and paths["changed"] is False

    refused = invoke("tool", "run", "retired/status", check=False)
    assert refused.returncode != 0
    error = validate(refused.stdout)
    assert error["status"] == "error"
    assert "unknown command tool" in error["error"]["detail"]
    print("typed CLI output schema passed")


if __name__ == "__main__":
    main()
