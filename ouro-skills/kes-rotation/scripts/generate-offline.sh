#!/usr/bin/env bash
# S0017 p4-6 — REAL offline-path KES generation, executed ON the block producer (dispatched).
# The OFFLINE rotation splits rotate.sh in two: this half runs `cardano-cli node key-gen-KES`
# on the BP and STAGES the new key pair WITHOUT touching the live key — the running node keeps
# forging on the OLD key until push-offline installs the matching (cold-signed) opcert. Only the
# PUBLIC kes.vkey + period leave here (in the tool output); the new kes.skey stays staged on the
# BP. The operator carries the vkey+period to the air-gapped machine (via `ouro-ops kes
# cold-sign-script`), signs there, and returns node.cert for push-offline.
#
# Non-destructive: no live-key change, no restart. Not confirm-bound.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$ROOT/ouro-skills/lib/ouro-lib.sh"

ouro_require_audit_context
MACHINE="${OURO_MACHINE:?OURO_MACHINE required}"
DEVNET="${OURO_DEVNET_DIR:-/opt/devnet}"
POOL="$DEVNET/pools-keys/pool1"
SOCK="$DEVNET/node.socket"
MAGIC="${OURO_NETWORK_MAGIC:-1}"
STAGE="$POOL/offline-stage"
export CARDANO_NODE_SOCKET_PATH="$SOCK"

ouro_cardano_cli_available || ouro_emit_error 20 "no_cardano_cli" "ouro_cardano_cli not on target"

# Target KES period = tip slot / slotsPerKESPeriod (the same computation rotate.sh uses).
SPK=$(python3 -c 'import json;print(json.load(open("'"$DEVNET"'/shelley-genesis.json"))["slotsPerKESPeriod"])' 2>/dev/null || echo 0)
SLOT=$(ouro_cardano_cli query tip --testnet-magic "$MAGIC" 2>/dev/null | python3 -c 'import json,sys
try: print(json.load(sys.stdin)["slot"])
except: print(-1)' 2>/dev/null || echo -1)
[ "${SPK:-0}" -gt 0 ] 2>/dev/null || ouro_emit_error 30 "kes_precheck_failed" "could not read slotsPerKESPeriod"
[ "${SLOT:--1}" -ge 0 ] 2>/dev/null || ouro_emit_error 30 "kes_precheck_failed" "could not read chain tip slot"
PERIOD=$(( SLOT / SPK ))

# Stage a fresh KES key pair. Generate into temp names, then atomically move into the staging
# slot — a crash mid-generation never leaves a half-written staged key that push would promote.
mkdir -p "$STAGE"
chmod 700 "$STAGE"
rm -f "$STAGE/kes.vkey.staged" "$STAGE/kes.skey.staged"
ouro_cardano_cli node key-gen-KES \
  --verification-key-file "$STAGE/kes.vkey.staged.tmp" \
  --signing-key-file "$STAGE/kes.skey.staged.tmp" >/dev/null
[ -s "$STAGE/kes.vkey.staged.tmp" ] && [ -s "$STAGE/kes.skey.staged.tmp" ] \
  || ouro_emit_error 30 "kes_keygen_failed" "ouro_cardano_cli produced no KES key pair"
chmod 600 "$STAGE/kes.skey.staged.tmp"
mv -f "$STAGE/kes.vkey.staged.tmp" "$STAGE/kes.vkey.staged"
mv -f "$STAGE/kes.skey.staged.tmp" "$STAGE/kes.skey.staged"
# Record the period this staged key targets, so push-offline can flag a stale hand-off.
printf '%s' "$PERIOD" > "$STAGE/kes.period.staged"

# p4-7 freshness bundle: the online snapshot the period was computed against. push-offline
# re-queries the chain and refuses to install if the period has since gone stale (the node would
# reject a cert issued for a period too far in the past). max_age_periods is the policy window.
GENESIS_FP="$(python3 -c 'import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$DEVNET/shelley-genesis.json")"
COLLECTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
MAX_AGE_PERIODS="${OURO_KES_MAX_AGE_PERIODS:-2}"
python3 - "$STAGE/kes.bundle.json" "$PERIOD" "$SLOT" "$SPK" "$GENESIS_FP" "$COLLECTED_AT" "$MAX_AGE_PERIODS" <<'PY'
import json, sys
path, period, slot, spk, gfp, collected_at, max_age = sys.argv[1:8]
json.dump({"period": int(period), "tip_slot": int(slot), "slots_per_kes_period": int(spk),
           "genesis_fingerprint": gfp, "collected_at": collected_at,
           "max_age_periods": int(max_age)}, open(path, "w"))
PY

# The public vkey content (JSON envelope) + a stable content hash go out in the tool output.
# This is PUBLIC material — it is what `ouro-ops kes cold-sign-script --kes-vkey` consumes.
VKEY_CONTENT="$(cat "$STAGE/kes.vkey.staged")"
VKEY_HASH="$(python3 -c 'import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$STAGE/kes.vkey.staged")"

python3 - "$MACHINE" "$PERIOD" "$VKEY_HASH" "${OURO_AUDIT_ID:-}" "$STAGE/kes.vkey.staged" <<'PY'
import json, sys
mid, period, vhash, audit_id, vkey_path = sys.argv[1:6]
vkey_content = open(vkey_path).read()
checks = [{"name": f"{mid}.kes_staged", "pass": True, "severity": "info",
           "exit_class": 0, "rollback_safe": True,
           "detail": f"staged new KES key for period {period}"}]
payload = {"tool": "kes-rotation/generate-offline", "machine": mid,
           "status": "ok", "changed": True, "checks": checks,
           "data": {"kes_period": int(period), "kes_vkey_hash": vhash,
                    "kes_vkey": vkey_content, "staged": True},
           "duration_s": 0.0, "audit_id": (audit_id or None)}
print(json.dumps(payload, separators=(",", ":")))
PY
