#!/usr/bin/env python3
"""S0015 p3 — scripted reference DRIVER. Reads scenarios.yaml and executes each step against the
running bed via `docker compose exec`, then captures the artefacts the invariant asserter needs:
transcript.jsonl, audit.json (fanned out + merged from every machine), corpus.txt, fingerprints.

This is ONE valid agent tool-path (E2E-10 allows tool-path variation). It deliberately HONORS the
failure-discipline contract (it does not issue a step marked expect_skipped after an exit-40),
so that a compliant run has 0 violations — and a bug in the skills (not the driver) is what the
asserter is meant to catch.
"""
import argparse, hashlib, json, os, subprocess, sys, glob


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

    def dc(*a):
        return sh(["docker", "compose", "-f", args.compose, *a])

    def ctl_tool(tool, dispatch=None, spec="bed"):
        cmd = ["exec", "-T", "control", "ouro", "tool", "run", tool]
        if dispatch:
            cmd += ["--dispatch", dispatch]
        cmd += ["--spec", SPEC[spec]]
        r = dc(*cmd)
        try:
            out = json.loads(r.stdout.strip().splitlines()[-1]) if r.stdout.strip() else None
        except Exception:
            out = None
        return r.returncode, out, r.stdout, r.stderr

    def max_seq(m):
        r = dc("exec", "-T", m, "ouro", "audit", "log", "--limit", "1")
        try:
            evs = json.loads(r.stdout.strip().splitlines()[-1])["data"]["events"]
            return evs[0]["seq"] if evs else 0
        except Exception:
            return 0
    # Watermark per machine so a run only asserts on ITS OWN audit events (the db accumulates
    # across the N runs) — crucial for the per-scenario failure-discipline windowing.
    watermark = {m: max_seq(m) for m in MACHINES}

    transcript = open(os.path.join(args.out, "transcript.jsonl"), "w")
    stop_scenarios = set()   # scenarios where an exit-40 stop is in effect

    for sc in spec_yaml["scenarios"]:
        name = sc["name"]
        for i, step in enumerate(sc["steps"]):
            if step.get("inject"):
                kind = step["inject"]; m = step["machine"]
                dc("exec", "-T", m, "bash", "-c",
                   f"mkdir -p /tmp/ouro-upgrade-state && touch /tmp/ouro-upgrade-state/__test_inject_{kind}__{m}")
                transcript.write(json.dumps({"scenario": name, "step": i, "kind": "inject",
                                             "inject": kind, "machine": m}) + "\n")
                continue
            if step.get("expect_skipped"):
                # Failure discipline: after an exit-40 stop the driver MUST NOT issue this write.
                if name in stop_scenarios:
                    transcript.write(json.dumps({"scenario": name, "step": i, "kind": "skipped",
                                                 "tool": step["tool"], "reason": "exit40-stop"}) + "\n")
                    continue
            tool = step["tool"]; dispatch = step.get("dispatch")
            spec = step.get("spec", "bed")
            rc, out, so, se = ctl_tool(tool, dispatch, spec)
            transcript.write(json.dumps({"scenario": name, "step": i, "kind": "tool", "tool": tool,
                                         "machine": dispatch, "exit": rc, "output": out}) + "\n")
            if rc == 40:
                stop_scenarios.add(name)
    transcript.close()

    # Fan out + merge the audit trail from every machine (audit is target-authoritative).
    events = []
    for m in MACHINES:
        r = dc("exec", "-T", m, "ouro", "audit", "log", "--limit", "300")
        try:
            evs = json.loads(r.stdout.strip().splitlines()[-1])["data"]["events"]
            for e in evs:
                if e.get("seq", 0) <= watermark.get(m, 0):
                    continue                               # only THIS run's events
                e["machine"] = e.get("machine") or m       # attribute machine-less events to host
                events.append(e)
        except Exception:
            pass
    json.dump({"events": events}, open(os.path.join(args.out, "audit.json"), "w"))

    # Corpus for the no-leak invariant: transcript + audit + every machine's logs + state dirs.
    with open(os.path.join(args.out, "corpus.txt"), "w") as cf:
        cf.write(open(os.path.join(args.out, "transcript.jsonl")).read())
        cf.write(json.dumps(events))
        for m in MACHINES:
            r = dc("exec", "-T", m, "bash", "-lc",
                   "cat /var/log/*.log 2>/dev/null; cat /tmp/ouro-*/* 2>/dev/null || true")
            cf.write(r.stdout or "")

    # Fingerprints: live secret content forms on bp1 + the committed canary (p2-4 corpus).
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
    fps = r.stdout.strip().splitlines()
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
    for line in open(os.path.join(root, "tests/fixtures/secrets/fingerprints.txt")):
        if line.startswith("canary-"):
            fps.append(line.split(":", 1)[1].strip())
    open(os.path.join(args.out, "fingerprints.txt"), "w").write("\n".join(f for f in fps if f) + "\n")
    print(f"driver: {sum(1 for _ in open(os.path.join(args.out,'transcript.jsonl')))} steps, "
          f"{len(events)} audit events, {len(fps)} fingerprints")


if __name__ == "__main__":
    main()
