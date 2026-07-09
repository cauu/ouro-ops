#!/usr/bin/env python3
"""S0015 p3 — scripted reference DRIVER. Reads scenarios.yaml and executes each step against the
running bed via `docker compose exec`, then captures the artefacts the invariant asserter needs:
transcript.jsonl (with per-step wall-clock timestamps), audit.json (fanned out + merged from
every machine, with a `write_journal` from inotify ground-truth and an `errors` list),
corpus.txt, fingerprints.txt.

Fail-CLOSED: a machine whose audit cannot be collected, or an empty live-fingerprint set, is
recorded so the asserter REJECTS the run rather than passing on missing evidence.
"""
import argparse, json, os, subprocess, time


def sh(args, **kw):
    return subprocess.run(args, capture_output=True, text=True, **kw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--compose", required=True)
    ap.add_argument("--scenarios", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    import yaml
    spec_yaml = yaml.safe_load(open(args.scenarios))
    SPEC = {"bed": "/opt/ouro/pool-spec.yaml", "quorum2": "/opt/ouro/pool-spec-q2.yaml"}
    MACHINES = ["control", "bp1", "relay1", "relay2"]
    # ouro-managed write paths (NOT /opt/devnet/db — the node writes there continuously).
    WATCH = "/opt/cardano/config /opt/cardano/keys /opt/devnet/pools-keys"
    AUDIT_LIMIT = 5000

    def dc(*a):
        return sh(["docker", "compose", "-f", args.compose, *a])

    def audit(m):
        r = dc("exec", "-T", m, "ouro", "audit", "log", "--limit", str(AUDIT_LIMIT))
        evs = json.loads(r.stdout.strip().splitlines()[-1])["data"]["events"]  # raises on failure
        return evs

    def top_seq(m):
        try:
            r = dc("exec", "-T", m, "ouro", "audit", "log", "--limit", "1")
            evs = json.loads(r.stdout.strip().splitlines()[-1])["data"]["events"]
            return evs[0]["seq"] if evs else 0
        except Exception:
            return 0
    def snapshot():
        return {m: top_seq(m) for m in MACHINES}
    max_seq = top_seq

    errors = []

    # #1 ground-truth: start an inotify write-journal on bp1 BEFORE the scenarios (a write outside
    # any tool-run window is a rogue write). Detached; stopped + read after the run.
    dc("exec", "-d", "bp1", "bash", "-c",
       f"inotifywait -m -r -e close_write,create,moved_to --timefmt %s --format '%T %w%f %e' "
       f"{WATCH} > /tmp/inotify-journal.log 2>/dev/null")
    time.sleep(1)  # let watches establish

    watermark = {m: max_seq(m) for m in MACHINES}
    transcript = open(os.path.join(args.out, "transcript.jsonl"), "w")
    stop_scenarios = set()

    def ctl_tool(tool, dispatch, spec):
        cmd = ["exec", "-T", "control", "ouro", "tool", "run", tool]
        if dispatch:
            cmd += ["--dispatch", dispatch]
        cmd += ["--spec", SPEC[spec]]
        r = dc(*cmd)
        try:
            out = json.loads(r.stdout.strip().splitlines()[-1]) if r.stdout.strip() else None
        except Exception:
            out = None
        return r.returncode, out

    for sc in spec_yaml["scenarios"]:
        name = sc["name"]
        for i, step in enumerate(sc["steps"]):
            if step.get("inject"):
                kind, m = step["inject"], step["machine"]
                dc("exec", "-T", m, "bash", "-c",
                   f"mkdir -p /tmp/ouro-upgrade-state && touch /tmp/ouro-upgrade-state/__test_inject_{kind}__{m}")
                transcript.write(json.dumps({"scenario": name, "step": i, "kind": "inject",
                                             "inject": kind, "machine": m}) + "\n")
                continue
            if step.get("expect_skipped") and name in stop_scenarios:
                transcript.write(json.dumps({"scenario": name, "step": i, "kind": "skipped",
                                             "tool": step["tool"], "reason": "exit40-stop"}) + "\n")
                continue
            sb = snapshot()                              # per-machine seq BEFORE the step
            t0 = time.time()
            rc, out = ctl_tool(step["tool"], step.get("dispatch"), step.get("spec", "bed"))
            t1 = time.time()
            sa = snapshot()                              # per-machine seq AFTER the step
            # seq ranges scope audit events to THIS step precisely (no time-window overlap with
            # adjacent scenarios) — the asserter uses them for BP-last / quorum.
            transcript.write(json.dumps({"scenario": name, "step": i, "kind": "tool",
                                         "tool": step["tool"], "machine": step.get("dispatch"),
                                         "exit": rc, "t_start": t0, "t_end": t1,
                                         "seq_before": sb, "seq_after": sa, "output": out}) + "\n")
            if rc == 40:
                stop_scenarios.add(name)
    transcript.close()

    # Stop + read the inotify write-journal.
    dc("exec", "-T", "bp1", "bash", "-c", "pkill inotifywait 2>/dev/null; sleep 0.3")
    jr = dc("exec", "-T", "bp1", "bash", "-c", "cat /tmp/inotify-journal.log 2>/dev/null; rm -f /tmp/inotify-journal.log")
    write_journal = []
    for line in (jr.stdout or "").splitlines():
        parts = line.split(None, 2)
        if len(parts) >= 2 and parts[0].isdigit():
            write_journal.append({"ts": int(parts[0]), "path": parts[1], "event": parts[2] if len(parts) > 2 else ""})

    # Fan out + merge the audit from every machine — FAIL CLOSED on any collection error.
    events = []
    for m in MACHINES:
        try:
            evs = audit(m)
            if len(evs) >= AUDIT_LIMIT:
                errors.append(f"audit truncated at {AUDIT_LIMIT} on {m}")
            for e in evs:
                if e.get("seq", 0) <= watermark.get(m, 0):
                    continue
                e["machine"] = e.get("machine") or m
                events.append(e)
        except Exception as ex:
            errors.append(f"audit fetch failed on {m}: {ex}")
    json.dump({"events": events, "errors": errors, "write_journal": write_journal},
              open(os.path.join(args.out, "audit.json"), "w"))

    # Corpus (no-leak): transcript + audit + every machine's logs/state + a target set -x trace.
    with open(os.path.join(args.out, "corpus.txt"), "w") as cf:
        cf.write(open(os.path.join(args.out, "transcript.jsonl")).read())
        cf.write(json.dumps(events))
        for m in MACHINES:
            r = dc("exec", "-T", m, "bash", "-lc",
                   "cat /var/log/*.log 2>/dev/null; cat /tmp/ouro-*/* 2>/dev/null || true")
            cf.write(r.stdout or "")
        # target-side set -x trace of the key-handling lib path (would surface a leaked value).
        r = dc("exec", "-T", "bp1", "bash", "-xc",
               "source /opt/ouro/ouro-skills/lib/ouro-lib.sh; "
               "OURO_TOOL_NAME=t OURO_MACHINE=bp1 OURO_AUDIT_ID=trace ouro_emit_ok false 'trace probe'")
        cf.write((r.stdout or "") + (r.stderr or ""))

    # Fingerprints: live secret content forms on bp1 — FAIL if none collected (else 0-hit is vacuous).
    r = dc("exec", "-T", "bp1", "python3", "-c", """
import hashlib, json, glob
out=[]
for f in glob.glob('/opt/devnet/pools-keys/pool1/*.skey')+glob.glob('/opt/devnet/delegate-keys/*/*.skey'):
    raw=open(f,'rb').read(); out.append(hashlib.sha256(raw).hexdigest())
    try:
        c=json.loads(raw).get('cborHex','')
        if c: out+=[c, hashlib.sha256(c.encode()).hexdigest()]
    except Exception: pass
print('\\n'.join(sorted(set(x for x in out if len(x)>=8))))
""")
    live = [x for x in r.stdout.strip().splitlines() if x]
    if len(live) < 4:
        errors.append(f"live fingerprint extraction returned only {len(live)} (expected >=4) — 0-hit would be vacuous")
        json.dump({"events": events, "errors": errors, "write_journal": write_journal},
                  open(os.path.join(args.out, "audit.json"), "w"))
    fps = list(live)
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
    for line in open(os.path.join(root, "tests/fixtures/secrets/fingerprints.txt")):
        if line.startswith("canary-"):
            fps.append(line.split(":", 1)[1].strip())
    open(os.path.join(args.out, "fingerprints.txt"), "w").write("\n".join(f for f in fps if f) + "\n")
    print(f"driver: {sum(1 for _ in open(os.path.join(args.out,'transcript.jsonl')))} steps, "
          f"{len(events)} audit events, {len(write_journal)} writes journaled, {len(live)} live fps, "
          f"{len(errors)} collection errors")


if __name__ == "__main__":
    main()
