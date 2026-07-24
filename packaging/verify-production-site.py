#!/usr/bin/env python3
"""Verify the deployed Ouro Site against its canonical repository sources."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import urllib.error
import urllib.request


PUBLIC_SKILLS = {
    "deploy": "deploy/SKILL.md",
    "kes-rotation": "kes-rotation/SKILL.md",
    "observability": "observability/SKILL.md",
    "runtime": "runtime/SKILL.md",
    "troubleshooting": "troubleshooting/SKILL.md",
    "upgrade": "upgrade/SKILL.md",
}
MAX_HTML_BYTES = 5 * 1024 * 1024


def verify(
    html: str, skills_root: pathlib.Path, install_source: pathlib.Path
) -> dict[str, object]:
    if "connect-src 'none'" not in html:
        raise ValueError("production HTML lacks the network-denying CSP")
    if 'href="https://github.com/cauu/ouro-ops"' not in html:
        raise ValueError("production HTML lacks the canonical GitHub link")
    if "./target/release-candidate-control/release/ouro-ops" in html:
        raise ValueError("production HTML contains the retired repo-local CLI candidate")

    payload_match = re.search(r"const SKILL_PAYLOADS = (\{[^\n]+\});", html)
    if not payload_match:
        raise ValueError("production HTML lacks the generated Skill payload")
    payload = json.loads(payload_match.group(1))
    if set(payload) != set(PUBLIC_SKILLS):
        raise ValueError("production HTML does not contain exactly six public Skills")
    for operation, relative in PUBLIC_SKILLS.items():
        canonical = (skills_root / relative).read_text(encoding="utf-8")
        item = payload.get(operation)
        if not isinstance(item, dict) or item.get("content") != canonical:
            raise ValueError(f"production {operation} Skill differs from canonical source")

    install_match = re.search(r'const VERIFIED_REINSTALL = (".*");', html)
    if not install_match:
        raise ValueError("production HTML lacks the verified reinstall payload")
    if json.loads(install_match.group(1)) != install_source.read_text(encoding="utf-8"):
        raise ValueError("production verified reinstall differs from canonical source")

    return {
        "status": "ok",
        "changed": False,
        "bytes": len(html.encode("utf-8")),
        "skill_count": len(payload),
        "csp_network_denied": True,
        "github_link": "https://github.com/cauu/ouro-ops",
        "repo_local_candidate": False,
    }


def fetch(url: str) -> tuple[str, str]:
    request = urllib.request.Request(
        url, headers={"User-Agent": "ouro-ops-production-acceptance/1"}
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        if response.status != 200:
            raise ValueError(f"production URL returned HTTP {response.status}")
        body = response.read(MAX_HTML_BYTES + 1)
        if len(body) > MAX_HTML_BYTES:
            raise ValueError("production HTML exceeds the bounded response size")
        return response.geturl(), body.decode("utf-8")


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser()
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--url")
    source.add_argument("--file", type=pathlib.Path)
    parser.add_argument("--skills-root", type=pathlib.Path, default=root / "ouro-skills")
    parser.add_argument(
        "--install-source",
        type=pathlib.Path,
        default=root / "packaging" / "verified-reinstall.sh",
    )
    args = parser.parse_args()
    try:
        if args.url:
            observed_url, html = fetch(args.url)
        else:
            observed_url = args.file.resolve().as_uri()
            html = args.file.read_text(encoding="utf-8")
        result = verify(html, args.skills_root, args.install_source)
        result["url"] = observed_url
    except (
        OSError,
        UnicodeDecodeError,
        ValueError,
        json.JSONDecodeError,
        urllib.error.URLError,
    ) as error:
        print(
            json.dumps(
                {"status": "error", "changed": False, "detail": str(error)},
                sort_keys=True,
            )
        )
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
