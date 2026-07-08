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
    assert_valid("examples/pool-spec.minimal.yaml")
    assert_valid("examples/pool-spec.complete.yaml")
    assert_invalid("tests/fixtures/pool-spec/invalid-network-magic.yaml")
    assert_invalid("tests/fixtures/pool-spec/invalid-plain-secret.yaml")
    print("pool-spec schema fixtures passed")


if __name__ == "__main__":
    main()
