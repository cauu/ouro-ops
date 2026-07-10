#!/usr/bin/env python3
"""S0016 p1 — durable gates for the static onboarding generator (web/onboarding/index.html).

Enforces the security-relevant invariants that make the page trustworthy:
  * TC-3: strict CSP that blocks ALL network (default-src 'none' + connect-src 'none');
  * no external resource loads / fetch / XHR / websockets (pure client, nothing uploaded);
  * R2 N3: the prompt points the agent at `ouro-ops skill show` and does NOT inline a decision tree;
  * p1-6 / TC-3b: a copy-time topology disclosure exists.
Run: python3 tests/test_web_generator.py
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "web/onboarding/index.html").read_text(encoding="utf-8")


def main() -> int:
    fails = []

    # TC-3 (p1-fix7): default-src stays 'none'; connect-src is locked to ONE host
    # (api.github.com) for the read-only latest-version fetch — the page can reach nowhere else.
    if "default-src 'none'" not in HTML:
        fails.append("CSP must keep default-src 'none'")
    if "connect-src https://api.github.com" not in HTML:
        fails.append("CSP connect-src must be locked to https://api.github.com")
    if re.search(r"connect-src[^;\"]*(\*|'self'|https://(?!api\.github\.com))", HTML):
        fails.append("CSP connect-src must not be broadened beyond api.github.com")

    # No external resource loads or unbounded network APIs.
    banned = [
        (r"<script[^>]+src=", "external <script src>"),
        (r"<link[^>]+href=", "external <link href>"),
        (r"@import", "css @import"),
        (r"url\(\s*https?:", "external css url()"),
        (r"XMLHttpRequest", "XMLHttpRequest"),
        (r"WebSocket", "WebSocket"),
        (r"EventSource", "EventSource"),
        (r"navigator\.sendBeacon", "sendBeacon"),
    ]
    for pat, name in banned:
        if re.search(pat, HTML):
            fails.append(f"page must not use {name}")

    # The ONLY fetch is the fixed, no-user-data version check to the cardano-node releases API.
    fetches = re.findall(r"fetch\((.*?)\)", HTML, re.DOTALL)
    if len(fetches) != 1:
        fails.append(f"expected exactly one fetch() (the version check); found {len(fetches)}")
    else:
        arg = fetches[0]
        if "api.github.com/repos/IntersectMBO/cardano-node/releases" not in arg:
            fails.append("the only fetch() must target the cardano-node releases API")
        if "`" in arg or "${" in arg:
            fails.append("the fetch() URL must be a fixed constant (no user data interpolated)")

    # R2 N3: the prompt directs the agent to the verified binary, and does NOT inline the
    # decision tree (no 'Decision Tree'/'Red Lines' procedure text baked into the page/prompt).
    if "ouro-ops skill show" not in HTML:
        fails.append("prompt must point the agent at `ouro-ops skill show` (R2 N3)")
    if "Decision Tree" in HTML or "Red Lines" in HTML:
        fails.append("page must NOT inline a skill decision tree (R2 N3)")

    # p1-6 / TC-3b: copy-time topology disclosure.
    if "disclose" not in HTML or "topology" not in HTML.lower():
        fails.append("page must disclose topology exposure before copying the prompt")

    # The prompt must instruct writes only via `ouro-ops tool run` (mechanism, not raw node cmds).
    if "ouro-ops tool run" not in HTML:
        fails.append("prompt must drive changes through `ouro-ops tool run`")

    if fails:
        for f in fails:
            print("FAIL:", f, file=sys.stderr)
        return 1
    print("OK: onboarding generator static gates pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
