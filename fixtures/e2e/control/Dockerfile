# S0015 E2E bed — control node (drives ouro; SSHes to targets).
# FROM the shared base so control runs the SAME pinned ouro + skills as the targets
# (E2E-12). SSH key / ~/.ouro/credentials provisioning is added by p1-4.
FROM ouro-e2e-base:local

# python3 (+ yaml) is required by control-side orchestrators (upgrade/rollout parses the
# spec to plan the relays-first/BP-last order + quorum) and to emit their JSON output.
RUN apt-get -o Acquire::Retries=3 update \
 && apt-get -o Acquire::Retries=3 install -y --no-install-recommends openssh-client python3 python3-yaml \
 && rm -rf /var/lib/apt/lists/*

CMD ["sleep", "infinity"]
