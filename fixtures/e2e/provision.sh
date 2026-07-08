#!/usr/bin/env bash
# S0015 p1-4 — provision the E2E bed with SSH keys, creds, and the spec at RUNTIME.
# Real key material is generated/placed at runtime (never baked into an image). Run this
# after `make e2e-bed-up`. Idempotent.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."   # repo root
COMPOSE=(docker compose -f fixtures/e2e/compose.yaml)
SPEC_SRC="${OURO_E2E_SPEC:-fixtures/e2e/pool-spec.bed.yaml}"
TARGETS=(bp1 relay1 relay2)

# Control gets the spec too (it reads it to resolve each machine's SSH target).
"${COMPOSE[@]}" exec -T control mkdir -p /opt/ouro /root/.ouro/credentials /root/.ssh
"${COMPOSE[@]}" cp "$SPEC_SRC" control:/opt/ouro/pool-spec.yaml

# Refresh control's known_hosts from the CURRENT bed (targets get fresh host keys on each
# rebuild). This keeps StrictHostKeyChecking meaningful without stale-key churn.
"${COMPOSE[@]}" exec -T control bash -c ': > /root/.ssh/known_hosts; chmod 600 /root/.ssh/known_hosts'
for m in "${TARGETS[@]}"; do
  "${COMPOSE[@]}" exec -T control bash -c "ssh-keyscan -t ed25519 $m 2>/dev/null >> /root/.ssh/known_hosts"
done

for m in "${TARGETS[@]}"; do
  # 1) Per-target ed25519 keypair on control at ~/.ouro/credentials/<m> (= creds://<m>).
  "${COMPOSE[@]}" exec -T control bash -c "
    set -e
    test -f /root/.ouro/credentials/$m || ssh-keygen -t ed25519 -N '' -q -f /root/.ouro/credentials/$m
    chmod 600 /root/.ouro/credentials/$m"
  pub="$("${COMPOSE[@]}" exec -T control cat "/root/.ouro/credentials/$m.pub")"

  # 2) Install the pubkey into the target's ouro-exec authorized_keys.
  "${COMPOSE[@]}" exec -T "$m" bash -c "
    set -e
    install -d -m700 -o ouro-exec -g ouro-exec /home/ouro-exec/.ssh
    printf '%s\n' '$pub' > /home/ouro-exec/.ssh/authorized_keys
    chown ouro-exec:ouro-exec /home/ouro-exec/.ssh/authorized_keys
    chmod 600 /home/ouro-exec/.ssh/authorized_keys"

  # 3) Push the spec to the target (p1-6 will push rendered config; here the spec only).
  "${COMPOSE[@]}" exec -T "$m" mkdir -p /opt/ouro
  "${COMPOSE[@]}" cp "$SPEC_SRC" "$m:/opt/ouro/pool-spec.yaml"
done

echo "provisioned: keys + creds + authorized_keys + spec for ${TARGETS[*]}"
