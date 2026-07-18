# Current website → agent → Ouro applicability boundary

This document supersedes S0016's embedded-decision model. The current static website copies one
complete canonical external Skill plus operation-scoped pool data into a prompt. The verified local
CLI contains the typed execution mechanism, not Skill prose. The Skill's mandatory first Ouro action
is the pure compatibility check; normal target operations use the release-paired ephemeral runner
and require no target-resident Ouro installation or state.

## What the flow enforces

| Dimension | Current boundary |
| --- | --- |
| Decision source | The copied prompt contains the complete canonical Skill generated from `ouro-skills/`; site build tests prove byte fidelity. |
| Compatibility | `ouro-ops contract check` validates the Skill's CLI/contract requirement without filesystem, credential, network, SSH or audit state. |
| Typed writes | Closed operation schema, target-side live plan, exact candidate confirmation and operation-specific fleet permit. |
| Runner identity | The macOS release embeds its paired static Linux/x86_64 runner; public commands cannot select runner bytes/path/hash. |
| Runtime images | A pinned-key-verified live catalog authorizes one fixed Blink Labs GHCR repository and exact OCI tuples/transitions; Upgrade never accepts node-image bytes. |
| Site data | The CSP-locked generator makes no ambient network request. Copying still discloses the rendered topology to the chosen agent/provider. |

## Residual risks — disclosed and accepted

- An agent with a general shell and the operator's SSH credential can deliberately act outside
  Ouro. The current product prioritizes a correct, low-friction typed workflow and does not claim
  mechanism isolation from raw terminal use.
- The control machine remains sensitive because it holds fleet-reaching credentials. Key contents
  must not enter prompts, JSON or audit output; hardware-backed keys remain preferable.
- The website is zero-upload, but the prompt contains hostnames/IPs/roles. Pasting it into a cloud
  agent sends those values to that provider and its logs.
- Root, the host kernel, Docker daemon and the selected node image's own code remain trusted. Digest
  pinning proves identity, not absence of upstream vulnerabilities.

## Supply-chain split

- The CLI release candidate is a macOS control binary with its exact embedded runner. Skills and
  node images are excluded. Formal CLI signing/publication is deferred to the next spec.
- The small signed `releases.json` catalog is already public and independently Ed25519-verified by
  the CLI. It contains metadata only; image layers come directly from Blink Labs GHCR.
- First installation remains a deliberate trust decision. Until formal CLI publication exists, do
  not present placeholder Homebrew/install vectors as production-ready.

## When not to use this mode

Do not give a general-purpose agent a credentialed control terminal if policy requires strict
mechanism isolation from raw SSH or forbids sending topology to the agent provider. In that case,
run the typed Ouro commands manually after reviewing the copied Skill and plans.
