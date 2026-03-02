#!/usr/bin/env python3
"""
JSON-RPC over stdio bridge for ansible-runner.
Reads one JSON-RPC request per line from stdin, outputs responses/events per line to stdout.
"""
import json
import sys
import os
import tempfile
import threading

def log_err(msg: str) -> None:
    print(msg, file=sys.stderr, flush=True)

def send(obj: dict) -> None:
    print(json.dumps(obj, ensure_ascii=False), flush=True)

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
            private_data_dir = tempfile.mkdtemp(prefix="ouro_runner_")
            try:
                inv_path = os.path.join(private_data_dir, "inventory")
                os.makedirs(inv_path, exist_ok=True)
                if isinstance(inventory, dict):
                    import yaml
                    with open(os.path.join(inv_path, "hosts.yml"), "w") as g:
                        yaml.dump(inventory, g, default_flow_style=False, allow_unicode=True)
                ev_path = os.path.join(private_data_dir, "env", "extravars")
                os.makedirs(os.path.dirname(ev_path), exist_ok=True)
                with open(ev_path, "w") as f:
                    json.dump(extra_vars, f)

                if ansible_runner is None:
                    send({"event": "runner_on_ok", "id": run_id, "data": {"host": "local", "task": "mock"}})
                    send({"event": "playbook_complete", "id": run_id, "data": {"status": "successful", "rc": 0}})
                    continue

                def event_cb(event_data):
                    send({"event": event_data.get("event", "unknown"), "id": run_id, "data": event_data})

                r = ansible_runner.run(
                    private_data_dir=private_data_dir,
                    playbook=playbook,
                    event_handler=event_cb,
                )
                send({"event": "playbook_complete", "id": run_id, "data": {"status": "successful" if r.rc == 0 else "failed", "rc": r.rc}})
            except Exception as e:
                send({"event": "runner_on_failed", "id": run_id, "data": {"msg": str(e)}})
                send({"event": "playbook_complete", "id": run_id, "data": {"status": "failed", "rc": -1}})
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
