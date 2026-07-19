#!/usr/bin/env bash
# S0019 p5-2 (§2.4) — the target-side observation probe. Gathers the CLOSED observation the adopt
# ceremony and the live re-attestation gate consume, so the control side no longer hand-supplies a
# `--observation` file. All docker access lives here (the supervisor gate confines docker to the
# lib); the probe emits a single-line JSON: { supervisor{...}, live{...} }.
#
# It reads FACTS from the running container (image config digest, container id, creation epoch,
# entrypoint/args, mounts) and the node (topology/config hashes, kes/opcert id, forging-key
# presence) — never any secret material. Missing facts are emitted as empty/false, never guessed.

# Resolve the running cardano-node candidates from Docker's image reference or the exact
# conventional container name. The Blink Labs image starts through `/usr/local/bin/entrypoint
# run`, so `.Command` does not contain `cardano-node`; `ancestor=cardano-node` also misses a fully
# qualified `ghcr.io/blinklabs-io/cardano-node:<tag>` reference. The allowlist subsequently binds
# the selected candidate to its image config digest, so this is discovery rather than trust.
ouro_probe_containers() {
  docker ps --no-trunc --format '{{.ID}}|{{.Image}}|{{.Names}}|{{.Command}}' 2>/dev/null |
    awk -F '|' '$2 ~ /(^|\/)cardano-node(:|@|$)/ || $3 == "cardano-node" {print $1}'
}

ouro_probe_container() {
  ouro_probe_containers | head -1
}

# docker inspect a single --format field for a container.
ouro_probe_inspect() {
  docker inspect --format "$2" "$1" 2>/dev/null | tr -d '\r'
}

# Fixed-path, metadata-only KES rotation permission contract. Production evaluates this inside the
# container namespace. Tests may supply a fixture root. Output is three closed booleans—never key
# bytes or raw owner identifiers.
OURO_KES_ROTATION_PERMISSION_CHECK='
root=$1
keys_dir="${root}/opt/cardano/config/keys"
kes_key="${keys_dir}/kes.skey"
vrf_key="${keys_dir}/vrf.skey"
keys_directory_safe=false
kes_skey_private=false
vrf_skey_private=false
if test -d "$keys_dir" && ! test -L "$keys_dir"; then
  mode=$(stat -c %a "$keys_dir" 2>/dev/null || true)
  case "$mode" in
    ""|*[!0-7]*) ;;
    *) if test $((0$mode & 0002)) -eq 0; then keys_directory_safe=true; fi ;;
  esac
fi
private_key_safe() {
  path=$1
  test -f "$path" && ! test -L "$path" || return 1
  mode=$(stat -c %a "$path" 2>/dev/null || true)
  case "$mode" in 400|600) return 0 ;; *) return 1 ;; esac
}
if private_key_safe "$kes_key"; then kes_skey_private=true; fi
if private_key_safe "$vrf_key"; then vrf_skey_private=true; fi
printf "keys_directory_safe=%s\nkes_skey_private=%s\nvrf_skey_private=%s\n" \
  "$keys_directory_safe" "$kes_skey_private" "$vrf_skey_private"
'

ouro_kes_rotation_permission_facts() {
  docker exec "$1" sh -c "$OURO_KES_ROTATION_PERMISSION_CHECK" ouro-kes-permission-check "" "" 2>/dev/null
}

ouro_kes_rotation_permission_fixture_facts() {
  sh -c "$OURO_KES_ROTATION_PERMISSION_CHECK" ouro-kes-permission-check "$1"
}

# Emit the observation JSON for the node. $1 = expected platform (e.g. linux/amd64).
ouro_observe() {
  # Platform is derived from immutable image metadata below; the optional legacy argument is
  # intentionally ignored so an environment/argv override cannot forge the allowlist selector.
  local cid image_cfg created entrypoint args count restart
  cid="$(ouro_probe_container)"
  count="$(ouro_probe_containers | awk 'NF {n++} END {print n+0}')"
  image_cfg="$(ouro_probe_inspect "$cid" '{{.Image}}')"
  created="$(ouro_probe_inspect "$cid" '{{.Created}}')"
  entrypoint="$(ouro_probe_inspect "$cid" '{{json .Config.Entrypoint}}')"
  args="$(ouro_probe_inspect "$cid" '{{json .Args}}')"
  restart="$(ouro_probe_inspect "$cid" '{{.HostConfig.RestartPolicy.Name}}')"

  # Node facts read INSIDE the container (paths from the layout contract); hashes only, no secrets.
  local topo_hash cfg_hash opcert_id has_keys key_perms kes_rotation_perms genesis_hash network tip1 tip2 creds_ok kes_info kes_genesis metrics peers
  topo_hash="$(docker exec "$cid" sh -c 'sha256sum /opt/cardano/config/mainnet/topology.json 2>/dev/null' 2>/dev/null | awk '{print $1}')"
  cfg_hash="$(docker exec "$cid" sh -c 'sha256sum /opt/cardano/config/mainnet/config.json 2>/dev/null' 2>/dev/null | awk '{print $1}')"
  opcert_id="$(docker exec "$cid" sh -c 'test -f /opt/cardano/config/keys/node.cert && sha256sum /opt/cardano/config/keys/node.cert' 2>/dev/null | awk '{print $1}')"
  has_keys="$(docker exec "$cid" sh -c 'if test -f /opt/cardano/config/keys/kes.skey || test -f /opt/cardano/config/keys/vrf.skey; then echo true; else echo false; fi' 2>/dev/null)"
  # A BP is not adoptable when its forging secrets are group/world accessible. `stat` emits only
  # modes; no private key content leaves the container. Relays normally have no such files and
  # therefore report false, which is ignored for the relay role while `has_forging_keys` still
  # causes a role-mismatch refusal.
  local diag_uid
  diag_uid="$(id -u ouro-diag 2>/dev/null || true)"
  key_perms="$(docker exec "$cid" sh -c '
    diag_uid=$1
    ok=true
    service_uid=
    for path in /opt/cardano/config/keys /opt/cardano/config/keys/kes.skey /opt/cardano/config/keys/vrf.skey; do
      mode=$(stat -c %a "$path" 2>/dev/null) || ok=false
      owner=$(stat -c %u "$path" 2>/dev/null) || ok=false
      case "$mode" in [0-7]00) ;; *) ok=false ;; esac
      if test -z "$service_uid"; then service_uid=$owner; fi
      test "$owner" = "$service_uid" || ok=false
      test -z "$diag_uid" || test "$owner" != "$diag_uid" || ok=false
    done
    if "$ok"; then echo true; else echo false; fi
  ' ouro-key-permission-check "$diag_uid" 2>/dev/null)"
  kes_rotation_perms="$(ouro_kes_rotation_permission_facts "$cid" || true)"
  # Use Cardano's semantic genesis hash, not the byte-level sha256sum of one JSON serialization.
  # The pool spec and website carry this canonical network identity; whitespace/key-order changes
  # to an equivalent genesis file must not invalidate every operation plan.
  genesis_hash="$(docker exec "$cid" cardano-cli hash genesis-file \
    --genesis /opt/cardano/config/mainnet/shelley-genesis.json 2>/dev/null | tr -d '\r\n')"
  network="$(ouro_probe_inspect "$cid" '{{range .Config.Env}}{{println .}}{{end}}' | awk -F= '$1 == "CARDANO_NETWORK" {print $2; exit}')"
  # Bounded readiness evidence. Two socket queries are sampled on the target; slot is preferred over
  # block because a low-stake BP need not forge, while the network tip should still advance.
  case "$network" in
    mainnet) tip1="$(docker exec "$cid" cardano-cli query tip --socket-path /ipc/node.socket --mainnet 2>/dev/null || true)" ;;
    preprod) tip1="$(docker exec "$cid" cardano-cli query tip --socket-path /ipc/node.socket --testnet-magic 1 2>/dev/null || true)" ;;
    preview) tip1="$(docker exec "$cid" cardano-cli query tip --socket-path /ipc/node.socket --testnet-magic 2 2>/dev/null || true)" ;;
    *) tip1="" ;;
  esac
  # Do not require a new block inside a two-second fleet window: block arrival is probabilistic.
  # One bounded successful query plus syncProgress is the liveness/freshness signal.
  tip2="$tip1"
  creds_ok="$(docker exec "$cid" sh -c 'test -f /opt/cardano/config/keys/kes.skey && test -f /opt/cardano/config/keys/vrf.skey && test -f /opt/cardano/config/keys/node.cert && echo true || echo false' 2>/dev/null)"
  case "$network" in
    mainnet) kes_info="$(docker exec "$cid" cardano-cli query kes-period-info --socket-path /ipc/node.socket --op-cert-file /opt/cardano/config/keys/node.cert --mainnet --output-json 2>/dev/null || true)" ;;
    preprod) kes_info="$(docker exec "$cid" cardano-cli query kes-period-info --socket-path /ipc/node.socket --op-cert-file /opt/cardano/config/keys/node.cert --testnet-magic 1 --output-json 2>/dev/null || true)" ;;
    preview) kes_info="$(docker exec "$cid" cardano-cli query kes-period-info --socket-path /ipc/node.socket --op-cert-file /opt/cardano/config/keys/node.cert --testnet-magic 2 --output-json 2>/dev/null || true)" ;;
    *) kes_info="" ;;
  esac
  # An expired opcert may make `cardano-cli query kes-period-info` exit without its JSON record.
  # Preserve a read-only, version-tolerant fallback from the node's local metrics plus the public
  # genesis parameter. The tip slot, not `currentKESPeriod` (observed as zero on some releases), is
  # the source for the current period. Counter evidence remains unavailable in this fallback, so a
  # period-valid BP still cannot be declared ready solely from metrics.
  kes_genesis="$(docker exec "$cid" cat /opt/cardano/config/mainnet/shelley-genesis.json 2>/dev/null || true)"
  metrics="$(curl -fsS --max-time 3 http://127.0.0.1:12798/metrics 2>/dev/null |
    awk '$1 ~ /operationalCertificate(Start|Expiry)KESPeriod_int$/ {print}' || true)"
  # Blink Labs images currently ship `ss` but not `netstat`. Prefer iproute2's `ss`, retain a
  # net-tools fallback for older images, and fail closed to zero when neither command exists.
  peers="$(docker exec "$cid" sh -c "if command -v ss >/dev/null 2>&1; then ss -Htn state established 2>/dev/null | awk 'NF {n++} END {print n+0}'; elif command -v netstat >/dev/null 2>&1; then netstat -tn 2>/dev/null | awk '\$6 == \"ESTABLISHED\" {n++} END {print n+0}'; else echo 0; fi" 2>/dev/null || echo 0)"
  local hostkey full_json image_json
  hostkey="${OURO_HOST_KEY_SHA256:-}"
  if [ -z "$hostkey" ] && [ -f /etc/ssh/ssh_host_ed25519_key.pub ]; then
    # Same OpenSSH SHA256 fingerprint representation used by onboarding/known_hosts. Hashing the
    # public-key text file would produce a different identity from the key that completed SSH.
    hostkey="$(ssh-keygen -E sha256 -lf /etc/ssh/ssh_host_ed25519_key.pub 2>/dev/null | awk '{print $2}')"
  fi
  # Full inspect JSON — the CLOSED source for the upgrade recreate spec (§2.10): name, restart,
  # network mode, bind mounts (src:dst:ro), env, published ports, and the resolved command. These
  # are TARGET-SIDE facts (never agent strings); the executor recreates onto the new digest from them.
  full_json="$(docker inspect "$cid" 2>/dev/null)"
  image_json="$(docker image inspect "$image_cfg" 2>/dev/null)"

  OURO_OBS_CID="$cid" OURO_OBS_COUNT="$count" \
  OURO_OBS_IMAGE="$image_cfg" OURO_OBS_CREATED="$created" OURO_OBS_ENTRY="$entrypoint" \
  OURO_OBS_ARGS="$args" OURO_OBS_RESTART="$restart" \
  OURO_OBS_TOPO="$topo_hash" OURO_OBS_CFG="$cfg_hash" OURO_OBS_OPCERT="$opcert_id" \
  OURO_OBS_HASKEYS="$has_keys" OURO_OBS_KEY_PERMS="$key_perms" \
  OURO_OBS_KEYS_DIRECTORY_SAFE="$(printf '%s\n' "$kes_rotation_perms" | awk -F= '$1 == "keys_directory_safe" {print $2}')" \
  OURO_OBS_KES_SKEY_PRIVATE="$(printf '%s\n' "$kes_rotation_perms" | awk -F= '$1 == "kes_skey_private" {print $2}')" \
  OURO_OBS_VRF_SKEY_PRIVATE="$(printf '%s\n' "$kes_rotation_perms" | awk -F= '$1 == "vrf_skey_private" {print $2}')" \
  OURO_OBS_GENESIS="$genesis_hash" OURO_OBS_NET="$network" \
  OURO_OBS_HOSTKEY="$hostkey" OURO_OBS_FULL="$full_json" OURO_OBS_IMAGE_FULL="$image_json" \
  OURO_OBS_TIP1="$tip1" OURO_OBS_TIP2="$tip2" OURO_OBS_CREDS="$creds_ok" \
  OURO_OBS_KES_INFO="$kes_info" OURO_OBS_KES_GENESIS="$kes_genesis" \
  OURO_OBS_METRICS="$metrics" OURO_OBS_PEERS="$peers" \
  python3 - <<'PY'
import json, os, stat
def env(k): return os.environ.get(k, "") or ""
def epoch(created):
    # docker Created is RFC3339; fall back to 0 if unparsable (no ambient clock use).
    try:
        import datetime, re
        # Docker commonly emits nanoseconds while Python datetime accepts microseconds. Preserve
        # the timestamp semantics and truncate only excess fractional precision.
        created = re.sub(r"(\.\d{6})\d+", r"\1", created)
        return int(datetime.datetime.fromisoformat(created.replace("Z","+00:00")).timestamp())
    except Exception:
        return 0
def jlist(s):
    try:
        v = json.loads(s)
        return v if isinstance(v, list) else []
    except Exception:
        return []
def tip_value(s):
    try:
        value = json.loads(s)
        return int(value.get("slot", value.get("block", -1)))
    except Exception:
        return -1
def tip_int(s, key):
    try:
        value = json.loads(s)
        raw = value.get(key)
        return int(raw) if raw is not None else None
    except Exception:
        return None
def tip_text(s, key):
    try:
        value = json.loads(s)
        raw = value.get(key)
        return str(raw) if raw is not None else None
    except Exception:
        return None
def tip_synced(s):
    try:
        value = json.loads(s)
        progress = str(value.get("syncProgress", "")).rstrip("%")
        return float(progress) >= 99.0
    except Exception:
        return False
def kes_facts(s):
    try:
        # cardano-cli prints two human diagnostics before the --output-json object. Parse the
        # unique terminal object and reject trailing/ambiguous structured output.
        start = s.find("{")
        if start < 0:
            return None
        value = json.loads(s[start:])
        current = int(value["qKesCurrentKesPeriod"])
        start = int(value["qKesStartKesInterval"])
        end = int(value["qKesEndKesInterval"])
        on_disk = int(value["qKesOnDiskOperationalCertificateNumber"])
        node_state_raw = value["qKesNodeStateOperationalCertificateNumber"]
        period_valid = start <= current < end
        if node_state_raw is None:
            node_state = None
            counter_consistent = None
            counter_status = "no_blocks_minted_yet"
            # A null node-state counter is meaningful evidence but does not independently bind the
            # cold pool identity. Only the typed install-opcert transaction may combine it with its
            # candidate-bound active-opcert check; ordinary readiness remains fail-closed.
            valid = False
        else:
            node_state = int(node_state_raw)
            counter_consistent = node_state <= on_disk <= node_state + 1
            counter_status = "present"
            valid = period_valid and counter_consistent
        return {
            "source": "cardano_cli",
            "current_period": current,
            "start_period": start,
            "end_period": end,
            "remaining_periods": end - current,
            "opcert_counter_on_disk": on_disk,
            "opcert_counter_node_state": node_state,
            "counter_consistent": counter_consistent,
            "counter_status": counter_status,
            "period_valid": period_valid,
            "valid": valid,
        }
    except Exception:
        return None
def prometheus_value(s, semantic_suffix):
    try:
        for line in s.splitlines():
            if not line or line.startswith("#"):
                continue
            name, raw = line.rsplit(None, 1)
            name = name.split("{", 1)[0]
            if name == semantic_suffix or name.endswith("_" + semantic_suffix):
                return int(float(raw))
    except Exception:
        pass
    return None
def kes_metric_facts(metrics, tip, genesis):
    try:
        genesis_value = json.loads(genesis)
        slots_per_period = int(genesis_value["slotsPerKESPeriod"])
        slot = tip_int(tip, "slot")
        start = prometheus_value(metrics, "operationalCertificateStartKESPeriod_int")
        end = prometheus_value(metrics, "operationalCertificateExpiryKESPeriod_int")
        if slot is None or slots_per_period <= 0 or start is None or end is None:
            return None
        current = slot // slots_per_period
        return {
            "source": "prometheus_tip_and_genesis",
            "current_period": current,
            "start_period": start,
            "end_period": end,
            "remaining_periods": end - current,
            "opcert_counter_on_disk": None,
            "opcert_counter_node_state": None,
            "counter_consistent": None,
            "counter_status": "unavailable",
            "period_valid": start <= current < end,
            "valid": start <= current < end,
        }
    except Exception:
        return None
def kes_state(s):
    facts = kes_facts(s)
    return bool(facts and facts["valid"])
def bp_configured():
    config = ((inspect_record() or {}).get("Config", {}) or {})
    return "CARDANO_BLOCK_PRODUCER=true" in list(config.get("Env", []) or [])
def inspect_record():
    try:
        value = json.loads(env("OURO_OBS_FULL"))
        return value[0] if isinstance(value, list) and len(value) == 1 else None
    except Exception:
        return None

def image_record():
    try:
        value = json.loads(env("OURO_OBS_IMAGE_FULL"))
        return value[0] if isinstance(value, list) and len(value) == 1 else None
    except Exception:
        return None

def typed_mounts():
    record = inspect_record()
    if record is None:
        return []
    result = []
    for mount in (record.get("Mounts", []) or []):
        source = mount.get("Source", "") or ""
        try:
            # Docker Desktop reports VM bind paths as /host_mnt/<host-path>. The stripped path is
            # used only when the literal source is absent; Linux targets always stat the literal
            # Docker source and therefore bind the real host device+inode.
            candidates = [source]
            if source.startswith("/var/"):
                candidates.append("/private" + source)
            if source.startswith("/host_mnt/"):
                stripped = source[len("/host_mnt"):]
                candidates.append(stripped)
                if stripped.startswith("/var/"):
                    candidates.append("/private" + stripped)
            stat_source = next(candidate for candidate in candidates if os.path.lexists(candidate))
            metadata = os.lstat(stat_source)
            source_id = f"{metadata.st_dev}:{metadata.st_ino}"
            owner = f"{metadata.st_uid}:{metadata.st_gid}"
            mode = format(stat.S_IMODE(metadata.st_mode), "04o")
            no_symlink = not stat.S_ISLNK(metadata.st_mode)
        except Exception:
            source_id, owner, mode, no_symlink = "", "", "", False
        result.append({
            "kind": mount.get("Type", "") or "",
            "source_id": source_id,
            "destination": mount.get("Destination", "") or "",
            "read_only": not mount.get("RW", True),
            "owner": owner,
            "mode": mode,
            "no_symlink": no_symlink,
        })
    return result

mounts = typed_mounts()

# Upgrade recreate spec (§2.10) — parsed from the full inspect JSON. Fail-closed: emit null when the
# container shape is not the standard single-container bind-mounted layout, so the executor refuses
# rather than recreate a container it cannot faithfully reproduce.
def recreate_spec():
    try:
        d = inspect_record()
    except Exception:
        return None
    if d is None:
        return None
    hc = d.get("HostConfig", {}) or {}
    cfg = d.get("Config", {}) or {}
    image = image_record()
    if image is None:
        return None
    image_cfg = image.get("Config", {}) or {}
    # v1 recreates only a deliberately small, direct `docker run` contract. Any explicit setting
    # outside the modeled fields below makes the spec null. Silently dropping one of these would
    # change privilege, namespace, identity, DNS, resource or filesystem semantics on upgrade.
    # Docker merges image defaults into container Config. Allow inherited labels/user/workdir/etc.
    # only when they exactly match the current image; any container-level override is unmodeled.
    inherited_fields = ["User", "WorkingDir", "Healthcheck", "Labels", "StopSignal", "StopTimeout"]
    def normalized(field, value):
        if field in ("Labels", "Healthcheck"):
            return value or {}
        return value if value is not None else ""
    if any(normalized(field, cfg.get(field)) != normalized(field, image_cfg.get(field))
           for field in inherited_fields):
        return None
    if any(bool(cfg.get(field, False)) for field in ["Tty", "OpenStdin", "StdinOnce"]):
        return None
    if (cfg.get("Domainname", "") or ""):
        return None
    hostname = cfg.get("Hostname", "") or ""
    container_id = d.get("Id", "") or ""
    if hostname and (not container_id or hostname != container_id[:12]):
        return None
    false_only = ["Privileged", "ReadonlyRootfs", "AutoRemove", "PublishAllPorts"]
    if any(bool(hc.get(field, False)) for field in false_only):
        return None
    empty_lists = [
        "CapAdd", "CapDrop", "SecurityOpt", "Devices", "DeviceRequests", "Dns",
        "DnsOptions", "DnsSearch", "ExtraHosts", "GroupAdd", "Links", "Ulimits",
    ]
    if any(bool(hc.get(field)) for field in empty_lists):
        return None
    empty_maps = ["Tmpfs", "Sysctls", "StorageOpt"]
    if any(bool(hc.get(field)) for field in empty_maps):
        return None
    allowed_modes = {
        "IpcMode": (None, "", "private"),
        "PidMode": (None, ""),
        "UTSMode": (None, ""),
        "UsernsMode": (None, ""),
        "CgroupnsMode": (None, "", "private"),
        "Runtime": (None, "", "runc"),
    }
    if any(hc.get(field) not in allowed for field, allowed in allowed_modes.items()):
        return None
    log_config = hc.get("LogConfig") or {}
    if log_config.get("Type") not in (None, "", "json-file") or bool(log_config.get("Config")):
        return None
    zero_resources = [
        "Memory", "MemoryReservation", "MemorySwap", "MemorySwappiness", "NanoCpus",
        "CpuShares", "CpuPeriod", "CpuQuota", "CpuRealtimePeriod", "CpuRealtimeRuntime",
        "BlkioWeight", "OomScoreAdj",
    ]
    if any((hc.get(field) or 0) != 0 for field in zero_resources):
        return None
    empty_resources = ["CpusetCpus", "CpusetMems", "CgroupParent"]
    if any(bool(hc.get(field)) for field in empty_resources):
        return None
    if hc.get("OomKillDisable") not in (None, False) or hc.get("PidsLimit") not in (None, 0):
        return None
    # Docker's standard 64 MiB shm default is reproducible by the same daemon contract; any custom
    # value is an unmodeled run option and therefore refused.
    if hc.get("ShmSize") not in (None, 0, 67108864):
        return None
    restart = hc.get("RestartPolicy", {}) or {}
    if (restart.get("MaximumRetryCount") or 0) != 0:
        return None
    binds = []
    for m in (d.get("Mounts", []) or []):
        if m.get("Type") != "bind":
            return None  # named volumes / tmpfs are not modeled — refuse (fail-closed)
        if m.get("Propagation") not in (None, "", "rprivate"):
            return None
        expected_mode = "rw" if m.get("RW", True) else "ro"
        if m.get("Mode") not in (None, "", expected_mode):
            return None
        binds.append({"source": m.get("Source", ""), "destination": m.get("Destination", ""),
                      "read_only": not m.get("RW", True)})
    network_mode = hc.get("NetworkMode", "") or ""
    networks = ((d.get("NetworkSettings", {}) or {}).get("Networks", {}) or {})
    expected_network = "bridge" if network_mode in ("", "default", "bridge") else network_mode
    if set(networks) != {expected_network}:
        return None
    network = networks[expected_network] or {}
    if network.get("IPAMConfig") not in (None, {}) or bool(network.get("Aliases")):
        return None
    ports = []
    for cont, confs in (hc.get("PortBindings", {}) or {}).items():
        for c in (confs or []):
            ports.append({"container": cont, "host_ip": c.get("HostIp", "") or "",
                          "host_port": c.get("HostPort", "") or ""})
    return {
        "name": (d.get("Name", "") or "").lstrip("/"),
        "restart_policy": ((hc.get("RestartPolicy", {}) or {}).get("Name", "") or ""),
        "network_mode": network_mode,
        "binds": binds,
        "env": list(cfg.get("Env", []) or []),
        "ports": ports,
        "entrypoint": d.get("Path", "") or "",
        "args": list(d.get("Args", []) or []),
    }

obs = {
  "supervisor": {
    "runtime": "docker", "rootful": True, "rootless": False,
    "node_container_count": int(env("OURO_OBS_COUNT") or 0),
    "uses_bind_mounts": bool(mounts) and all(m["kind"] == "bind" for m in mounts),
    "daemon_socket": "/var/run/docker.sock",
    "restart_policy": env("OURO_OBS_RESTART") or "unless-stopped", "orchestration": "run",
  },
  "live": {
    "image_config_digest": env("OURO_OBS_IMAGE"),
    "platform": "/".join(filter(None, [
        str((image_record() or {}).get("Os", "") or ""),
        str((image_record() or {}).get("Architecture", "") or ""),
    ])),
    "container_id": env("OURO_OBS_CID"), "container_creation_epoch": epoch(env("OURO_OBS_CREATED")),
    "container_name": (((inspect_record() or {}).get("Name", "") or "").lstrip("/")),
    "image_reference": (((inspect_record() or {}).get("Config", {}) or {}).get("Image", "") or ""),
    "entrypoint": jlist(env("OURO_OBS_ENTRY")), "args": jlist(env("OURO_OBS_ARGS")),
    "image_entrypoint": list(((image_record() or {}).get("Config", {}) or {}).get("Entrypoint", []) or []),
    "image_cmd": list(((image_record() or {}).get("Config", {}) or {}).get("Cmd", []) or []),
    "mounts": mounts,
    "topology_hash": env("OURO_OBS_TOPO"), "config_hash": env("OURO_OBS_CFG"),
    "kes_opcert_id": env("OURO_OBS_OPCERT"), "has_forging_keys": env("OURO_OBS_HASKEYS") == "true",
    "forging_key_permissions_safe": env("OURO_OBS_KEY_PERMS") == "true",
    "keys_directory_safe": env("OURO_OBS_KEYS_DIRECTORY_SAFE") == "true",
    "kes_skey_private": env("OURO_OBS_KES_SKEY_PRIVATE") == "true",
    "vrf_skey_private": env("OURO_OBS_VRF_SKEY_PRIVATE") == "true",
    "host_key_sha256": env("OURO_OBS_HOSTKEY"), "genesis_hash": env("OURO_OBS_GENESIS"),
    "network": env("OURO_OBS_NET"),
  },
  "readiness": {
    "node_running": bool(env("OURO_OBS_CID")),
    "socket_answers": tip_value(env("OURO_OBS_TIP1")) >= 0 and tip_value(env("OURO_OBS_TIP2")) >= 0,
    "tip_block": tip_value(env("OURO_OBS_TIP1")),
    "tip_block_next": tip_value(env("OURO_OBS_TIP2")),
    "tip_block_height": tip_int(env("OURO_OBS_TIP2"), "block"),
    "tip_slot": tip_int(env("OURO_OBS_TIP2"), "slot"),
    "tip_era": tip_text(env("OURO_OBS_TIP2"), "era"),
    "sync_progress": tip_text(env("OURO_OBS_TIP2"), "syncProgress"),
    "tip_synced": tip_synced(env("OURO_OBS_TIP2")),
    "kes_opcert_valid": kes_state(env("OURO_OBS_KES_INFO")),
    "kes": (
        kes_facts(env("OURO_OBS_KES_INFO"))
        or kes_metric_facts(env("OURO_OBS_METRICS"), env("OURO_OBS_TIP2"),
                            env("OURO_OBS_KES_GENESIS"))
    ),
    "block_producer_configured": bp_configured(),
    "forging_credentials_ready": (
        env("OURO_OBS_CREDS") == "true" and bp_configured()
        and kes_state(env("OURO_OBS_KES_INFO")) and tip_value(env("OURO_OBS_TIP1")) >= 0
    ),
    "established_peers": int(env("OURO_OBS_PEERS") or 0),
  },
  "recreate": recreate_spec(),
}
print(json.dumps(obs, separators=(",", ":")))
PY
}
