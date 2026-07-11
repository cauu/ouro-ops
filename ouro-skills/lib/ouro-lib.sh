#!/usr/bin/env bash
set +o xtrace

OURO_STARTED_AT="${OURO_STARTED_AT:-$(date +%s)}"

ouro_duration_s() {
  local now
  now="$(date +%s)"
  printf '%s' "$((now - OURO_STARTED_AT))"
}

ouro_redact() {
  sed -E \
    -e 's/(cold|vrf|skey)[A-Za-z0-9._\/:=+-]*/<redacted>/Ig' \
    -e 's/creds:\/\/[A-Za-z0-9._\/:@-]+/<credential-ref>/g'
}

ouro_json_string() {
  python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"
}

ouro_emit_ok() {
  local tool="${OURO_TOOL_NAME:-unknown}"
  local machine="${OURO_MACHINE:-}"
  local changed="${1:-false}"
  local detail="${2:-ok}"
  local audit_json=null
  local machine_json=null
  if [[ -n "${OURO_AUDIT_ID:-}" ]]; then
    audit_json="$(ouro_json_string "$OURO_AUDIT_ID")"
  fi
  if [[ -n "$machine" ]]; then
    machine_json="$(ouro_json_string "$machine")"
  fi
  python3 - "$tool" "$machine_json" "$changed" "$(ouro_duration_s)" "$audit_json" "$detail" <<'PY'
import json, sys
tool, machine_json, changed, duration, audit_json, detail = sys.argv[1:]
print(json.dumps({
  "tool": tool,
  "machine": json.loads(machine_json),
  "status": "ok",
  "changed": changed == "true",
  "checks": [{
    "name": "completed",
    "pass": True,
    "severity": "info",
    "exit_class": 0,
    "rollback_safe": True,
    "detail": detail,
  }],
  "duration_s": float(duration),
  "audit_id": json.loads(audit_json),
}, separators=(",", ":")))
PY
}

ouro_emit_error() {
  local exit_class="${1:-20}"
  local code="${2:-error}"
  local detail
  detail="$(printf '%s' "${3:-failed}" | ouro_redact)"
  local tool="${OURO_TOOL_NAME:-unknown}"
  local machine="${OURO_MACHINE:-}"
  local audit_json=null
  local machine_json=null
  if [[ -n "${OURO_AUDIT_ID:-}" ]]; then
    audit_json="$(ouro_json_string "$OURO_AUDIT_ID")"
  fi
  if [[ -n "$machine" ]]; then
    machine_json="$(ouro_json_string "$machine")"
  fi
  python3 - "$tool" "$machine_json" "$exit_class" "$code" "$detail" "$(ouro_duration_s)" "$audit_json" <<'PY'
import json, sys
tool, machine_json, exit_class, code, detail, duration, audit_json = sys.argv[1:]
print(json.dumps({
  "tool": tool,
  "machine": json.loads(machine_json),
  "status": "error",
  "changed": False,
  "checks": [],
  "duration_s": float(duration),
  "audit_id": json.loads(audit_json),
  "error": {
    "code": code,
    "detail": detail,
    "hint": "rerun through ouro-ops tool run with an audit context",
  }
}, separators=(",", ":")))
PY
  exit "$exit_class"
}

ouro_emit_unknown() {
  # Exit class 40: state is UNKNOWN (e.g. verify could not determine changed state
  # after a partial action). Callers must stop all writes and escalate to a human.
  local code="${1:-unknown_state}"
  ouro_emit_error 40 "$code" "${2:-state unknown; stop writes and escalate to a human}"
}

ouro_require_audit_context() {
  # Presence of the env vars alone is NOT sufficient — an agent could `export` them.
  # The gate is only satisfied when a CLI-signed invocation token verifies against the
  # audit context, which only `ouro-ops tool run` can produce (§2.2#2).
  if [[ -z "${OURO_AUDIT_ID:-}" || -z "${OURO_TOOL_NAME:-}" || -z "${OURO_INVOCATION_TOKEN:-}" ]]; then
    ouro_emit_error 10 "missing_audit_context" "write operation refused; run via 'ouro-ops tool run'"
  fi
  local bin="${OURO_BIN:-ouro-ops}"
  if ! "$bin" tool verify-context --audit-id "$OURO_AUDIT_ID" --token "$OURO_INVOCATION_TOKEN" >/dev/null 2>&1; then
    ouro_emit_error 10 "invalid_audit_context" "invocation token failed verification; run via 'ouro-ops tool run'"
  fi
}

ouro_check_then_act() {
  local detect_cmd="$1"
  local act_cmd="$2"
  if bash -c "$detect_cmd" >/dev/null 2>&1; then
    ouro_emit_ok false "already converged"
  else
    bash -c "$act_cmd"
    ouro_emit_ok true "changed"
  fi
}

ouro_detect_package_manager() {
  if command -v apt-get >/dev/null 2>&1; then
    printf 'apt\n'
  elif command -v dnf >/dev/null 2>&1; then
    printf 'dnf\n'
  else
    ouro_emit_error 10 "package_manager_unsupported" "expected apt-get or dnf"
  fi
}

ouro_detect_firewall() {
  if command -v ufw >/dev/null 2>&1; then
    printf 'ufw\n'
  elif command -v firewall-cmd >/dev/null 2>&1; then
    printf 'firewalld\n'
  else
    printf 'none\n'
  fi
}

# --- Supervisor adapter (S0017 p2-8) -----------------------------------------
# The ONE place allowed to call the raw process-supervision primitives
# (pgrep/pkill/setsid). Every lifecycle skill (runtime/upgrade/kes-rotation/
# deploy/observability) must route node + daemon start/stop/detect through
# these functions, never inline. A static gate (tests/test_supervisor_gate.py,
# TC-14) forbids those primitives anywhere else, so a node started here cannot
# be half-managed by a stray pkill elsewhere (split-brain).
#
# Bare mode only for now: this wraps the current host-process behavior behind a
# stable API. Supervisor-mode awareness (systemd unit restart, container image
# re-pin + recreate) is layered onto ouro_node_* by p2-5 without touching the
# call sites here.

# Generic process primitives — match by full command line (`pgrep -f`).
ouro_proc_running() { pgrep -f "$1" >/dev/null 2>&1; }
ouro_proc_pid()     { pgrep -f "$1" 2>/dev/null | head -1 || true; }
ouro_proc_stop()    { pkill -f "$1" 2>/dev/null || true; sleep "${2:-2}"; }

# Spawn a detached background daemon: $1 = logfile, rest = command + args.
# Centralizes `setsid` so the supervisor gate can forbid it elsewhere. The
# daemon inherits the caller's environment (callers export any needed vars
# before invoking — e.g. the telemetry gateway's non-secret auth-file path).
ouro_daemon_spawn() {
  local log="$1"; shift
  setsid "$@" >"$log" 2>&1 </dev/null &
}

# cardano-node lifecycle. `OURO_NODE_MATCH` is the single source of the
# process-match pattern; all node argv is derived from OURO_DEVNET_DIR so the
# four call sites (restart/topology-apply/upgrade/rotate) share one definition.
OURO_NODE_MATCH="${OURO_NODE_MATCH:-cardano-node run}"
ouro_node_running() { ouro_proc_running "$OURO_NODE_MATCH"; }
ouro_node_pid()     { ouro_proc_pid "$OURO_NODE_MATCH"; }
ouro_node_stop()    { ouro_proc_stop "$OURO_NODE_MATCH" "${1:-2}"; }

ouro_node_start() {
  local devnet="${OURO_DEVNET_DIR:-/opt/devnet}"
  local pool="$devnet/pools-keys/pool1"
  local sock="$devnet/node.socket"
  # KEEP the existing db across restarts (wiping it re-triggers the p2-0 cold-start
  # trap). Port + log path are fixed and identical across all four call sites.
  ouro_daemon_spawn /var/log/cardano-node.log \
    cardano-node run \
    --config "$devnet/config.json" --topology "$devnet/topology.json" \
    --database-path "$devnet/db" --socket-path "$sock" \
    --shelley-kes-key "$pool/kes.skey" --shelley-vrf-key "$pool/vrf.skey" \
    --shelley-operational-certificate "$pool/opcert.cert" --port 3001
}

# Rolling restart: stop (with settle) then start onto the on-disk config/keys.
ouro_node_restart() { ouro_node_stop; ouro_node_start; }
