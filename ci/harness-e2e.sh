#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export OURO_HOME=/tmp/ouro-harness-home
export OURO_STATE_DIR=/tmp/ouro-harness-state
rm -rf "$OURO_HOME" "$OURO_STATE_DIR" /tmp/ouro-harness-*

cargo run -q -- spec validate --spec examples/pool-spec.minimal.yaml >/tmp/ouro-harness-spec.json

export OURO_AUDIT_ID=audit-harness-deploy
export OURO_TOOL_NAME=deploy/preflight
export OURO_SPEC=examples/pool-spec.minimal.yaml
export OURO_MACHINE=bp1
bash ouro-skills/deploy/scripts/preflight.sh >/tmp/ouro-harness-deploy-preflight.json
bash ouro-skills/deploy/scripts/provision.sh >/tmp/ouro-harness-deploy-provision.json
bash ouro-skills/deploy/scripts/sync.sh >/tmp/ouro-harness-deploy-sync.json
bash ouro-skills/deploy/scripts/start.sh >/tmp/ouro-harness-deploy-start.json
export OURO_STATUS_SNAPSHOT=tests/fixtures/deploy/verify-healthy.json
bash ouro-skills/deploy/scripts/verify.sh >/tmp/ouro-harness-deploy-verify.json

export OURO_AUDIT_ID=audit-harness-upgrade
export OURO_TOOL_NAME=upgrade/run
bash ouro-skills/upgrade/scripts/run.sh >/tmp/ouro-harness-upgrade.json

cargo run -q -- kes counter status --state tests/fixtures/kes/counter-state.json >/tmp/ouro-harness-kes-counter.json
cargo run -q -- kes generate --spec examples/pool-spec.minimal.yaml --machine bp1 --out /tmp/ouro-harness-kes >/tmp/ouro-harness-kes-generate.json
cargo run -q -- confirm create --action kes-push --machine bp1 --ttl 60s >/tmp/ouro-harness-confirm.json
TOKEN="$(python3 - <<'PY'
import json
print(json.load(open('/tmp/ouro-harness-confirm.json'))['data']['token'])
PY
)"
cargo run -q -- kes push --spec examples/pool-spec.minimal.yaml --machine bp1 --cert tests/fixtures/kes/node-cert-valid.json --counter-state tests/fixtures/kes/counter-state.json --confirm-token "$TOKEN" >/tmp/ouro-harness-kes-push.json

export OURO_AUDIT_ID=audit-harness-takeover
export OURO_TOOL_NAME=deploy/takeover
export OURO_LEGACY_MANIFEST=tests/fixtures/deploy/legacy-manifest.json
bash ouro-skills/deploy/scripts/takeover.sh >/tmp/ouro-harness-takeover.json
export OURO_TOOL_NAME=deploy/takeover-verify
bash ouro-skills/deploy/scripts/takeover-verify.sh >/tmp/ouro-harness-takeover-verify.json

python3 - <<'PY'
import glob, json
for path in glob.glob('/tmp/ouro-harness-*.json'):
    payload = json.load(open(path))
    assert payload['status'] == 'ok', path
print('harness e2e passed')
PY
