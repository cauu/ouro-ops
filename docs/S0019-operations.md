# S0019 Supported / Retired / Unsupported Operations (§2.15)

Every real-operator remediation is classified. A supported mutation has a registry schema +
executor + transaction policy; an unsupported one returns a typed refusal with a documented
operator-only recovery path. No operation may fall through to the bootstrap bypass or ad-hoc
manual commands.

## Supported (in the deny-by-default managed-operation registry, §2.5)
| operation_id | mutability | confirm-token | notes |
|---|---|---|---|
| runtime/restart | dangerous | yes | BP restart interrupts forging (§2.6a) |
| kes-rotation/install-opcert | dangerous | yes | installs only the public opcert via inbox (§2.7); no KES private-key work |
| deploy/register-submit | dangerous | yes | irreversible on-chain; tx via inbox |
| observability/health | read | no | managed read; no mutation, no confirm |
| fleet/status | read | no | internal closed role/network/genesis/host-key/readiness/image/generation projection; no agent-supplied counts |
| upgrade/preload-image | dangerous | yes | loads one staged Docker-save archive only after exact archive→config-digest→signed-allowlist binding; running node untouched |
| upgrade/step | dangerous | yes | one N→N+1 step to an exact preloaded image config digest |

Disruptive operations bind the final plan to the stable pool id, exact pool-spec revision and
minimum-online-relay policy. After exact approval, confirmation is minted; a target/host-key/fleet
snapshot permit is minted last, expires after 30 seconds, and is used immediately without replanning.
Plan rejects both permit and confirmation capabilities.

Upgrade image ingress is a separate non-disruptive managed write: `inbox stage --type image`
content-addresses the operator-named Docker-save archive, then `upgrade/preload-image` proves it has
exactly one config matching the approved allowlisted digest and that the digest was absent before
`docker load`. It verifies the exact digest afterward and has a fixed image-removal rollback; it
does not receive a fleet permit because it never restarts or recreates the running node. Only after
that succeeds may `upgrade/step` enter the permit-last disruptive flow.

## Retired (S0017 tools NOT carried into S0019; disabled by §2.8)
- config/render — retired until a closed config artifact and real sealed renderer exist; a restart is not rendering.
- runtime/topology-apply — retired until topology bytes are delivered and verified by a typed intent; a restart is not apply.
- kes-rotation/rotate — renamed to kes-rotation/install-opcert because ouro installs only the public opcert and never rotates the KES signing key.
- kes-rotation/generate-offline, kes-rotation/push-offline — the private-key ceremony remains operator-owned and offline.
- deploy/register-build, deploy/provision, deploy/sync, deploy/start, deploy/takeover — greenfield deploy is a non-goal; the operator stands up a conforming node, ouro adopts.
- observability/install-gateway — the BP never gets a public endpoint; health is read via the unprivileged diag/read path.
- upgrade/rollout, upgrade/upgrade-one — replaced by the node-runtime upgrade protocol (§2.10).

## Unsupported (typed refusal + operator-only recovery path)
- disk cleanup / volume expansion — operator performs on the host; ouro reports pressure (read path) and refuses to mutate the host filesystem.
- time / firewall / host service repair — host administration is the operator's; out of scope.
- host service-definition changes — the supervisor contract is fixed (§2.2); a change means re-adopt.
- clearing the write-seal — operator-only recovery (§2.6); never an agent write.
- config or topology mutation — operator-only until a typed, sealed executor is implemented; the
  retired ids return a typed refusal.
- pending transaction reconciliation — ordinary plan/read/write commands fail closed and never
  auto-verify or auto-rollback a journal before fresh authorization. Recovery is operator-owned
  until a separately planned, fleet/confirmation-bound recovery intent exists.
- any operation not in the registry — refused deny-by-default (§2.5).
