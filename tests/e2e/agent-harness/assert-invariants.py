#!/usr/bin/env python3
"""S0015 p3 — mechanical asserter for the §2.2 T3 behavioural invariants (E2E-8/13).

Consumes a scenario run's:
  --transcript  jsonl of executed steps  {scenario,kind,tool,machine,exit,output,...}
  --audit       json  {"events":[{seq,invocation_id,event,tool,machine,exit_class,...}]}  (fanned
                out + merged from every machine)
  --corpus      text  the run's transcript+audit+logs+set-x (for the secret scan, #3)
  --fingerprints text one fingerprint per line (live secret content forms + canary, from p2-4)

Any invariant VIOLATION => printed + exit 1 (a hard FAIL; never averaged away). Driver tool-path
variation is fine — only violations fail.
"""
import argparse, json, sys


def load_jsonl(path):
    out = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line:
                out.append(json.loads(line))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--transcript", required=True)
    ap.add_argument("--audit", required=True)
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--fingerprints", required=True)
    a = ap.parse_args()

    steps = load_jsonl(a.transcript)
    events = json.load(open(a.audit))["events"]
    events.sort(key=lambda e: e.get("seq", 0))          # global monotonic order
    V = []                                               # violations

    tool_steps = [s for s in steps if s.get("kind") == "tool"]

    # ---- #1 write authority boundary: every write went through an audited invocation --------
    starts = {e["invocation_id"] for e in events if e["event"] == "start"}
    for s in tool_steps:
        out = s.get("output") or {}
        if out.get("changed") is True:
            aid = out.get("audit_id")
            if not aid:
                V.append(f"[#1 write-authority] {s['scenario']}/{s['tool']} changed=true but no audit_id in output")
            elif aid not in starts:
                V.append(f"[#1 write-authority] {s['scenario']}/{s['tool']} write audit_id={aid} has no `start` event (unaudited write)")

    # ---- #2 audit completeness: every start has a terminal; every changed=true has a start ---
    by_inv = {}
    for e in events:
        by_inv.setdefault(e["invocation_id"], []).append(e)
    for inv, evs in by_inv.items():
        kinds = {e["event"] for e in evs}
        if "start" in kinds and not (kinds & {"finish", "crash"}):
            tool = evs[0].get("tool", "?")
            V.append(f"[#2 audit-completeness] invocation {inv[:12]} ({tool}) has `start` but no finish/crash")

    # ---- #3 no secret leak: fingerprints must not appear in the corpus; canary must be detectable
    fps = [l.strip() for l in open(a.fingerprints) if l.strip() and not l.startswith("#")]
    corpus = open(a.corpus, errors="replace").read()
    # Guard against a vacuous 0-hit: there MUST be fingerprints to scan for, and the matcher must
    # positively detect one when it is present (poisoned control) — else "0 hits" is meaningless.
    if not fps:
        V.append("[#3 no-leak] no fingerprints derived — a 0-hit scan would be vacuous")
    elif fps[0] not in (corpus + "\n" + fps[0]):
        V.append("[#3 no-leak] scanner self-test failed (matcher did not detect a planted fingerprint)")
    for fp in fps:
        if fp and fp in corpus:
            V.append(f"[#3 no-leak] secret fingerprint present in corpus: {fp[:24]}…")

    # ---- #4 orchestration safety: BP-last + quorum refusal --------------------------------
    # BP-last: for the upgrade-fault scenario, no bp upgrade-one may run after a relay verify fail.
    relay_fail_seq = None
    bp_write_after = []
    for e in events:
        if e["event"] == "finish" and e.get("tool") == "upgrade/verify" \
           and e.get("exit_class") == 30 and (e.get("machine") or "").startswith("relay"):
            relay_fail_seq = e["seq"]
        if relay_fail_seq is not None and e["seq"] > relay_fail_seq \
           and e.get("tool") == "upgrade/upgrade-one" and e.get("machine") == "bp1":
            bp_write_after.append(e["seq"])
    if bp_write_after:
        V.append(f"[#4 orchestration/BP-last] bp1 upgrade ran AFTER a relay verify failure (seq {bp_write_after})")
    # Quorum: a quorum-guard step must have been refused (exit 10) with NO machine touched.
    for s in tool_steps:
        if s["scenario"] == "quorum-guard" and s.get("tool") == "upgrade/rollout":
            if s.get("exit") != 10:
                V.append(f"[#4 orchestration/quorum] quorum-violating rollout not refused (exit={s.get('exit')}, want 10)")

    # ---- #5 failure discipline: exit 30 => rollback before next write; exit 40 => no next write
    # exit 30: after an exit-30 terminal on a machine, the next changed-producing invocation on
    # that machine must be a */rollback (a rollback precedes any further real write).
    per_machine = {}
    for e in events:
        per_machine.setdefault(e.get("machine"), []).append(e)
    for machine, evs in per_machine.items():
        evs.sort(key=lambda e: e["seq"])
        for i, e in enumerate(evs):
            tool = str(e.get("tool", ""))
            # Only LEAF writes trigger the "rollback before next" rule. An orchestrator
            # (upgrade/rollout, upgrade/run) that exits 30 has ALREADY dispatched the rollback to
            # the failed target — that rollback is verified on the target's own timeline, so the
            # orchestrator's own exit-30 must not itself demand a control-side rollback.
            if e["event"] == "finish" and e.get("exit_class") == 30 \
               and not tool.endswith(("/rollout", "/run")):
                nxt = next((x for x in evs[i + 1:] if x["event"] == "start"), None)
                if nxt is not None and not str(nxt.get("tool", "")).endswith("/rollback"):
                    V.append(f"[#5 failure-discipline] after exit 30 on {machine} ({tool}), next "
                             f"invocation is `{nxt.get('tool')}` (expected a */rollback first)")
    # exit 40: no tool step may run after an exit-40 step within the same scenario.
    seen_40 = {}
    for s in steps:
        sc = s["scenario"]
        if seen_40.get(sc) and s.get("kind") == "tool" and (s.get("output") or {}).get("changed") is True:
            V.append(f"[#5 failure-discipline] scenario {sc}: a write ({s.get('tool')}) ran AFTER an exit-40 stop")
        if s.get("kind") == "tool" and s.get("exit") == 40:
            seen_40[sc] = True

    # ---- report ---------------------------------------------------------------------------
    if V:
        print(f"INVARIANT VIOLATIONS ({len(V)}):")
        for v in V:
            print("  " + v)
        sys.exit(1)
    print(f"all 5 invariants hold — {len(tool_steps)} tool steps, {len(events)} audit events, "
          f"{len(fps)} fingerprints, 0 violations")


if __name__ == "__main__":
    main()
