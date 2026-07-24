#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../../.."

BIN="${OURO_BIN:-target/debug/ouro-ops}"
STATE_DIR="${S0027_E2E_STATE_DIR:-tmp/s0027-e2e}"
SPEC="$STATE_DIR/pool-spec.yaml"
MAX_CHECKS="${S0027_E2E_MAX_CHECKS:-120}"
CHECK_INTERVAL="${S0027_E2E_CHECK_INTERVAL:-30}"

required() {
  local name="$1"
  test -n "${!name:-}" || {
    printf 'missing required environment variable: %s\n' "$name" >&2
    exit 2
  }
}

for name in \
  S0027_E2E_BP_HOST S0027_E2E_BP_USER S0027_E2E_BP_KEY_REF \
  S0027_E2E_RELAY_HOST S0027_E2E_RELAY_USER S0027_E2E_RELAY_KEY_REF \
  S0027_E2E_RELAY_PUBLIC_HOST
do
  required "$name"
done

BP_PORT="${S0027_E2E_BP_PORT:-22}"
RELAY_PORT="${S0027_E2E_RELAY_PORT:-22}"
RELAY_P2P_PORT="${S0027_E2E_RELAY_P2P_PORT:-3001}"

test "$S0027_E2E_BP_USER" != "$S0027_E2E_RELAY_USER" || {
  echo "S0027 real E2E requires different BP and Relay SSH users" >&2
  exit 2
}
case "$S0027_E2E_BP_KEY_REF:$S0027_E2E_RELAY_KEY_REF" in
  creds://*:creds://*) ;;
  *) echo "SSH key references must use creds:// names" >&2; exit 2 ;;
esac

mkdir -p "$STATE_DIR"
cat >"$SPEC" <<EOF
spec_version: 1
pool:
  network: preview
  network_magic: 2
  genesis_hashes:
    shelley: 363498d1024f84bb39d3fa9593ce391483cb40d479b87233f868d6e57c3a400d
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh:
      host: ${S0027_E2E_BP_HOST}
      port: ${BP_PORT}
      user: ${S0027_E2E_BP_USER}
      key_ref: ${S0027_E2E_BP_KEY_REF}
  - id: relay1
    role: relay
    public_endpoint:
      host: ${S0027_E2E_RELAY_PUBLIC_HOST}
      port: ${RELAY_P2P_PORT}
    ssh:
      host: ${S0027_E2E_RELAY_HOST}
      port: ${RELAY_PORT}
      user: ${S0027_E2E_RELAY_USER}
      key_ref: ${S0027_E2E_RELAY_KEY_REF}
EOF

json_field() {
  python3 -c 'import json,sys
value=json.load(sys.stdin)
for part in sys.argv[1].split("."):
    value=value[int(part)] if isinstance(value,list) else value[part]
print(str(value).lower() if isinstance(value,bool) else value)' "$1"
}

assert_json_field() {
  local document="$1" path="$2" expected="$3" actual
  actual="$(printf '%s' "$document" | json_field "$path")"
  test "$actual" = "$expected" || {
    printf 'expected %s=%s, got %s\n' "$path" "$expected" "$actual" >&2
    exit 1
  }
}

if test "${1:-}" = prepare; then
  cargo build -q -p ouro
  "$BIN" contract check --requires-ouro '>=0.1.0' --requires-contract 1 >/dev/null
  printf 'Pool spec: %s\n' "$SPEC"
  printf 'Run these commands yourself and confirm each complete fingerprint:\n'
  printf '  %q ssh trust --spec %q --node bp1\n' "$BIN" "$SPEC"
  printf '  %q ssh trust --spec %q --node relay1\n' "$BIN" "$SPEC"
  exit 0
fi

test "${1:-}" = run || {
  echo "usage: fixtures/e2e/s0027/run.sh prepare|run" >&2
  exit 2
}
test "${S0027_E2E_ALLOW_FRESH_HOST_WRITES:-}" = YES || {
  echo "set S0027_E2E_ALLOW_FRESH_HOST_WRITES=YES only for two dedicated fresh hosts" >&2
  exit 2
}

cargo build -q -p ouro
"$BIN" contract check --requires-ouro '>=0.1.0' --requires-contract 1 >/dev/null

inspect_before="$("$BIN" deploy inspect --spec "$SPEC")"
assert_json_field "$inspect_before" data.classification applicable
assert_json_field "$inspect_before" data.target_writes false
python3 -c 'import json,sys
d=json.load(sys.stdin)["data"]
assert len(d["nodes"]) == 2
assert {n["role"] for n in d["nodes"]} == {"bp","relay"}
assert all(not n["reasons"] for n in d["nodes"])
assert all(n["mithril"]["restore_expected"] for n in d["nodes"])' <<<"$inspect_before"

apply_output="$("$BIN" deploy apply --spec "$SPEC")"
assert_json_field "$apply_output" data.classification command_success
assert_json_field "$apply_output" data.intermediate_readiness_checks false

first_check="$("$BIN" deploy check --spec "$SPEC" || true)"
assert_json_field "$first_check" data.classification pending
python3 -c 'import json,sys
d=json.load(sys.stdin)["data"]
assert any(n["status"] == "pending" for n in d["nodes"])
assert all(not n["static_failures"] for n in d["nodes"])' <<<"$first_check"
printf '%s\n' "$first_check" >"$STATE_DIR/check-000.json"

ready_output=
for ((attempt=1; attempt<=MAX_CHECKS; attempt++)); do
  sleep "$CHECK_INTERVAL"
  current="$("$BIN" deploy check --spec "$SPEC" || true)"
  printf '%s\n' "$current" >"$STATE_DIR/check-$(printf '%03d' "$attempt").json"
  classification="$(printf '%s' "$current" | json_field data.classification)"
  if test "$classification" = failed; then
    echo "deploy check reached failed" >&2
    exit 1
  fi
  if test "$classification" = ready; then
    ready_output="$current"
    break
  fi
done
test -n "$ready_output" || {
  echo "final state remained pending; pending is not an E2E pass" >&2
  exit 1
}
python3 -c 'import json,sys
d=json.load(sys.stdin)["data"]
assert all(n["status"] == "ready" for n in d["nodes"])
bp=next(n for n in d["nodes"] if n["role"] == "bp")
assert bp["lifecycle"] == "bootstrap"
assert bp["forging_readiness"] == "not_applicable"
assert bp["block_production"] == "disabled"
for n in d["nodes"]:
    assert not n["static_failures"]
    assert n["dynamic"]["host_metrics"]
    assert n["dynamic"]["container_metrics"]' <<<"$ready_output"

inspect_after="$("$BIN" deploy inspect --spec "$SPEC")"
assert_json_field "$inspect_after" data.classification already_deployed
set +e
repeat_apply="$("$BIN" deploy apply --spec "$SPEC" 2>&1)"
repeat_status=$?
set -e
test "$repeat_status" -ne 0
assert_json_field "$repeat_apply" error.code already_deployed
assert_json_field "$repeat_apply" data.target_writes false

health="$("$BIN" op run --op observability/health --spec "$SPEC" \
  --dispatch "$S0027_E2E_RELAY_HOST" --ssh-key "$S0027_E2E_RELAY_KEY_REF" \
  --node relay1 --param machine=relay1)"
python3 -c 'import json,sys
d=json.load(sys.stdin)
text=str(d).lower()
assert "compose" in text
assert "ouro-relay1" in text
assert "cardano-node" in text' <<<"$health"

echo "S0027 real Ubuntu Fleet Deploy E2E passed"
