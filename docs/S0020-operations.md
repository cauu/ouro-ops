# Agentless Operations

This is the current typed-operation contract. The website-copied prompt contains one complete
canonical external Skill; its mandatory first action is the pure `ouro-ops contract check`
compatibility preflight. The CLI carries execution mechanisms and no decision Skill text.

## Target model

- The control machine holds the complete CLI, Skills, named `creds://` references, known-hosts,
  confirmations, permits and audit state.
- Every ordinary target command automatically transports the control release's static Linux runner
  as that machine's declared existing SSH account, verifies it in a run-unique private directory, executes a
  closed target action, bounds output/deadline and removes the directory.
- A target-installed Ouro CLI, remote Ouro version, adoption attestation, global managed marker,
  resident gate/daemon and persistent remote transaction journal are not prerequisites.
- Deployment and Upgrade fetch the current signed `releases.json` without caching. A tag is
  descriptive; config/index/manifest digests are the identity. Other operations enforce the stable
  layout contract without requiring the current image to be listed in a changing release catalog.

## Supported operations

| Operation | Read / write | Authorization | Acceptance-safe endpoint |
| --- | --- | --- | --- |
| `observability/health` | read | none | real BP + relay live read |
| `troubleshooting/snapshot` | read | none | real role-aware baseline; no overall-health claim |
| `runtime/restart` | disruptive write | exact confirmation + fleet permit | stop after `--plan` |
| `kes-rotation/stage-key` | non-disruptive private-key staging | exact confirmation; no fleet permit | target `--plan` only |
| `kes-rotation/discard-stage` | destructive pending-key cleanup | exact confirmation; no fleet permit | target `--plan` only |
| `kes-rotation/install-opcert` | disruptive matched KES-pair + public-opcert activation | exact confirmation + fleet permit | local preview + target preflight/`--plan` only |
| `upgrade/preload-image` | non-disruptive exact GHCR pull | exact confirmation | target `--plan` only |
| `upgrade/step` | direct Docker-run disruptive recreate | exact confirmation + fleet permit + signed recommended target | Compose/unsupported refuse before mutation; run uses typed safe refusal / `--plan` only |
| `deploy/register-submit` | irreversible transaction submission | exact confirmation; no fleet permit | signed transaction preview + target `--plan` only |
| `diag exec` | diagnostic command through existing operator SSH | no write capability minted | real diagnostic-only commands |

`config/render` and topology mutation remain unsupported. Legacy `onboard`/`adopt` are
migration/recovery tools only.

## Plan → approval → apply

For runtime, KES, upgrade and Deploy, the public sequence is:

1. Run the operation with `--spec <pool-spec> ... --plan`, without confirmation or permit.
2. Show the exact returned plan and final `candidate_hash`/`intent_hash`; wait for operator approval.
3. Mint `ouro-ops confirm create --op <op> --node <id> --intent-hash <final-hash>`.
4. For a disruptive runtime/KES-activation/upgrade operation, mint `ouro-ops fleet permit create
   ...` last. KES staging and Deploy take no fleet permit.
5. Immediately rerun the same operation without `--plan`, adding `--candidate-hash`, the exact
   confirmation and (when required) fleet permit. KES and Deploy artifact operations also add
   `--artifact-file`; Upgrade never accepts an image artifact.

Apply re-probes and refuses candidate drift before mutation. Capabilities must never be included in
plan mode or interpreted from target output.

Before Upgrade, select the current platform's signed recommended release from the live image config
digest:

```text
ouro-ops release select --platform linux/amd64 --from sha256:<current-config-digest>
```

The command returns the recommended release label, fixed Blink Labs repository and exact OCI tuple.
It does not walk intermediate releases. The current image must be trusted and the target must equal
the signed recommendation. Exact source-to-target transition metadata is optional: when it is absent
the upgrade remains valid, but automatic rollback is unavailable and failure recovery is forward or
re-sync. Upgrade
preparation pulls the signed `repository@platform-manifest-digest` directly on the target only after
candidate approval, then verifies repository/platform/config while proving the active container is
unchanged. Upgrade plan/apply and its fleet permit
fetch and verify the document again; a changed release policy changes the candidate and invalidates
the old approval. No command writes a release cache or target policy file.

Route Upgrade from `observability/health` container facts before planning activation:

- `orchestration: run`: use the sealed preload/step flow. RecreateSpec preserves the observed name,
  restart policy, network, ports, env, binds, entrypoint, args, user, supplementary groups and
  labels, plus the `json-file` log driver and its supported `max-file`/`max-size` rotation options.
  Apply revalidates this spec before mutation and verifies it with the target digest and readiness
  afterwards. Other logging drivers/options remain fail-closed rather than being dropped.
- `orchestration: compose`: the agent shows the signed release and immutable
  `repository@platform-manifest-digest`, observed project/service/working directory/config files,
  and manual `docker compose config` plus `docker compose up -d --no-deps <service>` templates.
  The user edits the Compose image and runs those commands. The agent performs no raw Compose write.
  After the user reports completion, rerun `observability/health` and check Compose ownership,
  project/service when observable, target config digest, container, socket, sync and role readiness.
  This is a fresh current-state check, not a transaction, pending/finalize flow or verify-rebind.
- `orchestration: unsupported`: stop and show `orchestration_reason`; do not guess an owner or fall
  back to a bare container recreate.

Compose facts may be incomplete without changing the classification. Ask the user for missing
project, service or config files; never infer paths. `upgrade/step` plan and apply return
`manual_compose_required` for Compose and `unsupported_orchestration` for other owners before any
Docker rename/run/remove action.

SSH usernames are also operator facts. After compatibility preflight and before writing the pool
spec or contacting a host, the agent first asks whether all machines share one SSH username. It asks
once when they do, otherwise collects a machine-id-to-username mapping. It never assumes `cardano`,
`root`, or the control machine's current user, and never asks for passwords or private-key content.

## Public artifacts

KES rotation first runs `kes-rotation/stage-key`. The target derives the current KES period and
generates a fresh pair in a fixed BP-private staging directory; only the public verification-key
envelope/hash leaves the BP. The existing KES/opcert may already be invalid (a primary reason to
rotate); Phase A requires an answering container/socket and proves that it preserved the complete
pre-existing active KES/readiness state rather than requiring the old credentials to forge. After
staging, the target's typed `cardano_cli_version` plus one operator-selected device class drives
`ouro-ops kes airgap-bundle`. The local command downloads the matching official Intersect release,
verifies its published archive checksum and atomically emits `kes.vkey`, `cold-sign.sh`, executable
`cardano-cli`, `manifest.json` and `SHA256SUMS`; Ouro does not host that binary. The four user-facing
choices are M-series Mac, Intel Mac, Intel/AMD Linux and ARM Linux, with `uname -s` plus `uname -m`
as the only fallback when the operator does not recognize the device. The cold script verifies its
adjacent public manifest, vkey, executable digest and reported version before it reads the counter,
so no preinstalled CLI or network is needed on the air-gapped machine. If a complete staged pair
already exists, the same Phase-A plan returns that PUBLIC vkey plus current period/version with no
executor steps, then requires the operator to choose whether to continue or discard it. Continue
uses the public evidence without target mutation. Discard is a separate candidate-bound confirmed
write that removes only the fixed stage; a new pair requires another plan and approval. Incomplete
or unsafe staging residue remains a typed refusal. After the offline cold-signing handoff,
`install-opcert` requires the returned certificate to name that exact staged public key. Approved
activation backs up and
promotes `kes.skey`, `kes.vkey` and `node.cert` together, restarts once, verifies typed readiness,
and polls candidate-bound KES evidence for a bounded interval. It never automatically restores or
restarts the previous triple because a running process does not prove that old disk credentials are
restart-safe. An unverified activation retains the promoted candidate, stage and previous-file
recovery material; the same Phase-B workflow can later verify and clean that state without another
install or restart. Success verifies the fixed stage and all recovery backups are absent before
reporting completion.

A BP Docker restart loop is handled only as a bounded Phase-B branch; it is not a third KES phase or
a general offline-operation framework. Ouro derives network/genesis/layout, public opcert hashes and
key metadata from the already signed fixed bind layout, while one declared healthy relay validates
the candidate's current KES window and protocol counter. The signed fleet permit binds that relay
evidence to the exact public `node.cert`. Apply then runs the exact current node image as one
network-disabled, no-pull, auto-removed filesystem helper, performs one stop/promote/start, and uses
the ordinary candidate-bound postcondition before deleting staged/recovery material. It never runs
Phase A, generates another key, advances the cold counter, or restores the known-bad old triple.

Use `ouro-ops inbox preview --type opcert|tx --file <operator-named-file>` on control. It validates
the public artifact and returns a content-addressed reference without writing an inbox. Put that ref
in the target plan. Approved apply reopens the same file, verifies its bytes against the candidate,
and streams `runner || artifact` in one private invocation. No separate remote-stage step exists.

For Deploy, the target reopens the exact signed transaction, derives its txid and normalized
effects with `cardano-cli`, queries each exact input against the live node, and accepts only one
matching pool-registration certificate with no unrelated chain effects. The sampled slot proves
that the validity interval was checked but does not make normal chain progress candidate drift;
apply rechecks the current slot. Approval authorizes at most one fixed submit attempt. A normal
rejection and an ambiguous transport result are both terminal and are never retried.
`accepted_by_node` does not prove ledger inclusion; reconciliation is reported separately as
confirmed, pending or unknown/not observed.

## Troubleshooting assurance

Start with the exact target's typed baseline:

```text
ouro-ops op run --op troubleshooting/snapshot --spec <pool-spec> --dispatch <spec-ssh-host> \
  --ssh-key creds://<spec-name> --node <machine-id> --param machine=<machine-id>
```

The snapshot reports current liveness, sync, peers and role-specific forging/KES evidence. A
`role_readiness: ready` result is bounded to those facts and explicitly does not claim overall
health. A BP is not forging-ready unless KES/opcert evidence is available and valid and
`block_production_ready` is true.

For symptom-relevant gaps only,
`ouro-ops diag exec --dispatch <machine-id> --spec <pool-spec> -- <command>` uses the spec's existing
operator account. Ouro adds no privilege escalation and enforces pinned transport, bounded
deadline/output and control audit. It does **not** enforce read-only OS permissions; the Skill
requires diagnostic-only intent. Repairs must use a supported typed operation.

## Failure routing

Access/credential/host-key errors go back to operator-owned control configuration. Signed-policy,
role/network/genesis/layout or live-state mismatches are reported as typed refusals. Ordinary flows
must not respond by installing target software, onboarding, adopting, synchronizing a remote CLI,
creating credentials, or reshaping the node.
