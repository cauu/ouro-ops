#!/usr/bin/env python3
"""S0025 p6-1-fix12 — typed forging-permission repair is fixed, verified, and reversible."""

import json
import os
import subprocess
import tempfile
from pathlib import Path

from test_s0020_stateless_apply import apply_args
from test_s0020_stateless_plan import ROOT, invoke, observation, target_args


def permission_observation(*, safe, network="mainnet"):
    value = observation()
    value["live"]["network"] = network
    for field in (
        "forging_key_permissions_safe",
        "keys_directory_safe",
        "kes_skey_private",
        "vrf_skey_private",
        "forging_key_owner_supported",
        "kes_rotation_permissions_ready",
    ):
        value["live"][field] = safe
    return value


def write_stateful_probe(path, state, *, drift_after_commit=False):
    unsafe = json.dumps(permission_observation(safe=False), separators=(",", ":"))
    safe = json.dumps(
        permission_observation(
            safe=True,
            network="preprod" if drift_after_commit else "mainnet",
        ),
        separators=(",", ":"),
    )
    path.write_text(
        "ouro_observe() {\n"
        f"  if test \"$(cat '{state}' 2>/dev/null || true)\" = safe; then\n"
        f"    printf '%s\\n' '{safe}'\n"
        "  else\n"
        f"    printf '%s\\n' '{unsafe}'\n"
        "  fi\n"
        "}\n"
    )


def main():
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0025-permission-normalize-"))
    fakebin = home / "fakebin"
    fakebin.mkdir()
    state = home / "permission-state"
    log = home / "docker.log"
    docker = fakebin / "docker"
    docker.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "printf '%s\\n' \"$*\" >>\"$OURO_PERMISSION_LOG\"\n"
        "current=$(cat \"$OURO_PERMISSION_STATE\" 2>/dev/null || true)\n"
        "case \"$*\" in\n"
        "  *'stat -c %f:%u:%g /proc/1'*) printf '41ed:1000:1000\\n' ;;\n"
        "  *'stat -c %f:%u:%g /opt/cardano/config/keys/kes.skey'*)\n"
        "    if test \"${OURO_PERMISSION_BAD_TYPE:-0}\" = 1; then printf 'a1ff:0:0\\n'; "
        "elif test \"$current\" = safe; then printf '8180:1000:1000\\n'; else printf '81a4:0:0\\n'; fi ;;\n"
        "  *'stat -c %f:%u:%g /opt/cardano/config/keys/vrf.skey'*)\n"
        "    if test \"$current\" = safe; then printf '8180:1000:1000\\n'; else printf '81a4:0:0\\n'; fi ;;\n"
        "  *'stat -c %f:%u:%g /opt/cardano/config/keys'*)\n"
        "    if test \"$current\" = safe; then printf '41c0:1000:1000\\n'; else printf '41fd:0:0\\n'; fi ;;\n"
        "  *'chmod 600 /opt/cardano/config/keys/kes.skey /opt/cardano/config/keys/vrf.skey'*)\n"
        "    printf safe >\"$OURO_PERMISSION_STATE\" ;;\n"
        "  *'chmod 0644 /opt/cardano/config/keys/vrf.skey'*)\n"
        "    printf unsafe >\"$OURO_PERMISSION_STATE\" ;;\n"
        "  *'chown --no-dereference '*|*'chmod 700 '*|*'chmod 0775 '*|*'chmod 0644 '*) ;;\n"
        "  *) exit 90 ;;\n"
        "esac\n"
    )
    docker.chmod(0o700)
    probe = home / "probe.sh"
    write_stateful_probe(probe, state)
    env = {
        "OURO_PROBE_LIB": str(probe),
        "OURO_PERMISSION_STATE": str(state),
        "OURO_PERMISSION_LOG": str(log),
    }
    params = ("--param", "machine=bp1")
    refused, refused_value = invoke(
        home,
        *target_args("credentials/normalize-forging-permissions", *params),
        env_extra={**env, "OURO_PERMISSION_BAD_TYPE": "1"},
        path=fakebin,
    )
    assert refused.returncode != 0
    assert "regular file" in json.dumps(refused_value) and "symlinks are refused" in json.dumps(refused_value)
    assert "chown" not in log.read_text() and "chmod" not in log.read_text()
    log.unlink()

    planned, planned_value = invoke(
        home,
        *target_args("credentials/normalize-forging-permissions", *params),
        env_extra=env,
        path=fakebin,
    )
    assert planned.returncode == 0, (planned, planned_value)
    candidate = planned_value["data"]["candidate_hash"]
    log.unlink()

    applied, applied_value = invoke(
        home,
        *apply_args("credentials/normalize-forging-permissions", candidate, *params),
        env_extra=env,
        path=fakebin,
    )
    assert applied.returncode == 0, (applied, applied_value)
    assert state.read_text() == "safe"
    assert applied_value["changed"] is True
    post = applied_value["data"]["live_postcondition"]
    assert post["verification"] == "fixed_forging_permissions_normalized"
    assert post["permissions"]["kes_rotation_permissions_ready"] is True
    assert post["key_contents_read"] is False and post["container_restarted"] is False
    commands = log.read_text()
    assert "restart" not in commands and "cat /opt/cardano/config/keys" not in commands
    assert "chown --no-dereference 1000:1000" in commands

    # A postcondition failure runs the exact candidate-bound inverse and restores the original
    # modes/owners. It never leaves the partially-normalized state in place.
    state.write_text("unsafe")
    log.unlink()
    write_stateful_probe(probe, state, drift_after_commit=True)
    planned, planned_value = invoke(
        home,
        *target_args("credentials/normalize-forging-permissions", *params),
        env_extra=env,
        path=fakebin,
    )
    assert planned.returncode == 0, (planned, planned_value)
    candidate = planned_value["data"]["candidate_hash"]
    log.unlink()
    failed, failed_value = invoke(
        home,
        *apply_args("credentials/normalize-forging-permissions", candidate, *params),
        env_extra=env,
        path=fakebin,
    )
    assert failed.returncode != 0, (failed, failed_value)
    assert "rollback completed" in json.dumps(failed_value)
    assert state.read_text() == "unsafe"
    rollback_commands = log.read_text()
    assert "chmod 0775 /opt/cardano/config/keys" in rollback_commands
    assert "chmod 0644 /opt/cardano/config/keys/kes.skey" in rollback_commands
    assert "chmod 0644 /opt/cardano/config/keys/vrf.skey" in rollback_commands
    assert rollback_commands.count("chown --no-dereference 0:0") == 3

    print("S0025 typed forging-permission normalization passed")


if __name__ == "__main__":
    main()
