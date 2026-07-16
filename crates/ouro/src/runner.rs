//! S0020 control-selected ephemeral target runner.
//!
//! Release builds embed the repository-built static Linux/x86_64 `ouro-ops` artifact. Debug builds
//! additionally accept an environment-only test seam so transport integration tests do not need to
//! cross-compile on every invocation. No public command accepts runner bytes, paths or digests.

#[cfg(debug_assertions)]
use std::path::PathBuf;

use crate::{OuroError, Result};

static EMBEDDED_LINUX_X86_64: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/ephemeral_linux_x86_64_runner.bin"
));

#[derive(Debug)]
pub struct RunnerArtifact {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub platform: &'static str,
}

pub fn linux_x86_64() -> Result<RunnerArtifact> {
    #[cfg(debug_assertions)]
    let bytes = match std::env::var_os("OURO_EPHEMERAL_RUNNER") {
        Some(path) => {
            let path = PathBuf::from(path);
            std::fs::read(&path).map_err(|error| {
                OuroError::Validation(format!(
                    "cannot read internal debug runner build artifact {}: {error}",
                    path.display()
                ))
            })?
        }
        None => EMBEDDED_LINUX_X86_64.to_vec(),
    };
    #[cfg(not(debug_assertions))]
    let bytes = EMBEDDED_LINUX_X86_64.to_vec();

    if bytes.is_empty() {
        return Err(OuroError::Validation(
            "this control build does not contain its Linux/x86_64 ephemeral runner; rebuild the \
             release bundle with OURO_EMBED_LINUX_X86_64_RUNNER set by release automation"
                .into(),
        ));
    }
    Ok(RunnerArtifact {
        sha256: crate::skills::sha256_hex(&bytes),
        bytes,
        platform: "linux/x86_64",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_artifact_is_digest_bound_when_embedded() {
        if EMBEDDED_LINUX_X86_64.is_empty() {
            return;
        }
        let runner = linux_x86_64().unwrap();
        assert_eq!(runner.sha256, crate::skills::sha256_hex(&runner.bytes));
        assert_eq!(runner.platform, "linux/x86_64");
    }
}
