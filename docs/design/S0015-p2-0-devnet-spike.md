# S0015 p2-0 — devnet + Mithril feasibility spike (decision record)

> p2-0 gate for p2-1..p2-8. Outcome: **a real forging private Cardano devnet is FEASIBLE and
> reproducible** (evidence below). Mithril is **OUT** for S0015 (waiver). Recorded so p2-1
> executes against a known-good recipe instead of re-discovering it.

## Verdict
- **Private devnet with real block production: FEASIBLE.** Proven end-to-end: a single stake
  pool forges Conway blocks via real KES/VRF/opcert; `cardano-cli query tip` returns a live,
  advancing chain. Reproducible via `fixtures/e2e/devnet/devnet-up.sh`.
- **Mithril: OUT (signed waiver).** Mithril needs its own aggregator + signer stack certifying a
  network; it is not runnable against a throwaway private devnet without significant extra infra.
  p2-6 → `E2E-14` is satisfied by this waiver; `deploy/sync.sh` keeps `sync.mode: genesis` on the
  devnet (Mithril-mode digest/cert-chain evidence remains unit-tested per S0014 TC-21).

## Evidence (real runs, this environment)
- Image: `ghcr.io/intersectmbo/cardano-node:10.5.4` (node+cli, cardano-cli 10.11.0.0). ~802 MB.
- `cardano-cli conway genesis create-testnet-data --genesis-keys 1 --pools 1 …` generates a full
  all-era genesis + genesis-delegate keys + `pools-keys/pool1/{cold,vrf,kes,opcert}`.
- Node boots into the **Conway** era (hardfork-at-epoch-0), all 4 genesis hashes validated.
- `devnet-up.sh` fresh run: `NodeIsLeader` + `AdoptedBlock` events; tip reached `block:3` by slot 5
  (a prior run reached `block:32` by slot 64). Chain advances.

## The recipe (what p2-1 must do)
1. `create-testnet-data --genesis-keys 1 --pools 1 --stake-delegators 1 --utxo-keys 1 …`.
2. Tune `shelley-genesis.json`: `activeSlotsCoeff:0.5, securityParam:10, epochLength:500, updateQuorum:1`.
3. **Set `systemStart` (shelley) + `startTime` (byron) to ~now at BOOT.** This is the load-bearing
   gotcha: a stale `systemStart` puts the node at slot N with no blocks to bridge the Praos
   forecast window → `NoLedgerView` → it never forges. Must be fresh per container start.
4. Recompute the byron/shelley/alonzo/conway hashes → node `config.json` (with `Test*HardForkAtEpoch:0`,
   `UseTraceDispatcher:true`, `TraceOptions:{}`, and legacy `TurnOnLogMetrics`/`minSeverity`).
5. Run the node with the **pool** keys (`pools-keys/pool1/*`) — NOT the genesis-delegate keys, since
   in Praos/Conway only stake pools forge.

## Architecture / fidelity (RESOLVED — run native)
- The official `intersectmbo` image is **amd64-only** → under **qemu on arm64** it works but is slow,
  and emulation timing is NOT faithful (the node can miss slots). Emulation is fine for FUNCTIONAL /
  crypto / security fidelity (same binary, bit-exact crypto, Linux-semantics isolation) but not for
  timing-sensitive assertions.
- **Resolution: use the arm64-NATIVE `ghcr.io/blinklabs-io/cardano-node:10.5.4` image** (multi-arch;
  `cardano-cli 10.14.0.0 - linux-aarch64`). Verified: the SAME recipe forges natively with **no
  emulation** (`uname: aarch64`), reaching `block:4` by slot 4 — healthier than the emulated run
  (`block:3` by slot 5), i.e. the timing penalty is gone. `devnet-up.sh` now defaults to this image
  (`PLATFORM=linux/arm64`); override `NODE_IMG`/`PLATFORM` for an amd64 CI runner.
- **Consequence for p2 test design (still holds):** assert on STATE (tip block>0, forging enabled,
  opcert installed, counter monotonic, sudoers denies), NOT on wall-clock rates/deadlines — so the
  gate is robust whether run on arm64-native or an amd64 runner. Pin the node image digest in p2-1 (E2E-11).
- **pool-spec schema gap**: `spec.network` is a fixed enum (mainnet/preprod/preview with fixed
  magics); an arbitrary devnet magic (42) has no representation. p2-1 either runs the devnet with
  **magic 1 (preprod)** — a private isolated network can reuse the magic harmlessly — or the schema
  gains a `devnet` network. `examples/pool-spec.devnet.yaml` uses preprod/magic-1 for schema validity.
- Cold-start: a single pool holding the delegated stake forges from epoch 0; this is why
  `--genesis-keys 1 --pools 1` + fresh systemStart is used rather than multi-delegate OBFT.

## Deliverables (p2-0)
- `fixtures/e2e/devnet/devnet-up.sh` — reproducible forging-devnet recipe (verified).
- `examples/pool-spec.devnet.yaml` — devnet pool-spec (preprod/magic-1, `sync.mode: genesis`).
- This decision record. p2-6/E2E-14 Mithril waiver recorded above.

## Recommendation for p2-1
Integrate `devnet-up.sh` as a `cardano-node` service/entrypoint in the bed (fresh systemStart on
boot) using the **arm64-native blinklabs image** (no emulation → faithful timing on this host);
then p2-2 wires `ouro status`/`deploy/verify` to query the real socket, replacing the injected
snapshot. The amd64 path remains available via `NODE_IMG`/`PLATFORM` for amd64 CI runners.
