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
echo "Bring $OUT back online and install it with: ouro-ops kes push"
"#,
        kes_period = kes_period,
        cardano_cli = cardano_cli,
        kes_vkey = kes_vkey.trim_end(),
        generated_at = generated_at,
    ))
}

/// A cold key the tx must be witnessed by. `role` is a short kebab id (e.g. `cold`, `stake`);
/// it names both the env var the operator points at their key (`<ROLE>_SKEY`) and the witness
/// output file (`<ROLE>_WITNESS`). Each role produces ONE independent witness — the operator
/// need not co-locate all cold keys, and only the public witnesses come back for assembly.
fn role_env(role: &str) -> String {
    role.to_uppercase().replace('-', "_")
}

/// Validate that `tx_body` is a cardano transaction BODY envelope (public — no signatures), not a
/// signing key. `transaction witness` consumes exactly this; embedding it leaks nothing private.
fn validate_tx_body(tx_body: &str) -> Result<()> {
    let lower = tx_body.to_lowercase();
    if lower.contains("signingkey") || lower.contains("signing key") {
        return Err(OuroError::Validation(
            "refusing: --tx-body looks like a SIGNING key, not an unsigned transaction body"
                .to_string(),
        ));
    }
    // cardano `transaction build` writes a JSON envelope: {"type":"Unwitnessed Tx …"/"TxBody…",
    // "description":…, "cborHex":…}. Require the envelope shape + a tx-body-ish type.
    let looks_like_txbody = tx_body.contains("cborHex")
        && (tx_body.contains("TxBody") || tx_body.contains("Unwitnessed") || tx_body.contains("Tx "));
    if !looks_like_txbody {
        return Err(OuroError::Validation(
            "--tx-body does not look like a cardano unsigned transaction body \
             (expected a JSON envelope with a TxBody/Unwitnessed type and cborHex)"
                .to_string(),
        ));
    }
    Ok(())
}

/// Build the deploy/registration cold-signing script (S0017 p4-2). `tx_body` is the PUBLIC
/// unsigned transaction body (from an online `transaction build`); `roles` are the cold keys
/// that must witness it. On the air-gapped machine the script runs, for each role, era-scoped
/// `cardano-cli <era> transaction witness`, reading the cold key IN PLACE → one witness file per
/// role. The witnesses (public) come back and are assembled + submitted online. No private key is
/// embedded; cold keys are referenced by path and never copied.
pub fn tx_cold_sign_script(
    tx_body: &str,
    roles: &[String],
    era: &str,
    network: &str,
    cardano_cli: &str,
    generated_at: &str,
) -> Result<String> {
    validate_tx_body(tx_body)?;
    if roles.is_empty() {
        return Err(OuroError::Validation(
            "at least one --cold-key role is required".to_string(),
        ));
    }
    // era guards against an injected shell token in a passthrough field.
    if !era.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(OuroError::Validation(
            "--era must be alphanumeric (e.g. conway)".to_string(),
        ));
    }
    // `network` is a pre-formed cardano-cli flag (`--mainnet` or `--testnet-magic <n>`) that some
    // cardano-cli versions require on `transaction witness`; validate it can't smuggle a token.
    let net = network.trim();
    let net_ok = net.is_empty()
        || net == "--mainnet"
        || net
            .strip_prefix("--testnet-magic ")
            .is_some_and(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()));
    if !net_ok {
        return Err(OuroError::Validation(
            "--network must be mainnet or a numeric testnet magic".to_string(),
        ));
    }
    let net_line = if net.is_empty() {
        String::new()
    } else {
        format!("  {net} \\\n")
    };
    let mut config = String::new();
    let mut witness_cmds = String::new();
    let mut assemble_hint = String::from("  ouro-ops deploy submit --tx-body <this body>");
    for role in roles {
        if !role.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || role.is_empty() {
            return Err(OuroError::Validation(format!(
                "cold-key role {role:?} must be a non-empty kebab/alnum id"
            )));
        }
        let env = role_env(role);
        config.push_str(&format!(
            "{env}_SKEY=\"${{{env}_SKEY:-./{role}.skey}}\"      # cold signing key for '{role}' (read in place)\n\
             {env}_WITNESS=\"${{{env}_WITNESS:-./{role}.witness}}\" # witness this script writes for '{role}'\n"
        ));
        witness_cmds.push_str(&format!(
            "[ -f \"${env}_SKEY\" ] || {{ echo \"{role} signing key not found at ${env}_SKEY (set {env}_SKEY=...)\" >&2; exit 1; }}\n\
             \"$CARDANO_CLI\" {era} transaction witness \\\n  \
             --tx-body-file \"$TXBODY\" \\\n  \
             --signing-key-file \"${env}_SKEY\" \\\n\
             {net_line}  \
             --out-file \"${env}_WITNESS\"\n\
             echo \"Wrote witness for {role}: ${env}_WITNESS\"\n"
        ));
        assemble_hint.push_str(&format!(" --witness ${env}_WITNESS"));
    }
    Ok(format!(
        r#"#!/usr/bin/env bash
# ==============================================================================
# ouro-ops DEPLOY cold-signing script (GENERATED). RUN ON THE AIR-GAPPED MACHINE.
#
# It embeds ONLY the PUBLIC unsigned transaction body. It reads your cold signing
# key(s) IN PLACE and never copies them anywhere; they do not move. Only the
# public witness file(s) it writes are brought back.
#
# Review it before running (`less` this file). It runs only
# `cardano-cli {era} transaction witness`. It contains NO private key.
#
# Generated: {generated_at}
# ==============================================================================
set -euo pipefail

# ---- EDIT to your cold machine's layout (or export these before running) -----
{config}# ------------------------------------------------------------------------------

CARDANO_CLI="${{CARDANO_CLI:-{cardano_cli}}}"

TXBODY="$(mktemp)"; trap 'rm -f "$TXBODY"' EXIT
cat > "$TXBODY" <<'OURO_TX_BODY'
{tx_body}
OURO_TX_BODY

{witness_cmds}
echo "Bring the witness file(s) back online and assemble + submit, e.g.:"
echo "{assemble_hint}"
"#,
        era = era,
        generated_at = generated_at,
        config = config,
        cardano_cli = cardano_cli,
        tx_body = tx_body.trim_end(),
        witness_cmds = witness_cmds.trim_end(),
        assemble_hint = assemble_hint,
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
    fn kes_script_backs_up_counter_and_writes_atomically() {
        // p4-7: the counter (anti-replay authority) is backed up before issuing and the cert is
        // written to a temp path then renamed — never a half-written $OUT, and recoverable on crash.
        let s = kes_cold_sign_script(REAL_VKEY, 5, "cardano-cli", "T").unwrap();
        assert!(s.contains("cp -f \"$COUNTER\" \"$COUNTER.ouro-bak\""), "counter not backed up");
        assert!(s.contains("--out-file \"$OUT_TMP\"") && s.contains("mv -f \"$OUT_TMP\" \"$OUT\""),
                "cert not written atomically via temp + rename");
        assert!(s.contains("counter did not advance"), "no post-issue counter-advance check");
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

    const REAL_TXBODY: &str = r#"{
    "type": "Unwitnessed Tx ConwayEra",
    "description": "Ledger Cddl Format",
    "cborHex": "84a300d9010281825820abcd00010182a2005839001122a1f5f6"
}"#;

    #[test]
    fn tx_script_witnesses_each_role_era_scoped() {
        let roles = vec!["cold".to_string(), "stake".to_string()];
        let s = tx_cold_sign_script(REAL_TXBODY, &roles, "conway", "--testnet-magic 1", "cardano-cli", "2026-07-12T00:00:00Z").unwrap();
        assert!(s.starts_with("#!/usr/bin/env bash"));
        // era-scoped transaction witness (tx commands are NOT era-neutral, unlike issue-op-cert):
        // exactly two command invocations (the header comment names it once more — not counted).
        assert_eq!(s.matches("\"$CARDANO_CLI\" conway transaction witness").count(), 2);
        // one independent witness per cold role, key read in place by path.
        assert!(s.contains("--signing-key-file \"$COLD_SKEY\""));
        assert!(s.contains("--signing-key-file \"$STAKE_SKEY\""));
        assert!(s.contains("COLD_WITNESS=") && s.contains("STAKE_WITNESS="));
        // the network flag is threaded into each witness call.
        assert_eq!(s.matches("--testnet-magic 1").count(), 2);
        // the public tx body is embedded; no private key.
        assert!(s.contains("Unwitnessed Tx ConwayEra"));
        assert!(!s.contains("SigningKey"));
    }

    #[test]
    fn tx_script_omits_network_flag_when_empty() {
        let s = tx_cold_sign_script(REAL_TXBODY, &["cold".into()], "conway", "", "cardano-cli", "T").unwrap();
        assert!(!s.contains("--testnet-magic") && !s.contains("--mainnet"));
    }

    #[test]
    fn tx_script_refuses_signing_key_and_non_txbody() {
        // a signing key smuggled as the "tx body"
        let skey = r#"{"type":"PaymentSigningKeyShelley_ed25519","description":"Payment Signing Key","cborHex":"5820dead"}"#;
        assert!(tx_cold_sign_script(skey, &["cold".into()], "conway", "", "cardano-cli", "T").is_err());
        // not a tx body at all
        assert!(tx_cold_sign_script("hello", &["cold".into()], "conway", "", "cardano-cli", "T").is_err());
        // no roles
        assert!(tx_cold_sign_script(REAL_TXBODY, &[], "conway", "", "cardano-cli", "T").is_err());
        // injection-resistant era + role + network
        assert!(tx_cold_sign_script(REAL_TXBODY, &["cold".into()], "conway;rm -rf /", "", "cardano-cli", "T").is_err());
        assert!(tx_cold_sign_script(REAL_TXBODY, &["a b".into()], "conway", "", "cardano-cli", "T").is_err());
        assert!(tx_cold_sign_script(REAL_TXBODY, &["cold".into()], "conway", "--testnet-magic 1; rm -rf /", "cardano-cli", "T").is_err());
    }
}
