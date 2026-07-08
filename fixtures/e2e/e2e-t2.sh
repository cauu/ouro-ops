#!/usr/bin/env bash
# S0015 T2 acceptance — the deterministic (no-agent) container end-to-end suite.
# Brings up the bed in an isolated compose project, provisions it, and asserts the
# E2E-* criteria for p1 (dispatch, real side effects, idempotency, principal isolation,
# audit authority, forged-context rejection), then tears down and checks for leaks.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-t2}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
# assert a command FAILS (used for negative security cases)
refute() { if "$@" >/dev/null 2>&1; then fail "expected failure but succeeded: $*"; fi; }

cleanup() { docker compose -f "$CF" down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

echo "[bed] up + provision (project=$OURO_E2E_PROJECT)"
docker compose -f "$CF" up -d --build --wait >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

# E2E-15 (isolation): the compose project name is honored (per-run isolation via env).
docker compose -f "$CF" ps --format '{{.Name}}' | grep -q "^${OURO_E2E_PROJECT}-bp1" \
  && pass "E2E-15 project isolation: name=$OURO_E2E_PROJECT" || fail "project name not honored"

# E2E-0: control SSHes to a target as ouro-exec by pubkey.
docker compose -f "$CF" exec -T control \
  ssh -i /root/.ouro/credentials/bp1 -o BatchMode=yes -o StrictHostKeyChecking=accept-new \
  ouro-exec@bp1 true && pass "E2E-0 ssh handshake ouro-exec@bp1" || fail "E2E-0 ssh"

# E2E-1: remote dispatch produces a REAL side effect on the target.
out1=$(docker compose -f "$CF" exec -T control ouro tool run deploy/provision --dispatch bp1 --spec /opt/ouro/pool-spec.yaml)
echo "$out1" | grep -q '"changed":true' || fail "E2E-1 first run not changed=true"
docker compose -f "$CF" exec -T bp1 test -f /opt/cardano/config/bp1/config.json || fail "E2E-1 no real config on target"
pass "E2E-1 dispatch -> real /opt/cardano/config/bp1/config.json"

# E2E-4: second dispatch is idempotent (no extra side effect).
out2=$(docker compose -f "$CF" exec -T control ouro tool run deploy/provision --dispatch bp1 --spec /opt/ouro/pool-spec.yaml)
echo "$out2" | grep -q '"changed":false' || fail "E2E-4 second run not changed=false"
pass "E2E-4 idempotent (changed=false)"

# audit is target-authoritative: the write is recorded on bp1, not on control.
docker compose -f "$CF" exec -T bp1 ouro audit log --limit 8 | grep -q '"event":"finish"' || fail "no finish audit on target"
docker compose -f "$CF" exec -T control ouro audit log --limit 8 | grep -q '"events":\[\]' || fail "control audit not empty"
pass "audit target-authoritative (writes on bp1, control empty)"

# E2E-2 (TC-14): ouro-diag cannot READ key material; dir 0700 + file 0400 both hold.
refute docker compose -f "$CF" exec -T -u ouro-diag bp1 cat /opt/cardano/keys/kes.skey
refute docker compose -f "$CF" exec -T -u ouro-diag bp1 bash -c 'tar cf /tmp/x /opt/cardano/keys/kes.skey'
docker compose -f "$CF" exec -T bp1 bash -c '[ "$(stat -c %a /opt/cardano/keys)" = 700 ] && [ "$(stat -c %a /opt/cardano/keys/kes.skey)" = 400 ]' \
  || fail "E2E-2 key perms not 0700/0400"
pass "E2E-2 diag key-read denied (cat/tar) + dir 0700 + file 0400"

# E2E-3 (TC-15): ouro-diag has no sudo; ouro-exec is confined to the tool-run wrapper.
refute docker compose -f "$CF" exec -T -u ouro-diag bp1 sudo -n /usr/local/sbin/ouro-tool-run deploy/preflight
refute docker compose -f "$CF" exec -T -u ouro-exec bp1 sudo -n /usr/local/bin/ouro confirm create --action x --machine bp1
pass "E2E-3 diag no-sudo; ouro-exec confined to tool-run wrapper (no other subcommands)"

# E2E-16 (TC-4): a forged audit context (env set, bogus token) is rejected on the target.
refute docker compose -f "$CF" exec -T \
  -e OURO_AUDIT_ID=fabricated -e OURO_TOOL_NAME=deploy/provision \
  -e OURO_INVOCATION_TOKEN=inv_bogus -e OURO_BIN=/usr/local/bin/ouro \
  bp1 bash /opt/ouro/ouro-skills/deploy/scripts/provision.sh
pass "E2E-16 forged audit context rejected on target"

# E2E-15 (teardown): down -v reaps everything for this project.
cleanup
trap - EXIT
left=$(docker ps -aq --filter "name=${OURO_E2E_PROJECT}" | wc -l | tr -d ' ')
nets=$(docker network ls --filter "name=${OURO_E2E_PROJECT}" -q | wc -l | tr -d ' ')
[ "$left" = 0 ] && [ "$nets" = 0 ] && pass "E2E-15 teardown clean (0 residual containers/networks)" || fail "E2E-15 residual: containers=$left networks=$nets"

echo "T2 E2E: ALL PASSED"
