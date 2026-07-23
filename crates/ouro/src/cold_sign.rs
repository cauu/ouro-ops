//! S0017 p4-1 — generate the self-contained KES cold-signing script.
//!
//! Cold-signing flow (cold.skey kept OFFLINE, the security-conscious default):
//!   1. On the BP: generate a fresh KES key pair; kes.skey STAYS on the BP, kes.vkey is public.
//!   2. `ouro-ops kes cold-sign-script` embeds ONLY that public vkey + the KES period into a
//!      standalone bash script (no `ouro-ops` needed on the cold machine — only cardano-cli).
//!   3. The operator carries the script to the AIR-GAPPED machine and runs it: it reads
//!      cold.skey + the opcert counter IN PLACE and issues the operational certificate.
//!      cold.skey never moves; only the public node.cert comes back.
//!   4. Back online: the KES Rotation Skill previews and deeply validates node.cert, obtains the
//!      final target plan, and waits for exact approval before the typed install/restart operation.
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
# Before running (p4-8 trusted delivery):
#   1. Verify this file's SHA256 matches the digest ouro-ops printed when it generated it
#      (compare out-of-band): `sha256sum <this file>`.
#   2. Review it (`less` this file). It runs exactly one command: `cardano-cli node
#      issue-op-cert`. It contains NO private key.
#   3. Run it on the AIR-GAPPED machine with networking OFF, from a fresh working directory.
#   4. Bring back ONLY the public $OUT (node.cert). Return nothing else.
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

# Counter authority + recovery (p4-7): the opcert issue counter is the anti-replay authority and
# lives here with the cold key. Back it up BEFORE issuing so a crash mid-write is recoverable, and
# confirm afterwards that it actually advanced (issue-op-cert consumes it and writes counter+1).
cp -f "$COUNTER" "$COUNTER.ouro-bak"
BEFORE="$(cat "$COUNTER")"

# Write the certificate to a temp path then atomically rename — never a half-written $OUT.
OUT_TMP="$OUT.ouro-partial"
"$CARDANO_CLI" node issue-op-cert \
  --kes-verification-key-file "$VKEY" \
  --cold-signing-key-file "$COLD_SKEY" \
  --operational-certificate-issue-counter-file "$COUNTER" \
  --kes-period "$KES_PERIOD" \
  --out-file "$OUT_TMP"

if [ "$(cat "$COUNTER")" = "$BEFORE" ]; then
  echo "counter did not advance; restoring backup and aborting" >&2
  cp -f "$COUNTER.ouro-bak" "$COUNTER"; rm -f "$OUT_TMP"; exit 1
fi
mv -f "$OUT_TMP" "$OUT"

echo "Wrote operational certificate: $OUT (counter advanced; backup at $COUNTER.ouro-bak)"
echo "Bring ONLY $OUT back online; follow the current KES Rotation Skill for preview, artifact preflight, final approval, and typed install."
"#,
        kes_period = kes_period,
        cardano_cli = cardano_cli,
        kes_vkey = kes_vkey.trim_end(),
        generated_at = generated_at,
    ))
}

/// Build the KES script used inside a verified platform-specific air-gap bundle. Unlike the legacy
/// stdout-only script above, this form consumes the adjacent public vkey and fixed `cardano-cli`.
/// It verifies the public manifest, vkey and executable before it reads or backs up the counter.
pub fn kes_bundle_cold_sign_script(
    kes_period: u64,
    cardano_cli_version: &str,
    cardano_cli_sha256: &str,
    kes_vkey_sha256: &str,
    manifest_sha256: &str,
    generated_at: &str,
) -> String {
    format!(
        r#"#!/usr/bin/env bash
# ==============================================================================
# ouro-ops KES air-gap bundle. RUN ON THE AIR-GAPPED MACHINE WITH NETWORKING OFF.
#
# This directory contains only public handoff data and the pinned signing tool. The script reads
# cold.skey and opcert.counter in place. Bring back ONLY the public node.cert.
# Generated: {generated_at}
# Targets KES period: {kes_period}
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${{BASH_SOURCE[0]}}")" && pwd)"
CARDANO_CLI="$SCRIPT_DIR/cardano-cli"
KES_VKEY="$SCRIPT_DIR/kes.vkey"
MANIFEST="$SCRIPT_DIR/manifest.json"

EXPECTED_CARDANO_CLI_VERSION='{cardano_cli_version}'
EXPECTED_CARDANO_CLI_SHA256='{cardano_cli_sha256}'
EXPECTED_KES_VKEY_SHA256='{kes_vkey_sha256}'
EXPECTED_MANIFEST_SHA256='{manifest_sha256}'

sha256_file() {{
  local line
  if command -v sha256sum >/dev/null 2>&1; then
    line="$(sha256sum "$1")"
  elif command -v shasum >/dev/null 2>&1; then
    line="$(shasum -a 256 "$1")"
  else
    echo "Neither sha256sum nor shasum is available; cannot verify the bundle" >&2
    return 1
  fi
  printf '%s\n' "${{line%% *}}"
}}

verify_digest() {{
  local label="$1" path="$2" expected="$3" actual
  [ -f "$path" ] || {{ echo "$label not found at $path" >&2; exit 1; }}
  actual="$(sha256_file "$path")"
  [ "$actual" = "$expected" ] || {{
    echo "$label SHA256 mismatch: expected $expected, got $actual" >&2
    exit 1
  }}
}}

# All public bundle integrity/version checks happen before the counter is read or backed up.
verify_digest "manifest.json" "$MANIFEST" "$EXPECTED_MANIFEST_SHA256"
verify_digest "kes.vkey" "$KES_VKEY" "$EXPECTED_KES_VKEY_SHA256"
verify_digest "cardano-cli" "$CARDANO_CLI" "$EXPECTED_CARDANO_CLI_SHA256"
[ -x "$CARDANO_CLI" ] || {{ echo "Bundled cardano-cli is not executable" >&2; exit 1; }}
CLI_VERSION="$("$CARDANO_CLI" --version)"
CLI_VERSION="${{CLI_VERSION%%$'\n'*}}"
case "$CLI_VERSION" in
  "cardano-cli $EXPECTED_CARDANO_CLI_VERSION"|"cardano-cli $EXPECTED_CARDANO_CLI_VERSION "*) ;;
  *) echo "Bundled cardano-cli version mismatch: $CLI_VERSION" >&2; exit 1 ;;
esac

# ---- EDIT to your cold machine's layout (or export these before running) -----
COLD_SKEY="${{COLD_SKEY:-./cold.skey}}"
COUNTER="${{COUNTER:-./opcert.counter}}"
OUT="${{OUT:-./node.cert}}"
# ------------------------------------------------------------------------------

[ -f "$COLD_SKEY" ] || {{ echo "cold.skey not found at $COLD_SKEY (set COLD_SKEY=...)" >&2; exit 1; }}
[ -f "$COUNTER" ]   || {{ echo "opcert counter not found at $COUNTER (set COUNTER=...)" >&2; exit 1; }}

# Counter authority + recovery: back up before issuing, then prove it advanced.
cp -f "$COUNTER" "$COUNTER.ouro-bak"
BEFORE="$(cat "$COUNTER")"
OUT_TMP="$OUT.ouro-partial"
"$CARDANO_CLI" node issue-op-cert \
  --kes-verification-key-file "$KES_VKEY" \
  --cold-signing-key-file "$COLD_SKEY" \
  --operational-certificate-issue-counter-file "$COUNTER" \
  --kes-period '{kes_period}' \
  --out-file "$OUT_TMP"

if [ "$(cat "$COUNTER")" = "$BEFORE" ]; then
  echo "counter did not advance; restoring backup and aborting" >&2
  cp -f "$COUNTER.ouro-bak" "$COUNTER"; rm -f "$OUT_TMP"; exit 1
fi
mv -f "$OUT_TMP" "$OUT"

echo "Wrote operational certificate: $OUT (counter advanced; backup at $COUNTER.ouro-bak)"
echo "Bring ONLY $OUT back online; follow the current KES Rotation Skill for typed preflight and activation."
"#,
        generated_at = generated_at,
        kes_period = kes_period,
        cardano_cli_version = cardano_cli_version,
        cardano_cli_sha256 = cardano_cli_sha256,
        kes_vkey_sha256 = kes_vkey_sha256,
        manifest_sha256 = manifest_sha256,
    )
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
    fn script_carries_trusted_delivery_guidance() {
        // p4-8: the KES script tells the operator to verify the digest out-of-band, review, run
        // air-gapped from a fresh dir, and return ONLY the public artifact.
        let kes = kes_cold_sign_script(REAL_VKEY, 1, "cardano-cli", "T").unwrap();
        assert!(
            kes.contains("SHA256") && kes.contains("out-of-band"),
            "no digest-verify guidance"
        );
        assert!(
            kes.contains("AIR-GAPPED machine with networking OFF"),
            "no confinement guidance"
        );
        assert!(
            kes.to_lowercase().contains("bring only"),
            "no return-whitelist guidance"
        );
    }

    #[test]
    fn kes_script_backs_up_counter_and_writes_atomically() {
        // p4-7: the counter (anti-replay authority) is backed up before issuing and the cert is
        // written to a temp path then renamed — never a half-written $OUT, and recoverable on crash.
        let s = kes_cold_sign_script(REAL_VKEY, 5, "cardano-cli", "T").unwrap();
        assert!(
            s.contains("cp -f \"$COUNTER\" \"$COUNTER.ouro-bak\""),
            "counter not backed up"
        );
        assert!(
            s.contains("--out-file \"$OUT_TMP\"") && s.contains("mv -f \"$OUT_TMP\" \"$OUT\""),
            "cert not written atomically via temp + rename"
        );
        assert!(
            s.contains("counter did not advance"),
            "no post-issue counter-advance check"
        );
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
        assert!(
            kes_cold_sign_script("{\"type\":\"VerificationKey\"}", 1, "cardano-cli", "T").is_err()
        ); // no cborHex
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
