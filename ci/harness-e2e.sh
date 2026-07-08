#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export OURO_HOME=/tmp/ouro-harness-home
export OURO_STATE_DIR=/tmp/ouro-harness-state
rm -rf "$OURO_HOME" "$OURO_STATE_DIR" /tmp/ouro-harness-*

# Build once so `ouro tool run` and the in-script `ouro tool verify-context`
# callback resolve to the same binary.
cargo build -q
OURO="$ROOT/target/debug/ouro"
SPEC=examples/pool-spec.minimal.yaml

"$OURO" spec validate --spec "$SPEC" >/tmp/ouro-harness-spec.json

# Deploy flow — every write goes through the audited `ouro tool run` entrypoint
# (no bare `bash script.sh`; the L2 gate now requires a CLI-signed audit context).
"$OURO" tool run deploy/preflight --spec "$SPEC" --machine bp1 >/tmp/ouro-harness-deploy-preflight.json
"$OURO" tool run deploy/provision --spec "$SPEC" --machine bp1 >/tmp/ouro-harness-deploy-provision.json
"$OURO" tool run deploy/sync --spec "$SPEC" --machine bp1 >/tmp/ouro-harness-deploy-sync.json
"$OURO" tool run deploy/start --spec "$SPEC" --machine bp1 >/tmp/ouro-harness-deploy-start.json
OURO_STATUS_SNAPSHOT=tests/fixtures/deploy/verify-healthy.json \
  "$OURO" tool run deploy/verify --spec "$SPEC" --machine bp1 >/tmp/ouro-harness-deploy-verify.json

# Upgrade — single-relay demo topology, so the operator explicitly accepts the brief
# relay downtime via the quorum override (a multi-relay pool would not need it).
OURO_QUORUM_MIN_RELAYS=0 \
  "$OURO" tool run upgrade/run --spec "$SPEC" >/tmp/ouro-harness-upgrade.json

# KES rotation with a real, out-of-band confirmation token.
"$OURO" kes counter status --state tests/fixtures/kes/counter-state.json >/tmp/ouro-harness-kes-counter.json
"$OURO" kes generate --spec "$SPEC" --machine bp1 --out /tmp/ouro-harness-kes >/tmp/ouro-harness-kes-generate.json
"$OURO" confirm create --action kes-push --machine bp1 --ttl 60s >/tmp/ouro-harness-confirm.json
TOKEN="$(python3 - <<'PY'
import json
print(json.load(open('/tmp/ouro-harness-confirm.json'))['data']['token'])
PY
)"
"$OURO" kes push --spec "$SPEC" --machine bp1 --cert tests/fixtures/kes/node-cert-valid.json --counter-state tests/fixtures/kes/counter-state.json --confirm-token "$TOKEN" >/tmp/ouro-harness-kes-push.json

# Point-in-time staking facts via the read-only overview (replaces the Delegators UI).
"$OURO" pool overview --spec "$SPEC" >/tmp/ouro-harness-pool-overview.json

# Takeover of an existing legacy node.
OURO_LEGACY_MANIFEST=tests/fixtures/deploy/legacy-manifest.json \
  "$OURO" tool run deploy/takeover --spec "$SPEC" --machine bp1 >/tmp/ouro-harness-takeover.json
OURO_LEGACY_MANIFEST=tests/fixtures/deploy/legacy-manifest.json \
  "$OURO" tool run deploy/takeover-verify --spec "$SPEC" --machine bp1 >/tmp/ouro-harness-takeover-verify.json

python3 - <<'PY'
import glob, json
for path in glob.glob('/tmp/ouro-harness-*.json'):
    payload = json.load(open(path))
    assert payload['status'] == 'ok', path
print('harness e2e passed')
PY
