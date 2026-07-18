#!/usr/bin/env python3
"""S0021 p3-1 — Deploy is a reviewed one-shot submit, not a resident target workflow."""

import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path

from test_s0020_stateless_plan import BIN, ROOT, invoke, observation, target_args, write_probe


POLICY = {
    "ouro_pool_namespace": "pool-0123456789abcdef01234567",
    "ticker": "TSTX",
    "metadata_url": "https://pool.example.com/metadata.json",
    "pledge_lovelace": 100_000_000_000,
    "margin": 0.02,
    "cost_lovelace": 340_000_000,
}


def transaction_view(extra_effect=False, expired=False):
    return {
        "era": "Conway",
        "certificates": [
            {
                "Pool registration": {
                    "pool params": {
                        "cost": POLICY["cost_lovelace"],
                        "margin": POLICY["margin"],
                        "metadata": {
                            "hash": "ab" * 32,
                            "url": POLICY["metadata_url"],
                        },
                        "owners": ["cd" * 28],
                        "pledge": POLICY["pledge_lovelace"],
                        "publicKey": "12" * 28,
                        "relays": [],
                        "rewardAccount": {"network": "Mainnet", "credential": "34" * 28},
                        "vrf": "56" * 32,
                    }
                }
            }
        ],
        "inputs": [{"txId": "00" * 32, "index": 0}],
        "outputs": [
            {
                "address": "addr1qmock",
                "amount": {"lovelace": 999_000_000},
                "network": "Mainnet",
            }
        ],
        "fee": 200_000,
        "validity range": {"lower bound": 0, "upper bound": 5 if expired else 100_000},
        "witnesses": [{"key": "payment"}, {"key": "pool-cold"}],
        "required signers (payment key hashes needed for scripts)": [],
        "metadata": None,
        "mint": {"forbidden": 1} if extra_effect else {},
    }


def deploy_args(candidate=None):
    args = list(
        target_args(
            "deploy/register-submit",
            "--param",
            "machine=bp1",
            "--param",
            f"tx={ARTIFACT_REF}",
            "--param",
            "network=mainnet",
            "--registration-policy",
            json.dumps(POLICY, separators=(",", ":")),
        )
    )
    if candidate is not None:
        args[1] = "apply"
        args.extend(("--approved-candidate", candidate))
    return args


def run_deploy(home, probe, fakebin, mode, candidate=None, view=None):
    env = {
        "OURO_PROBE_LIB": str(probe),
        "OURO_EPHEMERAL_PAYLOAD": str(TX),
        "OURO_TEST_DOCKER_LOG": str(home / "docker.log"),
        "OURO_TEST_SUBMIT_MODE": mode,
        "OURO_TEST_TX_VIEW": json.dumps(view or transaction_view(), separators=(",", ":")),
    }
    return invoke(home, *deploy_args(candidate), env_extra=env, path=fakebin)


subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
WORK = Path(tempfile.mkdtemp(prefix="ouro-s0021-deploy-"))
TX = WORK / "mock.signed"
TX.write_text('{"type":"Tx ConwayEra","description":"sealed mock","cborHex":"aa"}')
TX_BYTES = TX.read_bytes()
TX_DIGEST = hashlib.sha256(TX_BYTES).hexdigest()
ARTIFACT_REF = f"tx-{TX_DIGEST[:8]}@sha256:{TX_DIGEST}"


def main():
    home = WORK / "home"
    home.mkdir()
    probe = home / "probe.sh"
    write_probe(probe, observation())
    fakebin = home / "fakebin"
    fakebin.mkdir()
    docker = fakebin / "docker"
    docker.write_text(
        "#!/usr/bin/env bash\n"
        "set -eu\n"
        "payload=$(mktemp)\n"
        "trap 'rm -f \"$payload\"' EXIT\n"
        "cat >\"$payload\"\n"
        "digest=$(sha256sum \"$payload\" | cut -d' ' -f1)\n"
        "case \" $* \" in\n"
        "  *' debug transaction view '*) printf '%s\\n' \"$OURO_TEST_TX_VIEW\" ;;\n"
        "  *' conway query utxo '*) printf '{}\\n' ;;\n"
        "  *' conway transaction txid '*) printf '%064d\\n' 7 ;;\n"
        "  *' conway transaction submit '*)\n"
        "    printf 'submit %s %s\\n' \"$digest\" \"$*\" >>\"$OURO_TEST_DOCKER_LOG\"\n"
        "    case \"$OURO_TEST_SUBMIT_MODE\" in\n"
        "      reject) printf 'BadInputsUTxO (guaranteed-invalid fixture)\\n' >&2; exit 1 ;;\n"
        "      accept) exit 0 ;;\n"
        "      ambiguous) kill -TERM $$ ;;\n"
        "      *) printf 'bad test mode\\n' >&2; exit 2 ;;\n"
        "    esac ;;\n"
        "  *) printf 'unexpected docker argv: %s\\n' \"$*\" >&2; exit 2 ;;\n"
        "esac\n"
    )
    docker.chmod(0o700)

    # Plan is stable, reviews exact bytes on the target, and does not submit or use a target file.
    plan, value = run_deploy(home, probe, fakebin, "reject")
    assert plan.returncode == 0, (plan, value)
    data = value["data"]
    candidate = data["candidate_hash"]
    assert data["fleet_permit_required"] is False
    assert data["confirmation_required"] is True
    assert data["persistent_target_state_written"] is False
    assert data["deploy_transaction"]["artifact_ref"] == ARTIFACT_REF
    assert data["deploy_transaction"]["txid"] == "0" * 63 + "7"
    assert data["deploy_transaction"]["stake_pool_key_hash"] == "12" * 28
    assert data["deploy_transaction"]["additional_chain_effects"] == "none"
    assert data["deploy_transaction"]["input_utxo_evidence"] == {
        "query": "live_node_utxo_by_exact_input",
        "inputs": [{"input": "0" * 64 + "#0", "state": "absent"}],
        "presence": "all_absent",
    }
    executor = data["executor_plan"]
    assert executor == [[
        "docker", "exec", "-i", "cid-plan", "cardano-cli", "conway", "transaction",
        "submit", "--tx-file", "/dev/stdin", "--socket-path", "/ipc/node.socket", "--mainnet",
    ]]
    assert not (home / "docker.log").exists(), "plan must not submit"
    repeat, repeated = run_deploy(home, probe, fakebin, "reject")
    assert repeat.returncode == 0 and repeated["data"]["candidate_hash"] == candidate
    advanced = observation()
    advanced["readiness"]["tip_slot"] = 11
    write_probe(probe, advanced)
    advanced_plan, advanced_value = run_deploy(home, probe, fakebin, "reject")
    assert advanced_plan.returncode == 0
    assert advanced_value["data"]["deploy_transaction"]["live_slot"] == 11
    assert advanced_value["data"]["candidate_hash"] == candidate, \
        "normal slot progress must not force the operator to chase a moving candidate"
    write_probe(probe, observation())

    # A changed artifact and policy/transaction mismatches refuse before the submit executor.
    original = TX.read_text()
    TX.write_text('{"type":"Tx ConwayEra","description":"changed","cborHex":"aa"}')
    changed, changed_value = run_deploy(home, probe, fakebin, "reject")
    assert changed.returncode != 0 and "artifact reference" in json.dumps(changed_value)
    assert not (home / "docker.log").exists()
    TX.write_text(original)
    for bad_view, needle in [
        (transaction_view(extra_effect=True), "additional effect"),
        (transaction_view(expired=True), "validity interval"),
    ]:
        refused, refused_value = run_deploy(home, probe, fakebin, "reject", view=bad_view)
        assert refused.returncode != 0 and needle in json.dumps(refused_value), refused_value
        assert not (home / "docker.log").exists()
    wrong_network = deploy_args()
    wrong_network[wrong_network.index("network=mainnet")] = "network=preview"
    refused, refused_value = invoke(
        home,
        *wrong_network,
        env_extra={
            "OURO_PROBE_LIB": str(probe),
            "OURO_EPHEMERAL_PAYLOAD": str(TX),
            "OURO_TEST_DOCKER_LOG": str(home / "docker.log"),
            "OURO_TEST_SUBMIT_MODE": "reject",
            "OURO_TEST_TX_VIEW": json.dumps(transaction_view(), separators=(",", ":")),
        },
        path=fakebin,
    )
    assert refused.returncode != 0 and "network parameter" in json.dumps(refused_value)
    assert not (home / "docker.log").exists()

    # One approved rejection is terminal and preserves exact transaction bytes on stdin.
    rejected, rejected_value = run_deploy(home, probe, fakebin, "reject", candidate=candidate)
    assert rejected.returncode == 10, (rejected, rejected_value)
    assert rejected_value["error"]["code"] == "submission_rejected"
    assert rejected_value["changed"] is False
    rejected_data = rejected_value["data"]
    assert rejected_data["outcome"] == "node_rejected"
    assert rejected_data["submission_attempted"] is True
    assert rejected_data["accepted_by_node"] is False
    assert rejected_data["retry_allowed"] is False
    assert rejected_data["ledger_inclusion"] == "not_inferred_from_rejection"
    submits = (home / "docker.log").read_text().splitlines()
    assert len(submits) == 1 and TX_DIGEST in submits[0]
    assert "/dev/stdin" in submits[0] and "/tmp/ouro-tx" not in submits[0]

    # Acceptance is only local-node acceptance; signal termination is explicitly ambiguous.
    for mode, expected_code, expected_outcome, expected_changed in [
        ("accept", None, "accepted_by_node", True),
        ("ambiguous", "submission_ambiguous", "submission_ambiguous", True),
    ]:
        case = WORK / mode
        case.mkdir()
        case_probe = case / "probe.sh"
        write_probe(case_probe, observation())
        case_fakebin = case / "fakebin"
        case_fakebin.mkdir()
        os.symlink(docker, case_fakebin / "docker")
        completed, terminal = run_deploy(case, case_probe, case_fakebin, mode, candidate=candidate)
        if expected_code is None:
            assert completed.returncode == 0, (completed, terminal)
            terminal_data = terminal["data"]["live_postcondition"]
            assert terminal_data["ledger_inclusion"] == "unknown"
            assert terminal_data["pool_registration"] == "unknown"
        else:
            assert completed.returncode == 20, (completed, terminal)
            assert terminal["error"]["code"] == expected_code
            terminal_data = terminal["data"]
        assert terminal["changed"] is expected_changed
        assert terminal_data["outcome"] == expected_outcome
        assert terminal_data["retry_allowed"] is False
        assert len((case / "docker.log").read_text().splitlines()) == 1

    print("S0021 Deploy one-shot workflow tests passed")


if __name__ == "__main__":
    main()
