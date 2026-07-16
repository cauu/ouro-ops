#!/usr/bin/env python3
"""S0020 p2-2 — approved stateless applies revalidate before sealed fake mutations."""

import hashlib
import io
import json
import os
import socket
import subprocess
import tarfile
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


def docker_save(path):
    config = b'{"rootfs":{"type":"layers","diff_ids":[]}}'
    config_digest = hashlib.sha256(config).hexdigest()
    manifest = json.dumps(
        [
            {
                "Config": f"{config_digest}.json",
                "RepoTags": None,
                "Layers": ["layer/layer.tar"],
            }
        ],
        separators=(",", ":"),
    ).encode()
    with tarfile.open(path, "w") as archive:
        for name, payload in (
            (f"{config_digest}.json", config),
            ("layer/layer.tar", b"layer"),
            ("manifest.json", manifest),
        ):
            info = tarfile.TarInfo(name)
            info.size = len(payload)
            info.mode = 0o600
            archive.addfile(info, io.BytesIO(payload))
    payload = path.read_bytes()
    artifact_digest = hashlib.sha256(payload).hexdigest()
    return (
        f"sha256:{config_digest}",
        f"image-{artifact_digest[:8]}@sha256:{artifact_digest}",
        payload,
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
    assert docker_log.read_text().splitlines() == ["restart cid-plan"]
    docker_log.unlink()

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

    # Build a domain-valid, tag-free Docker-save artifact. Preload is intentionally non-disruptive,
    # so it lets this test prove the whole control approval/payload transport chain without a fleet
    # permit or any real target mutation.
    image_path = home / "image.tar"
    _, artifact_ref, artifact_bytes = docker_save(image_path)
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
        f"artifact={artifact_ref}",
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
            f"artifact={artifact_ref}",
            "--param",
            f"image={target_image}",
        ),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(image_path)},
        path=fakebin,
    )
    assert target_preload.returncode != 0
    assert "differs from approved" in json.dumps(target_preload_value), target_preload_value
    assert not docker_log.exists(), "artifact domain mismatch must precede docker load"

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
    fleet_remote = fleet_ssh_log.read_text()
    assert fleet_remote.count("'target' 'status'") == 2
    assert "/usr/local/bin/ouro-ops" not in fleet_remote and "ouro-op-run" not in fleet_remote

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
        f"artifact={artifact_ref}",
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
        "--artifact-file",
        str(image_path),
    )

    missing, missing_value = invoke(
        home, *base_control, env_extra=control_env, path=fakebin
    )
    assert missing.returncode != 0 and "missing --confirm-token" in json.dumps(missing_value)
    assert not ssh_count.exists(), "missing approval must not open SSH"

    bad_ref_args = list(base_control)
    bad_ref_args[bad_ref_args.index(f"artifact={artifact_ref}")] = (
        "artifact=image-00000000@sha256:" + "0" * 64
    )
    mismatch, mismatch_value = invoke(
        home, *bad_ref_args, env_extra=control_env, path=fakebin
    )
    assert mismatch.returncode != 0 and "do not match" in json.dumps(mismatch_value)
    assert not ssh_count.exists(), "artifact mismatch must not open SSH"

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
    assert stream.read_bytes() == runner_bytes + artifact_bytes
    remote = ssh_args.read_text()
    assert "'target' 'apply'" in remote
    assert f"'--approved-candidate' '{preload_candidate}'" in remote
    assert str(image_path) not in remote and "/usr/local/bin/ouro-ops" not in remote
    assert "ouro-op-run" not in remote and "OURO_EPHEMERAL_PAYLOAD" in remote
    assert ssh_count.read_text().splitlines() == ["1"]

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
