# S0015 E2E bed — control node (drives ouro; SSHes to targets).
# Skeleton only (p1-1): ssh client + bash. `ouro` install (p1-2) and SSH key /
# ~/.ouro/credentials provisioning (p1-4) are added by later items.
FROM debian:12-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df

RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-client ca-certificates \
 && rm -rf /var/lib/apt/lists/*

CMD ["sleep", "infinity"]
