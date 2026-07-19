# Current typed-operation threat matrix

Prompt text is a decision procedure; enforcement belongs to the local CLI and one-shot target
runner. Ordinary operations do not use the former S0019 adoption attestation, installed target CLI,
resident gate or persistent target transaction journal.

## Trusted computing base

- Control-host kernel/process environment and the operator-selected `ouro-ops` binary.
- Target root/kernel and rootful Docker daemon.
- The running Cardano node image's code; exact OCI identity does not prove vulnerability absence.
- The operator's independently pinned SSH host key and credential custody.
- For signed release policy, the pinned Ouro Ed25519 public key and Blink Labs GHCR availability.

## Enforced boundaries

| Adversary / fault | Enforcing component | Acceptance evidence |
| --- | --- | --- |
| Incompatible external Skill | pure `contract check`, closed requirement grammar (§2.2) | contract unit/subprocess and website first-action tests |
| Hostile or extra operation parameter | deny-by-default registry, closed typed params, fixed argv (§2.2) | intent/executor negative tests |
| Replaced runner in transport | release-paired embedded bytes, control-known SHA-256, private run directory (§2.6) | dispatch and release-candidate tests |
| Wrong target / role / network / genesis / mounts | pool-spec binding plus fresh typed target probe (§2.5) | stateless plan/apply tests |
| Live drift after approval | candidate regeneration immediately before mutation (§2.5) | stateless apply drift tests |
| Confirmation or fleet capability misuse | exact intent binding, single use, expiry, quorum and BP-last checks (§2.5) | confirm/fleet tests and live Runtime acceptance |
| Hostile public opcert/transaction | bounded one-shot payload, digest and domain validation (§2.5) | KES/Deploy workflow tests |
| Tag, alternate registry or wrong OCI tuple | signed fixed repository, exact platform manifest/config verification (§2.4–§2.5) | release-catalog and Upgrade workflow tests |
| Node image supplied through Ouro | no image ArtifactType, inbox path, payload flag or package content (§2.5–§2.6) | source/package inventory and control transport tests |
| Unsafe runtime upgrade rollback claim | signed directed transition and explicit DB compatibility (§2.4–§2.7) | Upgrade success/rollback/forward-recovery tests |
| Diagnostic overclaim | role-aware typed snapshot first; gaps disclosed; repair remains typed (§2.1–§2.2) | troubleshooting Skill and real conclusion acceptance |
| Agent deliberately bypassing Ouro | not mechanism-isolated in the current product (§2.7) | explicitly residual; no stronger claim |

## Stateful legacy boundary

`onboard` and `adopt` remain explicit migration/recovery utilities only. Their resident wrapper,
attestation and journal model is not an ordinary-operation prerequisite and must not be proposed in
response to a missing remote Ouro binary or version mismatch.

**OUT OF SCOPE:** convenience-mode custody of a bootstrap credential during those explicit legacy
migration/recovery utilities remains part of the control-host trust boundary; ordinary operations do
not request or transport that credential.
