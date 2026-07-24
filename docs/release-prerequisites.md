# Release Prerequisites

S0028 uses one read-only verifier for the external settings required by the CLI and Site release
workflows:

```sh
python3 packaging/release-prerequisites.py --repo cauu/ouro-ops
```

The verifier calls only GitHub `GET` endpoints. It lists GitHub environment secret names but never
reads secret values, and it does not write repository, release, environment, or Cloudflare state.
`status=prerequisites_missing` is an actionable report during repository implementation. Immediately
before the production acceptance run, require every externally configurable fact:

```sh
python3 packaging/release-prerequisites.py --repo cauu/ouro-ops --require-ready
```

Configure the reported missing facts in the GitHub UI:

1. Enable immutable releases for `cauu/ouro-ops`.
2. Keep Actions enabled. Repository rules must allow the scoped release workflow to non-force push
   its generated `chore(release): vX.Y.Z` commit and `vX.Y.Z` tag to `main`; the real p5 release is
   the final proof because a read-only API probe cannot prove a future token write.
3. Create the `production` environment, restrict it to protected branches or custom branch `main`,
   and do not add required reviewers or a wait timer.
4. Add environment secrets named `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID`. Do not copy
   their values into logs, fixtures, specs, or command arguments.
5. Keep the Worker identity in `web/onboarding/wrangler.jsonc` equal to `ouro-ops-site`.

The verifier needs authenticated GitHub CLI access with repository metadata, Actions, environments,
environment-secret names, and rulesets read access. It deliberately does not require Cloudflare
credentials. Worker existence and its production URL are verified only after the first deployment.
