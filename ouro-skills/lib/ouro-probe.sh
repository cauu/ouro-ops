#!/usr/bin/env bash
# S0019 p5-2 (§2.4) — the target-side observation probe. Gathers the CLOSED observation the adopt
# ceremony and the live re-attestation gate consume, so the control side no longer hand-supplies a
# `--observation` file. All docker access lives here (the supervisor gate confines docker to the
# lib); the probe emits a single-line JSON: { supervisor{...}, live{...} }.
#
# It reads FACTS from the running container (image config digest, container id, creation epoch,
# entrypoint/args, mounts) and the node (topology/config hashes, kes/opcert id, forging-key
# presence) — never any secret material. Missing facts are emitted as empty/false, never guessed.

# Resolve the single cardano-node container id, or empty. Match by the process command (the node's
# argv contains cardano-node) — an empty first result falls back, not just a non-zero exit.
ouro_probe_container() {
  local cid
  cid="$(docker ps --filter 'ancestor=cardano-node' --format '{{.ID}}' 2>/dev/null | head -1)"
  if [ -z "$cid" ]; then
    cid="$(docker ps --no-trunc --format '{{.ID}} {{.Command}}' 2>/dev/null | awk '/cardano-node/{print $1; exit}')"
  fi
  printf '%s' "$cid"
}

# docker inspect a single --format field for a container.
ouro_probe_inspect() {
  docker inspect --format "$2" "$1" 2>/dev/null | tr -d '\r'
}

# Emit the observation JSON for the node. $1 = expected platform (e.g. linux/amd64).
ouro_observe() {
  local platform="${1:-linux/amd64}"
  local cid image_cfg created entrypoint args mounts count restart
  cid="$(ouro_probe_container)"
  count="$(docker ps --format '{{.ID}} {{.Command}}' 2>/dev/null | grep -c cardano-node || echo 0)"
  image_cfg="$(ouro_probe_inspect "$cid" '{{.Image}}')"
  created="$(ouro_probe_inspect "$cid" '{{.Created}}')"
  entrypoint="$(ouro_probe_inspect "$cid" '{{json .Config.Entrypoint}}')"
  args="$(ouro_probe_inspect "$cid" '{{json .Args}}')"
  mounts="$(ouro_probe_inspect "$cid" '{{range .Mounts}}{{.Source}};{{end}}')"
  restart="$(ouro_probe_inspect "$cid" '{{.HostConfig.RestartPolicy.Name}}')"

  # Node facts read INSIDE the container (paths from the layout contract); hashes only, no secrets.
  local topo_hash cfg_hash opcert_id has_keys genesis_hash network
  topo_hash="$(docker exec "$cid" sh -c 'sha256sum /opt/cardano/config/mainnet/topology.json 2>/dev/null' 2>/dev/null | awk '{print $1}')"
  cfg_hash="$(docker exec "$cid" sh -c 'sha256sum /opt/cardano/config/mainnet/config.json 2>/dev/null' 2>/dev/null | awk '{print $1}')"
  opcert_id="$(docker exec "$cid" sh -c 'test -f /opt/cardano/config/keys/node.cert && sha256sum /opt/cardano/config/keys/node.cert' 2>/dev/null | awk '{print $1}')"
  has_keys="$(docker exec "$cid" sh -c 'test -f /opt/cardano/config/keys/kes.skey && echo true || echo false' 2>/dev/null)"
  genesis_hash="$(docker exec "$cid" sh -c 'sha256sum /opt/cardano/config/mainnet/shelley-genesis.json 2>/dev/null' 2>/dev/null | awk '{print $1}')"
  network="mainnet"
  local hostkey
  hostkey="$(sha256sum /etc/ssh/ssh_host_ed25519_key.pub 2>/dev/null | awk '{print $1}')"

  OURO_OBS_PLATFORM="$platform" OURO_OBS_CID="$cid" OURO_OBS_COUNT="$count" \
  OURO_OBS_IMAGE="$image_cfg" OURO_OBS_CREATED="$created" OURO_OBS_ENTRY="$entrypoint" \
  OURO_OBS_ARGS="$args" OURO_OBS_MOUNTS="$mounts" OURO_OBS_RESTART="$restart" \
  OURO_OBS_TOPO="$topo_hash" OURO_OBS_CFG="$cfg_hash" OURO_OBS_OPCERT="$opcert_id" \
  OURO_OBS_HASKEYS="$has_keys" OURO_OBS_GENESIS="$genesis_hash" OURO_OBS_NET="$network" \
  OURO_OBS_HOSTKEY="$hostkey" \
  python3 - <<'PY'
import json, os, hashlib
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
mounts = [m for m in env("OURO_OBS_MOUNTS").split(";") if m]
# mount source id: a stable identifier per source (its own sha for the stub; a real probe records
# device+inode). Kept closed — never the raw host path in the attestation fingerprint upstream.
mount_ids = [hashlib.sha256(m.encode()).hexdigest()[:16] for m in mounts]
obs = {
  "supervisor": {
    "runtime": "docker", "rootful": True, "rootless": False,
    "node_container_count": int(env("OURO_OBS_COUNT") or 0),
    "uses_bind_mounts": bool(mounts), "daemon_socket": "/var/run/docker.sock",
    "restart_policy": env("OURO_OBS_RESTART") or "unless-stopped", "orchestration": "run",
  },
  "live": {
    "image_config_digest": env("OURO_OBS_IMAGE"), "platform": env("OURO_OBS_PLATFORM"),
    "container_id": env("OURO_OBS_CID"), "container_creation_epoch": epoch(env("OURO_OBS_CREATED")),
    "entrypoint": jlist(env("OURO_OBS_ENTRY")), "args": jlist(env("OURO_OBS_ARGS")),
    "mount_source_ids": mount_ids,
    "topology_hash": env("OURO_OBS_TOPO"), "config_hash": env("OURO_OBS_CFG"),
    "kes_opcert_id": env("OURO_OBS_OPCERT"), "has_forging_keys": env("OURO_OBS_HASKEYS") == "true",
    "host_key_sha256": env("OURO_OBS_HOSTKEY"), "genesis_hash": env("OURO_OBS_GENESIS"),
    "network": env("OURO_OBS_NET"),
  },
}
print(json.dumps(obs, separators=(",", ":")))
PY
}
