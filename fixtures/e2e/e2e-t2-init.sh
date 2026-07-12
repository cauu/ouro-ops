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

export OURO_HOME="$WORK/ouro-home"; mkdir -p "$OURO_HOME/credentials"
cp "$WORK/bootstrap" "$OURO_HOME/credentials/boot"; chmod 600 "$OURO_HOME/credentials/boot"
initcmd() { OURO_HOME="$OURO_HOME" ./target/debug/ouro-ops init \
  --host 127.0.0.1 --port "$PORT" --bootstrap-user boot --bootstrap-key creds://boot \
  --control-pubkey "$WORK/control.pub" --ouro-binary "$OURO_BIN_LINUX" "$@"; }

echo "[init] --expected-host-key MISMATCH refuses BEFORE any write (first-hop MITM defense)"
if initcmd --expected-host-key "SHA256:bogusbogusbogusbogusbogusbogusbogusbogus00" >/dev/null 2>&1; then
  fail "init did NOT refuse a mismatched --expected-host-key"
fi
# The fresh box must be UNTOUCHED — the refusal happened before provisioning wrote anything.
dex id ouro-exec >/dev/null 2>&1 && fail "mismatched init still provisioned (verify was NOT before writes)"
dex test -e /usr/local/bin/ouro-ops && fail "mismatched init still pushed the binary"
pass "mismatched expected host key refused with ZERO writes to the target"

echo "[init] run ouro-ops init with the CORRECT expected host key (also proves pin-only-matching)"
# The real ed25519 host key fingerprint, captured out-of-band (here: directly from the box).
REAL_FP=$(ssh-keyscan -T 5 -t ed25519 -p "$PORT" 127.0.0.1 2>/dev/null | ssh-keygen -lf - 2>/dev/null | awk '{print $2}')
[ -n "$REAL_FP" ] || fail "could not capture the target's real host key fingerprint"
OUT=$(initcmd --expected-host-key "$REAL_FP" 2>&1) || fail "init (correct expected key) exited non-zero: $OUT"
echo "$OUT" | python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["data"]["manifest"]["ok"], d' \
  || fail "init manifest not ok: $OUT"
pass "init completed with verified host key; install manifest ok"

echo "[init] host key pinned (p3-3), and ONLY the matching key was pinned (not every scanned key)"
echo "$OUT" | python3 -c 'import json,sys; assert json.load(sys.stdin)["data"]["pinned_host_key"], "no pinned_host_key"' \
  || fail "init did not report a pinned host key"
grep -q "\[127.0.0.1\]:$PORT" "$OURO_HOME/known_hosts" 2>/dev/null \
  || fail "target host key not pinned into $OURO_HOME/known_hosts"
# The box offers rsa/ecdsa/ed25519 (ssh-keygen -A); with --expected-host-key only the ONE
# matching entry must be pinned, not all three.
KH_ENTRIES=$(grep -c "\[127.0.0.1\]:$PORT" "$OURO_HOME/known_hosts")
[ "$KH_ENTRIES" = 1 ] || fail "expected exactly 1 pinned key (the matching one), got $KH_ENTRIES"
pass "host key pinned; only the expected-matching key was written (1 entry)"

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

echo "[deinit] running-node safety gate — deinit REFUSES while a node runs"
# Stand-in `cardano-node run` process (bare box has no real node) to trip the gate.
dex bash -c 'printf "#!/bin/sh\ntrap \"exit 0\" TERM\nwhile true; do sleep 1; done\n" > /usr/local/bin/cardano-node && chmod +x /usr/local/bin/cardano-node && setsid /usr/local/bin/cardano-node run </dev/null >/dev/null 2>&1 &'
sleep 1
dex pgrep -f 'cardano-node run' >/dev/null 2>&1 || fail "stand-in node did not start"
if OURO_HOME="$OURO_HOME" ./target/debug/ouro-ops deinit \
     --host 127.0.0.1 --port "$PORT" --bootstrap-user boot --bootstrap-key creds://boot >/dev/null 2>&1; then
  fail "deinit did NOT refuse while a node was running"
fi
dex id ouro-exec >/dev/null 2>&1 || fail "deinit removed the base despite refusing (should be untouched)"
pass "deinit refused while a node was running (base untouched)"
dex pkill -9 -f 'cardano-node run' 2>/dev/null || true
for _ in $(seq 1 12); do dex pgrep -f 'cardano-node run' >/dev/null 2>&1 || break; sleep 0.5; done
dex pgrep -f 'cardano-node run' >/dev/null 2>&1 && fail "stand-in node did not stop"

echo "[deinit] node stopped — deinit restores the box (base removed, boot preserved)"
OUT4=$(OURO_HOME="$OURO_HOME" ./target/debug/ouro-ops deinit \
  --host 127.0.0.1 --port "$PORT" --bootstrap-user boot --bootstrap-key creds://boot 2>&1) \
  || fail "deinit exited non-zero: $OUT4"
echo "$OUT4" | python3 -c 'import json,sys; assert json.load(sys.stdin)["data"]["manifest"]["ok"]' || fail "deinit manifest not ok: $OUT4"
dex id ouro-exec >/dev/null 2>&1 && fail "ouro-exec still present after deinit"
dex id ouro-diag >/dev/null 2>&1 && fail "ouro-diag still present after deinit"
dex test -e /usr/local/bin/ouro-ops && fail "ouro-ops binary still present after deinit"
dex test -e /usr/local/sbin/ouro-tool-run && fail "wrapper still present after deinit"
dex test -e /etc/sudoers.d/ouro-exec && fail "sudoers still present after deinit"
dex test -e /etc/ssh/sshd_config.d/10-ouro.conf && fail "sshd drop-in still present after deinit"
# The operator's bootstrap account is preserved (never locked out).
ssh_as "$WORK/bootstrap" boot "true" || fail "bootstrap user 'boot' no longer reachable after deinit"
pass "deinit restored the box: base removed, bootstrap user 'boot' preserved"

echo "ouro-ops init/deinit (bare <-> constrained target) E2E: ALL PASSED"
