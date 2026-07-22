#!/usr/bin/env python3
"""S0020 p4-13 — one Upgrade prompt, two sealed transaction boundaries."""

import copy
import hashlib
import hmac
import json
import os
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path

from test_s0020_stateless_apply import apply_args
from test_s0020_stateless_plan import BIN, GENESIS, ROOT, invoke, observation, target_args


TEST_KEY = "s0020-upgrade-workflow-key"


def write_allowlist(
    path,
    old_image,
    target_image,
    backward_compatible,
    repository="ghcr.io/blinklabs-io/cardano-node",
):
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
        "release": "fixture-old",
        "platform": "linux/amd64",
        "oci_index_digest": "sha256:" + "1" * 64,
        "platform_manifest_digest": "sha256:" + "2" * 64,
        "image_config_digest": old_image,
    }
    allowed_target = {
        "release": "fixture-target",
        "platform": "linux/amd64",
        "oci_index_digest": "sha256:" + "3" * 64,
        "platform_manifest_digest": "sha256:" + "4" * 64,
        "image_config_digest": target_image,
    }
    document = {
        "allowlist_version": 77,
        "signature": "pending",
        "repository": repository,
        "recommended": {"linux/amd64": target_image},
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


def reset_state(
    path,
    old_image,
    target_image,
    *,
    target_present,
    fail_readiness=False,
    inspect_config=None,
    inspect_platform="linux/amd64",
    inspect_repository="ghcr.io/blinklabs-io/cardano-node",
    inspect_manifest=None,
    pull_fail=False,
):
    path.write_text(
        json.dumps(
            {
                "target": target_image,
                "images": [target_image] if target_present else [],
                "inspect_config": inspect_config or target_image,
                "inspect_platform": inspect_platform,
                "inspect_repository": inspect_repository,
                "inspect_manifest": inspect_manifest or "sha256:" + "4" * 64,
                "pull_fail": pull_fail,
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
        " reference=args[-1]\n"
        " if target not in state['images']:\n"
        "  print('No such image',file=sys.stderr); raise SystemExit(1)\n"
        " if '{{json .}}' in args:\n"
        "  os_name,arch=state['inspect_platform'].split('/',1)\n"
        "  manifest=state['inspect_manifest']\n"
        "  print(json.dumps({'Id':state['inspect_config'],'RepoDigests':[state['inspect_repository']+'@'+manifest],'Os':os_name,'Architecture':arch},separators=(',',':'))); raise SystemExit(0)\n"
        " print(target); raise SystemExit(0)\n"
        "if args and args[0]=='pull':\n"
        " if state['pull_fail']: print('registry unavailable',file=sys.stderr); raise SystemExit(1)\n"
        " state['images']=sorted(set(state['images']+[target])); save(); print('Pulled'); raise SystemExit(0)\n"
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


def plan(home, env, fakebin, operation, target_image):
    params = ["--param", "machine=bp1"]
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
        "OURO_RELEASES_FILE": str(ROOT / "data/releases.json"),
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
        target_image = "sha256:" + "b" * 64
        assert target_image != old_image
        policy = home / "allowlist.json"
        write_allowlist(policy, old_image, target_image, True)
        state, probe, fakebin = write_sealed_runtime(home, old_image, target_image)
        docker_log = home / "docker.log"
        env = {
            "OURO_PROBE_LIB": str(probe),
            "OURO_RELEASES_FILE": str(policy),
            "OURO_ALLOWLIST_TEST_KEY": TEST_KEY,
            "OURO_TEST_BASE_OBSERVATION": str(home / "base-observation.json"),
            "OURO_TEST_RUNTIME_STATE": str(state),
            "OURO_TEST_DOCKER_LOG": str(docker_log),
            "OURO_READINESS_SAMPLE_DELAY": "0",
        }

        # upgrade/step is direct-run only. Compose and unsupported ownership refuse during both
        # plan and apply before even an image-store lookup, let alone docker rename/run.
        reset_state(state, old_image, target_image, target_present=True)
        base_path = Path(env["OURO_TEST_BASE_OBSERVATION"])
        direct_run_observation = json.loads(base_path.read_text())
        for orchestration, reason_code in [
            ("compose", "manual_compose_required"),
            ("unsupported", "unsupported_orchestration"),
        ]:
            routed = copy.deepcopy(direct_run_observation)
            routed["supervisor"]["orchestration"] = orchestration
            if orchestration == "compose":
                routed["supervisor"]["compose"] = {
                    "project": "cardano",
                    "service": "cardano-node",
                    "working_dir": "/opt/cardano",
                    "config_files": ["/opt/cardano/compose.yaml"],
                    "config_hash": "cfg-hash",
                }
            else:
                routed["supervisor"]["orchestration_reason"] = (
                    "unsupported_orchestration:portainer"
                )
            base_path.write_text(json.dumps(routed, separators=(",", ":")))
            docker_log.unlink(missing_ok=True)
            routed_plan, routed_plan_value = invoke(
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
            assert routed_plan.returncode != 0
            assert reason_code in json.dumps(routed_plan_value), routed_plan_value
            routed_apply, routed_apply_value = invoke(
                home,
                *apply_args(
                    "upgrade/step",
                    "0" * 64,
                    "--param",
                    "machine=bp1",
                    "--param",
                    f"image={target_image}",
                ),
                env_extra=env,
                path=fakebin,
            )
            assert routed_apply.returncode != 0
            assert reason_code in json.dumps(routed_apply_value), routed_apply_value
            assert not docker_log.exists(), "routing refusal must precede every docker command"
        base_path.write_text(json.dumps(direct_run_observation, separators=(",", ":")))

        # Read-only planning exposes the signed pull tuple and never contacts the registry.
        reset_state(state, old_image, target_image, target_present=False)
        preload_plan = plan(home, env, fakebin, "upgrade/preload-image", target_image)
        preload_candidate = preload_plan["data"]["candidate_hash"]
        runtime_policy = preload_plan["data"]["runtime_policy"]
        assert runtime_policy["target_pull_reference"] == (
            "ghcr.io/blinklabs-io/cardano-node@sha256:" + "4" * 64
        )
        assert runtime_policy["target_platform"] == "linux/amd64"
        assert runtime_policy["target_image_config_digest"] == target_image
        assert preload_plan["data"]["executor_plan"] == [[
            "docker",
            "pull",
            "--platform",
            "linux/amd64",
            runtime_policy["target_pull_reference"],
        ]]
        assert not docker_log.exists(), "plan must not pull or inspect the target image store"

        preload_args = apply_args(
            "upgrade/preload-image",
            preload_candidate,
            "--param",
            "machine=bp1",
            "--param",
            f"image={target_image}",
        )
        applied, applied_value = invoke(home, *preload_args, env_extra=env, path=fakebin)
        assert applied.returncode == 0, (applied, applied_value)
        postcondition = applied_value["data"]["live_postcondition"]
        assert postcondition["verification"] == "signed_exact_oci_tuple_present"
        assert postcondition["pulled_image"] == {
            "reference": runtime_policy["target_pull_reference"],
            "repository": "ghcr.io/blinklabs-io/cardano-node",
            "platform": "linux/amd64",
            "platform_manifest_digest": "sha256:" + "4" * 64,
            "image_config_digest": target_image,
        }
        assert postcondition["running_container_unchanged"]["id"] == "cid-old"
        assert postcondition["running_container_unchanged"]["creation_epoch"] == 1234
        assert postcondition["running_container_unchanged"]["image_config_digest"] == old_image
        assert postcondition["running_container_unchanged"]["readiness"] == (
            "passed_before_and_after_pull"
        )
        commands = mutation_commands(docker_log)
        assert sum(command and command[0] == "pull" for command in commands) == 1
        assert commands[0] == [
            "pull",
            "--platform",
            "linux/amd64",
            runtime_policy["target_pull_reference"],
        ]
        assert not any(command and command[0] in {"rename", "run", "restart"} for command in commands)
        assert json.loads(state.read_text())["current"]["id"] == "cid-old"

        # Pull and post-pull tuple failures refuse without touching the active container.
        failure_cases = [
            ({"pull_fail": True}, "exited"),
            ({"inspect_config": "sha256:" + "c" * 64}, "config digest mismatch"),
            ({"inspect_platform": "linux/arm64"}, "platform mismatch"),
            ({"inspect_repository": "docker.io/untrusted/cardano-node"}, "not bound"),
            ({"inspect_manifest": "sha256:" + "9" * 64}, "not bound"),
        ]
        for index, (state_options, expected_error) in enumerate(failure_cases):
            docker_log.unlink(missing_ok=True)
            reset_state(
                state,
                old_image,
                target_image,
                target_present=False,
                **state_options,
            )
            failed, failed_value = invoke(home, *preload_args, env_extra=env, path=fakebin)
            assert failed.returncode != 0, (index, failed_value)
            assert expected_error in json.dumps(failed_value), (index, failed_value)
            failed_state = json.loads(state.read_text())
            assert failed_state["current"] == {"id": "cid-old", "image": old_image, "epoch": 1234}
            assert not any(
                command and command[0] in {"rename", "run", "restart"}
                for command in mutation_commands(docker_log)
            )

        # Tags and an alternate signed repository are rejected before any pull.
        docker_log.unlink(missing_ok=True)
        tagged, tagged_value = invoke(
            home,
            *target_args(
                "upgrade/preload-image",
                "--param",
                "machine=bp1",
                "--param",
                "image=10.5.4-1",
            ),
            env_extra=env,
            path=fakebin,
        )
        assert tagged.returncode != 0, tagged_value
        assert not docker_log.exists()

        alternate_policy = home / "alternate-repository.json"
        write_allowlist(
            alternate_policy,
            old_image,
            target_image,
            True,
            repository="docker.io/untrusted/cardano-node",
        )
        alternate, alternate_value = invoke(
            home,
            *target_args(
                "upgrade/preload-image",
                "--param",
                "machine=bp1",
                "--param",
                f"image={target_image}",
            ),
            env_extra={**env, "OURO_RELEASES_FILE": str(alternate_policy)},
            path=fakebin,
        )
        assert alternate.returncode != 0 and "repository must be exactly" in json.dumps(alternate_value)
        assert not docker_log.exists()

        # A supported logging-policy drift after approval changes the candidate and refuses apply
        # before the first rename/run mutation.
        reset_state(state, old_image, target_image, target_present=True)
        drift_plan = plan(home, env, fakebin, "upgrade/step", target_image)
        drifted_observation = copy.deepcopy(direct_run_observation)
        drifted_observation["recreate"]["log_options"]["max-size"] = "100m"
        base_path.write_text(json.dumps(drifted_observation, separators=(",", ":")))
        docker_log.unlink(missing_ok=True)
        drift_apply, drift_apply_value = invoke(
            home,
            *apply_args(
                "upgrade/step",
                drift_plan["data"]["candidate_hash"],
                "--param",
                "machine=bp1",
                "--param",
                f"image={target_image}",
            ),
            env_extra=env,
            path=fakebin,
        )
        assert drift_apply.returncode != 0
        assert "approved candidate does not match current live state" in json.dumps(drift_apply_value)
        assert not any(
            command and command[0] in {"rename", "run", "rm", "start"}
            for command in mutation_commands(docker_log)
        )
        base_path.write_text(json.dumps(direct_run_observation, separators=(",", ":")))

        # Activation success: signed transition is candidate-bound and reviewable; N is retained
        # until N+1 passes readiness, then finalized. Environment values stay redacted.
        docker_log.unlink(missing_ok=True)
        reset_state(state, old_image, target_image, target_present=True)
        step_plan = plan(home, env, fakebin, "upgrade/step", target_image)
        repeated = plan(home, env, fakebin, "upgrade/step", target_image)
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
        assert success_value["data"]["live_postcondition"]["recreate_spec"] == (
            "matched_approved_supported_fields"
        )
        success_state = json.loads(state.read_text())
        assert success_state["current"]["image"] == target_image and success_state["previous"] is None
        commands = mutation_commands(docker_log)
        assert sum(command and command[0] == "run" for command in commands) == 1
        run_command = next(command for command in commands if command and command[0] == "run")
        assert run_command[run_command.index("--log-driver") + 1] == "json-file", run_command
        assert {
            run_command[index + 1]
            for index, token in enumerate(run_command[:-1])
            if token == "--log-opt"
        } == {"max-file=3", "max-size=50m"}, run_command
        assert ["rm", "-f", "cardano-node.ouro-prev"] in commands

        # Backward-compatible failure restores and verifies N.
        docker_log.unlink(missing_ok=True)
        reset_state(state, old_image, target_image, target_present=True, fail_readiness=True)
        rollback_plan = plan(home, env, fakebin, "upgrade/step", target_image)
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
        docker_log.unlink(missing_ok=True)
        reset_state(state, old_image, target_image, target_present=True, fail_readiness=True)
        forward_plan = plan(home, env, fakebin, "upgrade/step", target_image)
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

        # Every trusted historical amd64 release upgrades directly to the signed recommendation.
        # Exact transition metadata is optional and only enables automatic rollback.
        production = json.loads((ROOT / "data/releases.json").read_text())
        assert production["allowlist_version"] == 6
        assert len(production["contracts"]) == 1
        assert production["contracts"][0]["convention_version"] == 1
        transitions = production["transitions"]
        recommended = production["recommended"]["linux/amd64"]
        amd64_images = [
            image
            for contract in production["contracts"]
            for image in contract["allowed"]
            if image["platform"] == "linux/amd64"
        ]
        historical = [
            image["image_config_digest"]
            for image in amd64_images
            if image["image_config_digest"] != recommended
        ]
        assert len(historical) == 4
        for index, current in enumerate(historical):
            completed, value = production_step_plan(
                home / f"production-edge-{index}",
                current,
                recommended,
            )
            assert completed.returncode == 0, (completed, value)
            direct = next(
                (
                    transition
                    for transition in transitions
                    if transition["from_image_config_digest"] == current
                    and transition["to_image_config_digest"] == recommended
                ),
                None,
            )
            assert value["data"]["upgrade_transition"] == direct
            assert value["data"]["upgrade_failure_outcome"] == (
                "verified_rollback_to_N"
                if direct and direct["db_backward_compatible"]
                else "forward_recovery_or_resync_required"
            )
            assert (value["data"]["rollback_executor_plan"] is not None) == bool(
                direct and direct["db_backward_compatible"]
            )
            assert value["data"]["runtime_policy"]["contract_id"] == "blinklabs-cardano-node-v1"

        nonrecommended, nonrecommended_value = production_step_plan(
            home / "production-nonrecommended",
            historical[0],
            historical[1],
        )
        assert nonrecommended.returncode != 0
        assert "is not the signed recommended image" in json.dumps(nonrecommended_value)

        reversed_result, reversed_value = production_step_plan(
            home / "production-reverse", recommended, historical[-1]
        )
        assert reversed_result.returncode != 0
        assert "already the signed recommended release" in json.dumps(reversed_value)

    print("S0020 single-prompt Upgrade sealed workflow passed")


if __name__ == "__main__":
    main()
