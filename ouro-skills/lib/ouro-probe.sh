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

# Emit the observation JSON for the node. $1 = expected platform (e.g. linux/amd64).
ouro_observe() {
  local platform="${1:-linux/amd64}"
  local cid image_cfg created entrypoint args count restart
  cid="$(ouro_probe_container)"
  count="$(ouro_probe_containers | awk 'NF {n++} END {print n+0}')"
  image_cfg="$(ouro_probe_inspect "$cid" '{{.Image}}')"
  created="$(ouro_probe_inspect "$cid" '{{.Created}}')"
  entrypoint="$(ouro_probe_inspect "$cid" '{{json .Config.Entrypoint}}')"
  args="$(ouro_probe_inspect "$cid" '{{json .Args}}')"
  restart="$(ouro_probe_inspect "$cid" '{{.HostConfig.RestartPolicy.Name}}')"

  # Node facts read INSIDE the container (paths from the layout contract); hashes only, no secrets.
  local topo_hash cfg_hash opcert_id has_keys genesis_hash network tip1 tip2 creds_ok kes_valid peers
  topo_hash="$(docker exec "$cid" sh -c 'sha256sum /opt/cardano/config/mainnet/topology.json 2>/dev/null' 2>/dev/null | awk '{print $1}')"
  cfg_hash="$(docker exec "$cid" sh -c 'sha256sum /opt/cardano/config/mainnet/config.json 2>/dev/null' 2>/dev/null | awk '{print $1}')"
  opcert_id="$(docker exec "$cid" sh -c 'test -f /opt/cardano/config/keys/node.cert && sha256sum /opt/cardano/config/keys/node.cert' 2>/dev/null | awk '{print $1}')"
  has_keys="$(docker exec "$cid" sh -c 'test -f /opt/cardano/config/keys/kes.skey && echo true || echo false' 2>/dev/null)"
  genesis_hash="$(docker exec "$cid" sh -c 'sha256sum /opt/cardano/config/mainnet/shelley-genesis.json 2>/dev/null' 2>/dev/null | awk '{print $1}')"
  network="mainnet"
  # Bounded readiness evidence. Two socket queries are sampled on the target; slot is preferred over
  # block because a low-stake BP need not forge, while the network tip should still advance.
  tip1="$(docker exec "$cid" cardano-cli query tip --socket-path /ipc/node.socket --mainnet 2>/dev/null || true)"
  sleep "${OURO_READINESS_SAMPLE_DELAY:-2}"
  tip2="$(docker exec "$cid" cardano-cli query tip --socket-path /ipc/node.socket --mainnet 2>/dev/null || true)"
  creds_ok="$(docker exec "$cid" sh -c 'test -f /opt/cardano/config/keys/kes.skey && test -f /opt/cardano/config/keys/vrf.skey && test -f /opt/cardano/config/keys/node.cert && echo true || echo false' 2>/dev/null)"
  kes_valid="$(docker exec "$cid" sh -c 'test -s /opt/cardano/config/keys/kes.skey && test -s /opt/cardano/config/keys/node.cert && echo true || echo false' 2>/dev/null)"
  peers="$(docker exec "$cid" sh -c "netstat -tn 2>/dev/null | awk '\$6 == \"ESTABLISHED\" {n++} END {print n+0}'" 2>/dev/null || echo 0)"
  local hostkey full_json
  hostkey="${OURO_HOST_KEY_SHA256:-}"
  if [ -z "$hostkey" ] && [ -f /etc/ssh/ssh_host_ed25519_key.pub ]; then
    if command -v sha256sum >/dev/null 2>&1; then
      hostkey="$(sha256sum /etc/ssh/ssh_host_ed25519_key.pub 2>/dev/null | awk '{print $1}')"
    else
      hostkey="$(shasum -a 256 /etc/ssh/ssh_host_ed25519_key.pub 2>/dev/null | awk '{print $1}')"
    fi
  fi
  # Full inspect JSON — the CLOSED source for the upgrade recreate spec (§2.10): name, restart,
  # network mode, bind mounts (src:dst:ro), env, published ports, and the resolved command. These
  # are TARGET-SIDE facts (never agent strings); the executor recreates onto the new digest from them.
  full_json="$(docker inspect "$cid" 2>/dev/null)"

  OURO_OBS_PLATFORM="$platform" OURO_OBS_CID="$cid" OURO_OBS_COUNT="$count" \
  OURO_OBS_IMAGE="$image_cfg" OURO_OBS_CREATED="$created" OURO_OBS_ENTRY="$entrypoint" \
  OURO_OBS_ARGS="$args" OURO_OBS_RESTART="$restart" \
  OURO_OBS_TOPO="$topo_hash" OURO_OBS_CFG="$cfg_hash" OURO_OBS_OPCERT="$opcert_id" \
  OURO_OBS_HASKEYS="$has_keys" OURO_OBS_GENESIS="$genesis_hash" OURO_OBS_NET="$network" \
  OURO_OBS_HOSTKEY="$hostkey" OURO_OBS_FULL="$full_json" \
  OURO_OBS_TIP1="$tip1" OURO_OBS_TIP2="$tip2" OURO_OBS_CREDS="$creds_ok" \
  OURO_OBS_KES_VALID="$kes_valid" OURO_OBS_PEERS="$peers" \
  python3 - <<'PY'
import json, os, stat
def env(k): return os.environ.get(k, "") or ""
def epoch(created):
    # docker Created is RFC3339; fall back to 0 if unparsable (no ambient clock use).
    try:
        import datetime
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
def inspect_record():
    try:
        value = json.loads(env("OURO_OBS_FULL"))
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
    binds = []
    for m in (d.get("Mounts", []) or []):
        if m.get("Type") != "bind":
            return None  # named volumes / tmpfs are not modeled — refuse (fail-closed)
        binds.append({"source": m.get("Source", ""), "destination": m.get("Destination", ""),
                      "read_only": not m.get("RW", True)})
    ports = []
    for cont, confs in (hc.get("PortBindings", {}) or {}).items():
        for c in (confs or []):
            ports.append({"container": cont, "host_ip": c.get("HostIp", "") or "",
                          "host_port": c.get("HostPort", "") or ""})
    return {
        "name": (d.get("Name", "") or "").lstrip("/"),
        "restart_policy": ((hc.get("RestartPolicy", {}) or {}).get("Name", "") or ""),
        "network_mode": hc.get("NetworkMode", "") or "",
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
    "image_config_digest": env("OURO_OBS_IMAGE"), "platform": env("OURO_OBS_PLATFORM"),
    "container_id": env("OURO_OBS_CID"), "container_creation_epoch": epoch(env("OURO_OBS_CREATED")),
    "entrypoint": jlist(env("OURO_OBS_ENTRY")), "args": jlist(env("OURO_OBS_ARGS")),
    "mounts": mounts,
    "topology_hash": env("OURO_OBS_TOPO"), "config_hash": env("OURO_OBS_CFG"),
    "kes_opcert_id": env("OURO_OBS_OPCERT"), "has_forging_keys": env("OURO_OBS_HASKEYS") == "true",
    "host_key_sha256": env("OURO_OBS_HOSTKEY"), "genesis_hash": env("OURO_OBS_GENESIS"),
    "network": env("OURO_OBS_NET"),
  },
  "readiness": {
    "node_running": bool(env("OURO_OBS_CID")),
    "socket_answers": tip_value(env("OURO_OBS_TIP1")) >= 0 and tip_value(env("OURO_OBS_TIP2")) >= 0,
    "tip_block": tip_value(env("OURO_OBS_TIP1")),
    "tip_block_next": tip_value(env("OURO_OBS_TIP2")),
    "kes_opcert_valid": env("OURO_OBS_KES_VALID") == "true",
    "credential_loaded": env("OURO_OBS_CREDS") == "true",
    "established_peers": int(env("OURO_OBS_PEERS") or 0),
  },
  "recreate": recreate_spec(),
}
print(json.dumps(obs, separators=(",", ":")))
PY
}
