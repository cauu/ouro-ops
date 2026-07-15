---
skill_version: 2
requires_ouro: ">=0.1.0"
---
# Detect Skill

## Purpose
Explain the S0019 detection boundary without reviving S0017's adaptive supervisor path. S0019
supports one digest-pinned container convention: detection occurs only in adoption preview and in the
live re-attestation gate before each managed operation.

## Decision Tree
- Do not run `ouro-ops tool run detect/runtime`: that S0017 command is retired and returns a typed
  refusal. Without `--dispatch` it used to inspect the control machine, and its remote privilege
  path no longer exists.
- For an unmanaged node, use the exact non-mutating adoption assessment:
  `ouro-ops adopt --dispatch <host> --bootstrap-user <account> --ssh-key creds://<name> --node <id>
  --role <bp|relay> --preview`.
- Interpret only the typed result. A conforming candidate reports the signed convention and exact
  candidate hash. A non-conforming runtime/image/layout is refused, never adapted.
- For an adopted node, do not run a separate detector before an operation. The mechanism probes and
  compares the live container to the attestation under the operation lock; drift is a typed refusal.

## Stop Conditions
- Stop when preview reports zero/multiple node containers, an unsupported/rootless supervisor,
  a non-allowlisted image or a layout/role mismatch.
- Stop on `not_ouro_managed` or `node_drift`; route unmanaged nodes to adoption and drifted nodes to
  operator review.
- Stop if a diagnosis would otherwise become an ad hoc mutation.

## Red Lines
- Adoption preview is non-mutating; free-form L3 diagnosis is unprivileged, not read-only.
- Unprivileged diagnosis has no secret directory access.
- No cold, KES secret, or VRF material enters context or output.
- Never recreate, rename or restart a node merely to make adoption pass.
- Writes go only through `ouro-ops op run`; detection never performs a change itself.
