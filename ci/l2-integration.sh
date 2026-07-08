#!/usr/bin/env bash
set -euo pipefail

python3 tests/test_pool_spec_schema.py
python3 tests/test_tool_output_schema.py
python3 tests/test_deploy_scripts.py
python3 tests/test_upgrade_scripts.py
python3 tests/test_runtime_observability_scripts.py
python3 tests/test_takeover_scripts.py
python3 tests/test_script_pairing.py
python3 tests/test_skill_docs.py
python3 tests/test_security_negative.py
python3 tests/test_parity_audit.py
python3 tests/test_retirement_inventory.py
cargo test -q
