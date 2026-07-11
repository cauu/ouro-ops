"""Shared test helper: drive L2 skill scripts through the audited `ouro-ops tool run`
entrypoint so they run inside a genuine, CLI-signed audit context.

After S0014's audit-gate hardening, an L2 script only writes when a valid signed
invocation token verifies against the audit DB — a bare `export OURO_AUDIT_ID=...`
no longer satisfies the gate. Tests must therefore go through `ouro-ops tool run`.
"""
import functools
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


@functools.lru_cache(maxsize=1)
def ouro_bin():
    subprocess.run(["cargo", "build", "-q"], cwd=ROOT, check=True)
    return str(ROOT / "target" / "debug" / "ouro-ops")


def tool_run(tool, *, spec=None, machine=None, env=None, home=None, check=True):
    """Run `ouro-ops tool run <tool>`; returns the CompletedProcess whose stdout is the
    executed script's single-line JSON and whose returncode is the script's exit code."""
    cmd = [ouro_bin(), "tool", "run", tool]
    if spec is not None:
        cmd += ["--spec", str(spec)]
    if machine is not None:
        cmd += ["--machine", machine]
    child_env = os.environ.copy()
    if home is not None:
        child_env["OURO_HOME"] = str(home)
    if env:
        child_env.update(env)
    return subprocess.run(
        cmd, cwd=ROOT, env=child_env, text=True, stdout=subprocess.PIPE, check=check
    )
