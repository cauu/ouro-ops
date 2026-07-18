#!/usr/bin/env bash
set -euo pipefail

python3 tests/test_pool_spec_schema.py
python3 tests/test_tool_output_schema.py
python3 tests/test_skill_docs.py
python3 tests/test_external_skill_boundary.py
python3 -m pytest -q tests/test_web_generator.py
python3 tests/test_parity_audit.py
python3 tests/test_retirement_inventory.py
python3 tests/test_dependency_convergence.py
cargo test -q
