#!/usr/bin/env bash
# S0015 p2-4 — secret-leak scanner E2E (E2E-9). Runs a real flow (dispatch + KES rotation),
# then derives MULTI-FORM fingerprints from the LIVE secret material on bp1 (raw cborHex,
# sha256(file), sha256(cborHex), basename) — NOT naive keyword regex — and scans the run's
# corpus (control+bp1 audit logs + /var/log + cardano-node.log + dispatch transcript +
# set -x trace) for any of them. Asserts:
#   (b) real flow => 0 hits (no secret leaks);
#   (a) canary => the scanner MUST detect an injected secret (else it is a no-op).
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-sec-$$}"
CF=fixtures/e2e/compose.yaml
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
dc() { docker compose -f "$CF" "$@"; }
ctl() { dc exec -T control "$@"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT
SPEC=/opt/ouro/pool-spec.yaml
WORK="$(pwd)/tmp/secret-scan"; rm -rf "$WORK"; mkdir -p "$WORK"

echo "[bed] rebuild base + up (bp1 forging) + provision + run a real flow"
docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
dc up -d --build --wait --wait-timeout 240 >/dev/null
bash fixtures/e2e/provision.sh >/dev/null
# Real flow that handles secrets: status collection + a KES rotation (touches KES/cold keys).
# The flow MUST actually succeed — a 0-hit scan over a flow that never ran is meaningless.
ctl ouro tool run deploy/status      --dispatch bp1 --spec "$SPEC" > "$WORK/transcript.status.json" 2>&1 || true
ctl ouro tool run kes-rotation/rotate --dispatch bp1 --spec "$SPEC" > "$WORK/transcript.kes.json"    2>&1 || true
for t in status kes; do
  grep -q '"status":"ok"' "$WORK/transcript.$t.json" || fail "real flow ($t) did not return status=ok — 0-hit scan would be meaningless: $(head -c200 "$WORK/transcript.$t.json")"
done
pass "real flow ran (deploy/status + kes-rotation/rotate both status=ok, so secrets were actually handled)"

echo "[scan] derive MULTI-FORM fingerprints from live secrets on bp1"
dc exec -T bp1 python3 - <<'PY' > "$WORK/fingerprints.live"
import hashlib, json, glob, os
forms=[]
for f in glob.glob("/opt/devnet/pools-keys/pool1/*.skey") + glob.glob("/opt/devnet/delegate-keys/*/*.skey"):
    raw=open(f,"rb").read()
    forms.append(hashlib.sha256(raw).hexdigest())            # sha256(file)
    # NOTE: the filename/path is public config (node startup references it) — NOT secret.
    # Only the CONTENT forms below indicate a leak.
    try:
        cbor=json.loads(raw).get("cborHex","")
        if cbor:
            forms.append(cbor)                                # raw cborHex (the material)
            forms.append(hashlib.sha256(cbor.encode()).hexdigest())  # sha256(cborHex)
    except Exception:
        pass
for x in sorted(set(f for f in forms if len(f) >= 8)):
    print(x)
PY
# Add the committed stable canary fingerprints.
grep -E '^canary-(plaintext|sha256):' tests/fixtures/secrets/fingerprints.txt | awk '{print $2}' >> "$WORK/fingerprints.live"
NFP=$(wc -l < "$WORK/fingerprints.live" | tr -d ' ')
pass "derived $NFP fingerprints (live secret forms + canary)"

echo "[scan] assemble the run corpus (audit + ALL machine logs + state/temp dirs + set -x traces)"
{
  ctl ouro audit log --limit 50 2>/dev/null
  cat "$WORK"/transcript.*.json
  # audit + logs + on-disk state/temp artifacts on EVERY machine (leak could land there too).
  for m in control bp1 relay1 relay2; do
    dc exec -T "$m" ouro audit log --limit 50 2>/dev/null || true
    dc exec -T "$m" bash -lc 'cat /var/log/*.log 2>/dev/null; cat /tmp/ouro-*/* 2>/dev/null; journalctl 2>/dev/null | tail -200 || true' 2>/dev/null || true
  done
  # a set -x trace of a real dispatch (control side)…
  ctl bash -xc 'ouro tool run deploy/status --dispatch bp1 --spec '"$SPEC" 2>&1 || true
  # …and a TARGET-side set -x trace of the key-handling lib path on bp1 (would surface a secret
  # value if the emit/redact helpers echoed it under xtrace).
  dc exec -T bp1 bash -xc 'set -x; source /opt/ouro/ouro-skills/lib/ouro-lib.sh; OURO_TOOL_NAME=t OURO_MACHINE=bp1 OURO_AUDIT_ID=trace ouro_emit_ok false "trace probe"' 2>&1 || true
} > "$WORK/corpus.txt" 2>&1
CORPUS_BYTES=$(wc -c < "$WORK/corpus.txt" | tr -d ' ')
pass "corpus assembled ($CORPUS_BYTES bytes)"

scan_hits() { # <fingerprint-file> <corpus-file> -> prints hit count
  local hits=0 fp
  while IFS= read -r fp; do
    [ -z "$fp" ] && continue
    grep -qF -- "$fp" "$2" && { hits=$((hits+1)); echo "    LEAK: $fp" >&2; }
  done < "$1"
  echo "$hits"
}

echo "[scan] (b) real flow must have 0 secret hits"
HITS=$(scan_hits "$WORK/fingerprints.live" "$WORK/corpus.txt")
[ "$HITS" = 0 ] || fail "real flow leaked $HITS secret fingerprint(s) into the corpus"
pass "(b) real flow: 0 secret fingerprints in corpus ($NFP fingerprints scanned)"

echo "[scan] (a) canary must FAIL — inject a secret and confirm the scanner detects it"
CANARY=$(grep '^canary-plaintext:' tests/fixtures/secrets/fingerprints.txt | awk '{print $2}')
LIVE_FORM=$(head -1 "$WORK/fingerprints.live")   # a real live-secret fingerprint form
cp "$WORK/corpus.txt" "$WORK/corpus.poisoned.txt"
printf 'leaked-canary %s\nleaked-live %s\n' "$CANARY" "$LIVE_FORM" >> "$WORK/corpus.poisoned.txt"
POISON_HITS=$(scan_hits "$WORK/fingerprints.live" "$WORK/corpus.poisoned.txt")
# Detecting the injected LIVE_FORM (a sha256/cborHex) proves detection is fingerprint-based,
# not a keyword regex — a "PRIVATE KEY" regex could never match that form.
[ "$POISON_HITS" -ge 2 ] 2>/dev/null || fail "canary/live secret injected but scanner found $POISON_HITS (<2) — scanner is a no-op"
pass "(a) canary: injected secrets detected ($POISON_HITS hits, incl. a live sha256/cbor form) — real, not a no-op"

rm -rf "$WORK"
echo "p2-4 secret-scan E2E: ALL PASSED"
