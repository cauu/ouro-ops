# S0020 Agentless Operations

This is the current non-deploy operator contract. The embedded Skill shown by
`ouro-ops skill show <name>` remains the executable runbook authority.

## Target model

- The control machine holds the complete CLI, Skills, named `creds://` references, known-hosts,
  confirmations, permits and audit state.
- Every ordinary target command automatically transports the control release's static Linux runner
  as the existing `cardano` account, verifies it in a run-unique private directory, executes a
  closed target action, bounds output/deadline and removes the directory.
- A target-installed Ouro CLI, remote Ouro version, adoption attestation, global managed marker,
  resident gate/daemon and persistent remote transaction journal are not prerequisites.
- Deployment and Upgrade fetch the current signed `releases.json` without caching. A tag is
  descriptive; config/index/manifest digests are the identity. Other operations enforce the stable
  layout contract without requiring the current image to be listed in a changing release catalog.

## Supported non-deploy operations

| Operation | Read / write | Authorization | Acceptance-safe endpoint |
| --- | --- | --- | --- |
| `observability/health` | read | none | real BP + relay live read |
| `troubleshooting/snapshot` | read | none | real role-aware baseline; no overall-health claim |
| `runtime/restart` | disruptive write | exact confirmation + fleet permit | stop after `--plan` |
| `kes-rotation/install-opcert` | disruptive public-artifact write | exact confirmation + fleet permit | local preview + target `--plan` only |
| `upgrade/preload-image` | non-disruptive image-store write | exact confirmation | local preview + target `--plan` only |
| `upgrade/step` | disruptive recreate | exact confirmation + fleet permit + signed N→N+1 transition | typed safe refusal / `--plan` only |
| `diag exec` | diagnostic command through existing operator SSH | no write capability minted | real diagnostic-only commands |

`config/render` and topology mutation remain unsupported. `deploy/register-submit` is outside
S0020. Legacy `onboard`/`adopt` are migration/recovery tools only.

## Plan → approval → apply

For runtime, KES and upgrade, the public sequence is:

1. Run the operation with `--spec <pool-spec> ... --plan`, without confirmation or permit.
2. Show the exact returned plan and final `candidate_hash`/`intent_hash`; wait for operator approval.
3. Mint `ouro-ops confirm create --op <op> --node <id> --intent-hash <final-hash>`.
4. For a disruptive operation, mint `ouro-ops fleet permit create ...` last.
5. Immediately rerun the same operation without `--plan`, adding `--candidate-hash`, the exact
   confirmation and (when required) fleet permit. Artifact operations also add `--artifact-file`.

Apply re-probes and refuses candidate drift before mutation. Capabilities must never be included in
plan mode or interpreted from target output.

Before Upgrade, select the signed next hop from the current live image config digest:

```text
ouro-ops release select --platform linux/amd64 --from sha256:<current-config-digest>
```

The command returns the release label and exact OCI tuple. Upgrade plan/apply and its fleet permit
fetch and verify the document again; a changed release policy changes the candidate and invalidates
the old approval. No command writes a release cache or target policy file.

## Public artifacts

Use `ouro-ops inbox preview --type opcert|image --file <operator-named-file>` on control. It validates
the public artifact and returns a content-addressed reference without writing an inbox. Put that ref
in the target plan. Approved apply reopens the same file, verifies its bytes against the candidate,
and streams `runner || artifact` in one private invocation. No separate remote-stage step exists.

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
