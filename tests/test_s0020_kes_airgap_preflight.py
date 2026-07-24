#!/usr/bin/env python3
"""S0020 p4-12 — executable mock air-gap ceremony and no-write target opcert preflight."""

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from test_s0020_stateless_plan import BIN, GENESIS, ROOT, invoke, observation, target_args, write_probe


KES_VKEY = {
    "type": "KesVerificationKey_ed25519_kes_2^6",
    "description": "S0020 disposable mock KES vkey",
    "cborHex": "582065666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f8081828384",
}
ACTIVE_KES_VKEY = {
    "type": "KesVerificationKey_ed25519_kes_2^6",
    "description": "S0020 current active KES vkey",
    "cborHex": "5820" + "11" * 32,
}
OPCERT = {
    "type": "NodeOperationalCertificate",
    "description": "S0020 disposable mock opcert",
    "cborHex": (
        "8284582065666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f8081828384"
        "07186458404a518ec54511b1cb23bcff75e3100383e1c88fd479773df7b7759b278e4f1e2b"
        "b960cabb31e72103625072d4e6ebbc8316a84e8fa6445c6a0eaac0c822a942095820ea4a6c"
        "63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"
    ),
}
ACTIVE_OPCERT = {
    "type": "NodeOperationalCertificate",
    "description": "S0025 current active opcert",
    "cborHex": (
        "82845820111111111111111111111111111111111111111111111111111111111111111106185a"
        "5840fd2e75c8a0b121ced1de2cd58f4272e90a79b082cc970afd968e3c47d3e93bed192571dd"
        "6b925eb2b7292dc0aab6f296aa3aab003755ab2a9cdfb079cf0ae20b5820ea4a6c63e29c520a"
        "bef5507b132ec5f9954776aebebe7b92421eea691446d22c"
    ),
}
WRONG_COLD_ACTIVE_OPCERT = {
    "type": "NodeOperationalCertificate",
    "description": "S0025 wrong cold identity fixture",
    "cborHex": (
        "82845820111111111111111111111111111111111111111111111111111111111111111106185a"
        "584020e33d310d381dc9b8064503c00d9bdb2b188e0871339409f63c6fb83e8c1263f6a220ad"
        "2cc4f146f4d0e282aa18787126e56f9a4391af895939150a98bd230d58201398f62c6d1a457c"
        "51ba6a4b5f3dbd2f69fca93216218dc8997e416bd17d93ca"
    ),
}


def main():
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0020-kes-airgap-"))
    vkey = home / "kes.vkey"
    vkey.write_text(json.dumps(KES_VKEY, separators=(",", ":")))
    expected_opcert = json.dumps(OPCERT, separators=(",", ":"))

    # Phase A: the generated Ouro script is executable in an isolated mock air-gap. The disposable
    # cardano-cli double records the exact fixed command, advances the in-place counter and emits a
    # protocol-valid, cold-key-signed public opcert fixture.
    mock_cli_log = home / "mock-cardano-cli.log"
    mock_cli = home / "cardano-cli"
    mock_cli.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "printf '%s\\n' \"$*\" >>\"$OURO_TEST_MOCK_CLI_LOG\"\n"
        "test \"${1:-} ${2:-}\" = 'node issue-op-cert'\n"
        "counter= out=\n"
        "while test $# -gt 0; do\n"
        "  case \"$1\" in\n"
        "    --operational-certificate-issue-counter-file) counter=$2; shift 2 ;;\n"
        "    --out-file) out=$2; shift 2 ;;\n"
        "    *) shift ;;\n"
        "  esac\n"
        "done\n"
        "test -n \"$counter\" && test -n \"$out\"\n"
        "printf '7\\n' >\"$counter\"\n"
        "printf '%s\\n' \"$OURO_TEST_OPCERT\" >\"$out\"\n"
    )
    mock_cli.chmod(0o700)
    generated = subprocess.run(
        [
            str(BIN),
            "kes",
            "cold-sign-script",
            "--kes-vkey",
            str(vkey),
            "--kes-period",
            "100",
            "--cardano-cli",
            str(mock_cli),
        ],
        text=True,
        capture_output=True,
        check=True,
    )
    script = home / "cold-sign.sh"
    script.write_text(generated.stdout)
    script.chmod(0o700)
    digest = hashlib.sha256(script.read_bytes()).hexdigest()
    assert generated.stderr.strip() == f"sha256={digest}"
    assert expected_opcert not in generated.stdout
    assert "cold.skey" in generated.stdout and "opcert.counter" in generated.stdout

    cold_key = home / "cold.skey"
    counter = home / "opcert.counter"
    output = home / "node.cert"
    cold_key.write_text("DISPOSABLE-MOCK-COLD-KEY\n")
    counter.write_text("6\n")
    ran = subprocess.run(
        [str(script)],
        cwd=home,
        env={
            **os.environ,
            "COLD_SKEY": str(cold_key),
            "COUNTER": str(counter),
            "OUT": str(output),
            "OURO_TEST_MOCK_CLI_LOG": str(mock_cli_log),
            "OURO_TEST_OPCERT": expected_opcert,
        },
        text=True,
        capture_output=True,
    )
    assert ran.returncode == 0, ran
    assert counter.read_text() == "7\n"
    assert (home / "opcert.counter.ouro-bak").read_text() == "6\n"
    assert json.loads(output.read_text()) == OPCERT
    issued = mock_cli_log.read_text()
    assert issued.count("node issue-op-cert") == 1
    assert "--kes-period 100" in issued

    # Phase B: preview/plan/preflight use the exact returned bytes and production deep validator,
    # but expose no executor and perform no backup/copy/restart.
    opcert_bytes = output.read_bytes()
    opcert_digest = hashlib.sha256(opcert_bytes).hexdigest()
    artifact_ref = f"opcert-{opcert_digest[:8]}@sha256:{opcert_digest}"
    preview = subprocess.run(
        [str(BIN), "inbox", "preview", "--type", "opcert", "--file", str(output)],
        text=True,
        capture_output=True,
    )
    assert preview.returncode == 0, preview
    assert json.loads(preview.stdout)["data"]["artifact_ref"] == artifact_ref

    probe = home / "probe.sh"
    active_opcert_bytes = json.dumps(ACTIVE_OPCERT, separators=(",", ":")).encode()
    base_observation = observation()
    base_observation["live"]["kes_opcert_id"] = hashlib.sha256(active_opcert_bytes).hexdigest()
    write_probe(probe, base_observation)
    fakebin = home / "fakebin"
    fakebin.mkdir()
    docker_log = home / "docker.log"
    captured = home / "validated-opcert"
    docker = fakebin / "docker"
    docker.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "printf '%s\\n' \"$*\" >>\"$OURO_TEST_DOCKER_LOG\"\n"
        "case \"$*\" in\n"
        "  *'exec cid-plan cardano-cli --version'*) printf 'cardano-cli 10.14.0.0 - linux-x86_64 - ghc-9.6\\n' ;;\n"
        "  *'.ouro-kes-stage/kes.vkey'*) printf '%s\\n' \"$OURO_TEST_KES_VKEY\" ;;\n"
        "  *'head -c 65537 /opt/cardano/config/keys/kes.vkey'*) printf '%s\\n' \"$OURO_TEST_ACTIVE_KES_VKEY\" ;;\n"
        "  *'head -c 65537 /opt/cardano/config/keys/node.cert'*) printf '%s' \"$OURO_TEST_ACTIVE_OPCERT\" ;;\n"
        "  *'.ouro-kes-stage/kes.skey'*)\n"
        "    if [[ \"$*\" == *'stat -c %a'* ]]; then printf '600\\n'; else exit 0; fi ;;\n"
        "  *kes-period-info*)\n"
        "    cat >\"$OURO_TEST_CAPTURED_OPCERT\"\n"
        "    printf '%s\\n' \"$OURO_TEST_KES_INFO\" ;;\n"
        "  *) exit 90 ;;\n"
        "esac\n"
    )
    docker.chmod(0o700)
    env = {
        "OURO_PROBE_LIB": str(probe),
        "OURO_EPHEMERAL_PAYLOAD": str(output),
        "OURO_TEST_DOCKER_LOG": str(docker_log),
        "OURO_TEST_CAPTURED_OPCERT": str(captured),
        "OURO_TEST_KES_VKEY": json.dumps(KES_VKEY, separators=(",", ":")),
        "OURO_TEST_ACTIVE_KES_VKEY": json.dumps(ACTIVE_KES_VKEY, separators=(",", ":")),
        "OURO_TEST_ACTIVE_OPCERT": active_opcert_bytes.decode(),
        "OURO_TEST_KES_INFO": json.dumps(
            {
                "qKesCurrentKesPeriod": 100,
                "qKesStartKesInterval": 100,
                "qKesEndKesInterval": 162,
                "qKesOnDiskOperationalCertificateNumber": 7,
                "qKesNodeStateOperationalCertificateNumber": 7,
            },
            separators=(",", ":"),
        ),
    }
    plan, plan_value = invoke(
        home,
        *target_args(
            "kes-rotation/install-opcert",
            "--param",
            "machine=bp1",
            "--param",
            f"opcert={artifact_ref}",
        ),
        env_extra=env,
        path=fakebin,
    )
    assert plan.returncode == 0, (plan, plan_value)
    candidate = plan_value["data"]["candidate_hash"]
    preflight_args = list(
        target_args(
            "kes-rotation/install-opcert",
            "--param",
            "machine=bp1",
            "--param",
            f"opcert={artifact_ref}",
        )
    )
    preflight_args[1] = "preflight"
    preflight_args.extend(("--candidate-hash", candidate))
    preflight, value = invoke(home, *preflight_args, env_extra=env, path=fakebin)
    assert preflight.returncode == 0, (preflight, value)
    assert value["tool"] == "ouro.op.artifact_preflight" and value["changed"] is False
    assert value["data"]["candidate_hash"] == candidate
    assert value["data"]["artifact_ref"] == artifact_ref
    assert value["data"]["validation"]["cold_key_signature"] == "valid"
    assert value["data"]["validation"]["hot_kes_key_matches_target"] is True
    assert value["data"]["validation"]["counter"] == 7
    assert value["data"]["validation"]["kes_period"] == 100
    assert value["data"]["validation"]["node_state_counter"] == 7
    assert value["data"]["validation"]["node_state_counter_status"] == "present"
    assert value["data"]["validation"]["active_opcert_counter"] is None
    assert value["data"]["executor_available"] is False
    assert value["data"]["confirmation_consumed"] is False
    assert value["data"]["fleet_permit_consumed"] is False
    assert captured.read_bytes() == opcert_bytes
    remote_calls = docker_log.read_text()
    assert "kes.vkey" in remote_calls and "kes-period-info" in remote_calls
    assert " restart " not in f" {remote_calls} " and " cp " not in f" {remote_calls} "

    # The same production gate refuses concrete invalid scenes: another hot KES key, a stale live
    # window/counter, and a changed cold signature. None can progress to an executor.
    wrong_vkey = dict(KES_VKEY)
    wrong_vkey["cborHex"] = "5820" + "00" * 32
    wrong_key, wrong_key_value = invoke(
        home,
        *preflight_args,
        env_extra={**env, "OURO_TEST_KES_VKEY": json.dumps(wrong_vkey)},
        path=fakebin,
    )
    assert wrong_key.returncode != 0
    assert "preflight candidate does not match current live state" in json.dumps(wrong_key_value)

    stale_info = json.loads(env["OURO_TEST_KES_INFO"])
    stale_info["qKesCurrentKesPeriod"] = 163
    stale, stale_value = invoke(
        home,
        *preflight_args,
        env_extra={**env, "OURO_TEST_KES_INFO": json.dumps(stale_info)},
        path=fakebin,
    )
    assert stale.returncode != 0
    assert "stale/out-of-period/inconsistent" in json.dumps(stale_value)

    tampered_opcert = json.loads(output.read_text())
    signature_offset = len("8284") + len("5820") + 64 + len("07") + len("1864") + len("5840")
    cbor = tampered_opcert["cborHex"]
    tampered_opcert["cborHex"] = (
        cbor[:signature_offset]
        + ("0" if cbor[signature_offset] != "0" else "1")
        + cbor[signature_offset + 1 :]
    )
    tampered = home / "tampered-node.cert"
    tampered.write_text(json.dumps(tampered_opcert, separators=(",", ":")))
    tampered_digest = hashlib.sha256(tampered.read_bytes()).hexdigest()
    tampered_ref = f"opcert-{tampered_digest[:8]}@sha256:{tampered_digest}"
    tampered_plan, tampered_plan_value = invoke(
        home,
        *target_args(
            "kes-rotation/install-opcert",
            "--param",
            "machine=bp1",
            "--param",
            f"opcert={tampered_ref}",
        ),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(tampered)},
        path=fakebin,
    )
    assert tampered_plan.returncode == 0, (tampered_plan, tampered_plan_value)
    tampered_args = list(
        target_args(
            "kes-rotation/install-opcert",
            "--param",
            "machine=bp1",
            "--param",
            f"opcert={tampered_ref}",
        )
    )
    tampered_args[1] = "preflight"
    tampered_args.extend(("--candidate-hash", tampered_plan_value["data"]["candidate_hash"]))
    bad_signature, bad_signature_value = invoke(
        home,
        *tampered_args,
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(tampered)},
        path=fakebin,
    )
    assert bad_signature.returncode != 0
    assert "cold-key signature is invalid" in json.dumps(bad_signature_value)

    def run_node_state_case(kes_info, active_opcert):
        active_bytes = json.dumps(active_opcert, separators=(",", ":")).encode()
        observed = observation()
        observed["live"]["kes_opcert_id"] = hashlib.sha256(active_bytes).hexdigest()
        write_probe(probe, observed)
        case_env = {
            **env,
            "OURO_TEST_KES_INFO": json.dumps(kes_info, separators=(",", ":")),
            "OURO_TEST_ACTIVE_OPCERT": active_bytes.decode(),
        }
        case_plan, case_plan_value = invoke(
            home,
            *target_args(
                "kes-rotation/install-opcert",
                "--param", "machine=bp1",
                "--param", f"opcert={artifact_ref}",
            ),
            env_extra=case_env,
            path=fakebin,
        )
        assert case_plan.returncode == 0, (case_plan, case_plan_value)
        case_args = list(preflight_args)
        case_args[-1] = case_plan_value["data"]["candidate_hash"]
        return invoke(home, *case_args, env_extra=case_env, path=fakebin)

    null_info = json.loads(env["OURO_TEST_KES_INFO"])
    null_info["qKesNodeStateOperationalCertificateNumber"] = None
    no_blocks, no_blocks_value = run_node_state_case(null_info, ACTIVE_OPCERT)
    assert no_blocks.returncode == 0, (no_blocks, no_blocks_value)
    null_validation = no_blocks_value["data"]["validation"]
    assert null_validation["node_state_counter"] is None
    assert null_validation["node_state_counter_status"] == "no_blocks_minted_yet"
    assert null_validation["active_opcert_counter"] == 6
    assert null_validation["cold_identity_bound"] is True
    assert no_blocks_value["changed"] is False
    assert no_blocks_value["data"]["executor_available"] is False
    assert no_blocks_value["data"]["confirmation_consumed"] is False
    assert no_blocks_value["data"]["fleet_permit_consumed"] is False

    absent_info = dict(null_info)
    absent_info.pop("qKesNodeStateOperationalCertificateNumber")
    absent, absent_value = run_node_state_case(absent_info, ACTIVE_OPCERT)
    assert absent.returncode != 0
    assert "schema incompatible" in json.dumps(absent_value)

    malformed_info = dict(null_info)
    malformed_info["qKesNodeStateOperationalCertificateNumber"] = "none"
    malformed, malformed_value = run_node_state_case(malformed_info, ACTIVE_OPCERT)
    assert malformed.returncode != 0
    assert "must be an unsigned integer or null" in json.dumps(malformed_value)

    wrong_cold, wrong_cold_value = run_node_state_case(null_info, WRONG_COLD_ACTIVE_OPCERT)
    assert wrong_cold.returncode != 0
    assert "cold key does not match" in json.dumps(wrong_cold_value)

    equal_counter_active = {**OPCERT, "description": "active equal-counter fixture"}
    replay, replay_value = run_node_state_case(null_info, equal_counter_active)
    assert replay.returncode != 0
    assert "must be greater than verified active opcert counter" in json.dumps(replay_value)

    invalid_active = json.loads(json.dumps(ACTIVE_OPCERT))
    invalid_cbor = invalid_active["cborHex"]
    signature_offset = len("8284") + len("5820") + 64 + len("06") + len("185a") + len("5840")
    invalid_active["cborHex"] = (
        invalid_cbor[:signature_offset]
        + ("0" if invalid_cbor[signature_offset] != "0" else "1")
        + invalid_cbor[signature_offset + 1:]
    )
    invalid, invalid_value = run_node_state_case(null_info, invalid_active)
    assert invalid.returncode != 0
    assert "cannot establish cold identity" in json.dumps(invalid_value)

    # Subsequent transport checks use the original integer-counter plan and observation.
    write_probe(probe, base_observation)

    # The public control command also remains capability-free: it derives the BP binding from the
    # spec, sends exactly runner || public opcert, and exposes only the closed target preflight argv.
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
    (credentials / "bp1").write_text("disposable-test-key")
    (credentials / "relay1").write_text("disposable-test-key")
    (home / "known_hosts").write_text(
        "192.0.2.1 ssh-ed25519 test\n192.0.2.2 ssh-ed25519 test\n"
    )
    runner = home / "runner"
    runner_bytes = b"sealed-linux-runner-for-kes-preflight"
    runner.write_bytes(runner_bytes)
    transport_stream = home / "transport.stream"
    transport_args = home / "transport.args"
    protocol_value = {
        "tool": "ouro.kes.protocol_evidence",
        "machine": None,
        "status": "ok",
        "changed": False,
        "checks": [],
        "duration_s": 0.0,
        "audit_id": None,
        "data": {
            "artifact_ref": artifact_ref,
            "evidence": {
                "artifact_sha256": opcert_digest,
                "relay_node": "relay1",
                "current_period": 100,
                "start_period": 100,
                "end_period": 162,
                "on_disk_counter": 7,
                "node_state_counter": 7,
                "node_state_counter_status": "present",
            },
            "source": "declared_healthy_relay_socket",
            "persistent_target_state_written": False,
        },
    }
    ssh = fakebin / "ssh"
    ssh.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "if [[ \"$*\" == *\"uname -s\"* && \"$*\" == *\"uname -m\"* ]]; then printf 'Linux\\nx86_64\\n'; exit 0; fi\n"
        "dd of=\"$OURO_TEST_TRANSPORT_STREAM\" bs=65536 status=none\n"
        "printf '%s' \"$*\" >\"$OURO_TEST_TRANSPORT_ARGS\"\n"
        "case \"$*\" in\n"
        f"  *cardano@192.0.2.2*kes-protocol*) printf '%s\\n' '{json.dumps(protocol_value, separators=(',', ':'))}' ;;\n"
        f"  *) printf '%s\\n' '{json.dumps(value, separators=(',', ':'))}' ;;\n"
        "esac\n"
    )
    ssh.chmod(0o700)
    dispatched, dispatched_value = invoke(
        home,
        "op",
        "run",
        "--op",
        "kes-rotation/install-opcert",
        "--spec",
        str(pool_spec),
        "--dispatch",
        "192.0.2.1",
        "--ssh-key",
        "creds://bp1",
        "--node",
        "bp1",
        "--param",
        "machine=bp1",
        "--param",
        f"opcert={artifact_ref}",
        "--candidate-hash",
        candidate,
        "--artifact-file",
        str(output),
        "--artifact-preflight",
        env_extra={
            "OURO_EPHEMERAL_RUNNER": str(runner),
            "OURO_TEST_TRANSPORT_STREAM": str(transport_stream),
            "OURO_TEST_TRANSPORT_ARGS": str(transport_args),
        },
        path=fakebin,
    )
    assert dispatched.returncode == 0, (dispatched, dispatched_value)
    assert dispatched_value["tool"] == "ouro.op.artifact_preflight"
    assert transport_stream.read_bytes() == runner_bytes + opcert_bytes
    remote_argv = transport_args.read_text()
    for expected in [
        "cardano@192.0.2.1",
        "'target' 'preflight'",
        f"'--candidate-hash' '{candidate}'",
        f"'--param' 'opcert={artifact_ref}'",
        'OURO_EPHEMERAL_PAYLOAD="$payload"',
    ]:
        assert expected in remote_argv, (expected, remote_argv)
    for forbidden in ["'target' 'apply'", "--confirm-token", "--fleet-permit"]:
        assert forbidden not in remote_argv, (forbidden, remote_argv)

    swapped = home / "swapped-node.cert"
    swapped.write_bytes(opcert_bytes + b"\n")
    stream_before_swap = transport_stream.read_bytes()
    swapped_result, swapped_value = invoke(
        home,
        "op",
        "run",
        "--op",
        "kes-rotation/install-opcert",
        "--spec",
        str(pool_spec),
        "--dispatch",
        "192.0.2.1",
        "--ssh-key",
        "creds://bp1",
        "--node",
        "bp1",
        "--param",
        "machine=bp1",
        "--param",
        f"opcert={artifact_ref}",
        "--candidate-hash",
        candidate,
        "--artifact-file",
        str(swapped),
        "--artifact-preflight",
        env_extra={
            "OURO_EPHEMERAL_RUNNER": str(runner),
            "OURO_TEST_TRANSPORT_STREAM": str(transport_stream),
            "OURO_TEST_TRANSPORT_ARGS": str(transport_args),
        },
        path=fakebin,
    )
    assert swapped_result.returncode != 0
    assert "bytes do not match" in json.dumps(swapped_value)
    assert transport_stream.read_bytes() == stream_before_swap

    shutil.rmtree(home)
    assert not home.exists()
    print("S0020 KES mock air-gap and no-write artifact preflight passed")


if __name__ == "__main__":
    main()
