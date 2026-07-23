#!/usr/bin/env python3
"""Enforced security invariants for the KES cold-signing script.

The offline cold-signing flow rests on a few properties that must hold for EVERY
generated script, or a private key could leak off the air-gapped machine. This gate
generates the real script via `ouro-ops kes cold-sign-script` and asserts it.
Fast (no docker), standalone:

    python3 tests/test_coldsign_invariants.py

Invariants:
  1. The script embeds ONLY public data — no signing-key content, no `SigningKey` marker.
  2. The cold key is read IN PLACE: referenced by its path variable and passed only to the
     signing command — never copied, printed, or piped anywhere.
  3. The ONLY external tool the script invokes is the intended cardano-cli signing subcommand;
     it carries no exfiltration primitive (scp/curl/wget/nc/base64/xxd/ssh) touching the key.
  4. The generator REFUSES a signing key (never emits a script that would embed private material).
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIX = ROOT / "tests" / "fixtures" / "kes"
VKEY = FIX / "kes-vkey-public.json"
SKEY = FIX / "kes-skey-private.json"
failures = []


def check(cond, msg):
    if not cond:
        failures.append(msg)


def ouro_bin():
    subprocess.run(["cargo", "build", "-q"], cwd=ROOT, check=True)
    return str(ROOT / "target" / "debug" / "ouro-ops")


EXFIL = ("scp", "curl", "wget", "nc ", "ncat", "base64", "xxd", "ssh ", "sftp", "rsync")


def cbor_of(path):
    return re.search(r'"cborHex"\s*:\s*"([0-9a-fA-F]+)"', Path(path).read_text()).group(1)


def strip_to_code(script, heredoc_marker):
    """Keep only unquoted, non-comment code — drop the header banner + the heredoc'd PUBLIC
    material (vkey / tx body). A forbidden token surviving this is real executable code."""
    out, in_heredoc = [], False
    for raw in script.splitlines():
        s = raw.strip()
        if s.startswith("cat >") and f"<<'{heredoc_marker}'" in s:
            in_heredoc = True
            out.append(raw)  # keep the cat line (it writes PUBLIC material to a temp)
            continue
        if in_heredoc:
            if s == heredoc_marker:
                in_heredoc = False
            continue
        if s.startswith("#"):
            continue
        out.append(raw)
    return "\n".join(out)


def kes_case(binary):
    def gen(vkey_path, period="123"):
        return subprocess.run(
            [binary, "kes", "cold-sign-script", "--kes-vkey", str(vkey_path), "--kes-period", period],
            cwd=ROOT, capture_output=True, text=True)

    # (4) refusing a signing key — on refusal a structured error JSON may NAME the reason, but must
    #     never contain a SCRIPT or the key's bytes.
    refused = gen(SKEY)
    check(refused.returncode != 0, "kes cold-sign-script MUST refuse a KES signing key (got exit 0)")
    skey_cbor = cbor_of(SKEY)
    check("#!/usr/bin/env bash" not in refused.stdout and "issue-op-cert" not in refused.stdout,
          "kes cold-sign-script emitted a SCRIPT for a signing-key input")
    check(skey_cbor not in refused.stdout, "LEAK: kes signing-key cborHex echoed back on refusal")

    ok = gen(VKEY)
    check(ok.returncode == 0, f"kes cold-sign-script failed on a valid vkey: {ok.stderr}")
    script = ok.stdout
    check(skey_cbor not in script, "LEAK: kes skey cborHex appears in the generated script")
    check("SigningKey" not in script, "kes script contains a SigningKey marker")

    code = strip_to_code(script, "OURO_KES_VKEY")
    subcmds = re.findall(r'node\s+issue-op-cert', code)
    check(len(subcmds) == 1, f"expected exactly one `node issue-op-cert`, found {len(subcmds)}")
    for call in re.findall(r'cardano-cli\s+([a-z-]+(?:\s+[a-z-]+)?)', code):
        check(call.startswith("node issue-op-cert") or call == "node",
              f"unexpected cardano-cli subcommand in kes script: {call!r}")
    for prim in EXFIL:
        check(prim not in code, f"exfil primitive {prim!r} in kes script code")
    check('--cold-signing-key-file "$COLD_SKEY"' in code, "kes: cold key not passed to issue-op-cert")
    check('cat "$COLD_SKEY"' not in code and 'echo "$COLD_SKEY"' not in code,
          "kes: cold key content read out (cat/echo of $COLD_SKEY)")


def main():
    binary = ouro_bin()
    kes_case(binary)

    if failures:
        print("FAIL — cold-sign invariant gate:")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("PASS — KES cold-sign invariant gate: public-only embed, in-place cold read, "
          "no exfil, signing-key refused")


if __name__ == "__main__":
    main()
