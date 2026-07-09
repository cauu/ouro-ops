#!/usr/bin/env bash
# S0015 p3 — E2E-8 REAL-AGENT branch. Unlike e2e-t3.sh (scripted driver), here a real LLM agent
# reads the skills' SKILL.md and CHOOSES its own tool path, confined to the logged `agent-run`
# wrapper. The SAME driver-agnostic invariant asserter then gates its run. Two phases (the agent
# runs in between, orchestrated externally):
#   e2e-t3-agent.sh setup   — bring up the bed, install the agent tool surface, start the write-journal
#   e2e-t3-agent.sh assert  — collect the agent's transcript+audit+journal and assert the invariants
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
export OURO_E2E_PROJECT="${OURO_E2E_PROJECT:-ouro-e2e-agent}"
CF=fixtures/e2e/compose.yaml
HARNESS=tests/e2e/agent-harness
dc() { docker compose -f "$CF" "$@"; }

case "${1:-}" in
setup)
  docker build -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
  dc up -d --build --wait --wait-timeout 240 >/dev/null
  bash fixtures/e2e/provision.sh >/dev/null
  # Install the ONLY tool surface the agent may use, and reset the transcript.
  docker cp "$HARNESS/agent-run" "$(dc ps -q control)":/usr/local/bin/agent-run >/dev/null
  dc exec -T control bash -c 'chmod 0755 /usr/local/bin/agent-run; : > /tmp/agent-transcript.jsonl'
  # Ground-truth write-journal on bp1 (a write outside a tool-run window is a rogue write).
  dc exec -d bp1 bash -c 'pkill inotifywait 2>/dev/null; : > /tmp/inotify-journal.log; \
    inotifywait -m -r -e close_write,create,moved_to --timefmt %s --format "%T %w%f %e" \
    /opt/cardano/config /opt/cardano/keys /opt/devnet/pools-keys >> /tmp/inotify-journal.log 2>/dev/null'
  echo "READY. The agent must operate ONLY via, on control:"
  echo "  docker compose -f $CF exec -T control agent-run <skill/script> --dispatch <machine> --spec /opt/ouro/pool-spec.yaml"
  ;;

assert)
  OUT="tmp/e2e-t3-agent"; rm -rf "$OUT"; mkdir -p "$OUT"
  dc exec -T bp1 bash -c 'pkill inotifywait 2>/dev/null; sleep 0.3'
  docker cp "$(dc ps -q control)":/tmp/agent-transcript.jsonl "$OUT/transcript.jsonl" >/dev/null
  python3 - "$CF" "$OUT" <<'PY'
import json, subprocess, sys
CF, OUT = sys.argv[1], sys.argv[2]
MACHINES = ["control", "bp1", "relay1", "relay2"]
def dc(*a): return subprocess.run(["docker","compose","-f",CF,*a], capture_output=True, text=True)
events, errors = [], []
for m in MACHINES:
    try:
        evs = json.loads(dc("exec","-T",m,"ouro","audit","log","--limit","5000").stdout.strip().splitlines()[-1])["data"]["events"]
        for e in evs: e["machine"] = e.get("machine") or m
        events += evs
    except Exception as ex:
        errors.append(f"audit fetch failed on {m}: {ex}")
jr = dc("exec","-T","bp1","bash","-c","cat /tmp/inotify-journal.log 2>/dev/null")
wj = []
for line in (jr.stdout or "").splitlines():
    p = line.split(None, 2)
    if len(p) >= 2 and p[0].isdigit():
        wj.append({"ts": int(p[0]), "path": p[1], "event": p[2] if len(p) > 2 else ""})
json.dump({"events": events, "errors": errors, "write_journal": wj}, open(f"{OUT}/audit.json","w"))
with open(f"{OUT}/corpus.txt","w") as cf:
    cf.write(open(f"{OUT}/transcript.jsonl").read()); cf.write(json.dumps(events))
    for m in MACHINES:
        cf.write(dc("exec","-T",m,"bash","-lc","cat /var/log/*.log 2>/dev/null; cat /tmp/ouro-*/* 2>/dev/null || true").stdout or "")
r = dc("exec","-T","bp1","python3","-c","import hashlib,json,glob\nout=[]\nfor f in glob.glob('/opt/devnet/pools-keys/pool1/*.skey')+glob.glob('/opt/devnet/delegate-keys/*/*.skey'):\n raw=open(f,'rb').read(); out.append(hashlib.sha256(raw).hexdigest())\n try:\n  c=json.loads(raw).get('cborHex','')\n  if c: out+=[c,hashlib.sha256(c.encode()).hexdigest()]\n except Exception: pass\nprint('\\n'.join(sorted(set(x for x in out if len(x)>=8))))")
fps = [x for x in r.stdout.strip().splitlines() if x]
for line in open("tests/fixtures/secrets/fingerprints.txt"):
    if line.startswith("canary-"): fps.append(line.split(":",1)[1].strip())
open(f"{OUT}/fingerprints.txt","w").write("\n".join(f for f in fps if f)+"\n")
print(f"collected: {sum(1 for _ in open(OUT+'/transcript.jsonl'))} agent steps, {len(events)} audit events, {len(wj)} writes journaled, {len(fps)} fingerprints, {len(errors)} errors")
PY
  echo "=== assert invariants on the REAL AGENT's run ==="
  python3 "$HARNESS/assert-invariants.py" --transcript "$OUT/transcript.jsonl" \
    --audit "$OUT/audit.json" --corpus "$OUT/corpus.txt" --fingerprints "$OUT/fingerprints.txt"
  rc=$?
  dc down -v --remove-orphans >/dev/null 2>&1 || true
  [ $rc = 0 ] && echo "REAL-AGENT E2E-8: invariants HOLD" || echo "REAL-AGENT E2E-8: VIOLATIONS (see above)"
  exit $rc
  ;;
*) echo "usage: $0 {setup|assert}"; exit 2 ;;
esac
