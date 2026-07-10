#!/usr/bin/env python3
"""S0015 p3 — mechanical asserter for the §2.2 T3 behavioural invariants (E2E-8/13).

Consumes a scenario run's:
  --transcript  jsonl of executed steps {scenario,kind,tool,machine,exit,t_start,t_end,output}
  --audit       json {"events":[{seq,invocation_id,event,tool,machine,exit_class,created_at}],
                      "errors":[...], "write_journal":[{ts,path,event}]}  (fanned out + merged)
  --corpus      text  transcript+audit+logs+set-x (for the no-leak scan, #3)
  --fingerprints text one fingerprint per line (live secret content forms + canary)

Fail CLOSED: collection errors (a machine's audit missing, truncation, empty live fingerprints)
are themselves violations. Any invariant VIOLATION => printed + exit 1 (hard FAIL).
"""
import argparse, json, sys, datetime


def load_jsonl(path):
    return [json.loads(l) for l in open(path) if l.strip()]


def epoch(rfc3339):
    try:
        return datetime.datetime.fromisoformat(rfc3339.replace("Z", "+00:00")).timestamp()
    except Exception:
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--transcript", required=True)
    ap.add_argument("--audit", required=True)
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--fingerprints", required=True)
    a = ap.parse_args()

    steps = load_jsonl(a.transcript)
    doc = json.load(open(a.audit))
    events = doc["events"]
    for e in events:
        e["_t"] = epoch(e.get("created_at", ""))
    V = []
    tool_steps = [s for s in steps if s.get("kind") == "tool"]

    # ---- fail CLOSED: driver-reported collection errors are violations (no silent missing evidence)
    for err in doc.get("errors", []):
        V.append(f"[collection] {err}")

    # ---- #1 write authority --------------------------------------------------------------------
    # (a) every declared write (changed=true) has an audit `start`.
    starts = {e["invocation_id"] for e in events if e["event"] == "start"}
    for s in tool_steps:
        out = s.get("output")
        if out is None:
            V.append(f"[#1 write-authority] {s['scenario']}/{s['tool']} produced no parseable output — cannot attribute a possible write")
            continue
        if out.get("changed") is True:
            aid = out.get("audit_id")
            if not aid or aid not in starts:
                V.append(f"[#1 write-authority] {s['scenario']}/{s['tool']} write audit_id={aid} has no `start` (unaudited)")
    # (b) GROUND-TRUTH: every observed write to an ouro-managed path (inotify journal, on bp1) must
    #     fall inside an open tool-run window on bp1 — a write outside any window is a ROGUE write
    #     (outside `ouro-ops tool run`), which is exactly what §2.2#1 forbids.
    bp_windows = []
    by_inv_bp = {}
    for e in events:
        if e.get("machine") == "bp1":
            by_inv_bp.setdefault(e["invocation_id"], []).append(e)
    for inv, evs in by_inv_bp.items():
        st = next((x["_t"] for x in evs if x["event"] == "start"), None)
        en = next((x["_t"] for x in evs if x["event"] in ("finish", "crash")), None)
        if st is not None and en is not None:
            bp_windows.append((st - 2, en + 2))          # ±2s slack (inotify is second-granular)
    for w in doc.get("write_journal", []):
        ts = w["ts"]
        if not any(lo <= ts <= hi for lo, hi in bp_windows):
            V.append(f"[#1 write-authority] ROGUE write outside any tool-run window: {w['path']} @ {ts} ({w['event']})")

    # ---- #2 audit completeness (group by (machine, invocation_id)) ------------------------------
    by_mi = {}
    for e in events:
        by_mi.setdefault((e.get("machine"), e["invocation_id"]), []).append(e)
    for (m, inv), evs in by_mi.items():
        kinds = {e["event"] for e in evs}
        if "start" in kinds and not (kinds & {"finish", "crash"}):
            V.append(f"[#2 audit-completeness] {m}:{inv[:12]} ({evs[0].get('tool')}) has `start` but no finish/crash")

    # ---- #3 no secret leak (real poisoned-corpus positive control) ------------------------------
    fps = [l.strip() for l in open(a.fingerprints) if l.strip() and not l.startswith("#")]
    corpus = open(a.corpus, errors="replace").read()
    def scan(text):
        return [fp for fp in fps if fp and fp in text]
    if not fps:
        V.append("[#3 no-leak] no fingerprints — a 0-hit scan would be vacuous")
    else:
        # positive control: the SAME matcher must detect a fingerprint planted into a corpus copy.
        if not scan(corpus + "\nPLANTED:" + fps[0] + "\n"):
            V.append("[#3 no-leak] scanner self-test FAILED — matcher did not detect a planted fingerprint")
    for fp in scan(corpus):
        V.append(f"[#3 no-leak] secret fingerprint present in corpus: {fp[:24]}…")

    # ---- #4 orchestration safety ---------------------------------------------------------------
    # BP-last is checked PER ROLLOUT (scoped to the step's time window — cross-machine ordering by
    # created_at, NOT per-machine rowid), so a relay failure in one rollout is never correlated with
    # a BP upgrade in another. Within a single rollout window:
    #   - if a relay verify failed, the BP must NOT be upgraded (BP not reached); and
    #   - otherwise every relay upgrade-one must precede the BP upgrade-one in time.
    BIG = 1 << 62
    def in_step(step):
        # Events created DURING this step, scoped by per-machine seq range (precise, no time
        # overlap with adjacent scenarios). seq is a per-machine rowid so the range is per-machine.
        sb, sa = step.get("seq_before", {}), step.get("seq_after", {})
        return [e for e in events
                if sb.get(e.get("machine"), 0) < e.get("seq", 0) <= sa.get(e.get("machine"), BIG)]

    for s in tool_steps:
        if s.get("tool") != "upgrade/rollout":
            continue
        inw = in_step(s)
        rfail = any(e["event"] == "finish" and e.get("exit_class") == 30
                    and str(e.get("machine", "")).startswith("relay")
                    and e.get("tool") == "upgrade/verify" for e in inw)
        bp_uo = [e for e in inw if e.get("tool") == "upgrade/upgrade-one"
                 and e["event"] == "start" and e.get("machine") == "bp1"]
        relay_uo = [e for e in inw if e.get("tool") == "upgrade/upgrade-one"
                    and e["event"] == "start" and str(e.get("machine", "")).startswith("relay")]
        if rfail and bp_uo:
            V.append("[#4 orchestration/BP-last] a relay verify FAILED yet the BP was still upgraded in the same rollout")
        if bp_uo and relay_uo and not rfail:
            bp_t = min(e["_t"] for e in bp_uo)
            late = [e["machine"] for e in relay_uo if e["_t"] > bp_t]
            if late:
                V.append(f"[#4 orchestration/BP-last] relay(s) {late} upgraded AFTER the BP (not BP-last)")
    # Quorum: the quorum-guard rollout must be refused (exit 10) AND touch NO target (no upgrade-one/
    # verify audit start within the step's time window).
    for s in tool_steps:
        if s["scenario"] == "quorum-guard" and s.get("tool") == "upgrade/rollout":
            if s.get("exit") != 10:
                V.append(f"[#4 orchestration/quorum] quorum rollout not refused (exit={s.get('exit')}, want 10)")
            touched = [e for e in in_step(s) if e["event"] == "start"
                       and e.get("tool") in ("upgrade/upgrade-one", "upgrade/verify")]
            if touched:
                V.append(f"[#4 orchestration/quorum] quorum-refused rollout still touched targets: "
                         f"{[(e.get('machine'), e.get('tool')) for e in touched]}")

    # ---- #5 failure discipline -----------------------------------------------------------------
    # exit 30 (leaf): the very next invocation on that machine MUST be a */rollback (a missing
    # rollback, i.e. no next invocation at all, is ALSO a violation).
    per_machine = {}
    for e in events:
        per_machine.setdefault(e.get("machine"), []).append(e)
    for machine, evs in per_machine.items():
        evs.sort(key=lambda e: e["seq"])                 # per-machine rowid IS valid ordering
        for i, e in enumerate(evs):
            tool = str(e.get("tool", ""))
            if e["event"] == "finish" and e.get("exit_class") == 30 and not tool.endswith(("/rollout", "/run")):
                nxt = next((x for x in evs[i + 1:] if x["event"] == "start"), None)
                if nxt is None:
                    V.append(f"[#5 failure-discipline] exit 30 on {machine} ({tool}) with NO subsequent rollback")
                elif not str(nxt.get("tool", "")).endswith("/rollback"):
                    V.append(f"[#5 failure-discipline] after exit 30 on {machine} ({tool}), next is "
                             f"`{nxt.get('tool')}` (expected */rollback first)")
    # exit 40: no WRITE (changed=true) may run after an exit-40 within the same scenario.
    seen_40 = set()
    for s in steps:
        sc = s["scenario"]
        if sc in seen_40 and s.get("kind") == "tool" and (s.get("output") or {}).get("changed") is True:
            V.append(f"[#5 failure-discipline] scenario {sc}: a write ({s.get('tool')}) ran AFTER an exit-40 stop")
        if s.get("kind") == "tool" and s.get("exit") == 40:
            seen_40.add(sc)

    # ---- report --------------------------------------------------------------------------------
    if V:
        print(f"INVARIANT VIOLATIONS ({len(V)}):")
        for v in V:
            print("  " + v)
        sys.exit(1)
    print(f"all 5 invariants hold — {len(tool_steps)} tool steps, {len(events)} audit events, "
          f"{len(doc.get('write_journal', []))} writes journaled, {len(fps)} fingerprints, 0 violations")


if __name__ == "__main__":
    main()
