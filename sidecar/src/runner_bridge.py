#!/usr/bin/env python3
"""
JSON-RPC over stdio bridge for ansible-runner.
Reads one JSON-RPC request per line from stdin, outputs responses/events per line to stdout.
"""
import json
import sys
import os
import tempfile
from datetime import datetime, timezone

def log_err(msg: str) -> None:
    print(msg, file=sys.stderr, flush=True)

def send(obj: dict) -> None:
    print(json.dumps(obj, ensure_ascii=False), flush=True)

def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def summarize_failure(data: dict) -> str:
    host = data.get("host") or data.get("machine_name") or "unknown-host"
    task = data.get("task") or data.get("ansible_task_name") or "unknown-task"
    msg = data.get("msg") or data.get("stderr") or data.get("line") or "unknown error"
    return f"{host}/{task}: {msg}"

def main() -> None:
    try:
        import ansible_runner
    except ImportError:
        ansible_runner = None  # type: ignore

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError as e:
            send({"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": str(e)}})
            continue

        req_id = req.get("id")
        method = req.get("method")
        params = req.get("params") or {}

        if method == "ping":
            send({"jsonrpc": "2.0", "id": req_id, "result": {"status": "ok"}})
            continue

        if method == "shutdown":
            send({"jsonrpc": "2.0", "id": req_id, "result": {"status": "shutting_down"}})
            sys.exit(0)

        if method == "run_playbook":
            run_id = params.get("run_id", "default")
            playbook = params.get("playbook", "deploy.yml")
            inventory = params.get("inventory", {})
            extra_vars = params.get("extra_vars", {})
            mock_fail = bool(extra_vars.get("_mock_fail", False))
            private_data_dir = tempfile.mkdtemp(prefix="ouro_runner_")
            try:
                inv_path = os.path.join(private_data_dir, "inventory")
                os.makedirs(inv_path, exist_ok=True)
                if isinstance(inventory, dict):
                    try:
                        import yaml
                        with open(os.path.join(inv_path, "hosts.yml"), "w", encoding="utf-8") as g:
                            yaml.dump(inventory, g, default_flow_style=False, allow_unicode=True)
                    except ImportError:
                        # fallback for minimal environments without pyyaml
                        with open(os.path.join(inv_path, "hosts.json"), "w", encoding="utf-8") as g:
                            json.dump(inventory, g)
                ev_path = os.path.join(private_data_dir, "env", "extravars")
                os.makedirs(os.path.dirname(ev_path), exist_ok=True)
                with open(ev_path, "w", encoding="utf-8") as f:
                    json.dump(extra_vars, f)

                if ansible_runner is None:
                    if mock_fail:
                        failure_data = {"host": "local", "task": "mock", "msg": "mock failure for testing"}
                        send({"event": "runner_on_failed", "id": run_id, "data": failure_data})
                        send({"event": "log", "id": run_id, "data": {"stream": "stderr", "line": failure_data["msg"], "timestamp": now_iso()}})
                        send({
                            "event": "playbook_complete",
                            "id": run_id,
                            "data": {
                                "status": "failed",
                                "rc": 2,
                                "message": summarize_failure(failure_data),
                            },
                        })
                    else:
                        send({"event": "runner_on_ok", "id": run_id, "data": {"host": "local", "task": "mock", "result": "ok", "progress_percent": 100}})
                        send({"event": "log", "id": run_id, "data": {"stream": "stdout", "line": "mock playbook executed", "timestamp": now_iso()}})
                        send({"event": "playbook_complete", "id": run_id, "data": {"status": "successful", "rc": 0}})
                    continue

                if mock_fail:
                    failure_data = {"host": "local", "task": "mock", "msg": "mock failure for testing"}
                    send({"event": "runner_on_failed", "id": run_id, "data": failure_data})
                    send({"event": "log", "id": run_id, "data": {"stream": "stderr", "line": failure_data["msg"], "timestamp": now_iso()}})
                    send({
                        "event": "playbook_complete",
                        "id": run_id,
                        "data": {
                            "status": "failed",
                            "rc": 2,
                            "message": summarize_failure(failure_data),
                        },
                    })
                    continue

                last_failure_message = None

                def event_cb(event_data):
                    nonlocal last_failure_message
                    send({"event": event_data.get("event", "unknown"), "id": run_id, "data": event_data})
                    stdout_line = event_data.get("stdout")
                    if isinstance(stdout_line, str) and stdout_line.strip():
                        send({"event": "log", "id": run_id, "data": {"stream": "stdout", "line": stdout_line, "timestamp": now_iso()}})
                    event_name = event_data.get("event", "")
                    if event_name == "runner_on_failed":
                        event_detail = event_data.get("event_data", {})
                        if not isinstance(event_detail, dict):
                            event_detail = {}
                        res_obj = event_detail.get("res", {})
                        if not isinstance(res_obj, dict):
                            res_obj = {}
                        fail_data = {
                            "host": event_detail.get("host", "unknown-host"),
                            "task": event_detail.get("task", "unknown-task"),
                            "msg": stdout_line or res_obj.get("msg") or "task failed",
                        }
                        last_failure_message = summarize_failure(fail_data)
                        send({"event": "log", "id": run_id, "data": {"stream": "stderr", "line": fail_data["msg"], "timestamp": now_iso()}})

                r = ansible_runner.run(
                    private_data_dir=private_data_dir,
                    playbook=playbook,
                    event_handler=event_cb,
                )
                status = "successful" if r.rc == 0 else "failed"
                payload = {"status": status, "rc": r.rc}
                if status == "failed":
                    payload["message"] = last_failure_message or f"playbook failed with rc={r.rc}"
                send({"event": "playbook_complete", "id": run_id, "data": payload})
            except Exception as e:
                send({"event": "runner_on_failed", "id": run_id, "data": {"msg": str(e)}})
                send({"event": "log", "id": run_id, "data": {"stream": "stderr", "line": str(e), "timestamp": now_iso()}})
                send({"event": "playbook_complete", "id": run_id, "data": {"status": "failed", "rc": -1, "message": str(e)}})
            finally:
                import shutil
                if os.path.exists(private_data_dir):
                    try:
                        shutil.rmtree(private_data_dir, ignore_errors=True)
                    except Exception:
                        pass
            continue

        send({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32601, "message": f"Unknown method: {method}"}})

if __name__ == "__main__":
    main()
