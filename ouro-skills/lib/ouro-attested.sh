#!/usr/bin/env bash
# S0019 p1-5 (§1.C, §2.3/§2.4) — greenfield layout helpers that READ the adoption attestation.
#
# The new skill set never detects the environment from process args or image quirks (S0017 failure
# mode). It reads the single source of truth written by the adopt ceremony:
#   /var/lib/ouro/node-attestation.json  (root-owned 0640; immutable identity + versioned state).
# A node without a matching attestation is REFUSED (not_ouro_managed) — no detection, no fallback.
#
# These helpers are pure reads of the attestation; the live re-attestation GATE (Rust, §2.4) is
# what binds the attestation to the running container before any write. No process/mode discovery,
# no host/container path guessing lives here — the S0017 detection machinery is not carried over.

OURO_ATTESTATION="${OURO_ATTESTATION:-/var/lib/ouro/node-attestation.json}"

# Refuse fast if this node was never adopted (or the attestation is unreadable). Every new op
# script's FIRST line. Prints a typed not_ouro_managed error and exits non-zero.
ouro_require_attested() {
  if [ ! -r "$OURO_ATTESTATION" ]; then
    printf '{"status":"error","error":{"code":"not_ouro_managed","detail":"no adoption attestation at %s — this node is not ouro-managed (run adopt); ops are refused, never adapted (S0019 §1.C)"}}\n' \
      "$OURO_ATTESTATION" >&2
    return 20
  fi
}

# Read a dotted field from the attestation (e.g. `.immutable.image_config_digest`). Reads only;
# no defaults, no discovery — an absent field is an error the caller surfaces.
ouro_attested() {
  ouro_require_attested || return 20
  python3 - "$OURO_ATTESTATION" "$1" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
cur = doc
for part in sys.argv[2].strip(".").split("."):
    if isinstance(cur, dict) and part in cur:
        cur = cur[part]
    else:
        sys.exit(3)  # absent field — never guessed
print(cur if not isinstance(cur, (dict, list)) else json.dumps(cur))
PY
}

# Named layout accessors — the ONLY way the new skills learn where things are. The paths are the
# CONTRACT's in-container paths, recorded at adopt; the socket path is what the executor passes to
# cardano-cli via --socket-path (S0017 p5-21 lesson, now a recorded fact not a guess).
ouro_attested_role()       { ouro_attested ".immutable.role"; }
ouro_attested_container()  { ouro_attested ".state.container_id"; }
ouro_attested_socket()     { ouro_attested ".immutable.contract.socket" 2>/dev/null || ouro_contract_path socket; }
ouro_attested_db()         { ouro_contract_path db; }
ouro_attested_keys()       { ouro_contract_path keys; }
ouro_attested_generation() { ouro_attested ".state.state_generation"; }

# In-container path for a contract resource. The adopt ceremony records the resolved contract on
# the attestation under `.contract` (written by the Rust adopt path); this reads it.
ouro_contract_path() {
  ouro_attested ".contract.in_container_paths.$1"
}
