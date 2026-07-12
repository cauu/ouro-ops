# S0017 p1-2 — a BARE target for the `ouro-ops init` E2E: a fresh box as it exists BEFORE
# provisioning. Only what a stock cloud image has — sshd, sudo, and one sudo-capable bootstrap
# account (`boot`) reachable by SSH key. Deliberately NO ouro base: no ouro-exec/ouro-diag, no
# tool-run wrapper, no sudoers.d/ouro-exec, no ouro-ops binary. `ouro-ops init` must create all
# of that. The bootstrap public key is injected at runtime (docker exec), not baked.
FROM debian:12-slim
RUN apt-get -o Acquire::Retries=3 update \
 && apt-get -o Acquire::Retries=3 install -y --no-install-recommends openssh-server sudo python3 \
 && rm -rf /var/lib/apt/lists/* \
 && rm -f /etc/ssh/ssh_host_* \
 && mkdir -p /run/sshd \
 # the existing sudo user init first-connects as (NOPASSWD so init's `sudo -n` works)
 && useradd -m -s /bin/bash boot \
 && printf 'boot ALL=(root) NOPASSWD: ALL\n' > /etc/sudoers.d/boot \
 && chmod 0440 /etc/sudoers.d/boot \
 && install -d -m 0700 -o boot -g boot /home/boot/.ssh
EXPOSE 22
HEALTHCHECK --interval=3s --timeout=3s --retries=20 --start-period=2s CMD bash -c ': < /dev/tcp/127.0.0.1/22' || exit 1
CMD ["bash", "-c", "ssh-keygen -A && exec /usr/sbin/sshd -D -e"]
