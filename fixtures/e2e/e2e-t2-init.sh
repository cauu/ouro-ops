#!/usr/bin/env bash
# S0017 p1-2 — REAL `ouro-ops init` E2E. Provisions a BARE container (only sshd + a sudo user)
# into a constrained target, then proves the confined path is live end to end:
#   host runs `ouro-ops init` over SSH (port-mapped) as the bootstrap sudo user, pushing the
#   LINUX ouro-ops binary; afterwards ouro-exec exists with the wrapper + sudoers + hardened
#   sshd, and logging in AS ouro-exec (control key) can only run `ouro-tool-run` -> a real
#   `ouro-ops tool run`. Also asserts idempotency (a second init converges).
# Self-contained: generates throwaway keys, maps sshd to a host port, self-cleans.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# The host-side orchestrator binary (runs init; pushes the linux binary separately).
cargo build -q 2>/dev/null || { echo "  FAIL  cargo build (host ouro-ops)"; exit 1; }

IMG=ouro-e2e-bare:local
NAME="ouro-e2e-init-$$"
PORT=$(( 22000 + (RANDOM % 2000) ))
WORK="$(pwd)/tmp/init-e2e-$$"; rm -rf "$WORK"; mkdir -p "$WORK"
OURO_BIN_LINUX="$WORK/ouro-ops.linux"
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*" >&2; exit 1; }
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; rm -rf "$WORK"; }
trap cleanup EXIT
dex() { docker exec "$NAME" "$@"; }
# host ssh helpers (StrictHostKeyChecking off — throwaway localhost container)
SSHOPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o BatchMode=yes -p $PORT"
ssh_as() { local key="$1" user="$2"; shift 2; ssh $SSHOPTS -i "$key" "$user@127.0.0.1" "$@"; }

echo "[init] extract the LINUX ouro-ops binary (from the E2E base) + generate throwaway keys"
docker build -q -f fixtures/e2e/Dockerfile.base -t ouro-e2e-base:local . >/dev/null
cid=$(docker create ouro-e2e-base:local); docker cp "$cid:/usr/local/bin/ouro-ops" "$OURO_BIN_LINUX" >/dev/null; docker rm "$cid" >/dev/null
[ -s "$OURO_BIN_LINUX" ] || fail "could not extract linux ouro-ops"
ssh-keygen -q -t ed25519 -N '' -f "$WORK/bootstrap" -C boot@control
ssh-keygen -q -t ed25519 -N '' -f "$WORK/control"   -C ouro-exec@control

echo "[init] boot a BARE target (sshd + sudo user 'boot', no ouro base) on host port $PORT"
docker build -q -f fixtures/e2e/bare-node/Dockerfile -t "$IMG" . >/dev/null
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" -p "127.0.0.1:$PORT:22" "$IMG" >/dev/null
i=0; until [ "$(docker inspect -f '{{.State.Health.Status}}' "$NAME" 2>/dev/null)" = healthy ] || [ $i -ge 30 ]; do sleep 1; i=$((i+1)); done
# authorize the bootstrap key for `boot`
docker cp "$WORK/bootstrap.pub" "$NAME:/tmp/boot.pub" >/dev/null
dex bash -c 'cat /tmp/boot.pub > /home/boot/.ssh/authorized_keys && chown boot:boot /home/boot/.ssh/authorized_keys && chmod 600 /home/boot/.ssh/authorized_keys'
# sanity: bare target has NO ouro-exec yet
dex id ouro-exec >/dev/null 2>&1 && fail "bare target already has ouro-exec (fixture not bare)"
pass "bare target up; sudo user 'boot' reachable; no ouro base present"

echo "[init] run ouro-ops init from the host (bootstrap over SSH, pushes the linux binary)"
export OURO_HOME="$WORK/ouro-home"; mkdir -p "$OURO_HOME/credentials"
cp "$WORK/bootstrap" "$OURO_HOME/credentials/boot"; chmod 600 "$OURO_HOME/credentials/boot"
OUT=$(OURO_HOME="$OURO_HOME" ./target/debug/ouro-ops init \
  --host 127.0.0.1 --port "$PORT" --bootstrap-user boot --bootstrap-key creds://boot \
  --control-pubkey "$WORK/control.pub" --ouro-binary "$OURO_BIN_LINUX" 2>&1) \
  || fail "init exited non-zero: $OUT"
echo "$OUT" | python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["data"]["manifest"]["ok"], d; assert d["changed"], d' \
  || fail "init manifest not ok/changed: $OUT"
pass "init completed; install manifest ok"

echo "[init] baseline installed on the (formerly bare) target"
dex id ouro-exec >/dev/null 2>&1 || fail "ouro-exec not created"
dex id ouro-diag >/dev/null 2>&1 || fail "ouro-diag not created"
dex test -x /usr/local/bin/ouro-ops || fail "ouro-ops binary not installed"
dex test -x /usr/local/sbin/ouro-tool-run || fail "tool-run wrapper not installed"
dex test -f /etc/sudoers.d/ouro-exec || fail "sudoers confinement not installed"
dex visudo -cf /etc/sudoers.d/ouro-exec >/dev/null 2>&1 || fail "installed sudoers is invalid"
dex grep -q 'PermitRootLogin no' /etc/ssh/sshd_config.d/10-ouro.conf || fail "sshd not hardened"
pass "baseline present: ouro-exec/ouro-diag, ouro-ops, wrapper, valid sudoers, hardened sshd"

echo "[init] the CONFINED path is live: login as ouro-exec -> sudo wrapper -> ouro-ops tool run"
OUT2=$(ssh_as "$WORK/control" ouro-exec "sudo -n /usr/local/sbin/ouro-tool-run detect/runtime --machine bare" 2>&1) \
  || fail "ouro-exec could not run the confined wrapper: $OUT2"
echo "$OUT2" | python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["tool"]=="detect/runtime", d' \
  || fail "confined tool-run did not return a detect/runtime projection: $OUT2"
# confinement holds: ouro-exec may run ONLY the wrapper, not arbitrary sudo.
if ssh_as "$WORK/control" ouro-exec "sudo -n id" >/dev/null 2>&1; then
  fail "ouro-exec was able to run arbitrary sudo (confinement broken)"
fi
pass "confined dispatch works (tool run via wrapper); arbitrary sudo denied"

echo "[init] idempotent — a second init converges (still ok)"
OUT3=$(OURO_HOME="$OURO_HOME" ./target/debug/ouro-ops init \
  --host 127.0.0.1 --port "$PORT" --bootstrap-user boot --bootstrap-key creds://boot \
  --control-pubkey "$WORK/control.pub" --ouro-binary "$OURO_BIN_LINUX" 2>&1) \
  || fail "second init exited non-zero: $OUT3"
echo "$OUT3" | python3 -c 'import json,sys; assert json.load(sys.stdin)["data"]["manifest"]["ok"]' \
  || fail "second init manifest not ok: $OUT3"
pass "second init converged (idempotent)"

echo "ouro-ops init (bare -> constrained target) E2E: ALL PASSED"
