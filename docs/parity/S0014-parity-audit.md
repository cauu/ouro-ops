# S0014 Parity Audit

Status: pass for v1 replacement gate

## B Class: Port Before Retire

| Legacy capability | New capability | Evidence |
| --- | --- | --- |
| `commands/kes.rs` KES flow | `ouro kes generate`, `ouro kes push`, `ouro kes counter status` | `cargo test -q`; `ci/harness-e2e.sh`; TC-2/TC-3/TC-18 |
| `commands/pool.rs` spec/config/register/status | `ouro spec validate`, `ouro config render/apply`, `ouro pool register-tx`, `ouro status --diff-spec` | TC-1/TC-13/TC-16/TC-17/TC-19 |
| `commands/staking.rs` point-in-time pool view | `ouro pool register-tx` manifest and planned `ouro pool overview` read path | drop trend UI; keep point facts as CLI JSON |
| `keychain.rs` SSH fingerprint helper | `creds://` references plus redacted runner command shape | TC-13/TC-15 |
| `db/` and `commands/machine.rs` audit/machine state | `AuditStore`, `pool-spec.yaml` machines, `ouro audit log` | TC-1/TC-4 |

## C Class: Convert To L2

| Legacy capability | New skill/script capability | Evidence |
| --- | --- | --- |
| deploy orchestration | `ouro-skills/deploy/scripts/{preflight,provision,sync,start,verify,takeover}` | TC-7/TC-8/TC-21/TC-23 |
| upgrade orchestration | `ouro-skills/upgrade/scripts/run.sh` with lock, BP-last, verify-before-next | TC-6/TC-22 |
| runtime config/restart | `ouro-skills/runtime/scripts/{topology-apply,restart,verify}` | TC-6 |
| observability bootstrap/rollback | `ouro-skills/observability/scripts/{install-gateway,verify,rollback}` | TC-6 |

## A Class Retirement Preconditions

| Direct retirement target | Replacement/drop decision | Gate |
| --- | --- | --- |
| React UI | Agent harness plus JSON CLI outputs | p4-4 may remove after dual-run tag |
| monitor page/catalog | Prometheus/Grafana gateway scripts | telemetry basic-auth must be handed to gateway |
| Python sidecar | Rust `ouro` runner/tool-run model | p1/p2 parity passed |
| Tauri task model | synchronous CLI plus append-only audit | `ouro audit log` available |
| Ansible playbooks | L2 scripts with idempotency helpers | p2 parity passed |

Decision: p4-4 may retire A-class UI/sidecar/task/monitor scaffolding only because B/C parity above has executable evidence. Staking trend UI is intentionally dropped; point facts remain in CLI JSON scope.
