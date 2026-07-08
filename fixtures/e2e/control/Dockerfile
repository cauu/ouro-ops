# S0015 E2E bed — control node (drives ouro; SSHes to targets).
# FROM the shared base so control runs the SAME pinned ouro + skills as the targets
# (E2E-12). SSH key / ~/.ouro/credentials provisioning is added by p1-4.
FROM ouro-e2e-base:local

RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-client \
 && rm -rf /var/lib/apt/lists/*

CMD ["sleep", "infinity"]
