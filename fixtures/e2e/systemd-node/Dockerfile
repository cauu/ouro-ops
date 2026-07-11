# S0017 p2-9 — systemd-managed node fixture for the supervision-mode E2E.
#
# A faithful systemd HOST: PID1 is systemd (private cgroupns, so a service's cgroup is a clean
# /system.slice/<unit>.service with NO container id — exactly what a bare-metal/VM systemd host
# looks like). It runs a stand-in `cardano-node run` process as cardano-node.service so the
# supervision-mode probe + the systemd restart-dispatch branch can be validated against REAL
# systemd. Forging is mode-independent (proven on the bare bed), so the stand-in only needs to
# carry the `cardano-node run` argv the detector matches and be restartable by systemctl.
FROM debian:12-slim
RUN apt-get update -qq \
 && apt-get install -y -qq --no-install-recommends systemd systemd-sysv python3 procps \
 && rm -rf /var/lib/apt/lists/* \
 && rm -rf /lib/systemd/system/multi-user.target.wants/* \
          /etc/systemd/system/*.wants/* \
          /lib/systemd/system/systemd-update-utmp*

# Stand-in node: a non-exec shell loop so its cmdline stays `.../cardano-node run …` (what
# OURO_NODE_MATCH='cardano-node run' + pgrep -f match on). Handles SIGTERM so `systemctl
# restart` stops it promptly and starts a fresh PID.
RUN printf '%s\n' '#!/bin/sh' "trap 'exit 0' TERM INT" 'while true; do sleep 1 & wait $!; done' \
      > /usr/local/bin/cardano-node \
 && chmod 0755 /usr/local/bin/cardano-node

RUN printf '%s\n' \
      '[Unit]' \
      'Description=Cardano Node (stand-in for the S0017 supervision-mode E2E)' \
      '[Service]' \
      'ExecStart=/usr/local/bin/cardano-node run --config /opt/devnet/config.json --port 3001' \
      'Restart=no' \
      '[Install]' \
      'WantedBy=multi-user.target' \
      > /etc/systemd/system/cardano-node.service \
 && systemctl enable cardano-node.service

# The supervisor adapter + probe under test (same source as every other bed container).
COPY ouro-skills /opt/ouro/ouro-skills

CMD ["/sbin/init"]
