# S0015 E2E bed — target node host (bp1 / relay1 / relay2).
# Skeleton only (p1-1): sshd + the two principals + a placeholder 0400 key dir.
# `ouro` install (p1-2), authorized_keys / creds (p1-4), sudoers.d body (p1-5),
# and cardano-node (p2-1) are added by later items.
# Base image is digest-pinned for reproducibility (p1-1 / E2E-11 image-pin branch).
FROM debian:12-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df

RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-server sudo ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/sshd \
 && ssh-keygen -A \
 && useradd -m -s /bin/bash ouro-exec \
 && useradd -m -s /bin/bash ouro-diag \
 && useradd -m -s /usr/sbin/nologin node \
 && mkdir -p /opt/cardano/keys \
 && printf 'placeholder-not-a-real-key\n' > /opt/cardano/keys/kes.skey \
 && chown -R node:node /opt/cardano/keys \
 && chmod 0700 /opt/cardano/keys \
 && chmod 0400 /opt/cardano/keys/kes.skey

EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
