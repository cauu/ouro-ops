# S0015 E2E bed — target node host (bp1 / relay1 / relay2).
# Skeleton (p1-1 + p1-1-fix1): hardened pubkey-only sshd + the two principals + a
# placeholder 0400 key dir. `ouro` install (p1-2), authorized_keys / creds (p1-4),
# sudoers.d body (p1-5), cardano-node (p2-1) are added by later items.
#
# Reproducibility: the base image is digest-pinned (E2E-11 covers the BASE digest
# only). apt packages are NOT version-pinned in the skeleton; the full apt lockfile /
# SBOM lands with p2-1 (see spec p1-1 note) — do not read the base pin as full image
# reproducibility.
# Secrets: real KES/VRF/cold material is provisioned at RUNTIME (mount/exec, p1-4).
# NEVER COPY/bake secret key material into an image layer (unremovable + leak-prone);
# only the non-secret placeholder below may live in a layer.
# Base carries the pinned `ouro` + ouro-skills + schemas (p1-2, built by `make e2e-build-base`).
FROM ouro-e2e-base:local

# python3 (+ yaml) is required by the L2 scripts: ouro-lib.sh emits JSON via python3 and
# several scripts parse pool-spec.yaml. bash/coreutils come from the base.
RUN apt-get -o Acquire::Retries=3 update \
 && apt-get -o Acquire::Retries=3 install -y --no-install-recommends openssh-server sudo python3 python3-yaml \
 && rm -rf /var/lib/apt/lists/* \
 && rm -f /etc/ssh/ssh_host_* \
 && mkdir -p /run/sshd \
 && useradd -m -s /bin/bash ouro-exec \
 && useradd -m -s /bin/bash ouro-diag \
 && useradd -m -s /usr/sbin/nologin node \
 && mkdir -p /opt/cardano/keys \
 && printf 'placeholder-not-a-real-key\n' > /opt/cardano/keys/kes.skey \
 && chown -R node:node /opt/cardano/keys \
 && chmod 0700 /opt/cardano/keys \
 && chmod 0400 /opt/cardano/keys/kes.skey

# Auth posture locked by CONFIG (not by the accident of empty accounts): pubkey-only,
# no passwords, no root login, and only the two principals may log in. authorized_keys
# are provisioned by p1-4; until then no login succeeds — by design.
RUN printf '%s\n' \
      'PasswordAuthentication no' \
      'KbdInteractiveAuthentication no' \
      'PermitRootLogin no' \
      'PubkeyAuthentication yes' \
      'AllowUsers ouro-exec ouro-diag' \
      > /etc/ssh/sshd_config.d/10-ouro-e2e.conf

# p1-5: sudoers confines ouro-exec to a fixed, root-owned wrapper that only ever runs
# `ouro tool run` — NO argv matching (fragile); the CLI self-guards audit/confirm (D3).
# ouro-exec cannot `sudo ouro confirm create` etc. — only tool run.
RUN printf '#!/bin/sh\nexec /usr/local/bin/ouro tool run "$@"\n' > /usr/local/sbin/ouro-tool-run \
 && chmod 0755 /usr/local/sbin/ouro-tool-run \
 && printf '%s\n%s\n' \
      'Defaults:ouro-exec env_reset, secure_path="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"' \
      'ouro-exec ALL=(root) NOPASSWD: /usr/local/sbin/ouro-tool-run' \
      > /etc/sudoers.d/ouro-exec \
 && chmod 0440 /etc/sudoers.d/ouro-exec \
 && visudo -cf /etc/sudoers.d/ouro-exec \
 && install -d -m0700 -o ouro-exec -g ouro-exec /home/ouro-exec/.ssh

EXPOSE 22
# Readiness signal so `docker compose up --wait` blocks until sshd is accepting.
HEALTHCHECK --interval=5s --timeout=3s --retries=12 --start-period=3s \
  CMD bash -c ': < /dev/tcp/127.0.0.1/22' || exit 1
# Generate host keys at first boot (distinct per-container identity), then run sshd.
CMD ["bash", "-c", "ssh-keygen -A && exec /usr/sbin/sshd -D -e"]
