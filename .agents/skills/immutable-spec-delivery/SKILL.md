---
name: immutable-spec-delivery
description: Execute software delivery with append-only specification documents as the single source of truth. Use when the user requires each request to be tracked in one spec containing requirement details, solution design, execution plan, and test acceptance criteria, and requires item-level commits (for example p3-1) that reference the spec filename.
---

# Immutable Spec Delivery

## Core Rules

1. Treat the spec as the only execution and acceptance standard for that request.
2. Keep specs append-only: never rewrite or delete existing text in a spec; add new sections or new entries only.
3. Create a new spec file for every new requirement or scope change.
4. Preserve traceability: every code change, test, and commit must map to a spec item ID.
5. Auto-commit after each completed item unless the user explicitly disables auto-commit.
6. Keep delivery linear: allow multiple historical spec files to exist, but permit only one active spec at any time.
7. Do not close or complete the active spec until the user explicitly confirms that the spec is finished, replaced, or cancelled.
8. Allow `draft` specs to be edited freely before execution starts.
9. Once a spec becomes `active`, treat it as append-only.

---

## Spec Location And Naming

Store specs under:

```text
docs/specs/                  # current active spec only
docs/specs/draft/            # not yet started
docs/specs/completed/        # finished specs
```

Rules:
- `docs/specs/` root may contain only one spec file, and it must be the current `active` spec.
- `docs/specs/draft/` may contain multiple draft specs.
- `docs/specs/completed/` contains all finished specs, regardless of whether they ended as delivered, replaced, or cancelled.

Filename rules:

1. Draft specs use a stable `Spec-ID` filename:

```text
docs/specs/draft/S0007-mithril-bootstrap.md
```

2. Active and completed specs use the execution start timestamp prefix plus the same `Spec-ID`:

```text
docs/specs/20260308T1030-S0007-mithril-bootstrap.md
docs/specs/completed/20260308T1030-S0007-mithril-bootstrap.md
```

3. When a draft becomes active:
- move it from `draft/` to `docs/specs/`
- rename it to `YYYYMMDDTHHMM-SpecID-slug.md`
- record `开始时间` and `前一个 Spec-ID` in the spec header

4. When an active spec becomes completed:
- move it to `docs/specs/completed/`
- keep the same filename
- record `完成时间` and `结项原因` in the spec header

## Spec File Contract

Use one spec file per request. Include all of these sections in that file:

```markdown
# <Spec Title>

Spec-ID: S0007
状态: draft | active | completed
创建时间:
开始时间:
完成时间:
前一个 Spec-ID:
结项原因:

## 1. Requirement Details
- Background
- Scope
- Constraints
- Non-goals

## 2. Outline Design
- Architecture / modules impacted
- Data model and interfaces
- Risk and rollback strategy

## 3. Execution Plan
- [ ] pX-1 <deliverable>
- [ ] pX-2 <deliverable>
- ...

## 4. Test And Acceptance Criteria
- TC-1 ...
- TC-2 ...
- Pass/fail criteria

## 5. Execution Log (append-only)
- <date> pX-1 started ...
- <date> pX-1 completed ...

## 6. Validation Evidence (append-only)
- <date> command/result summary mapped to TC-*

## 7. Change Requests (append-only)
- <date> requirement change / replacement / cancellation note
```

State rules:
- `draft`: written but not yet used for execution; content may be edited
- `active`: the only spec allowed to drive current work; content is append-only
- `completed`: execution is finished and the spec is closed with a terminal reason

Do not allow more than one `active` spec at the same time.

Completion rules:
- A spec can remain `active` even when all currently known items are marked `[x]`.
- Only change a spec from `active` to `completed` after the user explicitly says the spec is done.
- If the user reports bugs, regressions, or missing acceptance details before explicit closure, keep using the same active spec and append the follow-up work there.
- `completed` does not imply only one success case. It means the spec has ended.
- Required `结项原因` values:
  - `delivered`
  - `replaced`
  - `cancelled`

## Delivery Workflow

1. Read the active spec and extract pending item IDs (`pX-*`) plus mapped `TC-*`.
2. Implement exactly one item at a time.
3. Run the minimal test set that proves the mapped acceptance criteria.
4. Append execution and validation evidence to the spec (do not edit prior entries).
5. Commit immediately for that item.
6. Repeat until all plan items are complete.
7. After all known items are complete, keep the spec open until the user explicitly closes it or declares it complete.
8. When the user explicitly closes the active spec:
   - update `状态` to `completed`
   - fill `完成时间`
   - fill `结项原因`
   - move the file into `docs/specs/completed/`

## Item State Progression

Use only these states in `Execution Plan`:

- `[ ]` not started
- `[~]` in progress
- `[x]` completed

Transition rules:
- Change `[ ]` to `[~]` when work on that item actually starts.
- Change `[~]` to `[x]` only after implementation is complete, the mapped `TC-*` evidence is appended, and the item is ready to commit.
- Do not mark `[x]` based on implementation alone.
- If a closed-looking spec receives a bug report before user sign-off, append a new item instead of reopening history by editing old log statements.

## Commit Standard

Use one commit per completed item with this format:

```text
spec(<spec-filename>): <item-id> <short action>
```

Use this commit body template:

```text
Spec: <spec path>
Item: <item-id>
Acceptance: <TC list>
```

Rules:
- Include the spec filename and item ID in every commit message.
- Do not mix multiple item IDs in one commit unless the user explicitly approves.
- Do not submit an item as done without evidence mapped to `TC-*`.
- Treat "item completed" as all of: implementation finished, acceptance verified, and spec evidence appended.
- If the worktree contains unrelated uncommitted changes, do not auto-commit until those changes are isolated or the user explicitly approves the mixed commit.
- Follow-up bug-fix commits before spec closure must still use the same active spec filename and the new appended item ID.

## Validation Evidence

Use one append-only evidence line per acceptance check. Keep the format stable:

```text
TC-<n> | stack: <rust|node|python|ansible|ui|other> | command: <cmd or manual step> | result: <pass|fail> | note: <short observation>
```

Examples:
- `TC-1 | stack: rust | command: cargo test -q | result: pass | note: deploy payload defaults covered`
- `TC-2 | stack: node | command: pnpm build | result: pass | note: deploy wizard renders with new defaults`
- `TC-3 | stack: ansible | command: ansible-playbook --syntax-check ansible/playbooks/deploy.yml | result: pass | note: playbook syntax valid`
- `TC-4 | stack: ui | command: manual validation of takeover banner | result: pass | note: probe warning shown for running node`

Choose commands based on the project stack. Standardize the evidence format, not the toolchain.

## Bug Fixes Before Spec Closure

If the user reports a bug, regression, or missing acceptance detail for an active spec:

1. Do not create a new spec by default.
2. Append a new item to the same active spec for that bug fix.
3. Append any new `TC-*` acceptance lines needed for the bug fix.
4. Add new execution log and validation evidence entries for that appended item.
5. Commit the bug fix with the same spec filename and the appended item ID.

Recommended item naming:
- Continue the existing numbering if practical, for example `p35-7`.
- If the plan is already frozen and you need a clearly scoped repair item, use a suffix such as `p35-6-fix1`.

Create a new spec only when the user changes scope, constraints, or requirements beyond the current spec boundary.

## Handling Requirement Changes

1. Do not modify active spec requirement/design/plan sections in-place beyond append-only updates.
2. Create a new draft spec file for new scope or changed constraints.
3. Draft specs may be edited until they become active.
4. When a new spec replaces the current active spec:
   - mark the current active spec as `completed`
   - set `结项原因: replaced`
   - move it to `docs/specs/completed/`
   - promote the new draft to the only active spec
5. Record the replaced spec's `Spec-ID` in the new active spec as `前一个 Spec-ID`.
6. Do not inherit unfinished plan items automatically into the new spec. Re-state the new scope and plan explicitly in the new spec.

## Exceptions

Allow delayed commit only in these cases:

1. The user explicitly requests a single combined commit.
2. The item is partially implemented but does not yet satisfy its acceptance criteria.
3. An external blocker prevents collecting the required evidence.

If an exception is used, append the reason to `Execution Log` before pausing the commit.

## Rollback And Recovery

Treat rollback as a new forward-only change, not as history rewrite.

Rules:
- Never rewrite or delete old spec content to represent a rollback.
- Prefer `git revert <commit>` over destructive history editing.
- Create a new rollback draft spec for committed or released changes that must be undone.
- Record which original spec and item are being rolled back.
- Add rollback acceptance evidence before marking the rollback item complete.

Rollback spec minimum content:
- rollback reason
- affected original spec filename
- affected item IDs or commit SHAs
- rollback steps
- validation criteria after rollback

Rollback execution rules:
1. For uncommitted local mistakes, fix them in the working tree; this is not a formal rollback.
2. For committed but unreleased changes, prefer a new item that reverts or corrects the prior commit.
3. For released, deployed, or externally validated changes, require a new rollback spec and a dedicated rollback commit.
