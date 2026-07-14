# S0019 Threat Model & Trust Matrix (§2.12)

Every invariant names its enforcing component and its negative test. Prompt text is
defense-in-depth only.

## Trusted computing base (OUT OF SCOPE — assumed honest)
- The host root account and the Docker daemon on the target.
- The host kernel.
- The allowlisted node image's own vulnerabilities (digest pins identity, not safety).
- An agent that ABANDONS the ouro-ops path via the bootstrap credential (P0-1 convenience boundary,
  carried from S0017; closing it is a separate control-plane hardening spec).

## Adversaries modeled → enforcing component → negative test
| Adversary / fault | Enforcing component (§) | Negative test |
|---|---|---|
| Misled / injected agent issuing a bad write | intent envelope + deny-by-default registry + sink rules (§2.5) | intent.rs `closed_schema_rejects_unknown_and_hostile_params`; TC-4 |
| Hostile parameter value (traversal/shell/parser) | closed typed params, no shell sink (§2.5) | intent.rs (hostile machine id / raw path refused); TC-4 |
| Unclassified/legacy write slipping through | registry deny-by-default + legacy disable (§2.5/§2.8) | intent.rs `registry_covers_all`; parity.rs `legacy_write_disabled_unless_registered`; TC-10 |
| Stale / replaced container (drift) | attestation immutable-id vs versioned state (§2.3) | attestation.rs `live_match_and_drift`; TC-3 |
| Fingerprint self-invalidation after a legit write | CAS state advance in the txn (§2.3/§2.6) | attestation.rs `legitimate_write_advances_generation_not_drift` |
| check→act TOCTOU swap | in-lock re-attest + pre-commit recheck (§2.4) | gate.rs `gate_refuses_drift_between_open_and_commit`; TC-3 |
| Non-conforming / look-alike image | signed digest allowlist, no tag trust (§2.1) | convention.rs `allowed_digest_resolves_denylist_and_unknown_refuse`; TC-2 |
| Non-v1 supervisor shape | pinned supervisor contract (§2.2) | supervisor.rs `every_non_v1_shape_refused`; TC-2 |
| Hostile / replaced artifact | content-addressed inbox, digest re-verify (§2.7) | inbox.rs `tamper_and_wrong_ref_refused`; TC-5 |
| Executor / registry downgrade | security-identity parity + anti-downgrade (§2.8) | parity.rs `downgrade_refused`; TC-10 |
| Crash / power loss mid-write | fsync'd txn state machine + recovery (§2.6) | transaction.rs recovery + seal tests; TC-6 |
| Two independent controllers breaking quorum | fleet lease + fencing + quorum re-eval (§2.9) | fleet.rs `target_fences_a_stale_controller` / `require_quorum`; TC-8 |
| Bad node-runtime upgrade / impossible rollback | DB-compat-gated transition, honest re-sync (§2.10) | upgrade.rs `rollback_only_when_recoverable_else_honest_resync`; TC-9 |
| Diagnostic DoS / world-readable exfil | honest unprivileged-diag labeling / optional sandbox (§2.11) | troubleshooting SKILL wording; skill-docs gate |
| Mis-adopted (wrong) container blessed | evidence-bound adoption approval (§2.14) | attestation.rs `approval_binding_is_candidate_and_hostkey_specific`; TC-2 |
| Relay bearing forging keys | role rules at adopt (§2.3) | attestation.rs `role_rule_relay_forbids_forging_keys` |
