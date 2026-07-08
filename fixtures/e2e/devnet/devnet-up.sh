#!/usr/bin/env bash
# S0015 p2-0/p2-1 — bring up a REAL forging private Cardano devnet (proven recipe).
# One stake pool forges Conway blocks via real KES/VRF/opcert. The key insight from the
# p2-0 spike: `systemStart` MUST be set to ~now at boot (else the node jumps past the
# Praos forecast window with no blocks to bridge it → NoLedgerView → never forges).
#
# Runs the amd64 cardano-node under emulation on arm64 (see the p2-0 decision record for
# the arch caveat). Idempotent-ish: regenerates genesis each call for a fresh systemStart.
set -euo pipefail

# Default to the arm64-NATIVE blinklabs image so this runs without qemu on arm64 hosts
# (faithful timing = results consistent with real hardware). Override NODE_IMG/PLATFORM
# for an amd64 runner (e.g. NODE_IMG=ghcr.io/intersectmbo/cardano-node:10.5.4 PLATFORM=linux/amd64).
NODE_IMG="${NODE_IMG:-ghcr.io/blinklabs-io/cardano-node:10.5.4}"
PLATFORM="${DEVNET_PLATFORM:-linux/arm64}"
DATA="${DEVNET_DATA:-/tmp/devnet}"
MAGIC="${DEVNET_MAGIC:-42}"
NAME="${DEVNET_NAME:-devnet-node}"
cli(){ docker run --rm --platform "$PLATFORM" -v "$DATA":/d --entrypoint cardano-cli "$NODE_IMG" "$@" 2>/dev/null | tr -d '\r'; }

rm -rf "$DATA"; mkdir -p "$DATA"
echo "[devnet] generating genesis (1 genesis-key, 1 pool)…"
docker run --rm --platform "$PLATFORM" -v "$DATA":/data --entrypoint cardano-cli "$NODE_IMG" \
  conway genesis create-testnet-data --genesis-keys 1 --pools 1 --stake-delegators 1 --utxo-keys 1 \
  --total-supply 30000000000000 --delegated-supply 15000000000000 --testnet-magic "$MAGIC" --out-dir /data >/dev/null

echo "[devnet] tuning shelley genesis for fast blocks + fresh systemStart…"
python3 - "$DATA" <<'PY'
import json,sys,datetime
d=sys.argv[1]
T=datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(seconds=20)
g=json.load(open(f"{d}/shelley-genesis.json"))
g.update(activeSlotsCoeff=0.5, securityParam=10, epochLength=500, slotLength=1, updateQuorum=1,
         systemStart=T.strftime("%Y-%m-%dT%H:%M:%SZ"))
json.dump(g,open(f"{d}/shelley-genesis.json","w"),indent=1)
b=json.load(open(f"{d}/byron-genesis.json")); b["startTime"]=int(T.timestamp())
json.dump(b,open(f"{d}/byron-genesis.json","w"),indent=1)
PY

echo "[devnet] computing hashes + writing node config…"
BH=$(cli byron genesis print-genesis-hash --genesis-json /d/byron-genesis.json)
SH=$(cli hash genesis-file --genesis /d/shelley-genesis.json)
AH=$(cli hash genesis-file --genesis /d/alonzo-genesis.json)
CH=$(cli hash genesis-file --genesis /d/conway-genesis.json)
python3 - "$DATA" "$BH" "$SH" "$AH" "$CH" <<'PY'
import json,sys
d,b,s,a,c=sys.argv[1:6]
cfg={"Protocol":"Cardano","RequiresNetworkMagic":"RequiresMagic","EnableP2P":False,
 "ByronGenesisFile":"/d/byron-genesis.json","ByronGenesisHash":b,
 "ShelleyGenesisFile":"/d/shelley-genesis.json","ShelleyGenesisHash":s,
 "AlonzoGenesisFile":"/d/alonzo-genesis.json","AlonzoGenesisHash":a,
 "ConwayGenesisFile":"/d/conway-genesis.json","ConwayGenesisHash":c,
 "LastKnownBlockVersion-Major":6,"LastKnownBlockVersion-Minor":0,"LastKnownBlockVersion-Alt":0,
 "TestShelleyHardForkAtEpoch":0,"TestAllegraHardForkAtEpoch":0,"TestMaryHardForkAtEpoch":0,
 "TestAlonzoHardForkAtEpoch":0,"TestBabbageHardForkAtEpoch":0,"TestConwayHardForkAtEpoch":0,
 "minSeverity":"Info","TurnOnLogging":True,"TurnOnLogMetrics":False,"UseTraceDispatcher":True,"TraceOptions":{}}
json.dump(cfg,open(f"{d}/config.json","w"),indent=1)
json.dump({"Producers":[]},open(f"{d}/topology.json","w"))
PY

echo "[devnet] starting node (forges with the POOL keys, not genesis-delegate)…"
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" --platform "$PLATFORM" -v "$DATA":/d --entrypoint cardano-node "$NODE_IMG" run \
  --config /d/config.json --topology /d/topology.json --database-path /d/db --socket-path /d/node.socket \
  --shelley-kes-key /d/pools-keys/pool1/kes.skey --shelley-vrf-key /d/pools-keys/pool1/vrf.skey \
  --shelley-operational-certificate /d/pools-keys/pool1/opcert.cert --port 3001 >/dev/null

echo "[devnet] waiting for the chain to advance (block height > 0)…"
for i in $(seq 1 24); do
  sleep 5
  tip=$(docker run --rm --platform "$PLATFORM" -v "$DATA":/d -e CARDANO_NODE_SOCKET_PATH=/d/node.socket \
        --entrypoint cardano-cli "$NODE_IMG" query tip --testnet-magic "$MAGIC" 2>/dev/null || true)
  blk=$(printf '%s' "$tip" | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("block",0))
except: print(0)' 2>/dev/null || echo 0)
  if [ "${blk:-0}" -gt 0 ] 2>/dev/null; then
    echo "[devnet] FORGING — $(printf '%s' "$tip" | tr -d '\n')"
    exit 0
  fi
done
echo "[devnet] chain did not advance in time; node logs:"; docker logs "$NAME" 2>&1 | tail -5
exit 1
