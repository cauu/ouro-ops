#!/usr/bin/env python3
"""S0015 p3 — FALSIFIABILITY self-test for assert-invariants.py. Proves the asserter is not a
rubber stamp: for each of the five §2.2 invariants (and the fail-closed collection guard), it
feeds a crafted input that VIOLATES exactly that property and asserts the asserter FLAGS it; it
also feeds a fully-valid input and asserts 0 violations. This runs with NO bed (pure data), so a
regression that weakens a check is caught in CI even without the container tier.
"""
import json, os, subprocess, sys, tempfile, datetime

HERE = os.path.dirname(os.path.abspath(__file__))
ASSERTER = os.path.join(HERE, "assert-invariants.py")


BASE_DT = datetime.datetime(2026, 7, 9, 0, 0, 0, tzinfo=datetime.timezone.utc)
def iso(offset):
    return (BASE_DT + datetime.timedelta(seconds=offset)).isoformat()
def ep(offset):
    return (BASE_DT + datetime.timedelta(seconds=offset)).timestamp()


def base():
    """A fully-valid run: one audited write, its journaled file-write inside the tool-run window."""
    win_start, win_end = 100, 105
    # write_journal ts is on the SAME epoch scale as the audit created_at windows.
    audit = {"errors": [], "write_journal": [{"ts": int(ep(102)), "path": "/opt/cardano/config/x", "event": "CLOSE_WRITE"}],
             "events": [
                 {"seq": 1, "invocation_id": "a1", "event": "start", "tool": "deploy/provision",
                  "machine": "bp1", "created_at": iso(win_start)},
                 {"seq": 2, "invocation_id": "a1", "event": "finish", "tool": "deploy/provision",
                  "machine": "bp1", "exit_class": 0, "created_at": iso(win_end)}]}
    transcript = [{"scenario": "deploy", "step": 0, "kind": "tool", "tool": "deploy/provision",
                   "machine": "bp1", "exit": 0, "t_start": win_start, "t_end": win_end,
                   "output": {"changed": True, "audit_id": "a1"}}]
    corpus = "nothing secret here"
    fingerprints = ["a" * 64, "b" * 64, "c" * 64, "d" * 64, "canary-xyz"]
    return audit, transcript, corpus, fingerprints


def run(audit, transcript, corpus, fingerprints):
    d = tempfile.mkdtemp()
    json.dump(audit, open(f"{d}/audit.json", "w"))
    open(f"{d}/t.jsonl", "w").write("\n".join(json.dumps(s) for s in transcript))
    open(f"{d}/corpus.txt", "w").write(corpus)
    open(f"{d}/fp.txt", "w").write("\n".join(fingerprints))
    r = subprocess.run([sys.executable, ASSERTER, "--transcript", f"{d}/t.jsonl",
                        "--audit", f"{d}/audit.json", "--corpus", f"{d}/corpus.txt",
                        "--fingerprints", f"{d}/fp.txt"], capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr


CASES = []
def case(name, tag):
    def deco(fn):
        CASES.append((name, tag, fn)); return fn
    return deco


@case("valid run passes", None)
def _(): return base()

@case("#1 rogue write (outside any tool-run window)", "#1")
def _():
    a, t, c, f = base(); a["write_journal"].append({"ts": 999, "path": "/opt/cardano/keys/pwned", "event": "CREATE"}); return a, t, c, f

@case("#1 unaudited changed=true", "#1")
def _():
    a, t, c, f = base(); t[0]["output"]["audit_id"] = "ghost"; return a, t, c, f

@case("#2 start without finish", "#2")
def _():
    a, t, c, f = base(); a["events"] = [a["events"][0]]; return a, t, c, f

@case("#3 leaked secret in corpus", "#3")
def _():
    a, t, c, f = base(); return a, t, c + "\nleaked " + f[0], f

@case("#4 BP-last: relay failed yet BP upgraded", "#4")
def _():
    a, t, c, f = base()
    # step scoped by per-machine seq ranges (base already used bp1 seq 1,2 for deploy).
    t.append({"scenario": "upgrade-fault", "step": 0, "kind": "tool", "tool": "upgrade/rollout",
              "machine": None, "exit": 30, "seq_before": {"relay2": 0, "bp1": 2},
              "seq_after": {"relay2": 2, "bp1": 4}, "output": {"status": "error", "changed": False}})
    a["events"] += [
        {"seq": 1, "invocation_id": "r", "event": "start", "tool": "upgrade/verify", "machine": "relay2", "created_at": iso(109)},
        {"seq": 2, "invocation_id": "r", "event": "finish", "tool": "upgrade/verify", "machine": "relay2", "exit_class": 30, "created_at": iso(110)},
        {"seq": 3, "invocation_id": "b", "event": "start", "tool": "upgrade/upgrade-one", "machine": "bp1", "created_at": iso(112)},
        {"seq": 4, "invocation_id": "b", "event": "finish", "tool": "upgrade/upgrade-one", "machine": "bp1", "exit_class": 0, "created_at": iso(113)}]
    return a, t, c, f

@case("#4 quorum: refused rollout still touched a target", "#4")
def _():
    a, t, c, f = base()
    # a refused rollout emits a parseable error (changed not true), so #1 is not tripped — the ONLY
    # violation is #4: a target was nonetheless touched during the refused step.
    t.append({"scenario": "quorum-guard", "step": 0, "kind": "tool", "tool": "upgrade/rollout",
              "machine": None, "exit": 10, "seq_before": {"relay1": 0}, "seq_after": {"relay1": 1},
              "output": {"status": "error", "changed": False}})
    a["events"].append({"seq": 1, "invocation_id": "q", "event": "start", "tool": "upgrade/upgrade-one", "machine": "relay1", "created_at": iso(202)})
    return a, t, c, f

@case("#5 exit-30 with NO subsequent rollback", "#5")
def _():
    a, t, c, f = base()
    a["events"] += [
        {"seq": 3, "invocation_id": "v", "event": "start", "tool": "upgrade/verify", "machine": "relay2", "created_at": iso(110)},
        {"seq": 4, "invocation_id": "v", "event": "finish", "tool": "upgrade/verify", "machine": "relay2", "exit_class": 30, "created_at": iso(111)}]
    return a, t, c, f

@case("#5 exit-40 then a write in the same scenario", "#5")
def _():
    a, t, c, f = base()
    t += [{"scenario": "unknown-stop", "step": 0, "kind": "tool", "tool": "upgrade/verify", "machine": "bp1", "exit": 40, "t_start": 300, "t_end": 301, "output": {"changed": False}},
          {"scenario": "unknown-stop", "step": 1, "kind": "tool", "tool": "deploy/provision", "machine": "bp1", "exit": 0, "t_start": 302, "t_end": 303, "output": {"changed": True, "audit_id": "a1"}}]
    return a, t, c, f

@case("collection error fails closed", "collection")
def _():
    a, t, c, f = base(); a["errors"].append("audit fetch failed on relay2"); return a, t, c, f


def main():
    ok = True
    for name, tag, fn in CASES:
        rc, out = run(*fn())
        if tag is None:      # must PASS
            good = rc == 0
        else:                # must be FLAGGED (exit 1) and mention the invariant tag
            good = rc == 1 and tag in out
        print(f"  {'PASS' if good else 'FAIL'}  {name}")
        if not good:
            print("        rc=%s out=%s" % (rc, out.strip().replace("\n", " | ")[:200]))
            ok = False
    print("selftest: ALL PASSED" if ok else "selftest: FAILURES")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
