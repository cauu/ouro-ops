# S0027 real Ubuntu acceptance

This harness is destructive only to two **fresh, dedicated** Ubuntu 22.04/24.04 hosts. It runs the
real signed Blink Labs image and the product `deploy inspect|apply|check` path. Docker containers
cannot satisfy this acceptance honestly: the signed Preview policy requires at least 8 GiB RAM and
100 GiB free disk per host, and the final result must be Cardano-ready after real Mithril restore and
replay.

The two hosts must:

- be reachable from the control machine;
- have different declared SSH users (the harness enforces this);
- have root or passwordless `sudo -n`;
- have at least 8 GiB RAM and 100 GiB free disk each;
- be fresh: no `/opt/ouro`, Cardano service/container, database or legacy config;
- allow outbound HTTPS and Preview Cardano traffic.

Credentials stay in `${OURO_HOME:-$HOME/.ouro}/credentials` and are referenced as `creds://...`.
The harness never accepts private-key bytes or passwords.

First generate the operation-scoped spec and print the two user-only trust commands:

```bash
export S0027_E2E_BP_HOST=...
export S0027_E2E_BP_USER=...
export S0027_E2E_BP_KEY_REF=creds://...
export S0027_E2E_RELAY_HOST=...
export S0027_E2E_RELAY_USER=...
export S0027_E2E_RELAY_KEY_REF=creds://...
export S0027_E2E_RELAY_PUBLIC_HOST=...
fixtures/e2e/s0027/run.sh prepare
```

Run both printed `ouro-ops ssh trust` commands yourself in a terminal and confirm the displayed
fingerprints. Then explicitly authorize writes to the dedicated fresh hosts:

```bash
export S0027_E2E_ALLOW_FRESH_HOST_WRITES=YES
fixtures/e2e/s0027/run.sh run
```

The run fails unless it observes clean Inspect, an immediate real Mithril/replay `pending`, eventual
all-node `ready`, private metrics, a no-write `already_deployed` rerun, and S0026 Compose ownership
evidence. It never treats a final `pending` as pass.
