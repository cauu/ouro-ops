//! S0019 p6-1 — greenfield host onboarding (`ouro-ops onboard`). Brings a target to the
//! `host-onboarded` state so the S0019 dispatch chain can reach it: installs the two confined
//! principals, the FIXED op wrapper (sudoers: only `ouro-ops op "$@"`), the ouro-ops binary, and a
//! hardened sshd; pins the host key. Operator-initiated via the bootstrap credential.
//!
//! This is a FRESH plan — it does NOT install or depend on the S0017 tool-run wrapper. The write
//! principal is `ouro-op` (confined to `op`); the read principal is `ouro-diag` (unprivileged).

use std::path::Path;

use serde::Serialize;

use crate::bootstrap::{BootstrapOutcome, BootstrapTarget, BootstrapTransport, HostKeyCheck};
use crate::provision::{execute_plan, InstallManifest, Step, StepResult};
use crate::{OuroError, Result};

/// The fixed root-owned wrapper the `ouro-op` sudoers entry allows: it ONLY ever runs
/// `ouro-ops op "$@"`, so the confined principal cannot invoke adopt/onboard/other subcommands.
pub const OP_WRAPPER: &str = "#!/bin/sh\n\
unset OURO_ATTESTATION OURO_ALLOWLIST_FILE OURO_PROBE_LIB OURO_PLATFORM OURO_HOST_KEY_SHA256 OURO_READINESS_SAMPLE_DELAY\n\
export HOME=/root OURO_HOME=/var/lib/ouro\n\
exec /usr/local/bin/ouro-ops op \"$@\"\n";

/// Separate fixed ingress wrapper. It accepts exactly one closed artifact kind and streams stdin
/// into the target-local inbox; it cannot invoke any other ouro command.
pub const INBOX_WRAPPER: &str = "#!/bin/sh\n\
[ \"$#\" -eq 2 ] || exit 64\n\
case \"$1\" in opcert|tx|image) ;; *) exit 64 ;; esac\n\
unset OURO_ATTESTATION OURO_ALLOWLIST_FILE OURO_PROBE_LIB OURO_PLATFORM OURO_HOST_KEY_SHA256 OURO_READINESS_SAMPLE_DELAY\n\
export HOME=/root OURO_HOME=/var/lib/ouro\n\
exec /usr/local/bin/ouro-ops inbox stage --local --type \"$1\" --stdin --expect-ref \"$2\"\n";

/// sudoers confines `ouro-op` to the op wrapper (NOPASSWD, env reset, fixed secure_path).
pub const OP_SUDOERS: &str = concat!(
    "Defaults:ouro-op env_reset, secure_path=\"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\"\n",
    "ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-op-run\n",
    "ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-inbox-stage\n",
);

/// sshd drop-in: pubkey-only, no root, only the two S0019 principals + the bootstrap account.
pub fn sshd_conf(bootstrap_user: &str) -> String {
    format!(
        "PasswordAuthentication no\n\
         KbdInteractiveAuthentication no\n\
         PermitRootLogin no\n\
         PubkeyAuthentication yes\n\
         AuthenticationMethods publickey\n\
         AuthorizedKeysFile .ssh/authorized_keys\n\
         AuthorizedKeysCommand none\n\
         TrustedUserCAKeys none\n\
         AllowUsers ouro-op ouro-diag {bootstrap_user}\n"
    )
}

/// Fixed target path for the SHARED confirm secret (p6-3): a control-minted confirm-token is bound
/// with this HMAC key, and the target verifies with the SAME key, so the operator's approval on the
/// control machine is honored by the target. Provisioned here (root-owned, 0400 for ouro-op).
pub const CONFIRM_SECRET_PATH: &str = "/var/lib/ouro/confirm.secret";

pub const SSHD_DROP_IN_PATH: &str = "/etc/ssh/sshd_config.d/20-ouro-s0019.conf";
pub const LEGACY_SSHD_DROP_IN_PATH: &str = "/etc/ssh/sshd_config.d/10-ouro.conf";
pub const LEGACY_SUDOERS_PATH: &str = "/etc/sudoers.d/ouro-exec";
pub const LEGACY_WRAPPER_PATH: &str = "/usr/local/sbin/ouro-tool-run";
const SSH_ROLLBACK_DIR: &str = "/var/lib/ouro/onboard-ssh-rollback";
const SSH_ROLLBACK_SCRIPT_PATH: &str = "/usr/local/sbin/ouro-onboard-ssh-rollback";
const SSH_ROLLBACK_SERVICE_PATH: &str =
    "/etc/systemd/system/ouro-onboard-ssh-rollback.service";
const SSH_ROLLBACK_TIMER_PATH: &str = "/etc/systemd/system/ouro-onboard-ssh-rollback.timer";
const SSH_ROLLBACK_LOCK: &str = "/var/lib/ouro/onboard-ssh-rollback.lock";
const SSH_ROLLBACK_SERVICE: &str = "[Unit]\n\
Description=Restore pre-onboarding Ouro SSH policy\n\
StartLimitIntervalSec=0\n\n\
[Service]\n\
Type=oneshot\n\
ExecStart=/usr/local/sbin/ouro-onboard-ssh-rollback\n\
Restart=on-failure\n\
RestartSec=10s\n";
const SSH_ROLLBACK_TIMER: &str = "[Unit]\n\
Description=Rollback unverified Ouro SSH onboarding policy\n\n\
[Timer]\n\
OnActiveSec=2min\n\
AccuracySec=1s\n\
Unit=ouro-onboard-ssh-rollback.service\n\n\
[Install]\n\
WantedBy=timers.target\n";
const SSH_ROLLBACK_SCRIPT: &str = "#!/bin/sh\n\
set -eu\n\
lock=/var/lib/ouro/onboard-ssh-rollback.lock\n\
test -f \"$lock\" && test ! -L \"$lock\"\n\
test \"$(stat -c '%U:%G:%a' \"$lock\")\" = root:root:600\n\
exec 9<>\"$lock\"\n\
flock -w 30 -x 9\n\
base=/var/lib/ouro/onboard-ssh-rollback\n\
for name in 10-ouro.conf 20-ouro-s0019.conf; do\n\
  target=/etc/ssh/sshd_config.d/$name\n\
  if test -f \"$base/$name.absent\"; then\n\
    rm -f \"$target\"\n\
  else\n\
    test -f \"$base/$name\"\n\
    rm -f \"$target\"\n\
    cp -a \"$base/$name\" \"$target\"\n\
  fi\n\
done\n\
sshd -t\n\
{ systemctl reload ssh 2>/dev/null || service ssh reload 2>/dev/null || systemctl reload sshd 2>/dev/null; }\n\
systemctl disable ouro-onboard-ssh-rollback.timer >/dev/null 2>&1 || true\n\
rm -f /etc/systemd/system/ouro-onboard-ssh-rollback.timer /etc/systemd/system/ouro-onboard-ssh-rollback.service\n\
systemctl daemon-reload >/dev/null 2>&1 || true\n\
rm -rf \"$base\"\n\
rm -f /usr/local/sbin/ouro-onboard-ssh-rollback\n";

/// Non-secret, fully rendered SSH policy returned by onboard preview/execute output. Agents must
/// inspect this typed value instead of trying to reconstruct dynamic formatting from binary
/// strings (which cannot reveal the runtime `bootstrap_user` interpolation).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SshAccessPolicy {
    pub drop_in: &'static str,
    pub allow_users: Vec<String>,
    pub bootstrap_user: String,
    pub bootstrap_user_preserved: bool,
    pub rendered_config: String,
    pub legacy_s0017_paths_retired: Vec<&'static str>,
}

pub fn ssh_access_policy(bootstrap_user: &str) -> SshAccessPolicy {
    SshAccessPolicy {
        drop_in: SSHD_DROP_IN_PATH,
        allow_users: vec!["ouro-op".into(), "ouro-diag".into(), bootstrap_user.into()],
        bootstrap_user: bootstrap_user.into(),
        bootstrap_user_preserved: true,
        rendered_config: sshd_conf(bootstrap_user),
        legacy_s0017_paths_retired: vec![
            LEGACY_SSHD_DROP_IN_PATH,
            LEGACY_SUDOERS_PATH,
            LEGACY_WRAPPER_PATH,
        ],
    }
}

pub fn validate_bootstrap_user(user: &str) -> Result<()> {
    let valid = !user.is_empty()
        && user.len() <= 32
        && !matches!(user, "root" | "ouro-op" | "ouro-diag")
        && user
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte == b'_')
        && user.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        });
    if valid {
        Ok(())
    } else {
        Err(crate::OuroError::Validation(
            "bootstrap user must match [a-z_][a-z0-9_-]*, be at most 32 bytes, and not be root, ouro-op or ouro-diag".into(),
        ))
    }
}

/// A pure read-only gate for the pre-change SSH policy and rollback prerequisites. It must run
/// before staging even a public key on the target. The exact one-argument Ubuntu Include is the
/// only accepted include graph; this makes the subsequent global-policy proof closed and bounded.
const SSHD_MAIN_INCLUDE_AWK: &str = "/^[[:space:]]*#/ || NF == 0 { next } \
    tolower($1) == \"include\" && NF == 2 \
      && $2 == \"/etc/ssh/sshd_config.d/*.conf\" { seen=1 } \
    END { if (!seen) exit 1 }";
const SSHD_GLOBAL_SHAPE_AWK: &str = "/^[[:space:]]*#/ || NF == 0 { next } \
    tolower($1) == \"match\" { bad=1 } \
    tolower($1) == \"include\" \
      && (NF != 2 || $2 != \"/etc/ssh/sshd_config.d/*.conf\") { bad=1 } \
    END { if (bad) exit 1 }";

fn sshd_policy_shape_preflight() -> String {
    format!(
        "test -f /etc/ssh/sshd_config && test ! -L /etc/ssh/sshd_config \
     && test -d /etc/ssh/sshd_config.d && test -d /run/systemd/system \
     && command -v sshd >/dev/null && command -v systemctl >/dev/null \
     && command -v flock >/dev/null \
     && test -z \"$(find /etc/ssh/sshd_config.d -maxdepth 1 -type l -print -quit 2>/dev/null)\" \
     && awk '{SSHD_MAIN_INCLUDE_AWK}' /etc/ssh/sshd_config \
     && find /etc/ssh/sshd_config /etc/ssh/sshd_config.d -maxdepth 1 -type f -print0 \
        | xargs -0 awk '{SSHD_GLOBAL_SHAPE_AWK}' \
     && sshd -t"
    )
}

fn inactive_ssh_guard_preflight() -> &'static str {
    "test ! -e /var/lib/ouro/onboard-ssh-rollback \
     && test ! -L /var/lib/ouro/onboard-ssh-rollback \
     && test ! -e /usr/local/sbin/ouro-onboard-ssh-rollback \
     && test ! -L /usr/local/sbin/ouro-onboard-ssh-rollback \
     && test ! -e /etc/systemd/system/ouro-onboard-ssh-rollback.service \
     && test ! -L /etc/systemd/system/ouro-onboard-ssh-rollback.service \
     && test ! -e /etc/systemd/system/ouro-onboard-ssh-rollback.timer \
     && test ! -L /etc/systemd/system/ouro-onboard-ssh-rollback.timer \
     && test ! -e /etc/systemd/system/timers.target.wants/ouro-onboard-ssh-rollback.timer \
     && test ! -L /etc/systemd/system/timers.target.wants/ouro-onboard-ssh-rollback.timer \
     && ! systemctl is-active --quiet ouro-onboard-ssh-rollback.timer 2>/dev/null \
     && ! systemctl is-active --quiet ouro-onboard-ssh-rollback.service 2>/dev/null \
     && { test ! -e /var/lib/ouro/onboard-ssh-rollback.lock \
          || { test -f /var/lib/ouro/onboard-ssh-rollback.lock \
               && test ! -L /var/lib/ouro/onboard-ssh-rollback.lock \
               && test \"$(stat -c '%U:%G:%a' /var/lib/ouro/onboard-ssh-rollback.lock)\" \
                       = \"root:root:600\"; }; }"
}

fn active_ssh_guard_lease_check() -> &'static str {
    "test \"$(systemctl show -p ActiveState --value ouro-onboard-ssh-rollback.timer)\" \
          = \"active\" \
     && test \"$(systemctl show -p SubState --value ouro-onboard-ssh-rollback.timer)\" \
             = \"waiting\" \
     && test \"$(systemctl show -p ActiveState --value ouro-onboard-ssh-rollback.service)\" \
             = \"inactive\""
}

fn principal_collision_preflight() -> &'static str {
    r#"for entry in 'ouro-op:/home/ouro-op:/bin/bash' 'ouro-diag:/home/ouro-diag:/bin/bash'; do
       user=${entry%%:*}; rest=${entry#*:}
       if id -u "$user" >/dev/null 2>&1; then
         test "$(getent passwd "$user" | cut -d: -f6-7)" = "$rest" || exit 1
         groups="$(id -nG "$user" | tr ' ' '\n' | LC_ALL=C sort | paste -sd ' ' -)" || exit 1
         case "$groups" in "$user"|"ouro-attest $user") ;; *) exit 1;; esac
         policy="$(LC_ALL=C sudo -n -l -U "$user" 2>/dev/null)" || exit 1
         grants="$(printf '%s\n' "$policy" | awk '
           /may run the following commands/ { commands=1; next }
           commands && NF { sub(/^[[:space:]]*/, ""); print }
         ')" || exit 1
         if test "$user" = ouro-diag; then
           test -z "$grants" || exit 1
         else
           expected='(root) NOPASSWD: /usr/local/sbin/ouro-inbox-stage
(root) NOPASSWD: /usr/local/sbin/ouro-op-run'
           test "$(printf '%s\n' "$grants" | LC_ALL=C sort)" = "$expected" || exit 1
         fi
       fi
     done"#
}

fn principal_converged_policy_check() -> String {
    format!(
        "{} && for user in ouro-op ouro-diag; do \
           test \"$(id -nG \"$user\" | tr ' ' '\\n' | LC_ALL=C sort | paste -sd ' ' -)\" \
                = \"ouro-attest $user\" || exit 1; \
         done",
        principal_collision_preflight()
    )
}

fn accepted_algorithm_check(control_pubkey: &str) -> Result<String> {
    let algorithm = control_pubkey.split_whitespace().next().unwrap_or("");
    let expression = match algorithm {
        "ssh-rsa" => {
            "(accepted[\"rsa-sha2-512\"] || accepted[\"rsa-sha2-256\"] || accepted[\"ssh-rsa\"])"
                .to_string()
        }
        "ssh-ed25519" | "sk-ssh-ed25519@openssh.com" | "ecdsa-sha2-nistp256"
        | "ecdsa-sha2-nistp384" => format!("accepted[\"{algorithm}\"]"),
        _ => {
            return Err(OuroError::Validation(
                "control public key algorithm is not supported for onboarding".into(),
            ))
        }
    };
    Ok(expression)
}

/// Validate the complete effective global sshd policy before reload. Besides the closed user set,
/// this proves that the selected public key can satisfy the only required authentication factor.
fn effective_sshd_values_awk(bootstrap_user: &str, control_pubkey: &str) -> Result<String> {
    let accepted = accepted_algorithm_check(control_pubkey)?;
    Ok(format!(
        "BEGIN {{ allowed[\"ouro-op\"]=1; allowed[\"ouro-diag\"]=1; allowed[\"{bootstrap_user}\"]=1 }} \
         $1 == \"allowusers\" {{ for (i=2; i<=NF; i++) {{ if (!($i in allowed)) bad=1; seen[$i]=1 }} }} \
         $1 == \"permitrootlogin\" {{ root=1; if ($2 != \"no\") bad=1 }} \
         $1 == \"pubkeyauthentication\" {{ pubkey=1; if ($2 != \"yes\") bad=1 }} \
         $1 == \"passwordauthentication\" {{ password=1; if ($2 != \"no\") bad=1 }} \
         $1 == \"kbdinteractiveauthentication\" {{ keyboard=1; if ($2 != \"no\") bad=1 }} \
         $1 == \"authenticationmethods\" {{ methods=1; if ($2 != \"publickey\" || NF != 2) bad=1 }} \
         $1 == \"authorizedkeysfile\" {{ keyfile=1; if ($2 != \".ssh/authorized_keys\" || NF != 2) bad=1 }} \
         $1 == \"authorizedkeyscommand\" {{ keycommand=1; if ($2 != \"none\" || NF != 2) bad=1 }} \
         $1 == \"authorizedkeyscommanduser\" {{ keycommanduser=1; if ($2 != \"none\" || NF != 2) bad=1 }} \
         $1 == \"trustedusercakeys\" {{ userca=1; if ($2 != \"none\" || NF != 2) bad=1 }} \
         $1 == \"strictmodes\" {{ strict=1; if ($2 != \"yes\" || NF != 2) bad=1 }} \
         $1 == \"pubkeyacceptedalgorithms\" {{ algorithms=1; split($2, values, \",\"); for (i in values) accepted[values[i]]=1 }} \
         $1 == \"denyusers\" || $1 == \"allowgroups\" || $1 == \"denygroups\" {{ bad=1 }} \
         END {{ if (bad || !root || !pubkey || !password || !keyboard || !methods || !keyfile \
                   || !keycommand || !keycommanduser || !userca || !strict || !algorithms \
                   || !({accepted}) || !seen[\"ouro-op\"] || !seen[\"ouro-diag\"] \
                   || !seen[\"{bootstrap_user}\"]) exit 1 }}"
    ))
}

fn effective_sshd_policy_check(bootstrap_user: &str, control_pubkey: &str) -> Result<String> {
    Ok(format!(
        "{} && sshd -T | awk '{}'",
        sshd_policy_shape_preflight(),
        effective_sshd_values_awk(bootstrap_user, control_pubkey)?
    ))
}

fn effective_sshd_policy_and_reload(
    bootstrap_user: &str,
    control_pubkey: &str,
) -> Result<String> {
    Ok(format!(
        "{} && {{ systemctl reload ssh 2>/dev/null || service ssh reload 2>/dev/null || systemctl reload sshd 2>/dev/null; }}",
        effective_sshd_policy_check(bootstrap_user, control_pubkey)?
    ))
}

fn validate_guard_id(guard_id: &str) -> Result<()> {
    if !guard_id.is_empty()
        && guard_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Ok(())
    } else {
        Err(OuroError::Validation("invalid internal SSH rollback guard id".into()))
    }
}

fn validate_authkey_stage(authkey_stage: &str) -> Result<()> {
    let suffix = authkey_stage.strip_prefix("/tmp/ouro-onboard-authkey-").unwrap_or("");
    if !suffix.is_empty()
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Ok(())
    } else {
        Err(OuroError::Validation("invalid internal authorized-key staging path".into()))
    }
}

fn guard_stage_path(kind: &str, guard_id: &str) -> Result<String> {
    validate_guard_id(guard_id)?;
    if !matches!(kind, "script" | "service" | "timer" | "sshd") {
        return Err(OuroError::Validation("invalid internal SSH guard stage kind".into()));
    }
    Ok(format!("/tmp/ouro-onboard-{kind}-{guard_id}"))
}

fn ssh_rollback_arm_command(guard_id: &str) -> Result<String> {
    let script_stage = guard_stage_path("script", guard_id)?;
    let service_stage = guard_stage_path("service", guard_id)?;
    let timer_stage = guard_stage_path("timer", guard_id)?;
    Ok(format!(
        "test ! -L {SSH_ROLLBACK_LOCK} \
         && {{ test -e {SSH_ROLLBACK_LOCK} \
              || {{ umask 077; : > {SSH_ROLLBACK_LOCK}; \
                   chown root:root {SSH_ROLLBACK_LOCK}; chmod 0600 {SSH_ROLLBACK_LOCK}; }}; }} \
         && test -f {SSH_ROLLBACK_LOCK} && test ! -L {SSH_ROLLBACK_LOCK} \
         && test \"$(stat -c '%U:%G:%a' {SSH_ROLLBACK_LOCK})\" = \"root:root:600\" \
         && exec 9<>{SSH_ROLLBACK_LOCK} && flock -w 30 -x 9 \
         && {} && {} \
         && rm -rf {SSH_ROLLBACK_DIR} \
         && install -d -m 0700 -o root -g root {SSH_ROLLBACK_DIR} \
         && for name in 10-ouro.conf 20-ouro-s0019.conf; do \
              source=/etc/ssh/sshd_config.d/$name; \
              if test -e \"$source\" || test -L \"$source\"; then \
                test -f \"$source\" && test ! -L \"$source\" \
                && cp -a \"$source\" {SSH_ROLLBACK_DIR}/$name || exit 1; \
              else \
                install -m 0600 -o root -g root /dev/null {SSH_ROLLBACK_DIR}/$name.absent \
                || exit 1; \
              fi; \
            done \
         && printf '%s\\n' '{guard_id}' > {SSH_ROLLBACK_DIR}/guard-id \
         && chown root:root {SSH_ROLLBACK_DIR}/guard-id \
         && chmod 0600 {SSH_ROLLBACK_DIR}/guard-id \
         && install -m 0700 -o root -g root {script_stage} {SSH_ROLLBACK_SCRIPT_PATH} \
         && install -m 0644 -o root -g root {service_stage} {SSH_ROLLBACK_SERVICE_PATH} \
         && install -m 0644 -o root -g root {timer_stage} {SSH_ROLLBACK_TIMER_PATH} \
         && rm -f {script_stage} {service_stage} {timer_stage} \
         && systemctl daemon-reload \
         && systemctl enable --now ouro-onboard-ssh-rollback.timer \
         && {}",
        inactive_ssh_guard_preflight(),
        sshd_policy_shape_preflight(),
        active_ssh_guard_lease_check()
    ))
}

fn guarded_ssh_policy_commit_command(
    guard_id: &str,
    bootstrap_user: &str,
    control_pubkey: &str,
) -> Result<String> {
    let sshd_stage = guard_stage_path("sshd", guard_id)?;
    Ok(format!(
        "test -f {SSH_ROLLBACK_LOCK} && test ! -L {SSH_ROLLBACK_LOCK} \
         && test \"$(stat -c '%U:%G:%a' {SSH_ROLLBACK_LOCK})\" = \"root:root:600\" \
         && exec 9<>{SSH_ROLLBACK_LOCK} && flock -w 30 -x 9 \
         && test \"$(cat {SSH_ROLLBACK_DIR}/guard-id 2>/dev/null)\" = '{guard_id}' \
         && {} \
         && install -m 0644 -o root -g root {sshd_stage} {SSHD_DROP_IN_PATH} \
         && rm -f {LEGACY_SSHD_DROP_IN_PATH} \
         && {} \
         && rm -f {sshd_stage} \
         && {}",
        active_ssh_guard_lease_check(),
        effective_sshd_policy_and_reload(bootstrap_user, control_pubkey)?,
        active_ssh_guard_lease_check()
    ))
}

fn ssh_rollback_disarm_command(
    guard_id: &str,
    bootstrap_user: &str,
    control_pubkey: &str,
) -> Result<String> {
    validate_guard_id(guard_id)?;
    Ok(format!(
        "test -f {SSH_ROLLBACK_LOCK} && test ! -L {SSH_ROLLBACK_LOCK} \
         && test \"$(stat -c '%U:%G:%a' {SSH_ROLLBACK_LOCK})\" = \"root:root:600\" \
         && exec 9<>{SSH_ROLLBACK_LOCK} && flock -w 30 -x 9 \
         && test \"$(cat {SSH_ROLLBACK_DIR}/guard-id 2>/dev/null)\" = '{guard_id}' \
         && {} \
         && {} && {} \
         && systemctl disable --now ouro-onboard-ssh-rollback.timer \
         && ! systemctl is-active --quiet ouro-onboard-ssh-rollback.service \
         && rm -f {SSH_ROLLBACK_TIMER_PATH} {SSH_ROLLBACK_SERVICE_PATH} \
                    {SSH_ROLLBACK_SCRIPT_PATH} \
         && systemctl daemon-reload \
         && rm -rf {SSH_ROLLBACK_DIR}",
        active_ssh_guard_lease_check(),
        principal_converged_policy_check(),
        effective_sshd_policy_check(bootstrap_user, control_pubkey)?
    ))
}

/// The greenfield onboard plan (S0019). Idempotent; installs the S0019 confinement, binary, the
/// /var/lib/ouro dir the adoption attestation lives in, and the shared confirm secret.
pub fn onboard_plan(
    bootstrap_user: &str,
    control_pubkey: &str,
    ouro_binary: &Path,
    confirm_secret: &str,
    authkey_stage: &str,
    guard_id: &str,
) -> Result<Vec<Step>> {
    validate_bootstrap_user(bootstrap_user)?;
    validate_authkey_stage(authkey_stage)?;
    validate_guard_id(guard_id)?;
    let run = |desc: &str, cmd: &str| Step::Run { desc: desc.into(), cmd: cmd.into() };
    let content = |desc: &str, content: String, remote: &str, mode: &str| Step::PushContent {
        desc: desc.into(),
        content,
        remote: remote.into(),
        mode: mode.into(),
    };
    // Create a confined principal only if absent (idempotent).
    let principal = |desc: &str, user: &str, cmd: &str| {
        run(desc, &format!("id -u {user} >/dev/null 2>&1 || {cmd}"))
    };
    let rollback_script_stage = guard_stage_path("script", guard_id)?;
    let rollback_service_stage = guard_stage_path("service", guard_id)?;
    let rollback_timer_stage = guard_stage_path("timer", guard_id)?;
    let sshd_stage = guard_stage_path("sshd", guard_id)?;
    Ok(vec![
        run("create attestation reader group", "getent group ouro-attest >/dev/null 2>&1 || groupadd --system ouro-attest"),
        // The two S0019 principals: ouro-op (write, via wrapper) and ouro-diag (read, unprivileged).
        principal("create ouro-op", "ouro-op", "useradd -m -s /bin/bash ouro-op"),
        principal("create ouro-diag", "ouro-diag", "useradd -m -s /bin/bash ouro-diag"),
        run("prepare ouro-op home", "install -d -m 0755 -o ouro-op -g ouro-op /home/ouro-op"),
        run("prepare ouro-diag home", "install -d -m 0755 -o ouro-diag -g ouro-diag /home/ouro-diag"),
        run("grant attestation read group", "usermod -a -G ouro-attest ouro-op && usermod -a -G ouro-attest ouro-diag"),
        // Root-owned state; only the dedicated read group can traverse/read attestations.
        run("prepare /var/lib/ouro", "install -d -m 0750 -o root -g ouro-attest /var/lib/ouro && install -d -m 0700 -o root -g root /var/lib/ouro/inbox"),
        // The single binary (the same one that ran onboard, pushed to the target).
        Step::Push {
            desc: "install ouro-ops binary".into(),
            local: ouro_binary.to_path_buf(),
            remote: "/usr/local/bin/ouro-ops".into(),
            mode: "0755".into(),
        },
        // Shared confirm secret (p6-3): the target verifies a control-minted confirm-token with the
        // SAME HMAC key, so the operator's approval on control is honored here.
        content("install shared confirm secret", confirm_secret.to_string(), CONFIRM_SECRET_PATH, "0400"),
        run("own confirm secret", &format!("chown root:root {CONFIRM_SECRET_PATH}")),
        // Confinement: the op wrapper + sudoers, validated by visudo.
        content("install op wrapper", OP_WRAPPER.to_string(), "/usr/local/sbin/ouro-op-run", "0755"),
        content("install inbox wrapper", INBOX_WRAPPER.to_string(), "/usr/local/sbin/ouro-inbox-stage", "0755"),
        content("install sudoers confinement", OP_SUDOERS.to_string(), "/etc/sudoers.d/ouro-op", "0440"),
        run("validate sudoers", "visudo -cf /etc/sudoers.d/ouro-op"),
        // Converge the non-SSH S0017 privilege paths after their S0019 replacements exist.
        run(
            "retire S0017 privilege path",
            &format!("rm -f {LEGACY_SUDOERS_PATH} {LEGACY_WRAPPER_PATH}"),
        ),
        // Control key -> BOTH principals' authorized_keys (staged root-owned, then install-chowned).
        run("prepare ouro-op .ssh", "install -d -m 0700 -o ouro-op -g ouro-op /home/ouro-op/.ssh"),
        run(
            "install control key (ouro-op)",
            &format!("install -m 0600 -o ouro-op -g ouro-op {authkey_stage} /home/ouro-op/.ssh/authorized_keys"),
        ),
        run("prepare ouro-diag .ssh", "install -d -m 0700 -o ouro-diag -g ouro-diag /home/ouro-diag/.ssh"),
        run(
            "install control key (ouro-diag)",
            &format!(
                "install -m 0600 -o ouro-diag -g ouro-diag {authkey_stage} /home/ouro-diag/.ssh/authorized_keys \
                 && rm -f {authkey_stage}"
            ),
        ),
        // Run-unique staging cannot overwrite another execution's active rollback guard. The arm
        // step installs these files only while holding the root-owned lock.
        content(
            "stage SSH rollback script",
            SSH_ROLLBACK_SCRIPT.to_string(),
            &rollback_script_stage,
            "0700",
        ),
        content(
            "stage SSH rollback service",
            SSH_ROLLBACK_SERVICE.to_string(),
            &rollback_service_stage,
            "0644",
        ),
        content(
            "stage SSH rollback timer",
            SSH_ROLLBACK_TIMER.to_string(),
            &rollback_timer_stage,
            "0644",
        ),
        content(
            "stage hardened sshd policy",
            sshd_conf(bootstrap_user),
            &sshd_stage,
            "0644",
        ),
        // Persistent, reboot-surviving rollback guard. The final step holds the same lock while it
        // verifies the fresh lease, installs/removes policy, checks effective sshd and reloads.
        run("arm SSH policy rollback", &ssh_rollback_arm_command(guard_id)?),
        run(
            "guarded install, validate and reload SSH policy",
            &guarded_ssh_policy_commit_command(guard_id, bootstrap_user, control_pubkey)?,
        ),
    ])
}

fn expected_file(path: &str, bytes: &[u8], ownership_and_mode: &str) -> String {
    format!(
        "test -f {path} && test ! -L {path} && \
         test \"$(sha256sum {path} 2>/dev/null | awk '{{print $1}}')\" = \"{digest}\" && \
         test \"$(stat -c '%U:%G:%a' {path} 2>/dev/null)\" = \"{ownership_and_mode}\"",
        digest = crate::skills::sha256_hex(bytes),
    )
}

fn expected_dir(path: &str, ownership_and_mode: &str) -> String {
    format!(
        "test -d {path} && test ! -L {path} && \
         test \"$(stat -c '%U:%G:%a' {path} 2>/dev/null)\" = \"{ownership_and_mode}\""
    )
}

/// A closed read-only state probe used to make an already-converged real rerun a genuine no-op.
/// It compares content hashes and ownership/modes without returning file contents or secret bytes.
fn convergence_probe(
    bootstrap_user: &str,
    control_pubkey: &str,
    ouro_binary: &Path,
    confirm_secret: &str,
) -> Result<String> {
    validate_bootstrap_user(bootstrap_user)?;
    let binary = std::fs::read(ouro_binary)?;
    let control_key = control_pubkey.trim_end().as_bytes();
    let checks = vec![
        "getent group ouro-attest >/dev/null".to_string(),
        "id -u ouro-op >/dev/null && id -u ouro-diag >/dev/null".to_string(),
        "id -nG ouro-op | tr ' ' '\n' | grep -qx ouro-attest".to_string(),
        "id -nG ouro-diag | tr ' ' '\n' | grep -qx ouro-attest".to_string(),
        "test \"$(getent passwd ouro-op | cut -d: -f6-7)\" = \"/home/ouro-op:/bin/bash\""
            .to_string(),
        "test \"$(getent passwd ouro-diag | cut -d: -f6-7)\" = \"/home/ouro-diag:/bin/bash\""
            .to_string(),
        principal_converged_policy_check(),
        expected_dir("/home/ouro-op", "ouro-op:ouro-op:755"),
        expected_dir("/home/ouro-diag", "ouro-diag:ouro-diag:755"),
        expected_dir("/home/ouro-op/.ssh", "ouro-op:ouro-op:700"),
        expected_dir("/home/ouro-diag/.ssh", "ouro-diag:ouro-diag:700"),
        expected_dir("/var/lib/ouro", "root:ouro-attest:750"),
        expected_dir("/var/lib/ouro/inbox", "root:root:700"),
        expected_file(SSH_ROLLBACK_LOCK, b"", "root:root:600"),
        expected_file("/usr/local/bin/ouro-ops", &binary, "root:root:755"),
        expected_file(
            CONFIRM_SECRET_PATH,
            confirm_secret.as_bytes(),
            "root:root:400",
        ),
        expected_file(
            "/usr/local/sbin/ouro-op-run",
            OP_WRAPPER.as_bytes(),
            "root:root:755",
        ),
        expected_file(
            "/usr/local/sbin/ouro-inbox-stage",
            INBOX_WRAPPER.as_bytes(),
            "root:root:755",
        ),
        expected_file(
            "/etc/sudoers.d/ouro-op",
            OP_SUDOERS.as_bytes(),
            "root:root:440",
        ),
        expected_file(
            SSHD_DROP_IN_PATH,
            sshd_conf(bootstrap_user).as_bytes(),
            "root:root:644",
        ),
        expected_file(
            "/home/ouro-op/.ssh/authorized_keys",
            control_key,
            "ouro-op:ouro-op:600",
        ),
        expected_file(
            "/home/ouro-diag/.ssh/authorized_keys",
            control_key,
            "ouro-diag:ouro-diag:600",
        ),
        format!(
            "test ! -e {LEGACY_SSHD_DROP_IN_PATH} && test ! -L {LEGACY_SSHD_DROP_IN_PATH} && \
             test ! -e {LEGACY_SUDOERS_PATH} && test ! -L {LEGACY_SUDOERS_PATH} && \
             test ! -e {LEGACY_WRAPPER_PATH} && test ! -L {LEGACY_WRAPPER_PATH} && \
             test ! -e {SSH_ROLLBACK_DIR} && test ! -L {SSH_ROLLBACK_DIR} && \
             test ! -e {SSH_ROLLBACK_SCRIPT_PATH} && test ! -L {SSH_ROLLBACK_SCRIPT_PATH} && \
             test ! -e {SSH_ROLLBACK_SERVICE_PATH} && test ! -L {SSH_ROLLBACK_SERVICE_PATH} && \
             test ! -e {SSH_ROLLBACK_TIMER_PATH} && test ! -L {SSH_ROLLBACK_TIMER_PATH} && \
             test ! -e /etc/systemd/system/timers.target.wants/ouro-onboard-ssh-rollback.timer \
             && test ! -L /etc/systemd/system/timers.target.wants/ouro-onboard-ssh-rollback.timer"
        ),
        inactive_ssh_guard_preflight().to_string(),
        effective_sshd_policy_check(bootstrap_user, control_pubkey)?,
    ];
    Ok(checks.join(" && "))
}

fn result_step(
    desc: String,
    kind: &str,
    outcome: &BootstrapOutcome,
    dry_run: bool,
    mutating: bool,
) -> StepResult {
    StepResult {
        desc,
        kind: kind.into(),
        remote: None,
        status: outcome.status,
        changed: mutating && !dry_run && outcome.status == 0,
        planned: dry_run,
        executed: !dry_run,
    }
}

fn execution_id() -> Result<String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| OuroError::Validation("system clock predates Unix epoch".into()))?
        .as_nanos();
    Ok(format!("{}-{nanos}", std::process::id()))
}

/// Run the greenfield onboard plan on a target. Returns the auditable install manifest.
pub fn execute_onboard(
    transport: &BootstrapTransport,
    target: &BootstrapTarget,
    key_path: &Path,
    host_key: HostKeyCheck,
    control_pubkey: &str,
    ouro_binary: &Path,
) -> Result<InstallManifest> {
    // The shared confirm secret provisioned to the target = the control's own confirm/tool-run
    // secret, so a control-minted confirm-token verifies target-side (p6-3).
    let paths = crate::config::ConfigPaths::discover();
    let confirm_secret = crate::confirm::load_or_create_secret(&paths.tool_run_secret)?;
    let run_id = execution_id()?;
    let authkey_stage = format!("/tmp/ouro-onboard-authkey-{run_id}");
    let plan = onboard_plan(
        &target.user,
        control_pubkey,
        ouro_binary,
        &confirm_secret,
        &authkey_stage,
        &run_id,
    )?;

    // This is the first remote action and is provably read-only. A policy shape, principal
    // collision, or persistent-timer prerequisite failure stops before execute_plan stages bytes.
    let preflight_command = format!(
        "{} && {} && {}",
        sshd_policy_shape_preflight(),
        inactive_ssh_guard_preflight(),
        principal_collision_preflight()
    );
    let preflight = transport.run(target, key_path, host_key, &preflight_command)?;
    if !transport.dry_run && preflight.status != 0 {
        let detail = if preflight.status == 255 {
            format!(": {}", preflight.stderr.trim())
        } else {
            String::new()
        };
        return Err(OuroError::Validation(format!(
            "onboard read-only SSH/principal preflight refused before any remote write{detail}"
        )));
    }
    let preflight_result = result_step(
        "read-only SSH policy and principal preflight".into(),
        "preflight",
        &preflight,
        transport.dry_run,
        false,
    );

    let mut plan_executed = transport.dry_run;
    let mut manifest = if transport.dry_run {
        let mut manifest = execute_plan(
            transport,
            target,
            key_path,
            host_key,
            control_pubkey,
            &authkey_stage,
            plan,
        )?
        .0;
        manifest.steps.insert(0, preflight_result);
        manifest
    } else {
        let command = convergence_probe(
            &target.user,
            control_pubkey,
            ouro_binary,
            &confirm_secret,
        )?;
        let current = transport.run(target, key_path, host_key, &command)?;
        if current.status == 255 {
            return Err(OuroError::Validation(format!(
                "could not probe existing S0019 onboarding state: {}",
                current.stderr.trim()
            )));
        }
        if current.status == 0 {
            InstallManifest {
                host: target.host.clone(),
                bootstrap_user: target.user.clone(),
                steps: vec![
                    preflight_result,
                    result_step(
                        "S0019 state already converged".into(),
                        "probe",
                        &current,
                        false,
                        false,
                    ),
                ],
                ok: true,
            }
        } else {
            plan_executed = true;
            let mut manifest = execute_plan(
                transport,
                target,
                key_path,
                host_key,
                control_pubkey,
                &authkey_stage,
                plan,
            )?
            .0;
            manifest.steps.insert(0, preflight_result);
            manifest
        }
    };

    if manifest.ok {
        for (label, user) in [
            ("bootstrap", target.user.as_str()),
            ("write principal", "ouro-op"),
            ("diagnostic principal", "ouro-diag"),
        ] {
            let login_target = BootstrapTarget {
                host: target.host.clone(),
                port: target.port,
                user: user.to_string(),
            };
            let outcome = transport.probe_login(&login_target, key_path, host_key)?;
            manifest.steps.push(result_step(
                format!("fresh SSH login ({label}: {user})"),
                "probe",
                &outcome,
                transport.dry_run,
                false,
            ));
            manifest.ok &= outcome.status == 0;
        }
    }

    // Only the execution that armed this guard may disarm it. The command takes the same lock as
    // rollback, verifies the guard id and the effective policy again, then removes timer artifacts.
    if manifest.ok && plan_executed {
        let command = ssh_rollback_disarm_command(&run_id, &target.user, control_pubkey)?;
        let outcome = transport.run(target, key_path, host_key, &command)?;
        manifest.steps.push(result_step(
            "disarm verified SSH policy rollback".into(),
            "run",
            &outcome,
            transport.dry_run,
            true,
        ));
        manifest.ok &= outcome.status == 0;
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    const TEST_KEY: &str = "ssh-ed25519 AAAA0123456789abcdef operator@control";
    const TEST_STAGE: &str = "/tmp/ouro-onboard-authkey-test-1";
    const TEST_GUARD: &str = "test-guard-1";

    fn test_plan(user: &str) -> Result<Vec<Step>> {
        onboard_plan(
            user,
            TEST_KEY,
            Path::new("/tmp/ouro-ops"),
            "secret",
            TEST_STAGE,
            TEST_GUARD,
        )
    }

    fn descs(plan: &[Step]) -> Vec<&str> {
        plan.iter().map(|s| s.desc()).collect()
    }

    fn awk_accepts(program: &str, input: &str) -> bool {
        let mut child = Command::new("awk")
            .arg(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(input.as_bytes()).unwrap();
        child.wait().unwrap().success()
    }

    #[test]
    fn plan_installs_s0019_confinement_not_s0017() {
        let plan = test_plan("ubuntu").unwrap();
        let d = descs(&plan);
        assert!(d.contains(&"create ouro-op") && d.contains(&"create ouro-diag"));
        assert!(d.contains(&"install op wrapper"));
        assert!(d.contains(&"install inbox wrapper"));
        // The op wrapper only runs `ouro-ops op`, never tool run.
        assert!(OP_WRAPPER.contains("ouro-ops op \"$@\""));
        assert!(OP_WRAPPER.contains("OURO_HOME=/var/lib/ouro"));
        assert!(OP_WRAPPER.contains("unset OURO_ATTESTATION OURO_ALLOWLIST_FILE OURO_PROBE_LIB"));
        assert!(INBOX_WRAPPER.contains("OURO_HOME=/var/lib/ouro"));
        assert!(INBOX_WRAPPER.contains("unset OURO_ATTESTATION OURO_ALLOWLIST_FILE OURO_PROBE_LIB"));
        assert!(!OP_WRAPPER.contains("tool run"), "greenfield: no S0017 tool-run wrapper");
        // sudoers confines ouro-op to the op wrapper only.
        assert!(OP_SUDOERS.contains("ouro-op ALL=(root) NOPASSWD: /usr/local/sbin/ouro-op-run"));
        assert!(OP_SUDOERS.contains("/usr/local/sbin/ouro-inbox-stage"));
        assert!(!OP_SUDOERS.contains("ouro-tool-run"));
        assert!(d.contains(&"retire S0017 privilege path"));
        assert!(d.contains(&"guarded install, validate and reload SSH policy"));
        assert!(plan
            .iter()
            .filter_map(|step| match step { Step::Run { cmd, .. } => Some(cmd), _ => None })
            .any(|command| command.contains(LEGACY_SSHD_DROP_IN_PATH)));
    }

    #[test]
    fn ordering_binary_and_wrapper_before_keys() {
        let plan = test_plan("ubuntu").unwrap();
        let d = descs(&plan);
        let pos = |x: &str| d.iter().position(|s| *s == x).unwrap();
        assert!(pos("install ouro-ops binary") < pos("install op wrapper"));
        assert!(pos("install op wrapper") < pos("install control key (ouro-op)"));
        assert!(pos("create attestation reader group") == 0, "attestation group first");
        assert!(pos("install control key (ouro-diag)") < pos("arm SSH policy rollback"));
        assert!(pos("stage hardened sshd policy") < pos("arm SSH policy rollback"));
        assert!(
            pos("arm SSH policy rollback")
                < pos("guarded install, validate and reload SSH policy")
        );
        assert!(
            pos("guarded install, validate and reload SSH policy") == d.len() - 1,
            "locked policy commit is last"
        );
    }

    #[test]
    fn sshd_allows_only_s0019_principals_and_bootstrap() {
        let c = sshd_conf("ubuntu");
        assert!(c.contains("PermitRootLogin no") && c.contains("PasswordAuthentication no"));
        assert!(c.contains("AuthenticationMethods publickey"));
        assert!(c.contains("AuthorizedKeysFile .ssh/authorized_keys"));
        assert!(c.contains("TrustedUserCAKeys none"));
        assert!(c.contains("AllowUsers ouro-op ouro-diag ubuntu"));
        assert!(!c.contains("ouro-exec"), "greenfield principals only");
    }

    #[test]
    fn sshd_shape_awk_rejects_match_missing_or_multi_path_include() {
        let standard = "Include /etc/ssh/sshd_config.d/*.conf\nPermitRootLogin prohibit-password\n";
        assert!(awk_accepts(SSHD_MAIN_INCLUDE_AWK, standard));
        assert!(awk_accepts(SSHD_GLOBAL_SHAPE_AWK, standard));
        assert!(!awk_accepts(
            SSHD_MAIN_INCLUDE_AWK,
            "PermitRootLogin prohibit-password\n"
        ));
        let multi =
            "Include /etc/ssh/sshd_config.d/*.conf /etc/ssh/custom.conf\n";
        assert!(!awk_accepts(SSHD_MAIN_INCLUDE_AWK, multi));
        assert!(!awk_accepts(SSHD_GLOBAL_SHAPE_AWK, multi));
        assert!(!awk_accepts(
            SSHD_GLOBAL_SHAPE_AWK,
            "Match User root\nPasswordAuthentication yes\n"
        ));
    }

    #[test]
    fn effective_sshd_awk_rejects_every_key_authentication_bypass() {
        let program = effective_sshd_values_awk("cardano", TEST_KEY).unwrap();
        let good = "allowusers ouro-op\n\
allowusers ouro-diag\n\
allowusers cardano\n\
permitrootlogin no\n\
pubkeyauthentication yes\n\
passwordauthentication no\n\
kbdinteractiveauthentication no\n\
authenticationmethods publickey\n\
authorizedkeysfile .ssh/authorized_keys\n\
authorizedkeyscommand none\n\
authorizedkeyscommanduser none\n\
trustedusercakeys none\n\
strictmodes yes\n\
pubkeyacceptedalgorithms ssh-ed25519,rsa-sha2-512\n";
        assert!(awk_accepts(&program, good));
        for bad in [
            good.replace("authenticationmethods publickey", "authenticationmethods any"),
            good.replace(
                "authorizedkeysfile .ssh/authorized_keys",
                "authorizedkeysfile .ssh/authorized_keys /etc/ssh/shared-keys",
            ),
            good.replace("authorizedkeyscommand none", "authorizedkeyscommand /usr/bin/keys"),
            good.replace("trustedusercakeys none", "trustedusercakeys /etc/ssh/user-ca.pub"),
            good.replace("strictmodes yes", "strictmodes no"),
            good.replace(
                "pubkeyacceptedalgorithms ssh-ed25519,rsa-sha2-512",
                "pubkeyacceptedalgorithms rsa-sha2-512",
            ),
        ] {
            assert!(!awk_accepts(&program, &bad), "unsafe fixture passed:\n{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn principal_policy_rejects_extra_groups_and_sudo_grants() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "ouro-principal-policy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let scripts = [
            (
                "id",
                "#!/bin/sh\ncase \"$1\" in\n  -u) exit 0;;\n  -nG)\n    if test \"$OURO_TEST_NO_ATTEST\" = \"$2\"; then echo \"$2\";\n    elif test \"$OURO_TEST_EXTRA_GROUP\" = \"$2\"; then echo \"$2 ouro-attest wheel\";\n    else echo \"$2 ouro-attest\"; fi;;\nesac\n",
            ),
            (
                "getent",
                "#!/bin/sh\nu=$2\nprintf '%s:x:1000:1000::/home/%s:/bin/bash\\n' \"$u\" \"$u\"\n",
            ),
            (
                "sudo",
                "#!/bin/sh\nfor arg in \"$@\"; do user=$arg; done\nif test \"$user\" = ouro-diag; then\n  echo 'User ouro-diag is not allowed to run sudo on fixture.'\nelse\n  echo 'User ouro-op may run the following commands on fixture:'\n  echo '    (root) NOPASSWD: /usr/local/sbin/ouro-op-run'\n  echo '    (root) NOPASSWD: /usr/local/sbin/ouro-inbox-stage'\n  test -z \"$OURO_TEST_EXTRA_SUDO\" || echo '    (ALL) NOPASSWD: ALL'\nfi\n",
            ),
        ];
        for (name, content) in scripts {
            let path = dir.join(name);
            std::fs::write(&path, content).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let run = |program: &str, key: Option<&str>, value: Option<&str>| {
            let mut command = Command::new("sh");
            command.arg("-c").arg(program).env("PATH", &path);
            if let (Some(key), Some(value)) = (key, value) {
                command.env(key, value);
            }
            command.status().unwrap().success()
        };
        assert!(run(principal_collision_preflight(), None, None));
        assert!(!run(
            principal_collision_preflight(),
            Some("OURO_TEST_EXTRA_GROUP"),
            Some("ouro-diag")
        ));
        assert!(!run(
            principal_collision_preflight(),
            Some("OURO_TEST_EXTRA_SUDO"),
            Some("1")
        ));
        assert!(run(
            principal_collision_preflight(),
            Some("OURO_TEST_NO_ATTEST"),
            Some("ouro-diag")
        ));
        assert!(!run(
            &principal_converged_policy_check(),
            Some("OURO_TEST_NO_ATTEST"),
            Some("ouro-diag")
        ));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn typed_ssh_policy_exposes_the_runtime_bootstrap_user() {
        let policy = ssh_access_policy("cardano");
        assert_eq!(policy.drop_in, SSHD_DROP_IN_PATH);
        assert_eq!(policy.allow_users, ["ouro-op", "ouro-diag", "cardano"]);
        assert_eq!(policy.bootstrap_user, "cardano");
        assert!(policy.bootstrap_user_preserved);
        assert!(policy.rendered_config.contains("AllowUsers ouro-op ouro-diag cardano"));
        assert_eq!(
            policy.legacy_s0017_paths_retired,
            [LEGACY_SSHD_DROP_IN_PATH, LEGACY_SUDOERS_PATH, LEGACY_WRAPPER_PATH]
        );
        let effective = effective_sshd_policy_and_reload("cardano", TEST_KEY).unwrap();
        assert!(effective.contains("sshd -T"));
        assert!(effective.contains("!seen[\"cardano\"]"));
        assert!(effective.contains("if (!($i in allowed)) bad=1"));
        assert!(effective.contains("tolower($1) == \"match\""));
        assert!(effective.contains("permitrootlogin"));
        assert!(effective.contains("pubkeyauthentication"));
        assert!(effective.contains("passwordauthentication"));
        assert!(effective.contains("kbdinteractiveauthentication"));
        assert!(effective.contains("authenticationmethods"));
        assert!(effective.contains("authorizedkeysfile"));
        assert!(effective.contains("authorizedkeyscommand"));
        assert!(effective.contains("trustedusercakeys"));
        assert!(effective.contains("strictmodes"));
        assert!(effective.contains("accepted[\"ssh-ed25519\"]"));
        assert!(!effective.contains("|| true"), "reload failure must fail onboarding");
        let shape = sshd_policy_shape_preflight();
        assert!(shape.contains("NF != 2"));
        assert!(shape.contains("if (!seen) exit 1"));
        assert!(shape.contains("command -v flock"));
    }

    #[test]
    fn plan_builder_rejects_untrusted_user_tokens() {
        for invalid in [
            "",
            "Cardano",
            "cardano' BEGIN { system(\"id\") }",
            "a/b",
            "root",
            "ouro-op",
            "ouro-diag",
        ] {
            assert!(
                test_plan(invalid).is_err(),
                "rejected {invalid:?}"
            );
        }
    }

    #[test]
    fn executor_stages_key_at_the_path_the_onboard_plan_consumes() {
        let transport = BootstrapTransport::new(true);
        let target = BootstrapTarget {
            host: "10.0.0.10".into(),
            port: 22,
            user: "ubuntu".into(),
        };
        let (manifest, _, _) = crate::provision::execute_plan(
            &transport,
            &target,
            Path::new("/k"),
            HostKeyCheck::AcceptNew,
            TEST_KEY,
            TEST_STAGE,
            test_plan("ubuntu").unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.steps[0].remote.as_deref(), Some(TEST_STAGE));
        assert!(test_plan("ubuntu")
            .unwrap()
            .iter()
            .filter_map(|step| match step { Step::Run { cmd, .. } => Some(cmd), _ => None })
            .any(|command| command.contains(TEST_STAGE)));
    }

    #[test]
    fn rollback_guard_is_persistent_locked_and_identity_bound() {
        assert!(SSH_ROLLBACK_TIMER.contains("WantedBy=timers.target"));
        assert!(SSH_ROLLBACK_TIMER.contains("OnActiveSec=2min"));
        assert!(SSH_ROLLBACK_SCRIPT.contains("flock -w 30 -x 9"));
        assert!(SSH_ROLLBACK_SCRIPT.contains("10-ouro.conf"));
        assert!(SSH_ROLLBACK_SCRIPT.contains("20-ouro-s0019.conf"));
        assert!(SSH_ROLLBACK_SCRIPT.contains("sshd -t"));
        let arm = ssh_rollback_arm_command(TEST_GUARD).unwrap();
        let commit =
            guarded_ssh_policy_commit_command(TEST_GUARD, "cardano", TEST_KEY).unwrap();
        let disarm = ssh_rollback_disarm_command(TEST_GUARD, "cardano", TEST_KEY).unwrap();
        assert!(arm.contains("/tmp/ouro-onboard-script-test-guard-1"));
        assert!(arm.contains("systemctl enable --now ouro-onboard-ssh-rollback.timer"));
        assert!(commit.contains("systemctl show -p SubState --value"));
        assert!(commit.contains("rm -f /etc/ssh/sshd_config.d/10-ouro.conf"));
        assert!(commit.contains("sshd -T"));
        assert!(disarm.contains("test \"$(cat /var/lib/ouro/onboard-ssh-rollback/guard-id"));
        assert!(disarm.contains("sshd -T"));
        assert!(disarm.contains("systemctl disable --now ouro-onboard-ssh-rollback.timer"));
    }
}
