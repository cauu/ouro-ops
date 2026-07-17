#!/usr/bin/env python3
"""S0020 p4-13 — one Upgrade prompt, two sealed transaction boundaries."""

import copy
import hashlib
import hmac
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

from test_s0020_stateless_apply import apply_args
from test_s0020_stateless_plan import BIN, GENESIS, ROOT, invoke, observation, target_args


TEST_KEY = "s0020-upgrade-workflow-key"


def make_archive(path, configs):
    manifests = []
    entries = []
    for index, config in enumerate(configs):
        digest = hashlib.sha256(config).hexdigest()
        layer = f"layer-{index}/layer.tar"
        manifests.append(
            {"Config": f"{digest}.json", "RepoTags": None, "Layers": [layer]}
        )
        entries.extend(((f"{digest}.json", config), (layer, f"layer-{index}".encode())))
    entries.append(("manifest.json", json.dumps(manifests, separators=(",", ":")).encode()))
    with tarfile.open(path, "w") as archive:
        for name, payload in entries:
            info = tarfile.TarInfo(name)
            info.size = len(payload)
            info.mode = 0o600
            archive.addfile(info, io.BytesIO(payload))
    payload = path.read_bytes()
    digest = hashlib.sha256(payload).hexdigest()
    return (
        [f"sha256:{hashlib.sha256(config).hexdigest()}" for config in configs],
        f"image-{digest[:8]}@sha256:{digest}",
        payload,
    )


def write_allowlist(path, old_image, target_image, backward_compatible):
    paths = {
        "socket": "/ipc/node.socket",
        "db": "/data/db",
        "keys": "/opt/cardano/config/keys",
        "config": "/opt/cardano/config",
        "topology": "/opt/cardano/config/mainnet/topology.json",
        "genesis": "/opt/cardano/config/mainnet/shelley-genesis.json",
    }
    role_rules = {
        "bp": {"requires_opcert": True, "forbids_forging_keys": False},
        "relay": {"requires_opcert": False, "forbids_forging_keys": True},
    }
    allowed_old = {
        "platform": "linux/amd64",
        "oci_index_digest": "sha256:" + "1" * 64,
        "platform_manifest_digest": "sha256:" + "2" * 64,
        "image_config_digest": old_image,
    }
    allowed_target = {
        "platform": "linux/amd64",
        "oci_index_digest": "sha256:" + "3" * 64,
        "platform_manifest_digest": "sha256:" + "4" * 64,
        "image_config_digest": target_image,
    }
    document = {
        "allowlist_version": 77,
        "signature": "pending",
        "contracts": [
            {
                "convention_version": 1,
                "contract_id": "blinklabs-cardano-node-v1",
                "in_container_paths": paths,
                "role_rules": role_rules,
                "allowed": [allowed_old, allowed_target],
            },
        ],
        "denylist": [],
        "transitions": [
            {
                "from_image_config_digest": old_image,
                "to_image_config_digest": target_image,
                "db_forward_compatible": True,
                "db_backward_compatible": backward_compatible,
                "snapshot_taken": False,
            }
        ],
    }
    unsigned = dict(document)
    unsigned.pop("signature")
    canonical = json.dumps(unsigned, sort_keys=True, separators=(",", ":")).encode()
    signature = hmac.new(TEST_KEY.encode(), canonical, hashlib.sha256).hexdigest()
    document["signature"] = f"test-hmac-sha256:{signature}"
    path.write_text(json.dumps(document, separators=(",", ":")))


def reset_state(path, old_image, target_image, *, target_present, fail_readiness=False,
                fail_present_once=False):
    path.write_text(
        json.dumps(
            {
                "target": target_image,
                "images": [target_image] if target_present else [],
                "loaded": False,
                "fail_present_once": fail_present_once,
                "fail_readiness": fail_readiness,
                "current": {"id": "cid-old", "image": old_image, "epoch": 1234},
                "previous": None,
            },
            separators=(",", ":"),
        )
    )


def write_sealed_runtime(home, old_image, target_image):
    state = home / "runtime-state.json"
    base = observation()
    base["live"]["image_config_digest"] = old_image
    base["live"]["container_id"] = "cid-old"
    base["live"]["container_name"] = "cardano-node"
    base["live"]["container_creation_epoch"] = 1234
    base_path = home / "base-observation.json"
    base_path.write_text(json.dumps(base, separators=(",", ":")))
    renderer = home / "render-observation.py"
    renderer.write_text(
        "#!/usr/bin/env python3\n"
        "import json,os\n"
        "base=json.load(open(os.environ['OURO_TEST_BASE_OBSERVATION']))\n"
        "state=json.load(open(os.environ['OURO_TEST_RUNTIME_STATE']))\n"
        "current=state['current']\n"
        "if current is None: raise SystemExit('sealed runtime has no current container')\n"
        "base['live']['container_id']=current['id']\n"
        "base['live']['container_name']='cardano-node'\n"
        "base['live']['image_config_digest']=current['image']\n"
        "base['live']['container_creation_epoch']=current['epoch']\n"
        "if state['fail_readiness'] and current['image']==state['target']:\n"
        " base['readiness']['socket_answers']=False\n"
        " base['readiness']['tip_synced']=False\n"
        "print(json.dumps(base,separators=(',',':')))\n"
    )
    renderer.chmod(0o700)
    probe = home / "probe.sh"
    probe.write_text(f"ouro_observe() {{ '{renderer}'; }}\n")

    fakebin = home / "fakebin"
    fakebin.mkdir()
    docker = fakebin / "docker"
    docker.write_text(
        "#!/usr/bin/env python3\n"
        "import json,os,sys\n"
        "args=sys.argv[1:]\n"
        "state_path=os.environ['OURO_TEST_RUNTIME_STATE']\n"
        "state=json.load(open(state_path))\n"
        "with open(os.environ['OURO_TEST_DOCKER_LOG'],'a') as log: log.write(json.dumps(args)+'\\n')\n"
        "def save():\n"
        " open(state_path,'w').write(json.dumps(state,separators=(',',':')))\n"
        "target=state['target']\n"
        "if args[:2]==['image','inspect']:\n"
        " if target in state['images']:\n"
        "  if state['loaded'] and state['fail_present_once']:\n"
        "   state['fail_present_once']=False; save(); print('No such image',file=sys.stderr); raise SystemExit(1)\n"
        "  print(target); raise SystemExit(0)\n"
        " print('No such image',file=sys.stderr); raise SystemExit(1)\n"
        "if args[:2]==['load','--input']:\n"
        " state['images']=sorted(set(state['images']+[target])); state['loaded']=True; save(); print('Loaded'); raise SystemExit(0)\n"
        "if args[:2]==['image','rm']:\n"
        " state['images']=[item for item in state['images'] if item!=args[2]]; save(); raise SystemExit(0)\n"
        "if args and args[0]=='rename':\n"
        " if args[1]=='cid-old': state['previous']=state['current']; state['current']=None\n"
        " else: state['current']=state['previous']; state['previous']=None\n"
        " save(); raise SystemExit(0)\n"
        "if args and args[0]=='stop': raise SystemExit(0)\n"
        "if args and args[0]=='run':\n"
        " state['current']={'id':'cid-new','image':target,'epoch':2345}; save(); print('cid-new'); raise SystemExit(0)\n"
        "if args[:2]==['rm','-f']:\n"
        " if args[2]=='cardano-node': state['current']=None\n"
        " elif args[2]=='cardano-node.ouro-prev': state['previous']=None\n"
        " save(); raise SystemExit(0)\n"
        "if args and args[0]=='start': save(); raise SystemExit(0)\n"
        "print('unsupported sealed docker argv: '+repr(args),file=sys.stderr); raise SystemExit(90)\n"
    )
    docker.chmod(0o700)
    return state, probe, fakebin


def plan(home, env, fakebin, operation, artifact_ref, target_image):
    params = ["--param", "machine=bp1"]
    if operation == "upgrade/preload-image":
        params += ["--param", f"artifact={artifact_ref}"]
    params += ["--param", f"image={target_image}"]
    completed, value = invoke(home, *target_args(operation, *params), env_extra=env, path=fakebin)
    assert completed.returncode == 0, (completed, value)
    return value


def permit(candidate, target_image, port, *, relays_remaining=0):
    now = int(time.time())
    return json.dumps(
        {
            "pool_id": "pool-0123456789abcdef01234567",
            "pool_spec_digest": "sha256:" + "b" * 64,
            "network": "mainnet",
            "genesis_hash": GENESIS,
            "target_host_key_sha256": "SHA256:" + "a" * 43,
            "node_id": "bp1",
            "operation_id": "upgrade/step",
            "intent_hash": candidate,
            "role": "bp",
            "target_image": target_image,
            "fencing_token": 1,
            "expiry_epoch": now + 60,
            "facts_epoch": now,
            "online_relays": 1,
            "min_online_relays": 1,
            "relays_remaining": relays_remaining,
            "relay_health_endpoints": [
                {"node_id": "relay1", "host": "127.0.0.1", "port": port}
            ],
            "permit_id": "sealed-upgrade-permit",
            "signature": "0" * 64,
        },
        separators=(",", ":"),
    )


def relay_listener():
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    port = listener.getsockname()[1]

    def accept():
        connection, _ = listener.accept()
        connection.close()
        listener.close()

    threading.Thread(target=accept, daemon=True).start()
    return port


def step_apply(home, env, fakebin, candidate, target_image, port, *, relays_remaining=0):
    args = apply_args(
        "upgrade/step",
        candidate,
        "--param",
        "machine=bp1",
        "--param",
        f"image={target_image}",
    ) + [
        "--verified-fleet-permit",
        permit(candidate, target_image, port, relays_remaining=relays_remaining),
    ]
    return invoke(home, *args, env_extra=env, path=fakebin)


def mutation_commands(log):
    return [json.loads(line) for line in log.read_text().splitlines() if line.strip()]


def production_step_plan(home, old_image, target_image):
    home.mkdir()
    state, probe, fakebin = write_sealed_runtime(home, old_image, target_image)
    reset_state(state, old_image, target_image, target_present=True)
    env = {
        "OURO_PROBE_LIB": str(probe),
        "OURO_TEST_BASE_OBSERVATION": str(home / "base-observation.json"),
        "OURO_TEST_RUNTIME_STATE": str(state),
        "OURO_TEST_DOCKER_LOG": str(home / "docker.log"),
        "OURO_READINESS_SAMPLE_DELAY": "0",
    }
    return invoke(
        home,
        *target_args(
            "upgrade/step",
            "--param",
            "machine=bp1",
            "--param",
            f"image={target_image}",
        ),
        env_extra=env,
        path=fakebin,
    )


def main():
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="ouro-s0020-upgrade-") as temporary:
        home = Path(temporary)
        old_image = observation()["live"]["image_config_digest"]
        archive = home / "target-image.tar"
        configs, artifact_ref, artifact_bytes = make_archive(
            archive, [b'{"rootfs":{"type":"layers","diff_ids":[]},"sealed":"target"}']
        )
        target_image = configs[0]
        assert target_image != old_image
        policy = home / "allowlist.json"
        write_allowlist(policy, old_image, target_image, True)
        state, probe, fakebin = write_sealed_runtime(home, old_image, target_image)
        docker_log = home / "docker.log"
        env = {
            "OURO_PROBE_LIB": str(probe),
            "OURO_ALLOWLIST_FILE": str(policy),
            "OURO_ALLOWLIST_TEST_KEY": TEST_KEY,
            "OURO_TEST_BASE_OBSERVATION": str(home / "base-observation.json"),
            "OURO_TEST_RUNTIME_STATE": str(state),
            "OURO_TEST_DOCKER_LOG": str(docker_log),
            "OURO_READINESS_SAMPLE_DELAY": "0",
        }

        # Preparation success: candidate-bound bytes are deeply inspected before exactly one load;
        # the running node identity remains unchanged and exact target presence is reported.
        reset_state(state, old_image, target_image, target_present=False)
        preload_plan = plan(
            home, env, fakebin, "upgrade/preload-image", artifact_ref, target_image
        )
        preload_candidate = preload_plan["data"]["candidate_hash"]
        preload_args = apply_args(
            "upgrade/preload-image",
            preload_candidate,
            "--param",
            "machine=bp1",
            "--param",
            f"artifact={artifact_ref}",
            "--param",
            f"image={target_image}",
        )
        applied, applied_value = invoke(
            home,
            *preload_args,
            env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(archive)},
            path=fakebin,
        )
        assert applied.returncode == 0, (applied, applied_value)
        assert applied_value["data"]["live_postcondition"] == {
            "verification": "exact_image_config_present",
            "image_config_digest": target_image,
            "running_container_unchanged": {
                "id": "cid-old",
                "creation_epoch": 1234,
                "image_config_digest": old_image,
            },
        }
        commands = mutation_commands(docker_log)
        assert sum(command[:2] == ["load", "--input"] for command in commands) == 1
        assert not any(command and command[0] in {"rename", "run", "restart"} for command in commands)
        assert json.loads(state.read_text())["current"]["id"] == "cid-old"

        # Preparation rollback: an injected post-load presence failure removes only the exact target.
        docker_log.unlink()
        reset_state(
            state,
            old_image,
            target_image,
            target_present=False,
            fail_present_once=True,
        )
        failed, failed_value = invoke(
            home,
            *preload_args,
            env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(archive)},
            path=fakebin,
        )
        assert failed.returncode != 0
        assert "live-state rollback completed" in json.dumps(failed_value)
        failed_state = json.loads(state.read_text())
        assert target_image not in failed_state["images"] and failed_state["current"]["id"] == "cid-old"
        commands = mutation_commands(docker_log)
        assert sum(command[:2] == ["load", "--input"] for command in commands) == 1
        assert ["image", "rm", target_image] in commands

        # Wrong archive config and a byte/path swap refuse before docker load.
        wrong_archive = home / "wrong-image.tar"
        _, wrong_ref, _ = make_archive(wrong_archive, [b'{"sealed":"wrong-config"}'])
        docker_log.unlink()
        reset_state(state, old_image, target_image, target_present=False)
        wrong_plan = plan(
            home, env, fakebin, "upgrade/preload-image", wrong_ref, target_image
        )
        wrong_args = apply_args(
            "upgrade/preload-image",
            wrong_plan["data"]["candidate_hash"],
            "--param",
            "machine=bp1",
            "--param",
            f"artifact={wrong_ref}",
            "--param",
            f"image={target_image}",
        )
        wrong, wrong_value = invoke(
            home,
            *wrong_args,
            env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(wrong_archive)},
            path=fakebin,
        )
        assert wrong.returncode != 0 and "differs from approved" in json.dumps(wrong_value), wrong_value
        assert not any(command[:2] == ["load", "--input"] for command in mutation_commands(docker_log))

        multi_archive = home / "multi-image.tar"
        _, multi_ref, _ = make_archive(
            multi_archive,
            [
                b'{"rootfs":{"type":"layers","diff_ids":[]},"sealed":"target"}',
                b'{"sealed":"unexpected-second-image"}',
            ],
        )
        docker_log.unlink()
        reset_state(state, old_image, target_image, target_present=False)
        multi_plan = plan(
            home, env, fakebin, "upgrade/preload-image", multi_ref, target_image
        )
        multi_args = apply_args(
            "upgrade/preload-image",
            multi_plan["data"]["candidate_hash"],
            "--param",
            "machine=bp1",
            "--param",
            f"artifact={multi_ref}",
            "--param",
            f"image={target_image}",
        )
        multi, multi_value = invoke(
            home,
            *multi_args,
            env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(multi_archive)},
            path=fakebin,
        )
        assert multi.returncode != 0 and "exactly one image" in json.dumps(multi_value), multi_value
        assert not any(command[:2] == ["load", "--input"] for command in mutation_commands(docker_log))

        swapped = home / "swapped-image.tar"
        swapped.write_bytes(artifact_bytes + b"\n")
        docker_log.unlink()
        reset_state(state, old_image, target_image, target_present=False)
        swapped_result, swapped_value = invoke(
            home,
            *preload_args,
            env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(swapped)},
            path=fakebin,
        )
        assert swapped_result.returncode != 0 and "bytes do not match" in json.dumps(swapped_value)
        assert not any(command[:2] == ["load", "--input"] for command in mutation_commands(docker_log))

        # Activation success: signed transition is candidate-bound and reviewable; N is retained
        # until N+1 passes readiness, then finalized. Environment values stay redacted.
        docker_log.unlink()
        reset_state(state, old_image, target_image, target_present=True)
        step_plan = plan(home, env, fakebin, "upgrade/step", "", target_image)
        repeated = plan(home, env, fakebin, "upgrade/step", "", target_image)
        assert repeated["data"]["candidate_hash"] == step_plan["data"]["candidate_hash"]
        assert step_plan["data"]["upgrade_transition"]["to_image_config_digest"] == target_image
        assert step_plan["data"]["upgrade_failure_outcome"] == "verified_rollback_to_N"
        assert step_plan["data"]["rollback_executor_plan"]
        assert "PRIVATE_VALUE=not-output" not in json.dumps(step_plan)
        step_candidate = step_plan["data"]["candidate_hash"]
        success, success_value = step_apply(
            home, env, fakebin, step_candidate, target_image, relay_listener()
        )
        assert success.returncode == 0, (success, success_value)
        assert success_value["data"]["live_postcondition"]["container"]["image_config_digest"] == target_image
        success_state = json.loads(state.read_text())
        assert success_state["current"]["image"] == target_image and success_state["previous"] is None
        commands = mutation_commands(docker_log)
        assert sum(command and command[0] == "run" for command in commands) == 1
        assert ["rm", "-f", "cardano-node.ouro-prev"] in commands

        # Backward-compatible failure restores and verifies N.
        docker_log.unlink()
        reset_state(state, old_image, target_image, target_present=True, fail_readiness=True)
        rollback_plan = plan(home, env, fakebin, "upgrade/step", "", target_image)
        rolled, rolled_value = step_apply(
            home,
            env,
            fakebin,
            rollback_plan["data"]["candidate_hash"],
            target_image,
            relay_listener(),
        )
        assert rolled.returncode != 0 and "live-state rollback completed" in json.dumps(rolled_value), rolled_value
        rolled_state = json.loads(state.read_text())
        assert rolled_state["current"]["image"] == old_image and rolled_state["previous"] is None
        commands = mutation_commands(docker_log)
        assert ["rename", "cardano-node.ouro-prev", "cardano-node"] in commands
        assert ["start", "cardano-node"] in commands

        # Forward-only policy changes the candidate and refuses an unsafe automatic rollback after
        # N+1 has run. The retained prior container is recovery evidence, not permission to start N.
        backward_candidate = rollback_plan["data"]["candidate_hash"]
        write_allowlist(policy, old_image, target_image, False)
        docker_log.unlink()
        reset_state(state, old_image, target_image, target_present=True, fail_readiness=True)
        forward_plan = plan(home, env, fakebin, "upgrade/step", "", target_image)
        assert forward_plan["data"]["candidate_hash"] != backward_candidate
        assert forward_plan["data"]["upgrade_failure_outcome"] == "forward_recovery_or_resync_required"
        assert forward_plan["data"]["rollback_executor_plan"] is None
        forward, forward_value = step_apply(
            home,
            env,
            fakebin,
            forward_plan["data"]["candidate_hash"],
            target_image,
            relay_listener(),
        )
        assert forward.returncode != 0
        assert "automatic rollback refused" in json.dumps(forward_value)
        forward_state = json.loads(state.read_text())
        assert forward_state["current"]["image"] == target_image
        assert forward_state["previous"]["image"] == old_image
        commands = mutation_commands(docker_log)
        assert ["start", "cardano-node"] not in commands

        # The production Ed25519 policy authorizes each reviewed adjacent runtime edge while all
        # releases retain the same Blink layout contract. Direct skips and reverse edges fail at
        # the target plan boundary before any executor can run.
        production = json.loads((ROOT / "data/allowlist.json").read_text())
        assert production["allowlist_version"] == 3
        assert len(production["contracts"]) == 1
        assert production["contracts"][0]["convention_version"] == 1
        transitions = production["transitions"]
        assert len(transitions) == 3
        for index, transition in enumerate(transitions):
            completed, value = production_step_plan(
                home / f"production-edge-{index}",
                transition["from_image_config_digest"],
                transition["to_image_config_digest"],
            )
            assert completed.returncode == 0, (completed, value)
            assert value["data"]["upgrade_transition"] == transition
            assert value["data"]["runtime_policy"]["contract_id"] == "blinklabs-cardano-node-v1"

        first = transitions[0]["from_image_config_digest"]
        final = transitions[-1]["to_image_config_digest"]
        skipped, skipped_value = production_step_plan(
            home / "production-skip", first, final
        )
        assert skipped.returncode != 0
        assert "allowlisting images alone is insufficient" in json.dumps(skipped_value)

        reverse_from = transitions[-1]["to_image_config_digest"]
        reverse_to = transitions[-1]["from_image_config_digest"]
        reversed_result, reversed_value = production_step_plan(
            home / "production-reverse", reverse_from, reverse_to
        )
        assert reversed_result.returncode != 0
        assert "allowlisting images alone is insufficient" in json.dumps(reversed_value)

    print("S0020 single-prompt Upgrade sealed workflow passed")


if __name__ == "__main__":
    main()
