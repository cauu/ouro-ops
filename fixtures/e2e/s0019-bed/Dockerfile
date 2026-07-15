# S0019 p5-6 — a lightweight stand-in for a conforming blinklabs node: the CONVENTION LAYOUT
# (in-container paths) + a fake cardano-node/cli, so the real adopt + op pipeline can run against a
# real container (real docker restart, real probe) without a multi-GB image or a chain sync.
FROM busybox:1.36
RUN mkdir -p /ipc /data/db /opt/cardano/config/mainnet /opt/cardano/config/keys /usr/local/bin
# Fake cardano-cli (answers `query tip` etc.) and cardano-node (keeps the process alive; its argv
# contains "cardano-node" so the probe finds it).
RUN printf '#!/bin/sh\necho "{\\"block\\":1,\\"era\\":\\"Conway\\"}"\n' > /usr/local/bin/cardano-cli \
 && printf '#!/bin/sh\nwhile true; do sleep 1; done\n' > /usr/local/bin/cardano-node \
 && chmod +x /usr/local/bin/cardano-cli /usr/local/bin/cardano-node
# Convention files (non-secret) + a BP key set so the role check (bp requires opcert) passes.
RUN echo '{"Producers":[]}' > /opt/cardano/config/mainnet/topology.json \
 && echo '{"NodeVersion":1}'  > /opt/cardano/config/mainnet/config.json \
 && echo '{"genesis":true}'   > /opt/cardano/config/mainnet/shelley-genesis.json \
 && echo 'opcert'             > /opt/cardano/config/keys/node.cert \
 && echo 'kes'                > /opt/cardano/config/keys/kes.skey
CMD ["cardano-node", "run", "--socket-path", "/ipc/node.socket", "--database-path", "/data/db"]
