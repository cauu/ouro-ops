//! S0017 p4-1 — generate the self-contained KES cold-signing script.
//!
//! Cold-signing flow (cold.skey kept OFFLINE, the security-conscious default):
//!   1. On the BP: generate a fresh KES key pair; kes.skey STAYS on the BP, kes.vkey is public.
//!   2. `ouro-ops kes cold-sign-script` embeds ONLY that public vkey + the KES period into a
//!      standalone bash script (no `ouro-ops` needed on the cold machine — only cardano-cli).
//!   3. The operator carries the script to the AIR-GAPPED machine and runs it: it reads
//!      cold.skey + the opcert counter IN PLACE and issues the operational certificate.
//!      cold.skey never moves; only the public node.cert comes back.
//!   4. Back online: `ouro-ops kes push` installs node.cert and restarts the node.
//!
//! Security invariant (p4-4): the generated script contains NO private key material — only the
//! public KES verification key and the (public) period. cold.skey and the counter are referenced
//! by PATH (the operator's cold-machine layout) and read in place. This module refuses to embed
//! anything that looks like a signing key.

use crate::{OuroError, Result};

/// Reject a vkey argument that is actually a signing (private) key, or not a KES verification key
/// at all — so the generated script can never carry private material.
fn validate_kes_vkey(kes_vkey: &str) -> Result<()> {
    let lower = kes_vkey.to_lowercase();
    if lower.contains("signingkey") || lower.contains("signing key") || lower.contains("_sk") {
        return Err(OuroError::Validation(
            "refusing: --kes-vkey looks like a SIGNING key, not a verification key \
             (a cold-sign script must embed ONLY public material)"
                .to_string(),
        ));
    }
    // A cardano KES vkey file is JSON with a "KesVerificationKey…" type + a cborHex.
    if !(kes_vkey.contains("VerificationKey") && kes_vkey.contains("cborHex")) {
        return Err(OuroError::Validation(
            "--kes-vkey does not look like a cardano KES verification key file \
             (expected a JSON envelope with a VerificationKey type and cborHex)"
                .to_string(),
        ));
    }
    Ok(())
}

/// Build the KES cold-signing script. `kes_vkey` is the PUBLIC verification-key file content;
/// `kes_period` is the target KES period (computed online from tip slot / slotsPerKESPeriod).
/// `cardano_cli` lets the operator pin the cold machine's binary (default `cardano-cli`).
/// `generated_at` is an RFC3339 timestamp stamped into the header so the operator can judge
/// freshness — a KES period is time-sensitive; a script left unrun for days targets a stale
/// period. This is an honest advisory (a comment), not enforced logic: the air-gapped machine
/// has no reliable clock or chain view to check against.
pub fn kes_cold_sign_script(
    kes_vkey: &str,
    kes_period: u64,
    cardano_cli: &str,
    generated_at: &str,
) -> Result<String> {
    validate_kes_vkey(kes_vkey)?;
    // The vkey is embedded via a QUOTED heredoc (`<<'OURO_KES_VKEY'`) so nothing in it is
    // expanded by the cold machine's shell. It is public, so quoting is for correctness, not
    // secrecy. `issue-op-cert` is era-neutral (no era prefix) — matches the pinned cardano-cli.
    Ok(format!(
        r#"#!/usr/bin/env bash
# ==============================================================================
# ouro-ops KES cold-signing script (GENERATED). RUN ON THE AIR-GAPPED MACHINE.
#
# It embeds ONLY public data (the KES verification key + period). It reads your
# cold signing key and opcert counter IN PLACE and never copies them anywhere;
# cold.skey does not move. Only the public node.cert it writes is brought back.
#
# Review it before running (`less` this file). It runs exactly one command:
# `cardano-cli node issue-op-cert`. It contains NO private key.
#
# Generated: {generated_at}
# Targets KES period: {kes_period}. This is TIME-SENSITIVE — the KES period advances
# with the chain (~1/day on mainnet). Run this SOON after generation. If days have
# passed, discard this script and regenerate, or the certificate may target a stale
# period and the node will refuse to forge.
# ==============================================================================
set -euo pipefail

# ---- EDIT to your cold machine's layout (or export these before running) -----
COLD_SKEY="${{COLD_SKEY:-./cold.skey}}"     # your offline cold signing key (read in place)
COUNTER="${{COUNTER:-./opcert.counter}}"    # the opcert issue counter (incremented in place)
OUT="${{OUT:-./node.cert}}"                 # the operational certificate to bring back
# ------------------------------------------------------------------------------

KES_PERIOD={kes_period}
CARDANO_CLI="${{CARDANO_CLI:-{cardano_cli}}}"

[ -f "$COLD_SKEY" ] || {{ echo "cold.skey not found at $COLD_SKEY (set COLD_SKEY=...)" >&2; exit 1; }}
[ -f "$COUNTER" ]   || {{ echo "opcert counter not found at $COUNTER (set COUNTER=...)" >&2; exit 1; }}

VKEY="$(mktemp)"; trap 'rm -f "$VKEY"' EXIT
cat > "$VKEY" <<'OURO_KES_VKEY'
{kes_vkey}
OURO_KES_VKEY

# issue-op-cert consumes the counter and writes counter+1 back (persisted, monotonic).
"$CARDANO_CLI" node issue-op-cert \
  --kes-verification-key-file "$VKEY" \
  --cold-signing-key-file "$COLD_SKEY" \
  --operational-certificate-issue-counter-file "$COUNTER" \
  --kes-period "$KES_PERIOD" \
  --out-file "$OUT"

echo "Wrote operational certificate: $OUT"
echo "Bring $OUT back online and install it with: ouro-ops kes push"
"#,
        kes_period = kes_period,
        cardano_cli = cardano_cli,
        kes_vkey = kes_vkey.trim_end(),
        generated_at = generated_at,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_VKEY: &str = r#"{
    "type": "KesVerificationKey_ed25519_kes_2^6",
    "description": "KES Verification Key",
    "cborHex": "5820aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
}"#;

    #[test]
    fn embeds_only_public_data_and_runs_issue_op_cert() {
        let s = kes_cold_sign_script(REAL_VKEY, 42, "cardano-cli", "2026-07-12T00:00:00Z").unwrap();
        assert!(s.starts_with("#!/usr/bin/env bash"));
        assert!(s.contains("KES_PERIOD=42"));
        assert!(s.contains("node issue-op-cert"));
        assert!(s.contains("--cold-signing-key-file \"$COLD_SKEY\""));
        // era-neutral (no `cardano-cli conway node issue-op-cert`)
        assert!(!s.contains("conway node issue-op-cert"));
        // the public vkey is embedded verbatim
        assert!(s.contains("KesVerificationKey_ed25519_kes"));
        // cold.skey is referenced by PATH, read in place — never copied/moved
        assert!(s.contains("$COLD_SKEY") && !s.contains("cp \"$COLD_SKEY\""));
    }

    #[test]
    fn refuses_to_embed_a_signing_key() {
        let skey = r#"{"type":"KesSigningKey_ed25519_kes_2^6","description":"KES Signing Key","cborHex":"5840deadbeef"}"#;
        let err = kes_cold_sign_script(skey, 1, "cardano-cli", "2026-07-12T00:00:00Z");
        assert!(err.is_err(), "must refuse a signing key");
        // a bech32-style private key name too
        assert!(kes_cold_sign_script("kes_sk1qqq...", 1, "cardano-cli", "T").is_err());
    }

    #[test]
    fn refuses_non_kes_vkey_input() {
        assert!(kes_cold_sign_script("not a key", 1, "cardano-cli", "T").is_err());
        assert!(kes_cold_sign_script("{\"type\":\"VerificationKey\"}", 1, "cardano-cli", "T").is_err()); // no cborHex
    }

    #[test]
    fn generated_script_has_no_private_key_fingerprint() {
        // The output must never contain a signing-key marker (the anti-leak invariant).
        let s = kes_cold_sign_script(REAL_VKEY, 7, "cardano-cli", "2026-07-12T00:00:00Z").unwrap();
        for bad in ["SigningKey", "cold.skey\ncat", "PRIVATE KEY"] {
            assert!(!s.contains(bad), "leaked {bad:?}");
        }
    }
}
