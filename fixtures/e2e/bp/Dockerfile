# S0015 E2E bed — block-producer host bp1 (p2-1). Unlike the relay hosts (node/Dockerfile),
# bp1 runs a REAL forging cardano-node, so it is based on the arm64-NATIVE blinklabs image
# (debian 12 + cardano-node/cardano-cli + IOG libsodium/libsecp256k1). The pinned `ouro`
# binary + skills are COPYed from ouro-e2e-base so bp1 runs the SAME ouro as every other
# container (E2E-12 identical-provenance holds despite the different node base).
#
# Node image is digest-pinned for reproducibility (E2E-11 image-pin branch). The devnet
# genesis is generated FRESH at boot (systemStart=now) by node-entrypoint.sh — never baked.
# Real KES/VRF/cold material is generated at runtime, never COPYed into a layer.
FROM ouro-e2e-base:local AS ouro
FROM ghcr.io/blinklabs-io/cardano-node:10.5.4@sha256:6de965784be4134deccb94ca8d92c11dfb3e140a9d0616210f29a1836fdb13d7

# Same pinned ouro + skills + schemas as the rest of the bed (identical provenance).
COPY --from=ouro /usr/local/bin/ouro /usr/local/bin/ouro
COPY --from=ouro /opt/ouro /opt/ouro

# sshd + sudo + python3 (L2 scripts emit JSON via python3 / parse the spec). Principals +
# 0400 placeholder key dir mirror node/Dockerfile so the security posture is identical.
RUN apt-get -o Acquire::Retries=3 update \
 && apt-get -o Acquire::Retries=3 install -y --no-install-recommends openssh-server sudo python3 python3-yaml inotify-tools \
 && rm -rf /var/lib/apt/lists/* \
 && rm -f /etc/ssh/ssh_host_* \
 && mkdir -p /run/sshd \
 && useradd -m -s /bin/bash ouro-exec \
 && useradd -m -s /bin/bash ouro-diag \
 && useradd -r -s /usr/sbin/nologin node \
 && mkdir -p /opt/cardano/keys \
 && printf 'placeholder-not-a-real-key\n' > /opt/cardano/keys/kes.skey \
 && chown -R node:node /opt/cardano/keys \
 && chmod 0700 /opt/cardano/keys \
 && chmod 0400 /opt/cardano/keys/kes.skey

# Pubkey-only sshd, no root, only the two principals (identical to node/Dockerfile).
RUN printf '%s\n' \
      'PasswordAuthentication no' \
      'KbdInteractiveAuthentication no' \
      'PermitRootLogin no' \
      'PubkeyAuthentication yes' \
      'AllowUsers ouro-exec ouro-diag' \
      > /etc/ssh/sshd_config.d/10-ouro-e2e.conf

# sudoers confines ouro-exec to the root-owned tool-run wrapper (identical to node/Dockerfile).
RUN printf '#!/bin/sh\nexec /usr/local/bin/ouro-ops tool run "$@"\n' > /usr/local/sbin/ouro-tool-run \
 && chmod 0755 /usr/local/sbin/ouro-tool-run \
 && printf '%s\n%s\n' \
      'Defaults:ouro-exec env_reset, secure_path="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"' \
      'ouro-exec ALL=(root) NOPASSWD: /usr/local/sbin/ouro-tool-run' \
      > /etc/sudoers.d/ouro-exec \
 && chmod 0440 /etc/sudoers.d/ouro-exec \
 && visudo -cf /etc/sudoers.d/ouro-exec \
 && install -d -m0700 -o ouro-exec -g ouro-exec /home/ouro-exec/.ssh

COPY bp/node-entrypoint.sh /usr/local/bin/node-entrypoint.sh
RUN chmod 0755 /usr/local/bin/node-entrypoint.sh

EXPOSE 22 3001
# Readiness probe (p2-1 就绪探针): healthy ONLY when sshd is up AND the node has forged a
# block (tip block > 0). `up --wait` therefore blocks until bp1 is genuinely producing.
HEALTHCHECK --interval=5s --timeout=10s --retries=30 --start-period=90s CMD bash -c '\
  : < /dev/tcp/127.0.0.1/22 \
  && CARDANO_NODE_SOCKET_PATH=/opt/devnet/node.socket cardano-cli query tip --testnet-magic 1 2>/dev/null \
     | grep -qE "\"block\": *[1-9]"' || exit 1

USER root
ENTRYPOINT ["/usr/local/bin/node-entrypoint.sh"]
