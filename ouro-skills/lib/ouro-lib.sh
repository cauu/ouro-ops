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
ouro_proc_count()   { pgrep -f "$1" 2>/dev/null | grep -c . || true; }
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
ouro_node_count()   { ouro_proc_count "$OURO_NODE_MATCH"; }
ouro_node_stop()    { ouro_proc_stop "$OURO_NODE_MATCH" "${1:-2}"; }

# --- node filesystem layout DISCOVERY (S0017 p5-3) --------------------------
# The node's real paths are DISCOVERED from the running cardano-node's own command line — its args
# carry --socket-path / --config / --database-path / --shelley-*-key — so any layout works with
# ZERO config (no hand-declared paths in the spec). In container mode the args are container-
# internal paths; the p5-1 same-path bind-mount makes them resolve on the host too. Each helper
# falls back to the OURO_* env / the /opt/devnet bed layout when no node is running.
ouro_node_cmdline() {
  local pid; pid="$(ouro_node_pid)"; [ -n "$pid" ] || return 1
  tr '\0' '\n' < "$(ouro_proc_root)/$pid/cmdline" 2>/dev/null
}
# Value of the `--flag <value>` pair in the node's argv (empty if the flag is absent).
ouro_node_arg() { ouro_node_cmdline 2>/dev/null | awk -v f="$1" 'p{print; exit} $0==f{p=1}'; }
ouro_node_socket()      { local v; v="$(ouro_node_arg --socket-path)";   printf '%s' "${v:-${OURO_NODE_SOCKET:-/opt/devnet/node.socket}}"; }
ouro_node_config_path() { local v; v="$(ouro_node_arg --config)";        printf '%s' "${v:-${OURO_DEVNET_DIR:-/opt/devnet}/config.json}"; }
# Pool key directory = dirname of the running node's KES key (holds kes/vrf/opcert/counter[/cold]).
ouro_node_pool_dir() {
  local kes; kes="$(ouro_node_arg --shelley-kes-key)"
  if [ -n "$kes" ]; then dirname "$kes"; else printf '%s' "${OURO_POOL_DIR:-${OURO_DEVNET_DIR:-/opt/devnet}/pools-keys/pool1}"; fi
}
# Shelley genesis file: resolved from the node config's ShelleyGenesisFile (relative → vs config dir).
ouro_node_genesis_shelley() {
  local cfg gf; cfg="$(ouro_node_config_path)"
  gf="$(python3 -c 'import json,sys,os
try:
    c=json.load(open(sys.argv[1])); g=c.get("ShelleyGenesisFile","")
    if g and not os.path.isabs(g): g=os.path.join(os.path.dirname(os.path.abspath(sys.argv[1])),g)
    print(g)
except Exception: print("")' "$cfg" 2>/dev/null)"
  printf '%s' "${gf:-${OURO_GENESIS_SHELLEY:-${OURO_DEVNET_DIR:-/opt/devnet}/shelley-genesis.json}}"
}

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

# Rolling restart (bare): capture the running node's EXACT argv, stop, and re-spawn it VERBATIM —
# so any layout restarts correctly with no reconstructed command. Falls back to ouro_node_start's
# reconstruction only when no prior argv could be captured (e.g. nothing was running).
ouro_node_restart() {
  local argv=() line
  while IFS= read -r line; do argv+=("$line"); done < <(ouro_node_cmdline 2>/dev/null)
  ouro_node_stop
  if [ "${#argv[@]}" -gt 0 ]; then
    ouro_daemon_spawn /var/log/cardano-node.log "${argv[@]}"
  else
    ouro_node_start
  fi
}

# --- Supervisor DETECTION (S0017 p2-1) — read-only, closed projection ---------
# The adapter is the sole supervisor-aware module (p2-8 gate), so read-only mode
# detection lives here too. Every function emits ONLY a closed projection:
# booleans, enums, opaque immutable ids (container id / systemd unit basename),
# and content hashes (image digest) — NEVER raw env/argv/mounts/labels or full
# `inspect`/`systemctl cat` output. `docker`/`podman` are called only with
# `--format` to project a single field. Test seam: OURO_PROC_ROOT overrides
# /proc so mode + canary fixtures can be injected without a real container.

ouro_proc_root() { printf '%s' "${OURO_PROC_ROOT:-/proc}"; }

# cgroup membership of a pid (never emitted raw; only regex-extracted below).
ouro_proc_cgroup() {
  local pid="$1"
  [ -n "$pid" ] || return 0
  cat "$(ouro_proc_root)/$pid/cgroup" 2>/dev/null || true
}

# 12-hex container id if the pid lives in a docker/podman container cgroup, else empty.
ouro_supervisor_container_id() {
  ouro_proc_cgroup "$1" \
    | grep -oE '(docker[-/]|libpod[-/])[0-9a-f]{64}' \
    | grep -oE '[0-9a-f]{64}' | head -1 | cut -c1-12 || true
}

# Container runtime enum from cgroup markers only: 'docker' | 'podman' | ''.
ouro_supervisor_container_runtime() {
  local cg; cg="$(ouro_proc_cgroup "$1")"
  if   printf '%s' "$cg" | grep -qE 'libpod'; then printf 'podman'
  elif printf '%s' "$cg" | grep -qE 'docker'; then printf 'docker'
  fi
}

# systemd unit basename if the pid is under a *.service slice, else empty. Emits
# only the safe-charset unit name — never the raw cgroup path or unit body.
ouro_supervisor_systemd_unit() {
  ouro_proc_cgroup "$1" | grep -oE '[A-Za-z0-9_.@-]+\.service' | head -1 || true
}

# --port integer from the node's cmdline (a closed extraction — the rest of the
# cmdline, incl. key file PATHS, is never emitted). Empty if absent.
ouro_node_port() {
  local pid="$1"
  [ -n "$pid" ] || return 0
  tr '\0' ' ' < "$(ouro_proc_root)/$pid/cmdline" 2>/dev/null \
    | grep -oE -- '--port[= ]+[0-9]+' | grep -oE '[0-9]+' | head -1 || true
}

# Container image digest via the runtime's OWN --format (projects to one field;
# never the raw inspect JSON). $1=runtime(docker|podman) $2=container-id.
ouro_supervisor_image_digest() {
  local rt="$1" cid="$2"
  [ -n "$rt" ] && [ -n "$cid" ] || return 0
  "$rt" inspect --format '{{.Image}}' "$cid" 2>/dev/null | head -1 || true
}

# One compose label of a container (single-field projection via --format index; never the
# raw label map). Empty if absent / not compose-managed. $1=rt $2=cid $3=label key.
ouro_supervisor_compose_label() {
  local rt="$1" cid="$2" key="$3" v
  [ -n "$rt" ] && [ -n "$cid" ] || return 0
  v="$("$rt" inspect --format "{{index .Config.Labels \"$key\"}}" "$cid" 2>/dev/null | head -1)"
  case "$v" in "<no value>") ;; *) printf '%s' "$v" ;; esac
}

# Image ID (content hash) of an image ref / of a container's running image. Used for the
# container-upgrade convergence check (running id == declared image's id => converged).
ouro_image_id_of()      { "$1" image inspect --format '{{.Id}}' "$2" 2>/dev/null | head -1 || true; }
ouro_container_image_id() { "$1" inspect --format '{{.Image}}' "$2" 2>/dev/null | head -1 || true; }

# Compose CLI on this host: the v2 plugin (`docker compose`) or the v1 binary
# (`docker-compose`). Empty if neither exists.
ouro_compose_cmd() {
  if docker compose version >/dev/null 2>&1; then printf 'docker compose'
  elif command -v docker-compose >/dev/null 2>&1; then printf 'docker-compose'
  fi
}

# --- Container upgrade: image re-pin + recreate via compose (S0017 p2-5 / TC-7) --------
# A container-managed node upgrades by RECREATING the container from the DECLARED image
# (spec runtime.image) — swapping a host binary under a container is a silent no-op. For a
# compose-managed container the compose file is the deployment's source of truth, so the
# mechanism converges IT (otherwise the next `compose up` would silently roll the node back):
#   pull/resolve declared image -> backup compose file -> rewrite services.<svc>.image ->
#   `compose up -d --no-deps <svc>` -> verify the service's running container is on the
#   declared image id; on any failure RESTORE the backup and re-up (rollback to the old
#   image), then exit 30. Plain `docker run` containers (no compose labels) fail closed:
#   generic config-cloning recreation is not yet modeled.
# Runs at TOP LEVEL of an L2 script (never in a command substitution): emits and exits.
ouro_node_upgrade_container() {
  local rt="$1" cid="$2" want="$3"
  local proj svc cfg wd ccmd want_id newcid new_id
  proj="$(ouro_supervisor_compose_label "$rt" "$cid" com.docker.compose.project)"
  svc="$(ouro_supervisor_compose_label "$rt" "$cid" com.docker.compose.service)"
  cfg="$(ouro_supervisor_compose_label "$rt" "$cid" com.docker.compose.project.config_files)"
  wd="$(ouro_supervisor_compose_label "$rt" "$cid" com.docker.compose.project.working_dir)"
  if [ -z "$proj" ] || [ -z "$svc" ] || [ -z "$cfg" ]; then
    ouro_emit_error 40 "container_unmanaged" \
      "container upgrade needs a compose-managed node (labels absent); plain-run recreation not yet modeled"
  fi
  # compose may list several config files comma-separated; converge the first that
  # defines the service (in practice the project file).
  cfg="${cfg%%,*}"
  [ -f "$cfg" ] || ouro_emit_error 40 "compose_file_missing" "compose file from container labels not found: $cfg"
  ccmd="$(ouro_compose_cmd)"
  [ -n "$ccmd" ] || ouro_emit_error 40 "compose_cli_missing" "no compose CLI on this host"

  # Resolve the declared image locally (pull only if absent) and its content id.
  "$rt" image inspect "$want" >/dev/null 2>&1 || "$rt" pull "$want" >/dev/null 2>&1 \
    || ouro_emit_error 30 "image_unavailable" "declared image not present and pull failed: $want"
  want_id="$(ouro_image_id_of "$rt" "$want")"
  [ -n "$want_id" ] || ouro_emit_error 30 "image_unavailable" "could not resolve image id for $want"

  # Rewrite the service's image in the compose file (backup first — the rollback artifact).
  cp "$cfg" "$cfg.ouro-backup" || ouro_emit_error 30 "compose_backup_failed" "could not back up $cfg"
  if ! python3 - "$cfg" "$svc" "$want" <<'PY'
import sys, yaml
cfg, svc, want = sys.argv[1:4]
doc = yaml.safe_load(open(cfg))
doc["services"][svc]["image"] = want
yaml.safe_dump(doc, open(cfg, "w"), default_flow_style=False, sort_keys=False)
PY
  then
    mv -f "$cfg.ouro-backup" "$cfg"
    ouro_emit_error 30 "compose_rewrite_failed" "could not set services.$svc.image in $cfg"
  fi

  # Recreate the service onto the new image, then verify convergence by IMAGE ID of the
  # RUNNING service container. `-p "$proj"` is REQUIRED — without it compose derives the
  # project from the cwd and would target a DIFFERENT deployment, not the running node. Any
  # failure => restore the compose file and re-up (rollback).
  if (cd "${wd:-$(dirname "$cfg")}" && $ccmd -p "$proj" -f "$cfg" up -d --no-deps "$svc" >/dev/null 2>&1); then
    sleep 2
    newcid="$("$rt" ps -q \
      --filter "label=com.docker.compose.project=$proj" \
      --filter "label=com.docker.compose.service=$svc" 2>/dev/null | head -1)"
    new_id="$(ouro_container_image_id "$rt" "$newcid")"
    if [ -n "$newcid" ] && [ "$new_id" = "$want_id" ]; then
      rm -f "$cfg.ouro-backup"
      return 0
    fi
  fi
  # Rollback: old compose file back, recreate the service on the previous image.
  mv -f "$cfg.ouro-backup" "$cfg"
  (cd "${wd:-$(dirname "$cfg")}" && $ccmd -p "$proj" -f "$cfg" up -d --no-deps "$svc" >/dev/null 2>&1) || true
  ouro_emit_error 30 "container_upgrade_failed" \
    "service $svc did not converge to $want; compose file restored and previous image re-upped"
}

# Detected supervision mode of the node from live signals:
# bare | systemd | docker | podman | ambiguous | none. Single source of the mode
# decision for lifecycle dispatch (kept consistent with detect/runtime by a test).
ouro_node_detect_mode() {
  local pid count cid runtime unit
  pid="$(ouro_node_pid)"
  [ -n "$pid" ] || { printf 'none'; return 0; }
  count="$(ouro_node_count)"
  [ "${count:-0}" -gt 1 ] 2>/dev/null && { printf 'ambiguous'; return 0; }
  cid="$(ouro_supervisor_container_id "$pid")"
  runtime="$(ouro_supervisor_container_runtime "$pid")"
  unit="$(ouro_supervisor_systemd_unit "$pid")"
  if   [ -n "$cid" ] && [ "$runtime" = docker ]; then printf 'docker'
  elif [ -n "$cid" ] && [ "$runtime" = podman ]; then printf 'podman'
  elif [ -z "$cid" ] && [ -n "$unit" ];          then printf 'systemd'
  elif [ -z "$cid" ] && [ -z "$unit" ];          then printf 'bare'
  else printf 'ambiguous'; fi
}

# --- cardano-cli managed-mode adapter (S0017 p5-1) --------------------------
# In bare/systemd mode cardano-cli is on the host PATH (the node runs as a host process). In
# docker/podman mode the node — and its cardano-cli binary and node socket — live INSIDE the
# container, so a host `cardano-cli` would not exist. This adapter dispatches every cardano-cli
# invocation to the right place: host, or `<runtime> exec <cid>` for a containerized node.
#
# CONVENTION (standard SPO container layout): the pool key/data directory and the node socket are
# bind-mounted at the SAME path on the host and inside the container, so file arguments and
# $CARDANO_NODE_SOCKET_PATH resolve identically either way. The socket env is forwarded into the
# container so `query` works. Resolution is cached per script run (a `docker restart` keeps the
# same container id, so the cache stays valid across the lifecycle scripts' own restarts).
_OURO_CLI_KIND=""   # "" unresolved | host | container
_OURO_CLI_RT=""     # docker | podman   (container kind only)
_OURO_CLI_CID=""    # container id      (container kind only)
ouro_cardano_cli_resolve() {
  local mode pid
  mode="$(ouro_node_detect_mode)"
  case "$mode" in
    docker|podman)
      pid="$(ouro_node_pid)"
      _OURO_CLI_CID="$(ouro_supervisor_container_id "$pid")"
      _OURO_CLI_RT="$(ouro_supervisor_container_runtime "$pid")"
      if [ -n "$_OURO_CLI_CID" ] && [ -n "$_OURO_CLI_RT" ]; then
        _OURO_CLI_KIND=container
      else
        _OURO_CLI_KIND=host   # container mode but id unresolved → fall back to host cardano-cli
      fi
      ;;
    *) _OURO_CLI_KIND=host ;;
  esac
}

# Run cardano-cli in the node's supervision context. Use this for EVERY cardano-cli call in a
# dispatched L2 script (the static gate forbids raw `cardano-cli` outside this adapter).
# p5-21: node-connecting subcommands (all `query`, plus `transaction submit|build`) need the
# socket. cardano-cli 10.x (e.g. the blinklabs image) does NOT honor CARDANO_NODE_SOCKET_PATH for
# these — it demands an explicit `--socket-path`. Decide whether to inject it.
_ouro_cli_wants_socket() {
  case "$1" in
    query) return 0 ;;
    transaction) case "${2:-}" in submit|build) return 0 ;; esac ;;
  esac
  return 1
}
ouro_cardano_cli() {
  [ -n "$_OURO_CLI_KIND" ] || ouro_cardano_cli_resolve
  local extra=()
  # Inject --socket-path for socket-needing commands when we know it and the caller did not
  # already pass one (appended at the end — valid position for these subcommands).
  if [ -n "${CARDANO_NODE_SOCKET_PATH:-}" ] && _ouro_cli_wants_socket "$@"; then
    case " $* " in *" --socket-path "*) ;; *) extra=(--socket-path "$CARDANO_NODE_SOCKET_PATH") ;; esac
  fi
  if [ "$_OURO_CLI_KIND" = container ]; then
    "$_OURO_CLI_RT" exec -e CARDANO_NODE_SOCKET_PATH="${CARDANO_NODE_SOCKET_PATH:-}" "$_OURO_CLI_CID" cardano-cli "$@" "${extra[@]}"
  else
    cardano-cli "$@" "${extra[@]}"
  fi
}

# p5-21: file existence + chain-db disk usage in the node's supervision context. For a
# containerized node the node paths (opcert, --database-path) are CONTAINER-internal, so these
# checks must run INSIDE the container — the host would not see them (the cause of health's
# opcert_present:false / disk:null on a docker node). Mirrors the cardano-cli adapter.
ouro_node_file_exists() {
  [ -n "$_OURO_CLI_KIND" ] || ouro_cardano_cli_resolve
  if [ "$_OURO_CLI_KIND" = container ]; then
    "$_OURO_CLI_RT" exec "$_OURO_CLI_CID" sh -c 'test -f "$1"' _ "$1" 2>/dev/null
  else
    [ -f "$1" ]
  fi
}
ouro_node_disk_pct() {
  [ -n "$_OURO_CLI_KIND" ] || ouro_cardano_cli_resolve
  if [ "$_OURO_CLI_KIND" = container ]; then
    "$_OURO_CLI_RT" exec "$_OURO_CLI_CID" sh -c 'df -P "$1"' _ "$1" 2>/dev/null | awk 'NR==2{gsub(/%/,"",$5);print $5}'
  else
    df -P "$1" 2>/dev/null | awk 'NR==2{gsub(/%/,"",$5);print $5}'
  fi
}
# Operational certificate path from the running node's argv (the REAL file — e.g. node.cert —
# not a guessed name).
ouro_node_opcert() { ouro_node_arg --shelley-operational-certificate; }

# Presence check for cardano-cli in the node's supervision context (host or inside the container).
ouro_cardano_cli_available() {
  [ -n "$_OURO_CLI_KIND" ] || ouro_cardano_cli_resolve
  if [ "$_OURO_CLI_KIND" = container ]; then
    "$_OURO_CLI_RT" exec "$_OURO_CLI_CID" sh -c 'command -v cardano-cli' >/dev/null 2>&1
  else
    command -v cardano-cli >/dev/null 2>&1
  fi
}

# --- Supervisor-mode lifecycle dispatch (S0017 p2-5) -------------------------
# Destructive lifecycle actions choose their path from the DETECTED mode,
# cross-checked against the spec-DECLARED mode. Any mismatch, ambiguity, mixed/
# nested/multi-node signal, or missing node => fail closed (exit 40): the
# mechanism never guesses which supervisor owns the node. Bare stays byte-identical
# to the pre-p2-5 path. systemd/container actions target the unit/container id
# resolved by read-only detection — never an LLM-supplied argument (p2-5 binding).

# Declared runtime.mode for a machine from the spec ('' if undeclared/no spec).
ouro_declared_mode() {
  local spec="$1" machine="$2"
  [ -n "$spec" ] && [ -n "$machine" ] || return 0
  python3 - "$spec" "$machine" <<'PY' 2>/dev/null || true
import sys, yaml
try:
    s = yaml.safe_load(open(sys.argv[1]))
    m = next((x for x in s.get("machines", []) if x.get("id") == sys.argv[2]), None)
    print((m or {}).get("runtime", {}).get("mode", ""), end="")
except Exception:
    pass
PY
}

# Resolve the effective mode for a destructive action. $1 = declared mode ('' when
# undeclared). Prints one of: bare|systemd|docker|podman (act) OR none|ambiguous|mismatch
# (the caller MUST fail closed). This is PURE — it never exits, because callers invoke it in
# a command substitution (`MODE="$(...)"`) where an `exit` would only kill the subshell and
# silently be swallowed. The fail-closed exit-40 therefore happens in the caller, at top level
# (see ouro_node_guard_mode), so it actually terminates the script.
ouro_node_effective_mode() {
  local declared="$1" detected
  detected="$(ouro_node_detect_mode)"
  if [ -n "$declared" ] && [ "$declared" != "$detected" ] \
     && [ "$detected" != ambiguous ] && [ "$detected" != none ]; then
    printf 'mismatch'; return 0
  fi
  printf '%s' "$detected"
}

# Caller-side fail-closed guard: emit exit-40 at TOP LEVEL (not in a subshell) when the
# resolved mode is not actionable. Usage in a script:  MODE="$(ouro_node_effective_mode "$d")"
# then:  ouro_node_guard_mode "$MODE"   # terminates the script on none/ambiguous/mismatch.
ouro_node_guard_mode() {
  local mid="${OURO_MACHINE:-target}"
  case "$1" in
    none)      ouro_emit_error 40 "node_not_running" "no running node to act on ($mid)" ;;
    ambiguous) ouro_emit_error 40 "runtime_mode_ambiguous" \
                 "supervision mode ambiguous on $mid (mixed/nested/multi-node); refusing to act" ;;
    mismatch)  ouro_emit_error 40 "runtime_mode_mismatch" \
                 "declared runtime differs from detected on $mid; refusing to act" ;;
  esac
}

# Rolling restart onto the on-disk config/keys, dispatched by mode. $1 = mode.
# systemd/container target the detected unit/container id (not a passed-in name).
ouro_node_restart_mode() {
  local mode="$1" pid unit cid
  pid="$(ouro_node_pid)"
  case "$mode" in
    systemd) unit="$(ouro_supervisor_systemd_unit "$pid")"
             [ -n "$unit" ] || ouro_emit_error 40 "systemd_unit_unresolved" "could not resolve node unit"
             systemctl restart "$unit" ;;
    docker)  cid="$(ouro_supervisor_container_id "$pid")"
             [ -n "$cid" ] || ouro_emit_error 40 "container_unresolved" "could not resolve node container"
             docker restart "$cid" >/dev/null ;;
    podman)  cid="$(ouro_supervisor_container_id "$pid")"
             [ -n "$cid" ] || ouro_emit_error 40 "container_unresolved" "could not resolve node container"
             podman restart "$cid" >/dev/null ;;
    *)       ouro_node_restart ;;   # bare — unchanged
  esac
}

# --- network selection (S0017 p5-14) -------------------------------------------------------
# cardano-cli network args from the SPEC — the single source of truth. (The old
# OURO_NETWORK_MAGIC env was never in the tool run env allowlist, so dispatched scripts
# silently pinned every query to --testnet-magic 1, and no script had a mainnet branch.)
# Prints "--mainnet" or "--testnet-magic <N>"; callers expand UNQUOTED so it word-splits
# into arguments: `ouro_cardano_cli query tip $NET`. Falls back to --testnet-magic 1 when
# no spec is available (local test beds).
ouro_network_args() {
  local spec="${OURO_SPEC:-}" net="" magic=""
  if [ -n "$spec" ] && [ -f "$spec" ]; then
    read -r net magic < <(python3 - "$spec" <<'PY' 2>/dev/null
import yaml, sys
pool = (yaml.safe_load(open(sys.argv[1])) or {}).get("pool") or {}
print(pool.get("network", "") or "-", pool.get("network_magic", "") or "-")
PY
) || true
  fi
  if [ "$net" = "mainnet" ]; then
    printf -- '--mainnet'
  else
    [ "$magic" = "-" ] && magic=""
    printf -- '--testnet-magic %s' "${magic:-1}"
  fi
}

# --- diagnostics primitives (S0017 p5-18) ---------------------------------------------------
# Privileged reads for troubleshooting, mode-dispatched. These live in the lib because the
# supervisor gate confines docker/podman/systemctl/pgrep to this file. Free-form (unprivileged)
# diagnosis goes through `ouro-ops diag exec` as ouro-diag; only reads that genuinely need
# supervisor privileges (journal, container logs, restart counters) are exposed here, consumed
# by the troubleshooting L2 scripts.

# Recent node log lines (bounded). $1 = max lines (default 400). Source by detected mode:
# systemd → the unit's journal; docker/podman → container logs; bare → the fixed spawn log.
ouro_node_logs() {
  local n="${1:-400}" pid mode unit cid rt
  pid="$(ouro_node_pid)"
  mode="$(ouro_node_detect_mode 2>/dev/null || echo bare)"
  case "$mode" in
    systemd) unit="$(ouro_supervisor_systemd_unit "$pid")"
             [ -n "$unit" ] && journalctl -u "$unit" --no-pager -n "$n" 2>/dev/null ;;
    docker|podman)
             cid="$(ouro_supervisor_container_id "$pid")"
             rt="$(ouro_supervisor_container_runtime "$pid")"
             [ -n "$cid" ] && "$rt" logs --tail "$n" "$cid" 2>&1 ;;
    *)       tail -n "$n" /var/log/cardano-node.log 2>/dev/null ;;
  esac
}

# Supervision-layer facts as `key=value` lines: mode, running, pid, uptime_s, restarts,
# oom_hits (kernel log evidence, bounded scan). Restart counters: systemd NRestarts /
# container RestartCount; bare has no supervisor, so restarts=-1 (unknown by design).
ouro_node_service_facts() {
  local pid mode unit cid rt restarts="-1" uptime_s="" oom=""
  pid="$(ouro_node_pid)"
  if [ -n "$pid" ]; then
    mode="$(ouro_node_detect_mode 2>/dev/null || echo bare)"
    uptime_s="$(ps -o etimes= -p "$pid" 2>/dev/null | tr -d ' ')"
    case "$mode" in
      systemd) unit="$(ouro_supervisor_systemd_unit "$pid")"
               [ -n "$unit" ] && restarts="$(systemctl show -p NRestarts --value "$unit" 2>/dev/null || echo -1)" ;;
      docker|podman)
               cid="$(ouro_supervisor_container_id "$pid")"
               rt="$(ouro_supervisor_container_runtime "$pid")"
               [ -n "$cid" ] && restarts="$("$rt" inspect --format '{{.RestartCount}}' "$cid" 2>/dev/null || echo -1)" ;;
    esac
  fi
  oom="$( (journalctl -k -n 5000 2>/dev/null || dmesg 2>/dev/null) | grep -ci 'out of memory\|oom-kill' || true)"
  printf 'mode=%s\nrunning=%s\npid=%s\nuptime_s=%s\nrestarts=%s\noom_hits=%s\n' \
    "${mode:-none}" "$([ -n "$pid" ] && echo true || echo false)" "${pid:-}" "${uptime_s:-}" "${restarts}" "${oom:-0}"
}
