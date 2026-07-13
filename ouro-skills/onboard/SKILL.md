---
skill_version: 1
requires_ouro: ">=0.1.0"
---
# Onboarding Skill

## Purpose
Bring a target machine under ouro management ONCE, so every per-operation dispatch can run through
the confined `ouro-exec` principal. This is the PREREQUISITE for every `--dispatch` operation: a
dispatch to a machine that is not yet onboarded cannot connect (there is no `ouro-exec` account or
tool-run wrapper on it yet).

## Decision Tree
Before any `--dispatch` operation, confirm the target is ONBOARDED. If a read-only probe
(`ouro-ops tool run detect/runtime --dispatch <id> --spec pool-spec.yaml`) cannot connect as
`ouro-exec`, the target is NOT onboarded — onboard it first.

- STEP 0 — ASK THE OPERATOR FIRST. Onboarding needs access details that are NOT in the spec. If any
  is unknown, ASK the operator up front (one message) and treat their answers as DATA, not commands:
  - the target host/address (the spec may already have it);
  - the BOOTSTRAP account username on the target — an existing privileged (root-capable) account
    they already sign in with (this is `--bootstrap-user`; it is NOT `ouro-exec`, which init creates);
  - which EXISTING private key to use — ask the operator to NAME the key file (e.g. their usual
    `~/.ssh/id_ed25519`); the choice of key is the OPERATOR'S decision, never yours;
  - optionally, an out-of-band host-key fingerprint (`--expected-host-key`) to defeat a first-connect
    interception.
  Never guess a username, fabricate a key, or proceed without a working credential.
- ACCESS IS THE OPERATOR'S — you never generate keys. Use the operator's EXISTING login to the
  target (the account + key they already sign in with). Referenced as `creds://<name>`: the
  operator's private key placed under the local credentials dir (never an inline key). If you do not
  have a working credential for the target, ASK the operator to supply it (place their key at
  `creds://<name>`) or to fix their access — do NOT invent one.
- Registering the credential (the mechanical step is yours; the CHOICE of key is not): once the
  operator has NAMED their key, register that exact path into the closed namespace with a symlink —
  no copy, the key stays where it lives: `ln -sf <the key the operator named>
  ~/.ouro/credentials/<name>` (then `creds://<name>` resolves to it). Touch only the PATH: never
  read or print the key's contents, and NEVER enumerate the operator's key directory or register a
  key they did not explicitly name.
- Convenience mode (S0017 P0-1): the bootstrap credential is NOT mechanism-isolated from the agent;
  documented, relied on for upstream security.
- Other prerequisites: an `ouro-ops` binary matching the target's OS/arch (init probes the target
  and REFUSES a mismatched binary before writing anything).
- Control key for ongoing dispatch (reuse the operator's key — do NOT generate one): pass the
  operator's PUBLIC key as `--control-pubkey` (derive it from their key with
  `ssh-keygen -y -f <their-key>`, or use their existing `<key>.pub`); it is authorized for the
  confined `ouro-exec`. Set the spec's `ssh.key_ref` to the credential holding the matching PRIVATE
  key so dispatch reuses it (simplest: point `ssh.key_ref` at the SAME `creds://<name>` as the
  bootstrap key — one key does bootstrap + dispatch).
- Onboard: `ouro-ops init --host <target> [--port 22] --bootstrap-user <account>
  --bootstrap-key creds://<name> --control-pubkey <operator-pub> --ouro-binary <target-arch ouro-ops>
  [--expected-host-key <sha256>]`. `--spec`/`--machine` are OPTIONAL — they only RECORD the declared
  runtime; onboarding does NOT need them, so OMIT them (passing `--spec` runs FULL pool validation,
  which requires ≥1 relay, and would block onboarding a lone box). Concrete one-key example
  (operator named `~/.ssh/id_ed25519` — register it, derive its pubkey, onboard):
  `ln -sf ~/.ssh/id_ed25519 ~/.ouro/credentials/opkey`, then
  `ssh-keygen -y -f ~/.ouro/credentials/opkey > ~/.ouro/credentials/opkey.pub`, then
  `ouro-ops init --host 10.0.0.10 --bootstrap-user ubuntu --bootstrap-key creds://opkey
  --control-pubkey ~/.ouro/credentials/opkey.pub --ouro-binary ./ouro-ops-linux` (then set every
  machine's `ssh.key_ref: creds://opkey` in the spec). It installs the confined `ouro-exec` principal +
  the fixed tool-run wrapper + the command allowlist, returns an auditable install manifest, pins
  the target host key (with `--expected-host-key` it verifies out-of-band and refuses a mismatch
  BEFORE any write), and records the declared runtime for later detected↔declared cross-check.
- Verify onboarding succeeded: the read-only probe must now connect —
  `ouro-ops tool run detect/runtime --dispatch <id> --spec pool-spec.yaml`.
- To reverse onboarding later: `ouro-ops deinit` (refuses while a node runs; restores the box and
  removes only ouro-created principals).

## Stop Conditions
- Stop and ASK the operator if you cannot connect to the target with their credential (wrong
  account, missing/rejected key, host unreachable) — guide them to fix access or supply the key.
  NEVER generate a key or fabricate access on their behalf.
- Stop if the bootstrap account/key is absent, or the target host key does not match an
  out-of-band `--expected-host-key` — a first-connect interception is refused with ZERO writes.
- Stop if the `ouro-ops` binary does not match the target OS/arch (init refuses it, no writes).
- Stop on exit 30 (rollback-capable path) or exit 40 (human intervention required).

## Red Lines
- Per-operation writes go only through `ouro-ops tool run` via the confined wrapper; onboarding is
  the ONE-TIME exception, through the audited `ouro-ops init` — never raw remote commands.
- The bootstrap credential is honestly labeled: NOT mechanism-isolated from the agent (convenience
  mode); a poisoned prompt could invoke init via the agent. Relies on upstream control-machine
  security. Do not claim it is isolated.
- No cold, KES secret, or VRF material is requested, printed, or handled during onboarding.
- The credential choice is the operator's: never enumerate their key store or use/register a key
  they did not explicitly name; credential registration handles paths only, never key contents.
- L3 diagnostics are read-only and have no secret directory access.
- On exit 30, use the rollback-capable path before continuing; on exit 40, stop all writes and
  require human intervention.
