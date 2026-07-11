# S0017 p2-9 — stand-in node image for the docker/compose upgrade E2E. No apt (avoids the
# flaky package fetch): just a long-lived process whose argv is `cardano-node run` so the
# supervisor adapter recognizes it. `ARG V` makes v1/v2 distinct image CONTENT ids so the
# upgrade's "running image id == declared image id" convergence check is meaningful.
FROM debian:12-slim
ARG V=1
RUN printf '%s\n' '#!/bin/sh' "trap 'exit 0' TERM INT" 'while true; do sleep 1 & wait $!; done' \
      > /usr/local/bin/cardano-node \
 && chmod 0755 /usr/local/bin/cardano-node \
 && printf 'node-image-version=%s\n' "$V" > /version
ENTRYPOINT ["/usr/local/bin/cardano-node"]
CMD ["run"]
