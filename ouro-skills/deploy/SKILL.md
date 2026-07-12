---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Deploy Skill

## Purpose
Deploy or take over one pool from a validated `pool-spec.yaml`.

## Decision Tree
- New machine path: `ouro-ops spec validate` -> `deploy/preflight` -> `deploy/provision` -> `deploy/sync` -> `deploy/start` -> `deploy/verify`.
- Existing node takeover path: `ouro-ops spec validate` -> `deploy/preflight` -> `deploy/takeover` -> `deploy/takeover-verify` -> `deploy/start` -> `deploy/verify`.
- Mithril sync requires snapshot digest and certificate-chain evidence before `deploy/sync` may pass.

Pool registration (cold key kept OFFLINE — staged cold-sign):
- The operator pre-creates the new pool's cold/vrf/stake keys OFFLINE and stages only the PUBLIC
  cold.vkey/vrf.vkey/stake.vkey (plus the operational stake key) on the online BP. cold.skey stays offline.
- Build the unsigned registration tx online with
  `ouro-ops tool run deploy/register-build --dispatch <bp> --spec <pool-spec>`. It gathers the live
  chain snapshot, builds the stake + pool registration certs and the unsigned tx, produces the ONLINE
  witnesses (payment + owner stake), and returns the public `tx_body` + `pool_id`. It never reads cold.skey.
- Write the returned `tx_body` to a file and generate the offline signing script with
  `ouro-ops deploy cold-sign-script --tx-body <that file> --cold-key cold --testnet-magic <magic>`
  (or `--mainnet`). It embeds ONLY the public tx body; it contains NO private key. Hand it to the operator.
- The operator runs it on the AIR-GAPPED machine; it witnesses with the pool cold key IN PLACE
  (`COLD_SKEY=`) and returns the public `cold.witness`, placed on the BP at the staging path.
- Request an evidence-bound confirmation (`ouro-ops tool run detect/runtime --dispatch <bp>` then
  `ouro-ops confirm create --action deploy/register-submit --machine <bp> --runtime-evidence <hash>`).
- Submit with `ouro-ops tool run deploy/register-submit --dispatch <bp> --spec <pool-spec>
  --confirm-token <tok>`. It assembles the online + cold witnesses, submits, and ground-truths the
  pool id is registered on chain. It refuses to resubmit an already-registered pool.

## Stop Conditions
- Stop on exit 10 and ask for corrected inputs or missing audit context.
- Stop on exit 20 and run L3 read-only diagnostics only.
- Stop on exit 30 and use rollback-capable path before any further write.
- Stop on exit 40 and require human intervention.

## Red Lines
- Writes only through `ouro-ops tool run`.
- L3 diagnostics are read-only and have no secret directory access.
- No cold, KES secret, or VRF material enters context or output.
- Every change step is followed by verify.
- On exit 30, run the rollback-capable path before continuing.
- On exit 40, stop all writes and require human intervention.
