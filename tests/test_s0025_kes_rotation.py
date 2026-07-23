#!/usr/bin/env python3
"""S0025 p6-1-fix3 — staged KES pair, public air-gap handoff, bound activation."""

import hashlib
import json
import os
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path

from test_s0020_kes_airgap_preflight import ACTIVE_KES_VKEY, ACTIVE_OPCERT, KES_VKEY, OPCERT
from test_s0020_stateless_apply import apply_args, target_fleet_permit
from test_s0020_stateless_plan import BIN, ROOT, invoke, observation, target_args


def write_dynamic_probe(path: Path, state: Path) -> None:
    base_observation = observation()
    base_observation["readiness"]["kes_opcert_valid"] = False
    base_observation["readiness"]["forging_credentials_ready"] = False
    base_observation["readiness"]["block_producer_configured"] = True
    base_observation["readiness"]["kes"]["valid"] = False
    base = json.dumps(base_observation, separators=(",", ":"))
    path.write_text(
        "ouro_observe() {\n"
        f"python3 - '{state}' <<'PY'\n"
        "import json, sys\n"
        f"obs=json.loads({base!r})\n"
        "state=json.load(open(sys.argv[1]))\n"
        "obs['live']['kes_opcert_id']=state['active_opcert']\n"
        "if state.get('restart_loop'):\n"
        "  obs['live']['container_running']=False\n"
        "  obs['live']['container_restarting']=True\n"
        "  obs['live']['container_status']='restarting'\n"
        "  obs['readiness']['node_running']=False\n"
        "  obs['readiness']['socket_answers']=False\n"
        "  obs['readiness']['tip_synced']=False\n"
        "  obs['readiness']['kes']=None\n"
        "if state['active_opcert'] != state['initial_opcert']:\n"
        "  if state.get('readiness_delay_remaining',0)>0:\n"
        "    state['readiness_delay_remaining']-=1\n"
        "    json.dump(state,open(sys.argv[1],'w'))\n"
        "    obs['readiness']['kes']=None\n"
        "    obs['readiness']['socket_answers']=False\n"
        "    print(json.dumps(obs,separators=(',',':')))\n"
        "    raise SystemExit\n"
        "  obs['readiness']['kes']={\n"
        "    'source':'cardano_cli','current_period':100,'start_period':100,\n"
        "    'end_period':162,'remaining_periods':62,'opcert_counter_on_disk':7,\n"
        "    'opcert_counter_node_state':None,'counter_consistent':None,\n"
        "    'counter_status':'no_blocks_minted_yet','period_valid':True,'valid':True}\n"
        "  if state.get('fail_post'): obs['readiness']['tip_synced']=False\n"
        "if state.get('drift_after_stage') and state['stage']: obs['live']['network']='preprod'\n"
        "print(json.dumps(obs,separators=(',',':')))\n"
        "PY\n"
        "}\n"
    )


def permit_for(candidate: str, port: int, artifact_digest: str | None = None) -> str:
    value = json.loads(target_fleet_permit(candidate, port, int(time.time()) + 30))
    value["operation_id"] = "kes-rotation/install-opcert"
    if artifact_digest is not None:
        value["kes_protocol_evidence"] = {
            "artifact_sha256": artifact_digest,
            "relay_node": "relay1",
            "current_period": 100,
            "start_period": 100,
            "end_period": 162,
            "on_disk_counter": 7,
            "node_state_counter": None,
            "node_state_counter_status": "no_blocks_minted_yet",
        }
    return json.dumps(value, separators=(",", ":"))


def main() -> None:
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-s0025-kes-rotation-"))
    state = home / "docker-state.json"
    active_opcert_bytes = json.dumps(ACTIVE_OPCERT, separators=(",", ":")).encode()
    active_opcert_digest = hashlib.sha256(active_opcert_bytes).hexdigest()
    state.write_text(json.dumps({
        "stage": False,
        "stage_complete": True,
        "active_vkey": ACTIVE_KES_VKEY,
        "staged_vkey": KES_VKEY,
        "active_opcert": active_opcert_digest,
        "initial_opcert": active_opcert_digest,
        "active_opcert_envelope": ACTIVE_OPCERT,
        "previous_vkey": None,
        "previous_opcert": None,
        "previous_opcert_envelope": None,
        "backups": False,
        "restart_count": 0,
        "drift_after_stage": True,
        "fail_post": False,
        "readiness_delay_remaining": 0,
        "fail_promotion_once": False,
        "restart_loop": False,
        "next_ready": False,
        "helper_prepare_count": 0,
        "helper_promote_count": 0,
        "helper_finalize_count": 0,
        "stop_count": 0,
        "start_count": 0,
    }))
    probe = home / "probe.sh"
    write_dynamic_probe(probe, state)
    fakebin = home / "fakebin"
    fakebin.mkdir()
    docker_log = home / "docker.log"
    docker = fakebin / "docker"
    docker.write_text(
        "#!/usr/bin/env python3\n"
        "import hashlib,json,os,sys\n"
        "p=os.environ['OURO_TEST_KES_STATE']; log=os.environ['OURO_TEST_DOCKER_LOG']\n"
        "s=json.load(open(p)); a=sys.argv[1:]; joined=' '.join(a)\n"
        "open(log,'a').write(joined+'\\n')\n"
        "stage='/opt/cardano/config/keys/.ouro-kes-stage'\n"
        "if a[:2]==['exec','cid-plan']: a=a[2:]; joined=' '.join(a)\n"
        "if a and a[0]=='run':\n"
        "  script=a[-1]\n"
        "  if 'cp -p $s/kes.skey $k/kes.skey.ouro-next' in script:\n"
        "    s['backups']=True; s['next_ready']=True; s['helper_prepare_count']+=1\n"
        "    s['previous_vkey']=s['active_vkey']; s['previous_opcert']=s['active_opcert']; s['previous_opcert_envelope']=s['active_opcert_envelope']\n"
        "    raw=open(os.environ['OURO_EPHEMERAL_PAYLOAD'],'rb').read(); s['next_opcert']=hashlib.sha256(raw).hexdigest(); s['next_opcert_envelope']=json.loads(raw)\n"
        "  elif 'mv -f $k/kes.skey.ouro-next' in script:\n"
        "    if not s['next_ready']: sys.exit(1)\n"
        "    s['active_vkey']=s['staged_vkey']; s['active_opcert']=s['next_opcert']; s['active_opcert_envelope']=s['next_opcert_envelope']; s['helper_promote_count']+=1; s['next_ready']=False\n"
        "  elif 'rm -f $k/kes.skey.ouro-prev' in script:\n"
        "    s['backups']=False; s['stage']=False; s['next_ready']=False; s['helper_finalize_count']+=1\n"
        "  json.dump(s,open(p,'w')); sys.exit(0)\n"
        "if a and a[0]=='stop': s['stop_count']+=1; json.dump(s,open(p,'w')); sys.exit(0)\n"
        "if a and a[0]=='start': s['start_count']+=1; s['restart_loop']=False; json.dump(s,open(p,'w')); sys.exit(0)\n"
        "if a==['cardano-cli','--version']:\n"
        "  print('cardano-cli 10.14.0.0 - linux-x86_64 - ghc-9.6'); sys.exit(0)\n"
        "if a[:2]==['test','!']:\n"
        "  target=a[-1]\n"
        "  exists=s['stage'] if target==stage else s['backups']\n"
        "  sys.exit(1 if exists else 0)\n"
        "if a[:2]==['test','-s']:\n"
        "  target=a[-1]\n"
        "  if target.endswith('.ouro-prev'): sys.exit(0 if s['backups'] else 1)\n"
        "  sys.exit(0 if s['stage'] and s['stage_complete'] else 1)\n"
        "if a[:3]==['stat','-c','%a']:\n"
        "  print('600'); sys.exit(0)\n"
        "if a[:3]==['head','-c','65537']:\n"
        "  if a[-1].endswith('node.cert.ouro-prev'): value=s['previous_opcert_envelope']\n"
        "  elif a[-1].endswith('node.cert'): value=s['active_opcert_envelope']\n"
        "  elif a[-1].endswith('kes.vkey.ouro-prev'): value=s['previous_vkey']\n"
        "  else: value=s['staged_vkey'] if stage in a[-1] else s['active_vkey']\n"
        "  sys.stdout.write(json.dumps(value,separators=(',',':'))); sys.exit(0)\n"
        "if a and a[0]=='mkdir': s['stage']=True\n"
        "elif 'key-gen-KES' in a: s['stage']=True; s['stage_complete']=True\n"
        "elif a and a[0] in ('chmod','mv'): pass\n"
        "elif a[:2]==['cp','-p']:\n"
        "  src,dst=a[-2:]\n"
        "  if src.endswith('.ouro-kes-stage/kes.vkey') and s.get('fail_promotion_once'):\n"
        "    s['fail_promotion_once']=False; json.dump(s,open(p,'w')); sys.exit(1)\n"
        "  if dst.endswith('.ouro-prev'):\n"
        "    s['backups']=True\n"
        "    if src.endswith('kes.vkey'): s['previous_vkey']=s['active_vkey']\n"
        "    if src.endswith('node.cert'):\n"
        "      s['previous_opcert']=s['active_opcert']; s['previous_opcert_envelope']=s['active_opcert_envelope']\n"
        "  elif src.endswith('kes.vkey.ouro-prev'): s['active_vkey']=s['previous_vkey']\n"
        "  elif src.endswith('node.cert.ouro-prev'):\n"
        "    s['active_opcert']=s['previous_opcert']; s['active_opcert_envelope']=s['previous_opcert_envelope']\n"
        "  elif src.endswith('.ouro-kes-stage/kes.vkey'): s['active_vkey']=s['staged_vkey']\n"
        "elif a and a[0]=='cp' and len(a)==3 and a[2].endswith(':/opt/cardano/config/keys/node.cert'):\n"
        "  raw=open(a[1],'rb').read(); s['active_opcert']=hashlib.sha256(raw).hexdigest(); s['active_opcert_envelope']=json.loads(raw)\n"
        "elif a and a[0]=='restart': s['restart_count']+=1\n"
        "elif a and a[0]=='rm':\n"
        "  if stage in a: s['stage']=False\n"
        "  if any(x.endswith('.ouro-prev') for x in a): s['backups']=False\n"
        "elif 'kes-period-info' in a:\n"
        "  sys.stdin.buffer.read()\n"
        "  print(json.dumps({'qKesCurrentKesPeriod':100,'qKesStartKesInterval':100,'qKesEndKesInterval':162,'qKesOnDiskOperationalCertificateNumber':7,'qKesNodeStateOperationalCertificateNumber':None},separators=(',',':'))); sys.exit(0)\n"
        "json.dump(s,open(p,'w')); sys.exit(0)\n"
    )
    docker.chmod(0o700)
    env = {
        "OURO_PROBE_LIB": str(probe),
        "OURO_TEST_KES_STATE": str(state),
        "OURO_TEST_DOCKER_LOG": str(docker_log),
    }

    # Phase A: typed plan derives period, approved apply stages a pair, and exposes public data only.
    stage_plan, stage_value = invoke(
        home,
        *target_args("kes-rotation/stage-key", "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert stage_plan.returncode == 0, (stage_plan, stage_value)
    stage_candidate = stage_value["data"]["candidate_hash"]
    assert stage_value["data"]["kes_rotation"]["preexisting_kes_opcert_valid"] is False
    assert stage_value["data"]["kes_rotation"]["preexisting_forging_credentials_ready"] is False
    assert len(stage_value["data"]["kes_rotation"]["preexisting_kes_evidence_sha256"]) == 64
    assert stage_value["data"]["kes_rotation"]["cardano_cli_version"] == "10.14.0.0"
    failed_stage, failed_stage_value = invoke(
        home,
        *apply_args("kes-rotation/stage-key", stage_candidate, "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert failed_stage.returncode != 0
    assert "postcondition failed" in json.dumps(failed_stage_value)
    failed_state = json.loads(state.read_text())
    assert failed_state["stage"] is False
    assert failed_state["active_vkey"] == ACTIVE_KES_VKEY
    failed_state["drift_after_stage"] = False
    state.write_text(json.dumps(failed_state))
    stage_plan, stage_value = invoke(
        home,
        *target_args("kes-rotation/stage-key", "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert stage_plan.returncode == 0, (stage_plan, stage_value)
    stage_candidate = stage_value["data"]["candidate_hash"]
    staged, staged_value = invoke(
        home,
        *apply_args("kes-rotation/stage-key", stage_candidate, "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert staged.returncode == 0, (staged, staged_value)
    stage_post = staged_value["data"]["live_postcondition"]
    assert stage_post["kes_period"] == 100 and stage_post["kes_vkey"] == KES_VKEY
    assert stage_post["kes_skey_mode"] == "0600"
    assert stage_post["active_container_unchanged"] is True
    assert stage_post["active_kes_key_unchanged"] is True
    assert stage_post["active_opcert_unchanged"] is True
    assert stage_post["preexisting_kes_opcert_valid"] is False
    assert stage_post["preexisting_forging_credentials_ready"] is False
    assert stage_post["cardano_cli_version"] == "10.14.0.0"
    assert "SigningKey" not in json.dumps(staged_value)
    assert json.loads(state.read_text())["active_vkey"] == ACTIVE_KES_VKEY

    # A fresh agent sees the complete pending pair through typed PUBLIC evidence and must ask the
    # operator to continue or discard it. Planning the choice performs no target mutation.
    before_pending_log = docker_log.read_text().splitlines()
    pending_plan, pending_value = invoke(
        home,
        *target_args("kes-rotation/stage-key", "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert pending_plan.returncode == 0, (pending_plan, pending_value)
    pending = pending_value["data"]["kes_rotation"]
    assert pending["pending_existing"] is True
    assert pending["staged_vkey"] == KES_VKEY
    assert pending["staged_vkey_sha256"] == stage_post["kes_vkey_sha256"]
    assert pending_value["data"]["executor_plan"] == []
    assert pending_value["data"]["confirmation_required"] is False
    pending_commands = docker_log.read_text().splitlines()[len(before_pending_log):]
    assert not any("key-gen-KES" in command or " rm " in f" {command} " for command in pending_commands)

    incomplete = json.loads(state.read_text())
    incomplete["stage_complete"] = False
    state.write_text(json.dumps(incomplete))
    for operation in ("kes-rotation/stage-key", "kes-rotation/discard-stage"):
        refused_incomplete, _ = invoke(
            home,
            *target_args(operation, "--param", "machine=bp1"),
            env_extra=env,
            path=fakebin,
        )
        assert refused_incomplete.returncode != 0
        assert json.loads(state.read_text())["stage"] is True
    incomplete = json.loads(state.read_text())
    incomplete["stage_complete"] = True
    state.write_text(json.dumps(incomplete))

    # The restart choice is a separate candidate-bound discard. Drift refuses before deletion;
    # exact approval removes only the staged pair and leaves the active BP untouched.
    discard_plan, discard_value = invoke(
        home,
        *target_args("kes-rotation/discard-stage", "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert discard_plan.returncode == 0, (discard_plan, discard_value)
    discard_candidate = discard_value["data"]["candidate_hash"]
    assert discard_value["data"]["confirmation_required"] is True
    assert discard_value["data"]["kes_rotation"]["staged_vkey_sha256"] == stage_post["kes_vkey_sha256"]
    drifted = json.loads(state.read_text())
    original_staged_vkey = drifted["staged_vkey"]
    drifted["staged_vkey"] = {**KES_VKEY, "cborHex": "5820" + "11" * 32}
    state.write_text(json.dumps(drifted))
    refused_discard, _ = invoke(
        home,
        *apply_args("kes-rotation/discard-stage", discard_candidate, "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert refused_discard.returncode != 0 and json.loads(state.read_text())["stage"] is True
    drifted = json.loads(state.read_text())
    drifted["staged_vkey"] = original_staged_vkey
    state.write_text(json.dumps(drifted))
    discard_plan, discard_value = invoke(
        home,
        *target_args("kes-rotation/discard-stage", "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    discarded, discarded_value = invoke(
        home,
        *apply_args(
            "kes-rotation/discard-stage",
            discard_value["data"]["candidate_hash"],
            "--param",
            "machine=bp1",
        ),
        env_extra=env,
        path=fakebin,
    )
    assert discarded.returncode == 0, (discarded, discarded_value)
    assert json.loads(state.read_text())["stage"] is False
    assert json.loads(state.read_text())["active_vkey"] == ACTIVE_KES_VKEY
    assert discarded_value["data"]["live_postcondition"]["staging_directory_absent"] is True

    # Starting over is a second, separately approved normal Phase A.
    stage_plan, stage_value = invoke(
        home,
        *target_args("kes-rotation/stage-key", "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert stage_plan.returncode == 0, (stage_plan, stage_value)
    assert stage_value["data"]["kes_rotation"]["pending_existing"] is False
    assert stage_value["data"]["confirmation_required"] is True
    assert any("key-gen-KES" in " ".join(command) for command in stage_value["data"]["executor_plan"])
    staged, staged_value = invoke(
        home,
        *apply_args(
            "kes-rotation/stage-key",
            stage_value["data"]["candidate_hash"],
            "--param",
            "machine=bp1",
        ),
        env_extra=env,
        path=fakebin,
    )
    assert staged.returncode == 0, (staged, staged_value)
    stage_post = staged_value["data"]["live_postcondition"]

    # Deterministic local public handoff consumes the typed period and returned public envelope.
    handoff = home / "ouro-kes-rotation" / "bp1-period-100"
    handoff.mkdir(parents=True)
    vkey = handoff / "kes.vkey"
    vkey.write_text(json.dumps(stage_post["kes_vkey"], separators=(",", ":")))
    generated = subprocess.run(
        [str(BIN), "kes", "cold-sign-script", "--kes-vkey", str(vkey), "--kes-period", "100"],
        text=True,
        capture_output=True,
        check=True,
    )
    script = handoff / "cold-sign.sh"
    script.write_text(generated.stdout)
    script.chmod(0o700)
    assert generated.stderr.strip() == f"sha256={hashlib.sha256(script.read_bytes()).hexdigest()}"
    assert "cold.skey" in generated.stdout and "SigningKey" not in generated.stdout

    # Phase B binds a mock public cold-signed opcert to that staged key, then activates all three
    # files with the fixed recoverable executor. This is a fake target; no real node is touched.
    opcert = handoff / "node.cert"
    opcert.write_text(json.dumps(OPCERT, separators=(",", ":")))
    opcert_digest = hashlib.sha256(opcert.read_bytes()).hexdigest()
    reference = f"opcert-{opcert_digest[:8]}@sha256:{opcert_digest}"
    install_params = ("--param", "machine=bp1", "--param", f"opcert={reference}")
    install_plan, install_value = invoke(
        home,
        *target_args("kes-rotation/install-opcert", *install_params),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert install_plan.returncode == 0, (install_plan, install_value)
    install_candidate = install_value["data"]["candidate_hash"]
    assert install_value["data"]["kes_rotation"]["staged_vkey_sha256"] == stage_post["kes_vkey_sha256"]

    def fresh_permit(
        candidate: str = install_candidate,
        artifact_digest: str | None = None,
    ) -> str:
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        relay_port = listener.getsockname()[1]

        def accept_probe() -> None:
            connection, _ = listener.accept()
            connection.close()
            listener.close()

        threading.Thread(target=accept_probe, daemon=True).start()
        return permit_for(candidate, relay_port, artifact_digest)

    # If promotion is interrupted before restart, the retained previous/staged transaction set is
    # recognized by the same operation. Re-entry finishes forward promotion and performs the one
    # restart; it never regenerates or cold-signs a key.
    pre_apply_state = json.loads(state.read_text())
    partial_state = dict(pre_apply_state)
    partial_state["fail_promotion_once"] = True
    state.write_text(json.dumps(partial_state))
    partial_apply, partial_value = invoke(
        home,
        *apply_args("kes-rotation/install-opcert", install_candidate, *install_params),
        "--verified-fleet-permit",
        fresh_permit(),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert partial_apply.returncode != 0
    assert partial_value["error"]["code"] == "activation_unverified"
    partial = json.loads(state.read_text())
    assert partial["active_vkey"] == ACTIVE_KES_VKEY
    assert partial["active_opcert"] == active_opcert_digest
    assert partial["backups"] is True and partial["stage"] is True
    assert partial["restart_count"] == 0
    partial_plan, partial_plan_value = invoke(
        home,
        *target_args("kes-rotation/install-opcert", *install_params),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert partial_plan.returncode == 0, (partial_plan, partial_plan_value)
    partial_kes = partial_plan_value["data"]["kes_rotation"]
    assert partial_kes["activation_pending"] is True
    assert partial_kes["activation_promoted"] is False
    assert ["docker", "restart", "cid-plan"] in partial_plan_value["data"]["executor_plan"]
    completed_partial, completed_partial_value = invoke(
        home,
        *apply_args("kes-rotation/install-opcert",
                    partial_plan_value["data"]["candidate_hash"], *install_params),
        "--verified-fleet-permit",
        fresh_permit(partial_plan_value["data"]["candidate_hash"]),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert completed_partial.returncode == 0, (completed_partial, completed_partial_value)
    completed_partial_state = json.loads(state.read_text())
    assert completed_partial_state["restart_count"] == 1
    assert completed_partial_state["stage"] is False
    assert completed_partial_state["backups"] is False

    state.write_text(json.dumps(pre_apply_state))

    # Once Phase B begins promotion it is forward-only. A bounded readiness failure retains the
    # promoted candidate and recovery material, performs no rollback/second restart, and reports
    # the mutation truthfully. Resetting this fake fixture afterwards creates an independent
    # delayed-success case; no production key/counter is regenerated here.
    failing_state = dict(pre_apply_state)
    failing_state["fail_post"] = True
    state.write_text(json.dumps(failing_state))
    failed_apply, failed_apply_value = invoke(
        home,
        *apply_args("kes-rotation/install-opcert", install_candidate, *install_params),
        "--verified-fleet-permit",
        fresh_permit(),
        env_extra={
            **env,
            "OURO_EPHEMERAL_PAYLOAD": str(opcert),
            "OURO_TEST_READINESS_TIMEOUT_SECONDS": "0",
        },
        path=fakebin,
    )
    assert failed_apply.returncode != 0
    assert failed_apply_value["error"]["code"] == "activation_unverified"
    assert failed_apply_value["changed"] is True
    unverified = json.loads(state.read_text())
    assert unverified["active_vkey"] == KES_VKEY
    assert unverified["active_opcert"] == opcert_digest
    assert unverified["backups"] is True and unverified["stage"] is True
    assert unverified["restart_count"] == 1
    assert failed_apply_value["data"]["automatic_rollback_performed"] is False
    assert failed_apply_value["data"]["recovery_material_retained"] is True

    # Once the retained candidate becomes ready, the same install workflow recognizes it and only
    # verifies/finalizes. It neither promotes files nor restarts again.
    resumable = json.loads(state.read_text())
    resumable["fail_post"] = False
    state.write_text(json.dumps(resumable))
    resume_plan, resume_value = invoke(
        home,
        *target_args("kes-rotation/install-opcert", *install_params),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert resume_plan.returncode == 0, (resume_plan, resume_value)
    assert resume_value["data"]["kes_rotation"]["activation_pending"] is True
    assert resume_value["data"]["executor_plan"] == [
        ["docker", "exec", "cid-plan", "rm", "-f",
         "/opt/cardano/config/keys/kes.skey.ouro-prev",
         "/opt/cardano/config/keys/kes.vkey.ouro-prev",
         "/opt/cardano/config/keys/node.cert.ouro-prev"],
        ["docker", "exec", "cid-plan", "rm", "-rf",
         "/opt/cardano/config/keys/.ouro-kes-stage"],
    ]
    resumed, resumed_value = invoke(
        home,
        *apply_args("kes-rotation/install-opcert", resume_value["data"]["candidate_hash"],
                    *install_params),
        "--verified-fleet-permit",
        fresh_permit(resume_value["data"]["candidate_hash"]),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert resumed.returncode == 0, (resumed, resumed_value)
    resumed_state = json.loads(state.read_text())
    assert resumed_state["restart_count"] == 1
    assert resumed_state["stage"] is False and resumed_state["backups"] is False
    assert resumed_value["data"]["live_postcondition"]["activation_resumed"] is True
    assert resumed_value["data"]["live_postcondition"]["restart_performed"] is False

    state.write_text(json.dumps(pre_apply_state))
    delayed = json.loads(state.read_text())
    delayed["readiness_delay_remaining"] = 2
    state.write_text(json.dumps(delayed))
    applied, applied_value = invoke(
        home,
        *apply_args("kes-rotation/install-opcert", install_candidate, *install_params),
        "--verified-fleet-permit",
        fresh_permit(),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert applied.returncode == 0, (applied, applied_value)
    final = json.loads(state.read_text())
    assert final["active_vkey"] == KES_VKEY
    assert final["active_opcert"] == opcert_digest
    assert final["stage"] is False and final["backups"] is False
    assert final["restart_count"] == 1
    assert final["readiness_delay_remaining"] == 0
    assert applied_value["data"]["live_postcondition"]["verification"] == \
        "staged_kes_pair_and_bound_opcert_activated"
    assert applied_value["data"]["live_postcondition"]["node_state_counter_status"] == \
        "no_blocks_minted_yet"
    assert applied_value["data"]["live_postcondition"]["cold_identity_bound"] is True
    assert applied_value["data"]["live_postcondition"]["staging_residue_removed"] is True
    assert "SigningKey" not in json.dumps(applied_value)

    # Successful activation leaves no transaction residue, so the next rotation is a fresh plan.
    next_plan, next_value = invoke(
        home,
        *target_args("kes-rotation/stage-key", "--param", "machine=bp1"),
        env_extra=env,
        path=fakebin,
    )
    assert next_plan.returncode == 0, (next_plan, next_value)
    assert next_value["data"]["kes_rotation"]["pending_existing"] is False
    assert next_value["data"]["confirmation_required"] is True

    # Phase B restart-loop recovery is not a third workflow. Given the already staged pair and the
    # same public opcert, it plans one fixed network-disabled helper around exactly one stop/start.
    # The healthy-relay protocol evidence is carried in the candidate-bound fleet permit.
    restart_fixture = dict(pre_apply_state)
    restart_fixture.update({
        "restart_loop": True,
        "next_ready": False,
        "helper_prepare_count": 0,
        "helper_promote_count": 0,
        "helper_finalize_count": 0,
        "stop_count": 0,
        "start_count": 0,
        "restart_count": 0,
    })
    state.write_text(json.dumps(restart_fixture))
    restart_plan, restart_plan_value = invoke(
        home,
        *target_args("kes-rotation/install-opcert", *install_params),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert restart_plan.returncode == 0, (restart_plan, restart_plan_value)
    restart_kes = restart_plan_value["data"]["kes_rotation"]
    assert restart_kes["restart_loop_repair"] is True
    assert restart_kes["current_period"] is None
    commands = restart_plan_value["data"]["executor_plan"]
    assert sum(command[:2] == ["docker", "stop"] for command in commands) == 1
    assert sum(command[:2] == ["docker", "start"] for command in commands) == 1
    helpers = [command for command in commands if command[:2] == ["docker", "run"]]
    assert len(helpers) == 3
    assert all("--network" in command and "none" in command for command in helpers)
    assert all("--pull" in command and "never" in command for command in helpers)
    assert not any(command[:2] == ["docker", "restart"] for command in commands)

    restart_candidate = restart_plan_value["data"]["candidate_hash"]
    repaired, repaired_value = invoke(
        home,
        *apply_args("kes-rotation/install-opcert", restart_candidate, *install_params),
        "--verified-fleet-permit",
        fresh_permit(restart_candidate, opcert_digest),
        env_extra={**env, "OURO_EPHEMERAL_PAYLOAD": str(opcert)},
        path=fakebin,
    )
    assert repaired.returncode == 0, (repaired, repaired_value)
    repaired_state = json.loads(state.read_text())
    assert repaired_state["active_vkey"] == KES_VKEY
    assert repaired_state["active_opcert"] == opcert_digest
    assert repaired_state["helper_prepare_count"] == 1
    assert repaired_state["helper_promote_count"] == 1
    assert repaired_state["helper_finalize_count"] == 1
    assert repaired_state["stop_count"] == 1 and repaired_state["start_count"] == 1
    assert repaired_state["stage"] is False and repaired_state["backups"] is False
    assert repaired_value["data"]["live_postcondition"]["restart_performed"] is True
    print("S0025 genuine staged KES rotation mock end-to-end passed")


if __name__ == "__main__":
    main()
