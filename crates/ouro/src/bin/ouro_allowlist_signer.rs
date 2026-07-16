#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("ouro-allowlist-signer is available only on macOS");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
mod macos {
    use ed25519_dalek::{Signer, SigningKey};
    use security_framework::passwords::{generic_password, PasswordOptions};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use zeroize::Zeroizing;

    const SERVICE: &str = "io.ouro-ops.allowlist-release";
    const ACCOUNT: &str = "production-ed25519-2026-07";

    type ToolResult<T> = Result<T, Box<dyn std::error::Error>>;

    pub fn run() -> ToolResult<()> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("public-key") if args.len() == 1 => {
                let signing = load_signing_key()?;
                print_json(json!({
                    "account": ACCOUNT,
                    "public_key": hex(&signing.verifying_key().to_bytes()),
                    "service": SERVICE,
                }))?;
            }
            Some("inspect") => {
                require_exact_flags(&args, &["--input"])?;
                let input = PathBuf::from(flag_value(&args, "--input")?);
                let text = fs::read_to_string(&input)?;
                let (allowlist, canonical) = ouro::convention::release_candidate(&text)?;
                print_json(json!({
                    "allowlist_version": allowlist.allowlist_version,
                    "canonical_sha256": hex(&Sha256::digest(&canonical)),
                    "input": input,
                    "validated": true,
                }))?;
            }
            Some("sign") => {
                require_exact_flags(&args, &["--input", "--output", "--expect-public-key"])?;
                let input = PathBuf::from(flag_value(&args, "--input")?);
                let output = PathBuf::from(flag_value(&args, "--output")?);
                let expected_public = decode_key(flag_value(&args, "--expect-public-key")?)?;
                sign(&input, &output, &expected_public)?;
            }
            _ => {
                return Err("usage: ouro-allowlist-signer public-key | inspect --input <allowlist.json> | sign --input <allowlist.json> --output <allowlist.json> --expect-public-key <64-lowercase-hex>".into());
            }
        }
        Ok(())
    }

    fn sign(input: &Path, output: &Path, expected_public: &[u8; 32]) -> ToolResult<()> {
        let text = fs::read_to_string(input)?;
        let (mut allowlist, canonical) = ouro::convention::release_candidate(&text)?;
        let signing = load_signing_key()?;
        let public = signing.verifying_key().to_bytes();
        if public != *expected_public {
            return Err("Keychain release key does not match --expect-public-key; refused".into());
        }

        let signature = signing.sign(&canonical);
        signing
            .verifying_key()
            .verify_strict(&canonical, &signature)
            .map_err(|_| "release signature self-verification failed")?;
        allowlist.signature = format!("ed25519:{}", hex(&signature.to_bytes()));

        let mut body = serde_json::to_vec_pretty(&allowlist)?;
        body.push(b'\n');
        atomic_write(output, &body)?;
        print_json(json!({
            "account": ACCOUNT,
            "allowlist_version": allowlist.allowlist_version,
            "canonical_sha256": hex(&Sha256::digest(&canonical)),
            "output": output,
            "public_key": hex(&public),
            "service": SERVICE,
            "signed": true,
        }))?;
        Ok(())
    }

    fn load_signing_key() -> ToolResult<SigningKey> {
        let options = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
        let stored = Zeroizing::new(generic_password(options).map_err(|error| {
            format!("cannot read the user-authorized Keychain release key: {error}")
        })?);
        if stored.len() != 32 {
            return Err("Keychain release key has the wrong length".into());
        }
        let mut seed = Zeroizing::new([0_u8; 32]);
        seed.copy_from_slice(&stored);
        Ok(SigningKey::from_bytes(&seed))
    }

    fn atomic_write(path: &Path, body: &[u8]) -> ToolResult<()> {
        if fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err("refusing to replace a symlink output".into());
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("output path needs a UTF-8 filename")?;
        let temporary = parent.join(format!(".{name}.tmp-{}", uuid::Uuid::new_v4().simple()));
        let mode = fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(0o644);
        let result = (|| -> ToolResult<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(body)?;
            file.sync_all()?;
            file.set_permissions(fs::Permissions::from_mode(mode))?;
            fs::rename(&temporary, path)?;
            fs::File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn require_exact_flags(args: &[String], expected: &[&str]) -> ToolResult<()> {
        if args.len() != 1 + expected.len() * 2 {
            return Err("unexpected or missing signer arguments".into());
        }
        for flag in expected {
            if args.iter().filter(|value| value.as_str() == *flag).count() != 1 {
                return Err(format!("expected exactly one {flag}").into());
            }
            let index = args.iter().position(|value| value == flag).unwrap();
            if args
                .get(index + 1)
                .map(|value| value.starts_with("--"))
                .unwrap_or(true)
            {
                return Err(format!("missing value for {flag}").into());
            }
        }
        if args
            .iter()
            .skip(1)
            .step_by(2)
            .any(|flag| !expected.contains(&flag.as_str()))
        {
            return Err("unknown signer flag".into());
        }
        Ok(())
    }

    fn flag_value<'a>(args: &'a [String], flag: &str) -> ToolResult<&'a str> {
        let index = args
            .iter()
            .position(|value| value == flag)
            .ok_or_else(|| format!("missing {flag}"))?;
        args.get(index + 1)
            .map(String::as_str)
            .ok_or_else(|| format!("missing value for {flag}").into())
    }

    fn decode_key(value: &str) -> ToolResult<[u8; 32]> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("--expect-public-key must be 64 lowercase hex characters".into());
        }
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)?;
        }
        Ok(bytes)
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn print_json(value: serde_json::Value) -> ToolResult<()> {
        println!("{}", serde_json::to_string(&value)?);
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn public_key_parser_is_closed_and_lowercase() {
            assert!(decode_key(&"a".repeat(64)).is_ok());
            for bad in ["a".repeat(63), "A".repeat(64), "g".repeat(64)] {
                assert!(decode_key(&bad).is_err());
            }
        }

        #[test]
        fn signer_arguments_are_exact_and_do_not_accept_secret_inputs() {
            let valid = vec![
                "sign".to_string(),
                "--input".to_string(),
                "in.json".to_string(),
                "--output".to_string(),
                "out.json".to_string(),
                "--expect-public-key".to_string(),
                "a".repeat(64),
            ];
            assert!(
                require_exact_flags(&valid, &["--input", "--output", "--expect-public-key"])
                    .is_ok()
            );
            let mut secret = valid.clone();
            secret.extend(["--private-key".to_string(), "forbidden".to_string()]);
            assert!(
                require_exact_flags(&secret, &["--input", "--output", "--expect-public-key"])
                    .is_err()
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = macos::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
