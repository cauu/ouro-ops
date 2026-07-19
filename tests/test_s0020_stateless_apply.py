#!/usr/bin/env python3
"""S0020 p2-2 — approved stateless applies revalidate before sealed fake mutations."""

import json
import hashlib
import os
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path

from test_s0020_stateless_plan import (
    BIN,
    GENESIS,
    ROOT,
    invoke,
    observation,
    target_args,
    write_probe,
)


def apply_args(operation, candidate, *params):
    args = list(target_args(operation, *params))
    args[1] = "apply"
    args.extend(("--approved-candidate", candidate))
    return args


def target_fleet_permit(candidate, port, expiry):
    now = int(time.time())
    return json.dumps(
        {
            "pool_id": "pool-0123456789abcdef01234567",
            "pool_spec_digest": "sha256:" + "b" * 64,
            "network": "mainnet",
            "genesis_hash": GENESIS,
            "target_host_key_sha256": "SHA256:" + "a" * 43,
            "node_id": "bp1",
            "operation_id": "runtime/restart",
            "intent_hash": candidate,
            "role": "bp",
            "target_image": None,
            "fencing_token": 1,
            "expiry_epoch": expiry,
            "facts_epoch": now,
            "online_relays": 1,
            "min_online_relays": 1,
            "relays_remaining": 0,
            "relay_health_endpoints": [
                {"node_id": "relay1", "host": "127.0.0.1", "port": port}
            ],
            "permit_id": "target-test-permit",
            "signature": "0" * 64,
        },
        separators=(",", ":"),
    )


def plan_candidate(home, probe, fakebin, operation, *params):
    completed, value = invoke(
        home,
        *target_args(operation, *params),
        env_extra={"OURO_PROBE_LIB": str(probe)},
        path=fakebin,
    )
    assert completed.returncode == 0, (completed, value)
    return value["data"]["candidate_hash"]


def main():
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0020-apply-"))
    probe = home / "probe.sh"
    write_probe(probe, observation())
    fakebin = home / "fakebin"
    fakebin.mkdir()
    docker_log = home / "docker.log"
    docker = fakebin / "docker"
    docker.write_text(
        "#!/usr/bin/env bash\n"
        "set -eu\n"
        "if test \"${1:-} ${2:-}\" = 'image inspect'; then\n"
        "  printf 'No such image\\n' >&2\n"
        "  exit 1\n"
        "fi\n"
        "printf '%s\\n' \"$*\" >>\"$OURO_TEST_DOCKER_LOG\"\n"
    )
    docker.chmod(0o700)
    env = {
        "OURO_PROBE_LIB": str(probe),
        "OURO_TEST_DOCKER_LOG": str(docker_log),
    }

    # A wrong approved candidate, malformed target flag, or role mismatch is rejected before the
    # fixed Docker executor can run.
    restart_candidate = plan_candidate(
        home, probe, fakebin, "runtime/restart", "--param", "machine=bp1"
    )
    for args, needle in (
        (
            apply_args(
                "runtime/restart",
                "0" * 64,
                "--param",
                "machine=bp1",
            ),
            "approved candidate does not match",
        ),
        (
            apply_args(
                "runtime/restart",
                restart_candidate,
                "--param",
                "machine=bp1",
            )
            + ["--shell", "id"],
            "unexpected argument",
        ),
    ):
        refused, value = invoke(home, *args, env_extra=env, path=fakebin)
        assert refused.returncode != 0 and needle in json.dumps(value), value
        assert not docker_log.exists(), "preflight refusal must not invoke Docker"

    wrong_role = apply_args(
        "runtime/restart",
        restart_candidate,
        "--param",
        "machine=bp1",
    )
    wrong_role[wrong_role.index("--role") + 1] = "relay"
    refused, value = invoke(home, *wrong_role, env_extra=env, path=fakebin)
    assert refused.returncode != 0 and "relay" in json.dumps(value), value
    assert not docker_log.exists()

    # Drift between the first apply probe and the immediate pre-mutation probe burns no target
    # state and never reaches Docker.
    counter = home / "probe-count"
    first = json.dumps(observation(), separators=(",", ":"))
    second = json.dumps(observation(container="cid-drifted"), separators=(",", ":"))
    probe.write_text(
        "ouro_observe() {\n"
        f"  n=$(cat '{counter}' 2>/dev/null || printf 0)\n"
        f"  if test \"$n\" = 0; then printf '%s\\n' '{first}'; else printf '%s\\n' '{second}'; fi\n"
        f"  printf '%s' $((n + 1)) >'{counter}'\n"
        "}\n"
    )
    drift, drift_value = invoke(
        home,
        *apply_args(
            "runtime/restart",
            restart_candidate,
            "--param",
            "machine=bp1",
        ),
        env_extra=env,
        path=fakebin,
    )
    assert drift.returncode != 0 and "live state changed" in json.dumps(drift_value), drift_value
    assert "container_id" in json.dumps(drift_value), drift_value
    assert not docker_log.exists()

    # Expired control-verified fleet evidence refuses at the target after final revalidation and
    # before Docker. A fresh permit then probes an immediate public relay endpoint and executes.
    write_probe(probe, observation())
    counter.unlink(missing_ok=True)
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    relay_port = listener.getsockname()[1]
    no_permit, no_permit_value = invoke(
        home,
        *apply_args(
            "runtime/restart",
            restart_candidate,
            "--param",
            "machine=bp1",
        ),
        env_extra=env,
        path=fakebin,
    )
    assert no_permit.returncode != 0 and "missing control-verified" in json.dumps(no_permit_value)
    assert not docker_log.exists(), "missing target permit must refuse before Docker"
    expired_args = apply_args(
        "runtime/restart",
        restart_candidate,
        "--param",
        "machine=bp1",
    ) + [
        "--verified-fleet-permit",
        target_fleet_permit(restart_candidate, relay_port, int(time.time()) - 1),
    ]
    expired, expired_value = invoke(home, *expired_args, env_extra=env, path=fakebin)
    assert expired.returncode != 0 and "expired before target mutation" in json.dumps(expired_value)
    assert not docker_log.exists(), "expired target permit must refuse before Docker"

    def accept_relay_probe():
        connection, _ = listener.accept()
        connection.close()
        listener.close()

    threading.Thread(target=accept_relay_probe, daemon=True).start()
    fresh_permit = target_fleet_permit(
        restart_candidate, relay_port, int(time.time()) + 30
    )
    # Docker restart returns before cardano-node necessarily answers on its socket. The target must
    # keep the same approved identity and wait through a transient unready sample instead of
    # declaring the already-executed restart failed immediately.
    readiness_counter = home / "runtime-readiness-count"
    ready_payload = json.dumps(observation(), separators=(",", ":"))
    unready_observation = observation()
    unready_observation["readiness"]["socket_answers"] = False
    unready_observation["readiness"]["tip_synced"] = False
    unready_payload = json.dumps(unready_observation, separators=(",", ":"))
    probe.write_text(
        "ouro_observe() {\n"
        f"  if test -f '{docker_log}'; then\n"
        f"    n=$(cat '{readiness_counter}' 2>/dev/null || printf 0)\n"
        f"    if test \"$n\" = 0; then printf '%s\\n' '{unready_payload}'; "
        f"else printf '%s\\n' '{ready_payload}'; fi\n"
        f"    printf '%s' $((n + 1)) >'{readiness_counter}'\n"
        f"  else printf '%s\\n' '{ready_payload}'; fi\n"
        "}\n"
    )
    applied, applied_value = invoke(
        home,
        *apply_args(
            "runtime/restart",
            restart_candidate,
            "--param",
            "machine=bp1",
        ),
        "--verified-fleet-permit",
        fresh_permit,
        env_extra=env,
        path=fakebin,
    )
    assert applied.returncode == 0, (applied, applied_value)
    assert applied_value["tool"] == "ouro.op.apply" and applied_value["changed"] is True
    post = applied_value["data"]["live_postcondition"]
    assert post == {
        "verification": "typed_role_readiness_passed",
        "container": {
            "id": "cid-plan",
            "creation_epoch": 1234,
            "image_config_digest": observation()["live"]["image_config_digest"],
        },
        "network": "mainnet",
        "genesis_hash": GENESIS,
        "node_running": True,
        "socket_answers": True,
        "tip_block": 9,
        "tip_slot": 10,
        "tip_era": "Conway",
        "sync_progress": "100.00",
        "tip_synced": True,
    }
    assert readiness_counter.read_text() == "2"
    assert docker_log.read_text().splitlines() == ["restart cid-plan"]
    docker_log.unlink()

    # Once the fixed restart argv has run, a postcondition failure must never fall back to the
    # generic changed:false error contract. Return typed mutation truth and tell the caller to
    # reconcile rather than retrying the restart.
    drifted_post = observation()
    drifted_post["live"]["network"] = "preprod"
    ready_payload = json.dumps(observation(), separators=(",", ":"))
    drifted_payload = json.dumps(drifted_post, separators=(",", ":"))
    probe.write_text(
        "ouro_observe() {\n"
        f"  if test -f '{docker_log}'; then printf '%s\\n' '{drifted_payload}'; "
        f"else printf '%s\\n' '{ready_payload}'; fi\n"
        "}\n"
    )
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    relay_port = listener.getsockname()[1]

    def accept_reconciliation_probe():
        connection, _ = listener.accept()
        connection.close()
        listener.close()

    threading.Thread(target=accept_reconciliation_probe, daemon=True).start()
    failed_after_restart, failed_value = invoke(
        home,
        *apply_args(
            "runtime/restart",
            restart_candidate,
            "--param",
            "machine=bp1",
        ),
        "--verified-fleet-permit",
        target_fleet_permit(
            restart_candidate, relay_port, int(time.time()) + 30
        ),
        env_extra=env,
        path=fakebin,
    )
    assert failed_after_restart.returncode != 0, (failed_after_restart, failed_value)
    assert failed_value["tool"] == "ouro.op.apply"
    assert failed_value["status"] == "error" and failed_value["changed"] is True
    assert failed_value["error"]["code"] == "postcondition_failed_after_mutation"
    assert failed_value["data"]["mutation_executed"] is True
    assert "do not retry restart" in failed_value["data"]["recovery"]
    assert docker_log.read_text().splitlines() == ["restart cid-plan"]
    docker_log.unlink()
    write_probe(probe, observation())

    allowlist_doc = json.loads((ROOT / "data/allowlist.json").read_text())
    allowlist_digest = "sha256:" + hashlib.sha256(
        json.dumps(allowlist_doc, separators=(",", ":")).encode()
    ).hexdigest()
    status, status_value = invoke(
        home,
        "target",
        "status",
        "--node",
        "bp1",
        "--role",
        "bp",
        "--network",
        "mainnet",
        "--genesis",
        GENESIS,
        "--expect-allowlist",
        allowlist_digest,
        env_extra=env,
        path=fakebin,
    )
    assert status.returncode == 0, (status, status_value)
    assert status_value["tool"] == "ouro.fleet.status"
    assert status_value["data"]["management_state"] == "not_required"
    assert status_value["data"]["online"] is True
    assert status_value["data"]["kes_rotation_repair_ready"] is True

    # Expired/invalid old KES credentials are valid repair input: they make ordinary BP readiness
    # false without erasing the narrower liveness/layout qualification used only by KES activation.
    expired_kes_bp = observation()
    expired_kes_bp["readiness"]["kes_opcert_valid"] = False
    expired_kes_bp["readiness"]["forging_credentials_ready"] = False
    write_probe(probe, expired_kes_bp)
    status, status_value = invoke(
        home,
        "target",
        "status",
        "--node",
        "bp1",
        "--role",
        "bp",
        "--network",
        "mainnet",
        "--genesis",
        GENESIS,
        "--expect-allowlist",
        allowlist_digest,
        env_extra=env,
        path=fakebin,
    )
    assert status.returncode == 0, (status, status_value)
    assert status_value["data"]["online"] is False
    assert status_value["data"]["kes_rotation_repair_ready"] is True

    for broken in [
        "node_running",
        "socket_answers",
        "tip_synced",
        "block_producer_configured",
        "has_forging_keys",
        "forging_key_permissions_safe",
        "kes_opcert_id",
    ]:
        broken_bp = observation()
        broken_bp["readiness"]["kes_opcert_valid"] = False
        broken_bp["readiness"]["forging_credentials_ready"] = False
        if broken in broken_bp["readiness"]:
            broken_bp["readiness"][broken] = False
        elif broken == "kes_opcert_id":
            broken_bp["live"][broken] = ""
        else:
            broken_bp["live"][broken] = False
        write_probe(probe, broken_bp)
        status, status_value = invoke(
            home,
            "target",
            "status",
            "--node",
            "bp1",
            "--role",
            "bp",
            "--network",
            "mainnet",
            "--genesis",
            GENESIS,
            "--expect-allowlist",
            allowlist_digest,
            env_extra=env,
            path=fakebin,
        )
        assert status.returncode == 0, (broken, status, status_value)
        assert status_value["data"]["kes_rotation_repair_ready"] is False, broken

    # Fleet collection must preserve an unready BP as typed offline evidence rather than refusing
    # the whole snapshot. Its forging state is irrelevant to the relay quorum count; a BP selected
    # for mutation remains protected by its own plan and post-write role-readiness gates.
    unready_bp = observation()
    unready_bp["live"]["forging_key_permissions_safe"] = False
    unready_bp["readiness"]["kes_opcert_valid"] = False
    unready_bp["readiness"]["forging_credentials_ready"] = False
    write_probe(probe, unready_bp)
    status, status_value = invoke(
        home,
        "target",
        "status",
        "--node",
        "bp1",
        "--role",
        "bp",
        "--network",
        "mainnet",
        "--genesis",
        GENESIS,
        "--expect-allowlist",
        allowlist_digest,
        env_extra=env,
        path=fakebin,
    )
    assert status.returncode == 0, (status, status_value)
    assert status_value["tool"] == "ouro.fleet.status"
    assert status_value["data"]["role"] == "bp"
    assert status_value["data"]["online"] is False
    assert status_value["data"]["kes_rotation_repair_ready"] is False
    write_probe(probe, observation())

    # Image preparation is non-disruptive but never accepts a payload. A stray payload must be
    # rejected before Docker, proving the image archive transport path is gone.
    stray_image_payload = home / "image.tar"
    stray_image_payload.write_bytes(b"operator image bytes are forbidden")
    allowed = json.loads((ROOT / "data/allowlist.json").read_text())["contracts"][0]["allowed"]
    target_image = next(
        image["image_config_digest"]
        for image in allowed
        if image["platform"] == "linux/amd64"
        and image["image_config_digest"] != observation()["live"]["image_config_digest"]
    )
    preload_candidate = plan_candidate(
        home,
        probe,
        fakebin,
        "upgrade/preload-image",
        "--param",
        "machine=bp1",
        "--param",
        f"image={target_image}",
    )

    target_preload, target_preload_value = invoke(
        home,
        *apply_args(
            "upgrade/preload-image",
            preload_candidate,
            "--param",
            "machine=bp1",
            "--param",
            f"image={target_image}",
        ),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(stray_image_payload)},
        path=fakebin,
    )
    assert target_preload.returncode != 0
    assert "does not accept an ephemeral artifact" in json.dumps(target_preload_value), target_preload_value
    assert not docker_log.exists(), "forbidden image payload must be refused before Docker"

    pool_spec = home / "pool-spec.yaml"
    pool_spec.write_text(
        f"""spec_version: 1
pool:
  network: mainnet
  network_magic: 764824073
  genesis_hashes:
    shelley: {GENESIS}
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh:
      host: 192.0.2.1
      port: 22
      user: cardano
      key_ref: creds://bp1
  - id: relay1
    role: relay
    public_endpoint:
      host: 192.0.2.2
      port: 3001
    ssh:
      host: 192.0.2.2
      port: 22
      user: cardano
      key_ref: creds://relay1
upgrade:
  min_online_relays: 1
"""
    )
    credentials = home / "credentials"
    credentials.mkdir()
    (credentials / "bp1").write_text("test-key")
    (credentials / "relay1").write_text("test-key")
    (home / "known_hosts").write_text(
        "192.0.2.1 ssh-ed25519 test\n192.0.2.2 ssh-ed25519 test\n"
    )
    runner = home / "runner"
    runner_bytes = b"stateless-apply-runner"
    runner.write_bytes(runner_bytes)

    # Fleet live-facts are also collected through the ephemeral target status command. Minting a
    # permit mutates only the control-local lease authority; both fake targets are read-only.
    bp_status = {
        "tool": "ouro.fleet.status",
        "machine": None,
        "status": "ok",
        "changed": False,
        "checks": [],
        "duration_s": 0.0,
        "audit_id": None,
        "data": {
            "node": "bp1",
            "role": "bp",
            "network": "mainnet",
            "genesis_hash": GENESIS,
            "host_key_sha256": "SHA256:" + "a" * 43,
            "online": True,
            "kes_rotation_repair_ready": True,
            "image_config_digest": observation()["live"]["image_config_digest"],
            "state_generation": 1234,
            "management_state": "not_required",
        },
    }
    relay_status = json.loads(json.dumps(bp_status))
    relay_status["data"].update(
        {
            "node": "relay1",
            "role": "relay",
            "host_key_sha256": "SHA256:" + "b" * 43,
        }
    )
    fleet_ssh_log = home / "fleet-ssh.log"
    ssh = fakebin / "ssh"
    ssh.write_text(
        "#!/usr/bin/env bash\n"
        "set -eu\n"
        "dd of=/dev/null bs=65536 status=none\n"
        "printf '%s\\n' \"$*\" >>\"$OURO_TEST_FLEET_SSH_LOG\"\n"
        "case \"$*\" in\n"
        f"  *cardano@192.0.2.1*) printf '%s\\n' '{json.dumps(bp_status, separators=(',', ':'))}' ;;\n"
        f"  *cardano@192.0.2.2*) printf '%s\\n' '{json.dumps(relay_status, separators=(',', ':'))}' ;;\n"
        "  *) exit 90 ;;\n"
        "esac\n"
    )
    ssh.chmod(0o700)
    permit, permit_value = invoke(
        home,
        "fleet",
        "permit",
        "create",
        "--spec",
        str(pool_spec),
        "--node",
        "bp1",
        "--op",
        "runtime/restart",
        "--intent-hash",
        restart_candidate,
        "--holder",
        "test-agent",
        env_extra={
            "OURO_EPHEMERAL_RUNNER": str(runner),
            "OURO_TEST_FLEET_SSH_LOG": str(fleet_ssh_log),
        },
        path=fakebin,
    )
    assert permit.returncode == 0, (permit, permit_value)
    assert permit_value["tool"] == "ouro.fleet.permit.create"
    assert (
        permit_value["data"]["facts"]["valid_until_epoch"]
        - permit_value["data"]["facts"]["collected_from_epoch"]
        == 180
    )
    assert permit_value["data"]["expires_at_epoch"] > int(time.time()) + 150
    fleet_remote = fleet_ssh_log.read_text()
    assert fleet_remote.count("'target' 'status'") == 2
    assert "/usr/local/bin/ouro-ops" not in fleet_remote and "ouro-op-run" not in fleet_remote

    # An expired-KES BP remains ineligible for generic disruption, but the exact KES install
    # operation may mint a permit from the separate repair qualification. Relay quorum still uses
    # only fully-online relays.
    repair_home = Path(tempfile.mkdtemp(prefix="ouro-s0025-kes-repair-permit-"))
    repair_credentials = repair_home / "credentials"
    repair_credentials.mkdir()
    (repair_credentials / "bp1").write_text("test-key")
    (repair_credentials / "relay1").write_text("test-key")
    (repair_home / "known_hosts").write_text(
        "192.0.2.1 ssh-ed25519 test\n192.0.2.2 ssh-ed25519 test\n"
    )
    repair_bp_status = json.loads(json.dumps(bp_status))
    repair_bp_status["data"]["online"] = False
    repair_bp_status["data"]["kes_rotation_repair_ready"] = True
    ssh.write_text(
        "#!/usr/bin/env bash\n"
        "set -eu\n"
        "dd of=/dev/null bs=65536 status=none\n"
        "printf '%s\\n' \"$*\" >>\"$OURO_TEST_FLEET_SSH_LOG\"\n"
        "case \"$*\" in\n"
        f"  *cardano@192.0.2.1*) printf '%s\\n' '{json.dumps(repair_bp_status, separators=(',', ':'))}' ;;\n"
        f"  *cardano@192.0.2.2*) printf '%s\\n' '{json.dumps(relay_status, separators=(',', ':'))}' ;;\n"
        "  *) exit 90 ;;\n"
        "esac\n"
    )
    ssh.chmod(0o700)
    repair_env = {
        "OURO_EPHEMERAL_RUNNER": str(runner),
        "OURO_TEST_FLEET_SSH_LOG": str(fleet_ssh_log),
    }
    generic_refused, generic_refused_value = invoke(
        repair_home,
        "fleet",
        "permit",
        "create",
        "--spec",
        str(pool_spec),
        "--node",
        "bp1",
        "--op",
        "runtime/restart",
        "--intent-hash",
        restart_candidate,
        "--holder",
        "test-agent",
        env_extra=repair_env,
        path=fakebin,
    )
    assert generic_refused.returncode != 0, (generic_refused, generic_refused_value)
    assert "full role readiness" in json.dumps(generic_refused_value)
    kes_permit, kes_permit_value = invoke(
        repair_home,
        "fleet",
        "permit",
        "create",
        "--spec",
        str(pool_spec),
        "--node",
        "bp1",
        "--op",
        "kes-rotation/install-opcert",
        "--intent-hash",
        "f" * 64,
        "--holder",
        "test-agent",
        env_extra=repair_env,
        path=fakebin,
    )
    assert kes_permit.returncode == 0, (kes_permit, kes_permit_value)
    assert kes_permit_value["data"]["facts"]["target_online"] is False
    assert kes_permit_value["data"]["facts"]["target_kes_rotation_repair_ready"] is True
    assert kes_permit_value["data"]["facts"]["target_qualification"] == "kes_rotation_repair_ready"

    stream = home / "stream.bin"
    ssh_count = home / "ssh.count"
    ssh_args = home / "ssh.args"
    remote_output = {
        "tool": "ouro.op.apply",
        "machine": "bp1",
        "status": "ok",
        "changed": True,
        "checks": [],
        "duration_s": 0.0,
        "audit_id": None,
        "data": {
            "op": "upgrade/preload-image",
            "node": "bp1",
            "candidate_hash": preload_candidate,
            "persistent_target_state_written": False,
        },
    }
    ssh = fakebin / "ssh"
    ssh.write_text(
        "#!/usr/bin/env bash\n"
        "set -eu\n"
        "dd of=\"$OURO_TEST_STREAM\" bs=65536 status=none\n"
        "printf '1\\n' >>\"$OURO_TEST_SSH_COUNT\"\n"
        "printf '%s' \"$*\" >\"$OURO_TEST_SSH_ARGS\"\n"
        f"printf '%s\\n' '{json.dumps(remote_output, separators=(',', ':'))}'\n"
    )
    ssh.chmod(0o700)
    control_env = {
        "OURO_EPHEMERAL_RUNNER": str(runner),
        "OURO_TEST_STREAM": str(stream),
        "OURO_TEST_SSH_COUNT": str(ssh_count),
        "OURO_TEST_SSH_ARGS": str(ssh_args),
    }
    base_control = (
        "op",
        "run",
        "--op",
        "upgrade/preload-image",
        "--node",
        "bp1",
        "--param",
        "machine=bp1",
        "--param",
        f"image={target_image}",
        "--dispatch",
        "192.0.2.1",
        "--ssh-key",
        "creds://bp1",
        "--spec",
        str(pool_spec),
        "--candidate-hash",
        preload_candidate,
    )

    missing, missing_value = invoke(
        home, *base_control, env_extra=control_env, path=fakebin
    )
    assert missing.returncode != 0 and "missing --confirm-token" in json.dumps(missing_value)
    assert not ssh_count.exists(), "missing approval must not open SSH"

    forbidden_archive, forbidden_archive_value = invoke(
        home,
        *base_control,
        "--artifact-file",
        str(stray_image_payload),
        env_extra=control_env,
        path=fakebin,
    )
    assert forbidden_archive.returncode != 0
    assert "accepted only for KES install or Deploy submit" in json.dumps(forbidden_archive_value)
    assert not ssh_count.exists(), "forbidden image archive must not open SSH"

    confirmed, confirmation_value = invoke(
        home,
        "confirm",
        "create",
        "--op",
        "upgrade/preload-image",
        "--node",
        "bp1",
        "--intent-hash",
        preload_candidate,
    )
    assert confirmed.returncode == 0, confirmation_value
    token = confirmation_value["data"]["confirm_token"]

    control_apply, control_value = invoke(
        home,
        *base_control,
        "--confirm-token",
        token,
        env_extra=control_env,
        path=fakebin,
    )
    assert control_apply.returncode == 0 and control_value["tool"] == "ouro.op.apply"
    assert stream.read_bytes() == runner_bytes
    remote = ssh_args.read_text()
    assert "'target' 'apply'" in remote
    assert f"'--approved-candidate' '{preload_candidate}'" in remote
    assert str(stray_image_payload) not in remote and "/usr/local/bin/ouro-ops" not in remote
    assert "ouro-op-run" not in remote and "OURO_EPHEMERAL_PAYLOAD" not in remote
    assert ssh_count.read_text().splitlines() == ["1"]
    audit_events = [json.loads(line) for line in (home / "s0019-audit.jsonl").read_text().splitlines()]
    apply_events = [
        event
        for event in audit_events
        if event.get("intent_hash") == preload_candidate
    ]
    assert [event["event"] for event in apply_events] == [
        "apply_attempt",
        "apply_succeeded",
    ], apply_events
    assert apply_events[-1]["outcome"] == "verified_success"

    replay, replay_value = invoke(
        home,
        *base_control,
        "--confirm-token",
        token,
        env_extra=control_env,
        path=fakebin,
    )
    assert replay.returncode != 0 and "already used" in json.dumps(replay_value), replay_value
    assert ssh_count.read_text().splitlines() == ["1"], "replay must stop before SSH"

    print("S0020 stateless apply safety passed")


if __name__ == "__main__":
    main()
