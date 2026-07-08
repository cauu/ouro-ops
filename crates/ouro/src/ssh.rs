use serde::Serialize;

use crate::domain::SshTarget;

#[derive(Debug, Clone, Serialize)]
pub struct SshRunner {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreparedCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl SshRunner {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    pub fn prepare_tool_run(
        &self,
        target: &SshTarget,
        tool: &str,
        spec_path: &str,
        invocation_id: &str,
    ) -> PreparedCommand {
        PreparedCommand {
            program: "ssh".to_string(),
            args: vec![
                "-p".to_string(),
                target.port.to_string(),
                "-i".to_string(),
                target.key_ref.as_str().to_string(),
                format!("{}@{}", target.user, target.host),
                "sudo".to_string(),
                "-n".to_string(),
                "ouro".to_string(),
                "tool".to_string(),
                "run".to_string(),
                tool.to_string(),
                "--spec".to_string(),
                spec_path.to_string(),
                "--audit-id".to_string(),
                invocation_id.to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{domain::SshTarget, secrets::CredentialRef};

    use super::SshRunner;

    #[test]
    fn only_prepares_allowlisted_tool_run_shape() {
        let target = SshTarget {
            host: "relay1.example.com".to_string(),
            port: 22,
            user: "ouro-exec".to_string(),
            key_ref: CredentialRef::parse("creds://relay1").unwrap(),
        };
        let cmd = SshRunner::new(true).prepare_tool_run(
            &target,
            "deploy/preflight",
            "pool-spec.json",
            "audit-1",
        );
        let joined = cmd.args.join(" ");
        assert!(joined.contains("sudo -n ouro tool run deploy/preflight"));
        assert!(!joined.contains(" docker rm "));
        assert!(!joined.contains(" scp "));
        assert!(!joined.contains(" sudo rm "));
    }
}
