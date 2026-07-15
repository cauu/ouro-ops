# S0019 Supported / Retired / Unsupported Operations (§2.15)

Every real-operator remediation is classified. A supported mutation has a registry schema +
executor + transaction policy; an unsupported one returns a typed refusal with a documented
operator-only recovery path. No operation may fall through to the bootstrap bypass or ad-hoc
manual commands.

## Supported (in the deny-by-default write registry, §2.5)
| operation_id | mutability | confirm-token | notes |
|---|---|---|---|
| runtime/restart | dangerous | yes | BP restart interrupts forging (§2.6a) |
| kes-rotation/install-opcert | dangerous | yes | installs only the public opcert via inbox (§2.7); no KES private-key work |
| deploy/register-submit | dangerous | yes | irreversible on-chain; tx via inbox |
| observability/health | read | no | managed read; no mutation, no confirm |
| upgrade/step | dangerous | yes | one N→N+1 step; image via inbox artifact |

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
  retired ids return a typed refusal. Existing durable journals for the retired ids remain
  recoverable when the next supported operation runs startup recovery.
- any operation not in the registry — refused deny-by-default (§2.5).
