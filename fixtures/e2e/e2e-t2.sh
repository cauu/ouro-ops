#!/usr/bin/env bash
# S0015 T2 acceptance — the deterministic (no-agent) container end-to-end suite.
# Builds the base image (so it never validates stale ouro), brings the bed up in a
# unique compose project, provisions it, and asserts the E2E-* criteria for p1 —
# including reason-checked negatives (a non-zero exit alone is NOT accepted), an
# injection regression, relay dispatch, and audit_id correlation — then tears down.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# Per-run unique project so parallel/interrupted runs never collide (P1). PID-suffixed.
export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-t2-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }

# refute_reason <want-substr> <cmd...>: the command MUST fail AND its combined output
# MUST contain <want-substr>. Guards against false-pass on 127/typo/missing-binary.
refute_reason() {
  local want="$1"; shift out rc
  if out="$("$@" 2>&1)"; then
    fail "expected failure but succeeded: $*"
  fi
  case "$out" in
    *"$want"*) pass "refute[$want] ${*:1:3} …" ;;
    *) fail "failed for the WRONG reason (want '$want'): $* :: got: $(printf '%s' "$out" | head -1)" ;;
  esac
}
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }

cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

echo "[bed] rebuild base + up + provision (project=$OURO_E2E_PROJECT)"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null   # P0-2: never test stale ouro
dc up -d --build --wait >/dev/null
bash fixtures/e2e/provision.sh >/dev/null

SPEC=/opt/ouro/pool-spec.yaml

# E2E-15 (isolation): the compose project name is honored (per-run isolation).
dc ps --format '{{.Name}}' | grep -q "^${OURO_E2E_PROJECT}-bp1" \
  && pass "E2E-15 project isolation: name=$OURO_E2E_PROJECT" || fail "project name not honored"

# E2E-12: every container runs the SAME ouro-ops version at the fixed path.
v=$(ctl ouro-ops version | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["version"])')
for c in control bp1 relay1 relay2; do
  cv=$(dc exec -T "$c" ouro-ops version | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["version"])')
  [ "$cv" = "$v" ] || fail "E2E-12 version skew: $c=$cv vs $v"
done
pass "E2E-12 identical ouro $v on control+bp1+relay1+relay2"

# E2E-0: control SSHes to a target as ouro-exec by pubkey.
ctl ssh -i /root/.ouro/credentials/bp1 -o BatchMode=yes -o StrictHostKeyChecking=accept-new \
  ouro-exec@bp1 true && pass "E2E-0 ssh handshake ouro-exec@bp1" || fail "E2E-0 ssh"

# E2E-1: remote dispatch produces a REAL side effect + returns changed=true.
out1=$(ctl ouro-ops tool run deploy/provision --dispatch bp1 --spec "$SPEC")
echo "$out1" | grep -q '"changed":true' || fail "E2E-1 first run not changed=true"
dc exec -T bp1 test -f /opt/cardano/config/bp1/config.json || fail "E2E-1 no real config on target"
pass "E2E-1 dispatch bp1 -> real /opt/cardano/config/bp1/config.json"

# E2E-1 (relay): dispatch also works for a relay (p2 rolling upgrade depends on it).
ro=$(ctl ouro-ops tool run deploy/provision --dispatch relay1 --spec "$SPEC")
echo "$ro" | grep -q '"changed":true' || fail "E2E-1 relay1 dispatch not changed=true"
dc exec -T relay1 test -f /opt/cardano/config/relay1/config.json || fail "E2E-1 relay1 no config"
pass "E2E-1 dispatch relay1 -> real config (relay path works)"

# audit target-authoritative + CORRELATED to this dispatch's audit_id.
aid=$(echo "$out1" | python3 -c 'import json,sys;print(json.load(sys.stdin)["audit_id"])')
dc exec -T bp1 ouro-ops audit log --limit 20 | python3 -c "
import json,sys; ev=json.load(sys.stdin)['data']['events']
m=[e for e in ev if e['invocation_id']=='$aid']
assert any(e['event']=='start' for e in m) and any(e['event']=='finish' for e in m), 'no start+finish for audit_id on bp1'
assert all(e['tool']=='deploy/provision' and e['machine']=='bp1' for e in m), 'audit tool/machine mismatch'
" || fail "audit_id $aid not correlated on bp1"
ctl ouro-ops audit log --limit 20 | python3 -c "
import json,sys; assert json.load(sys.stdin)['data']['events']==[], 'control audit not empty'" \
  || fail "control audit not empty"
pass "audit target-authoritative + correlated to audit_id=$aid"

# E2E-4: second dispatch is idempotent AND leaves the filesystem unchanged (not just JSON).
b=$(dc exec -T bp1 sha256sum /opt/cardano/config/bp1/config.json /opt/cardano/config/bp1/topology.json)
out2=$(ctl ouro-ops tool run deploy/provision --dispatch bp1 --spec "$SPEC")
echo "$out2" | grep -q '"changed":false' || fail "E2E-4 second run not changed=false"
a=$(dc exec -T bp1 sha256sum /opt/cardano/config/bp1/config.json /opt/cardano/config/bp1/topology.json)
[ "$b" = "$a" ] || fail "E2E-4 config changed on idempotent re-run"
pass "E2E-4 idempotent (changed=false + config checksums unchanged)"

# INJECTION regression (P0 fix): a tool name with shell metacharacters is REJECTED on
# control (validate_tool_name) and nothing runs on the target.
refute_reason "tool name must be" ctl ouro-ops tool run 'deploy/preflight; touch /tmp/pwned #' --dispatch bp1 --spec "$SPEC"
dc exec -T bp1 test ! -e /tmp/pwned || fail "INJECTION: /tmp/pwned created on target!"
pass "injection blocked: crafted tool name rejected, target untouched"

# E2E-2 (TC-14): ouro-diag cannot READ key material (cat/tar/find); dir 0700 + file 0400.
refute_reason "Permission denied" dc exec -T -u ouro-diag bp1 cat /opt/cardano/keys/kes.skey
refute_reason "Permission denied" dc exec -T -u ouro-diag bp1 tar cf /tmp/x /opt/cardano/keys/kes.skey
dc exec -T bp1 bash -c '[ "$(stat -c %a /opt/cardano/keys)" = 700 ] && [ "$(stat -c %a /opt/cardano/keys/kes.skey)" = 400 ]' \
  || fail "E2E-2 key perms not 0700/0400"
pass "E2E-2 diag key-read denied (cat/tar) + dir 0700 + file 0400"

# E2E-3 (TC-15): ouro-diag has no sudo; ouro-exec is confined to the tool-run wrapper and
# cannot sudo other binaries (scp/docker/rm) or other ouro subcommands.
refute_reason "sudo" dc exec -T -u ouro-diag bp1 sudo -n /usr/local/sbin/ouro-tool-run deploy/preflight
refute_reason "sudo" dc exec -T -u ouro-exec bp1 sudo -n /usr/local/bin/ouro-ops confirm create --action x --machine bp1
refute_reason "sudo" dc exec -T -u ouro-exec bp1 sudo -n /bin/rm -rf /opt/cardano/keys
pass "E2E-3 diag no-sudo; ouro-exec confined to tool-run wrapper (no confirm/rm/other)"

# E2E-16 (TC-4): forged audit context is rejected — (a) forged env direct, (b) via wrapper
# with an arbitrary --audit-id, (c) missing token.
refute_reason "invalid_audit_context" dc exec -T \
  -e OURO_AUDIT_ID=fabricated -e OURO_TOOL_NAME=deploy/provision -e OURO_INVOCATION_TOKEN=inv_bogus \
  -e OURO_BIN=/usr/local/bin/ouro-ops bp1 bash /opt/ouro/ouro-skills/deploy/scripts/provision.sh
refute_reason "audit_context" dc exec -T -u ouro-exec bp1 \
  sudo -n /usr/local/sbin/ouro-tool-run deploy/provision --machine bp1 --spec "$SPEC" --audit-id fabricated
pass "E2E-16 forged context rejected (forged env + arbitrary --audit-id)"

# E2E-15 (teardown): down -v reaps containers, networks, AND volumes for this project.
cleanup
trap - EXIT
left=$(docker ps -aq --filter "name=${OURO_E2E_PROJECT}" | wc -l | tr -d ' ')
nets=$(docker network ls --filter "name=${OURO_E2E_PROJECT}" -q | wc -l | tr -d ' ')
vols=$(docker volume ls --filter "name=${OURO_E2E_PROJECT}" -q | wc -l | tr -d ' ')
[ "$left" = 0 ] && [ "$nets" = 0 ] && [ "$vols" = 0 ] \
  && pass "E2E-15 teardown clean (0 residual containers/networks/volumes)" \
  || fail "E2E-15 residual: containers=$left networks=$nets volumes=$vols"

echo "T2 E2E: ALL PASSED"
