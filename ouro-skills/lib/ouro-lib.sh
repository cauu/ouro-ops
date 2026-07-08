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
    "hint": "rerun through ouro tool run with an audit context",
  }
}, separators=(",", ":")))
PY
  exit "$exit_class"
}

ouro_require_audit_context() {
  if [[ -z "${OURO_AUDIT_ID:-}" || -z "${OURO_TOOL_NAME:-}" ]]; then
    ouro_emit_error 10 "missing_audit_context" "write operation refused without OURO_AUDIT_ID/OURO_TOOL_NAME"
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
