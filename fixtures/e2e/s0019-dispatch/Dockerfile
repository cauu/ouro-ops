# S0019 p6-4 — a Linux TARGET host for the dispatch path: sshd + the S0019 confinement (ouro-op
# principal + op wrapper + sudoers) + the linux ouro-ops binary + a STUB docker (standing in for
# the node container the executor manages). Control (the test host) SSH-dispatches adopt + op here,
# proving the full chain: SSH -> confined wrapper -> op --local -> auto-probe -> gates -> executor.
FROM alpine:3.19
RUN apk add --no-cache openssh sudo python3 bash coreutils \
 && mkdir -p /run/sshd /var/lib/ouro /usr/local/sbin /usr/local/bin /opt /srv/data
# The S0019 confined principals + op wrapper + sudoers (what `ouro-ops onboard` installs).
RUN addgroup -S ouro-attest \
 && adduser -D -s /bin/bash ouro-op && adduser -D -s /bin/bash ouro-diag \
 && addgroup ouro-op ouro-attest && addgroup ouro-diag ouro-attest \
 && chown root:ouro-attest /var/lib/ouro && chmod 0750 /var/lib/ouro \
 && printf '#!/bin/sh\nexec /usr/local/bin/ouro-ops op "$@"\n' > /usr/local/sbin/ouro-op-run \
 && chmod 0755 /usr/local/sbin/ouro-op-run \
 && printf 'ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-op-run\n' > /etc/sudoers.d/ouro-op \
 && chmod 0440 /etc/sudoers.d/ouro-op
# A STUB docker that mimics the node container the probe/executor act on (records restart calls).
COPY docker-stub.sh /usr/local/bin/docker
COPY ouro-ops /usr/local/bin/ouro-ops
COPY ouro-probe.sh /opt/ouro-probe.sh
RUN chmod 0755 /usr/local/bin/docker /usr/local/bin/ouro-ops
RUN ssh-keygen -A && sed -i 's/#PasswordAuthentication yes/PasswordAuthentication no/' /etc/ssh/sshd_config
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
