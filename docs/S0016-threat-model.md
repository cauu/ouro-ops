# S0016 — Threat Model & Applicability Boundary

> Delivered for S0016 p4-3. This is the honest statement of **what the web-prompt →
> agent → `ouro` flow protects, what it does not, and when it should not be used.**
> It exists so an operator can calibrate risk correctly instead of over-trusting a
> plausible-looking convenience.

## The flow in one line

A static website turns a form into a `pool-spec.yaml` + a **thin prompt** (data +
which operation + a pointer). The operator pastes the prompt into their agent
(Claude/Codex). The agent reads the **authoritative procedure from the verified local
`ouro` binary** (`ouro skill show <op>`) — not from the prompt — and drives every change
through `ouro tool run`, which enforces the S0015 mechanism (audit gate, sudoers-confined
target wrapper, key isolation, exit-code discipline).

## Central invariant

**The prompt supplies only data + operation + a pointer. It never supplies code, and it
never supplies the decision tree.** All code (`ouro`, skill scripts) and the decision
layer (`SKILL.md`) come from the verified binary, whose identity is fixed and integrity is
checkable — not chosen by the prompt.

## What this protects (✅) vs does not (⚠️)

| Dimension | Protected by | Verdict |
|---|---|---|
| `ouro tool run` path — write authority, key isolation, target-side execution | Mechanism (wrapper/sudoers/audit, S0015) | ✅ Mechanism-enforced; a malicious prompt cannot exceed it |
| `ouro` binary + skill **code** source | Fixed identity + integrity verification (not prompt-chosen) | ✅ See distribution red lines |
| **Decision-layer integrity** (the procedure the agent follows) | Verified binary via `ouro skill show`; never the prompt | ✅ A spoofed site cannot poison it (R2 N3) |
| Version floor (no downgrade via a low prompt value) | `required = max(prompt, embedded, monotonic-rollback, security)` | ✅ Prompt can only raise; tamper-evident floor |
| **Control machine itself** (agent acting *outside* `ouro`) | **Nothing** | ⚠️ = trust in the agent runtime; **weaker than manual SSH** |
| **Topology confidentiality** | Website does not upload; pasting sends it to the agent provider | ⚠️ Disclosed at copy time; not confidential once pasted |

## Residual risks — inherent, disclosed, accepted

- **#2 Control-machine bypass.** The agent has a shell on the machine that holds fleet
  credentials, and it ingests an untrusted pasted prompt. A hostile prompt can try to make
  the agent act *outside* `ouro` entirely — read `~/.ssh`, `curl … | sh`, exfiltrate. The
  mechanism only guards the `ouro` path; it does **not** protect the control machine. This
  cannot be patched away; it is a property of "untrusted text into a capable agent on a
  credentialed host." **Manual SSH does not add a cloud/logged interpreter with shell
  access to that host — so this flow is weaker than manual SSH on this axis.**
- **#3 Control machine = crown jewel.** It both ingests untrusted input and holds
  fleet-reaching credentials → the highest-value target. Mitigation: hardware-backed keys
  (Secure Enclave / hardware token) so key material cannot be exfiltrated even if the host
  is compromised.
- **#4 Topology to the agent provider.** The pasted prompt contains hostnames/IPs/roles;
  the agent provider (and its logs/retention) sees them. The website's "nothing uploaded"
  claim is scoped to the website only. Mitigation: copy-time disclosure (the page requires
  an explicit confirm), and a future minimized / local-agent mode.

## Supply chain

Universal cost (true of manual operation too), not unique to this flow: trusting the tool's
provenance, and a laptop that holds SSH keys. Mitigations that are load-bearing here:

- **Distribution:** single self-updating static binary; **strict signature verification +
  monotonic anti-rollback** on self-update; transparency log + reproducible builds so a rogue
  signature is publicly detectable and the binary is verifiable against source; signing key
  hardware-held.
- **First install (TOFU):** the whole chain is only as good as the first binary. Use the
  **single** official install vector; pin the expected signing identity in a repo-controlled
  artifact so the installer verifies automatically; cross-check the fingerprint across ≥2
  independent channels. Defends typosquat / fake-package / bootstrap-key substitution.

## Applicability boundary — when NOT to use this

This flow is a **convenience mode whose control-machine posture is weaker than manual SSH.**
It is appropriate only when **you trust your agent's runtime environment.**

If you cannot accept that (e.g. compliance forbids untrusted input on a credentialed host,
or the agent runs in a shared/loggable environment you do not control), then for
key-touching or destructive operations you should **operate manually**: use the website to
generate the `pool-spec.yaml` and a command checklist, and run `ouro` yourself — do not let
an agent drive it. The optional hardened path (a restricted agent-run surface exposing only
`ouro` subcommands, with no shell/network/credential access + adversarial bypass tests,
S0016 p4-4) is what would raise this to a genuine "safe path"; until it is enabled, the
default is the convenience mode described here.
