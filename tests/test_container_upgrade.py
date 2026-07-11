#!/usr/bin/env python3
"""S0017 p2-5/TC-7 — container upgrade = compose image re-pin + recreate, with rollback.

Drives ouro_node_upgrade_container with a REAL compose file and a stubbed docker/compose
(so the compose rewrite + backup + verify + rollback logic is exercised for real, without a
docker daemon). Asserts:
  * success: the service's image is rewritten to the declared one, the backup is removed,
    and the function converges (running image id == declared image id);
  * rollback: when the recreated container does NOT converge to the declared image, the
    compose file is RESTORED to the original image and the function fails (exit 30);
  * unmanaged: a plain-run container (no compose labels) fails closed (exit 40).
Standalone: `python3 tests/test_container_upgrade.py`.
"""
import os
import subprocess
import tempfile
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / "ouro-skills/lib/ouro-lib.sh"

DOCKER_STUB = r"""#!/usr/bin/env bash
case "$1" in
  compose) exit 0 ;;                                   # `compose version` and `compose -f .. up`
  image)
    if [ "$2" = inspect ]; then
      [ "$3" = --format ] && { echo "$FAKE_WANT_ID"; exit 0; }
      exit 0                                           # image present locally
    fi ;;
  inspect)                                             # inspect --format <fmt> <ref>
    case "$3" in
      *config_files*) echo "$FAKE_CFG" ;;
      *working_dir*)  echo "$FAKE_WD" ;;
      *compose.project*) echo "$FAKE_PROJ" ;;
      *compose.service*) echo "cardano-node" ;;
      *.Image*) echo "$FAKE_RUN_ID" ;;
      *) echo "<no value>" ;;
    esac; exit 0 ;;
  pull) exit 0 ;;
  ps) echo "newcid123"; exit 0 ;;
esac
exit 0
"""


def write_compose(tmp, image="cnode:v1"):
    cfg = Path(tmp) / "docker-compose.yaml"
    yaml.safe_dump(
        {"services": {"cardano-node": {"image": image, "ports": ["3001:3001"]}}},
        open(cfg, "w"),
        sort_keys=False,
    )
    return cfg


def run_upgrade(tmp, cfg, *, want="cnode:v2", proj="ouro-pool", want_id="sha256:NEW",
                run_id="sha256:NEW"):
    binp = Path(tmp) / "bin"
    binp.mkdir(exist_ok=True)
    stub = binp / "docker"
    stub.write_text(DOCKER_STUB)
    stub.chmod(0o755)
    env = dict(os.environ)
    env.update(
        PATH=f"{binp}:{env['PATH']}",
        OURO_TOOL_NAME="test/upgrade",
        OURO_MACHINE="bp1",
        FAKE_CFG=str(cfg),
        FAKE_WD=str(tmp),
        FAKE_PROJ=proj,
        FAKE_WANT_ID=want_id,
        FAKE_RUN_ID=run_id,
    )
    return subprocess.run(
        ["bash", "-c", f"source {LIB}\nouro_node_upgrade_container docker oldcid {want}"],
        env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )


def image_of(cfg):
    return yaml.safe_load(open(cfg))["services"]["cardano-node"]["image"]


def main():
    # --- success: compose rewritten to the declared image, backup removed, converges ---
    with tempfile.TemporaryDirectory() as tmp:
        cfg = write_compose(tmp, "cnode:v1")
        r = run_upgrade(tmp, cfg, want="cnode:v2", want_id="sha256:NEW", run_id="sha256:NEW")
        assert r.returncode == 0, (r.returncode, r.stdout, r.stderr)
        assert image_of(cfg) == "cnode:v2", "compose image not re-pinned"
        assert not (Path(tmp) / "docker-compose.yaml.ouro-backup").exists(), "backup not cleaned"

    # --- rollback: recreated container does NOT converge => restore compose + exit 30 ---
    with tempfile.TemporaryDirectory() as tmp:
        cfg = write_compose(tmp, "cnode:v1")
        r = run_upgrade(tmp, cfg, want="cnode:v2", want_id="sha256:NEW", run_id="sha256:STALE")
        assert r.returncode == 30, (r.returncode, r.stdout, r.stderr)
        assert image_of(cfg) == "cnode:v1", "compose file NOT restored on rollback"
        assert '"code":"container_upgrade_failed"' in r.stdout, r.stdout
        assert not (Path(tmp) / "docker-compose.yaml.ouro-backup").exists(), "backup left behind"

    # --- unmanaged: no compose labels => fail closed (exit 40), compose untouched ---
    with tempfile.TemporaryDirectory() as tmp:
        cfg = write_compose(tmp, "cnode:v1")
        r = run_upgrade(tmp, cfg, proj="")   # empty compose.project label
        assert r.returncode == 40, (r.returncode, r.stdout, r.stderr)
        assert '"code":"container_unmanaged"' in r.stdout, r.stdout
        assert image_of(cfg) == "cnode:v1", "compose file touched for an unmanaged container"

    print("container upgrade (compose re-pin + rollback + fail-closed) passed")


if __name__ == "__main__":
    main()
