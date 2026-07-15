#!/usr/bin/env python3
import json
from pathlib import Path

import jsonschema
import yaml


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT / "schemas" / "pool-spec.schema.json").read_text())


def load_yaml(path):
    return yaml.safe_load((ROOT / path).read_text())


def assert_valid(path):
    jsonschema.Draft202012Validator(SCHEMA).validate(load_yaml(path))


def assert_invalid(path):
    validator = jsonschema.Draft202012Validator(SCHEMA)
    errors = sorted(validator.iter_errors(load_yaml(path)), key=lambda e: list(e.path))
    assert errors, f"{path} unexpectedly passed"


def main():
    assert_valid("examples/pool-spec.minimal.yaml")  # p2-4: no runtime => still valid (optional)
    assert_valid("examples/pool-spec.complete.yaml")  # p2-4: declared runtime (systemd + docker)
    # p5-12: operation-scoped omission — no economics/node_version/sync/upgrade is still valid.
    assert_valid("tests/fixtures/pool-spec/valid-operation-scoped.yaml")
    assert_invalid("tests/fixtures/pool-spec/invalid-network-magic.yaml")
    assert_invalid("tests/fixtures/pool-spec/invalid-plain-secret.yaml")
    assert_invalid("tests/fixtures/pool-spec/invalid-runtime-mode.yaml")  # p2-4: bad mode enum

    # S0019 may use a bootstrap account in the operator-owned spec; the retired ouro-exec equality
    # must not disagree with Rust validation. The schema still rejects an SSH-option injection.
    bootstrap_spec = load_yaml("examples/pool-spec.minimal.yaml")
    bootstrap_spec["machines"][0]["ssh"]["user"] = "cardano"
    jsonschema.Draft202012Validator(SCHEMA).validate(bootstrap_spec)
    bootstrap_spec["machines"][0]["ssh"]["user"] = "-oProxyCommand=evil"
    errors = list(jsonschema.Draft202012Validator(SCHEMA).iter_errors(bootstrap_spec))
    assert errors, "unsafe ssh.user unexpectedly passed"
    print("pool-spec schema fixtures passed")


if __name__ == "__main__":
    main()
