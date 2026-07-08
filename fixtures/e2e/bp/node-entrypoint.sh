#!/usr/bin/env bash
# bp1 boot: bring up a REAL forging private Cardano devnet locally (fresh systemStart at
# boot — the p2-0 load-bearing insight), then start sshd so control can dispatch to it.
# cardano-node/cardano-cli are IN this image (no docker-in-docker). Genesis is generated
# on first boot into $DEVNET so it is fresh; restarts reuse it.
set -euo pipefail

# Chain state lives OUTSIDE /opt/cardano so `deploy/provision` (which chowns /opt/cardano
# to the node user) never touches the live db/socket. p2-2 status queries this socket path.
DEVNET=/opt/devnet
SOCK=/opt/devnet/node.socket
MAGIC="${DEVNET_MAGIC:-1}"           # preprod magic (schema-valid; see p2-0 decision)
export CARDANO_NODE_SOCKET_PATH="$SOCK"

if [ ! -f "$DEVNET/config.json" ]; then
  echo "[bp1] generating fresh forging devnet (magic=$MAGIC)…"
  rm -rf "$DEVNET"; mkdir -p "$DEVNET"
  cardano-cli conway genesis create-testnet-data --genesis-keys 1 --pools 1 \
    --stake-delegators 1 --utxo-keys 1 --total-supply 30000000000000 \
    --delegated-supply 15000000000000 --testnet-magic "$MAGIC" --out-dir "$DEVNET" >/dev/null

  python3 - "$DEVNET" <<'PY'
import json,sys,datetime
d=sys.argv[1]
T=datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(seconds=15)
g=json.load(open(f"{d}/shelley-genesis.json"))
g.update(activeSlotsCoeff=0.5, securityParam=10, epochLength=500, slotLength=1,
         updateQuorum=1, systemStart=T.strftime("%Y-%m-%dT%H:%M:%SZ"))
json.dump(g,open(f"{d}/shelley-genesis.json","w"),indent=1)
b=json.load(open(f"{d}/byron-genesis.json")); b["startTime"]=int(T.timestamp())
json.dump(b,open(f"{d}/byron-genesis.json","w"),indent=1)
PY

  BH=$(cardano-cli byron genesis print-genesis-hash --genesis-json "$DEVNET/byron-genesis.json" | tr -d '\r')
  SH=$(cardano-cli hash genesis-file --genesis "$DEVNET/shelley-genesis.json" | tr -d '\r')
  AH=$(cardano-cli hash genesis-file --genesis "$DEVNET/alonzo-genesis.json" | tr -d '\r')
  CH=$(cardano-cli hash genesis-file --genesis "$DEVNET/conway-genesis.json" | tr -d '\r')
  python3 - "$DEVNET" "$BH" "$SH" "$AH" "$CH" <<'PY'
import json,sys
d,b,s,a,c=sys.argv[1:6]
cfg={"Protocol":"Cardano","RequiresNetworkMagic":"RequiresMagic","EnableP2P":False,
 "ByronGenesisFile":"byron-genesis.json","ByronGenesisHash":b,
 "ShelleyGenesisFile":"shelley-genesis.json","ShelleyGenesisHash":s,
 "AlonzoGenesisFile":"alonzo-genesis.json","AlonzoGenesisHash":a,
 "ConwayGenesisFile":"conway-genesis.json","ConwayGenesisHash":c,
 "LastKnownBlockVersion-Major":6,"LastKnownBlockVersion-Minor":0,"LastKnownBlockVersion-Alt":0,
 "TestShelleyHardForkAtEpoch":0,"TestAllegraHardForkAtEpoch":0,"TestMaryHardForkAtEpoch":0,
 "TestAlonzoHardForkAtEpoch":0,"TestBabbageHardForkAtEpoch":0,"TestConwayHardForkAtEpoch":0,
 "minSeverity":"Info","TurnOnLogging":True,"TurnOnLogMetrics":False,"UseTraceDispatcher":True,"TraceOptions":{}}
json.dump(cfg,open(f"{d}/config.json","w"),indent=1)
json.dump({"Producers":[]},open(f"{d}/topology.json","w"))
PY
  echo '{"Producers":[]}' > "$DEVNET/topology.json"
fi

echo "[bp1] starting cardano-node (forging with pool keys)…"
rm -rf "$DEVNET/db"
cardano-node run \
  --config "$DEVNET/config.json" --topology "$DEVNET/topology.json" \
  --database-path "$DEVNET/db" --socket-path "$SOCK" \
  --shelley-kes-key "$DEVNET/pools-keys/pool1/kes.skey" \
  --shelley-vrf-key "$DEVNET/pools-keys/pool1/vrf.skey" \
  --shelley-operational-certificate "$DEVNET/pools-keys/pool1/opcert.cert" \
  --port 3001 > /var/log/cardano-node.log 2>&1 &

# Socket must be group-readable so ouro-exec (dispatch principal) can query tip in p2-2.
( for _ in $(seq 1 30); do [ -S "$SOCK" ] && { chgrp ouro-exec "$SOCK" 2>/dev/null; chmod 0660 "$SOCK" 2>/dev/null; break; }; sleep 1; done ) &

echo "[bp1] starting sshd…"
ssh-keygen -A
exec /usr/sbin/sshd -D -e
