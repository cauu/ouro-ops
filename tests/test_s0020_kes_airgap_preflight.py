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
OPCERT = {
    "type": "NodeOperationalCertificate",
    "description": "S0020 disposable mock opcert",
    "cborHex": (
        "8284582065666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f8081828384"
        "07186458401ffe2cd67931570cc03b9805fcbe0360e5e14829f3ac2e7b523e7f6f1d3fc5b1"
        "ef3b6bbc7039235cf8a2f80a06104837eb4a3687fff774d92dd1cf304e0b9e0f582079b556"
        "2e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664"
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
    write_probe(probe, observation())
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
        "  *kes.vkey*) printf '%s\\n' \"$OURO_TEST_KES_VKEY\" ;;\n"
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
    assert "does not match the target's public kes.vkey" in json.dumps(wrong_key_value)

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
    (home / "known_hosts").write_text("192.0.2.1 ssh-ed25519 test\n")
    runner = home / "runner"
    runner_bytes = b"sealed-linux-runner-for-kes-preflight"
    runner.write_bytes(runner_bytes)
    transport_stream = home / "transport.stream"
    transport_args = home / "transport.args"
    ssh = fakebin / "ssh"
    ssh.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "dd of=\"$OURO_TEST_TRANSPORT_STREAM\" bs=65536 status=none\n"
        "printf '%s' \"$*\" >\"$OURO_TEST_TRANSPORT_ARGS\"\n"
        f"printf '%s\\n' '{json.dumps(value, separators=(',', ':'))}'\n"
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
