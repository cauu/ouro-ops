# Runtime Skill

## Purpose
Apply runtime topology/config changes and verify the node returns healthy.

## Decision Tree
- Validate spec with `ouro spec validate`.
- Render config with `ouro config render`.
- Apply topology with `runtime/topology-apply`.
- Restart only with `runtime/restart`.
- Verify with `runtime/verify`.

## Stop Conditions
- Stop on verify failure and use L3 read-only diagnostics.
- Stop on repeated restart failure and do not retry writes.

## Red Lines
- No direct file copy or container mutation outside tool entrypoints.
- No secret paths in output.
- Verify after every topology or restart change.
