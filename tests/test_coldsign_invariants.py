#!/usr/bin/env python3
"""S0017 p4-4 — enforced security invariants for the KES cold-signing script.

The offline cold-signing flow rests on a few properties that must hold for EVERY
generated script, or a private key could leak off the air-gapped machine. This gate
generates a real script via `ouro-ops kes cold-sign-script` and asserts them, plus
the module-level refusals. It is fast (no docker) and runs standalone:

    python3 tests/test_coldsign_invariants.py

Invariants (kes scope; deploy/VRF scope is p4-2/p4-9):
  1. The script embeds ONLY public data — no cold.skey / KES signing-key content, no
     `SigningKey` marker.
  2. The cold key is read IN PLACE: referenced by the COLD_SKEY path and passed only to
     `issue-op-cert` — never copied, printed, or piped anywhere.
  3. The ONLY external tool the script invokes is `cardano-cli node issue-op-cert`; it
     carries no exfiltration primitive (scp/curl/wget/nc/base64/xxd/ssh) touching the key.
  4. `ouro-ops kes cold-sign-script` REFUSES a signing key (never emits a script that
     would embed private material).
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


def gen_script(binary, vkey_path, period="123"):
    return subprocess.run(
        [binary, "kes", "cold-sign-script", "--kes-vkey", str(vkey_path), "--kes-period", period],
        cwd=ROOT, capture_output=True, text=True,
    )


def main():
    binary = ouro_bin()

    # (4) refusing a signing key — do this first; nothing else matters if it can leak.
    #     On refusal the CLI emits a structured error JSON (which may *name* the reason,
    #     e.g. "SigningKey"); what must never appear is an actual SCRIPT or the key's bytes.
    refused = gen_script(binary, SKEY)
    check(refused.returncode != 0, "cold-sign-script MUST refuse a KES signing key (got exit 0)")
    skey_cbor = re.search(r'"cborHex"\s*:\s*"([0-9a-fA-F]+)"', SKEY.read_text()).group(1)
    check("#!/usr/bin/env bash" not in refused.stdout and "issue-op-cert" not in refused.stdout,
          "cold-sign-script emitted a SCRIPT for a signing-key input")
    check(skey_cbor not in refused.stdout,
          "LEAK: signing-key cborHex echoed back on refusal")

    ok = gen_script(binary, VKEY)
    check(ok.returncode == 0, f"cold-sign-script failed on a valid vkey: {ok.stderr}")
    script = ok.stdout

    # (1) only public data embedded.
    check(skey_cbor not in script, "LEAK: cold.skey cborHex appears in the generated script")
    check("SigningKey" not in script, "generated script contains a SigningKey marker")
    check("kes.skey" not in script and "cold.skey\ncat" not in script,
          "generated script references a signing-key file for reading its content")

    # keep only unquoted, non-comment code (drop the header banner + heredoc'd public vkey).
    code_lines = []
    in_heredoc = False
    for raw in script.splitlines():
        s = raw.strip()
        if s.startswith("cat >") and "<<'OURO_KES_VKEY'" in s:
            in_heredoc = True
            code_lines.append(raw)  # keep the cat line itself (it writes the PUBLIC vkey to a temp)
            continue
        if in_heredoc:
            if s == "OURO_KES_VKEY":
                in_heredoc = False
            continue
        if s.startswith("#"):
            continue
        code_lines.append(raw)
    code = "\n".join(code_lines)

    # (3) the only external cardano subcommand is `node issue-op-cert`.
    cardano_calls = re.findall(r'cardano-cli\s+([a-z-]+(?:\s+[a-z-]+)?)', code)
    # the generated code references cardano-cli via "$CARDANO_CLI"; match the subcommand line too.
    subcmds = re.findall(r'node\s+issue-op-cert', code)
    check(len(subcmds) == 1, f"expected exactly one `node issue-op-cert`, found {len(subcmds)}")
    for call in cardano_calls:
        check(call.startswith("node issue-op-cert") or call == "node",
              f"unexpected cardano-cli subcommand in script: {call!r}")

    # (2)+(3) no exfiltration primitive anywhere in the executable code.
    EXFIL = ("scp", "curl", "wget", "nc ", "ncat", "base64", "xxd", "ssh ", "sftp", "rsync")
    for prim in EXFIL:
        check(prim not in code, f"exfiltration primitive {prim!r} present in cold-sign script code")

    # (2) the cold key is passed ONLY as the issue-op-cert argument, never cat/echo'd out.
    check('--cold-signing-key-file "$COLD_SKEY"' in code,
          "cold key is not passed to issue-op-cert as expected")
    check("cat \"$COLD_SKEY\"" not in code and 'echo "$COLD_SKEY"' not in code,
          "cold key content is read out (cat/echo of $COLD_SKEY)")

    if failures:
        print("FAIL — cold-sign invariant gate:")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("PASS — cold-sign invariant gate: public-only embed, in-place cold read, no exfil, skey refused")


if __name__ == "__main__":
    main()
