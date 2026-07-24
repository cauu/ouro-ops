# Ouro Ops Website

The onboarding website is a client-only static page. Build time injects exactly the six complete
canonical external Skills; the CLI carries no second decision copy. The page has no package-manager
or frontend-runtime dependency.

## Local Build

```sh
./web/onboarding/build.sh
```

The tracked `web/onboarding/index.html` is the template. The generated
`web/onboarding/dist/index.html` injects the six canonical Skills and the exact copyable verified
reinstall commands from `packaging/verified-reinstall.sh`. The repository `.gitignore` excludes
`dist/`.

## CI/CD

`.github/workflows/site.yml` has two paths:

| Trigger | Result |
| --- | --- |
| Pull request, including forks | Build and test only; no Cloudflare secret or deployment |
| Push to current `main` | Build/test, then automatic production deployment |
| Manual dispatch on current `main` | Build/test, then idempotent production deployment |

The deploy job uses GitHub's `production` environment with secret names
`CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID`. It publishes the `dist/` directory through the
fixed `ouro-ops-site` assets-only Worker. There is no PR Preview, PR comment, custom-domain gate,
required reviewer, or wait timer in this workflow.

Before the Cloudflare action, the workflow resolves the latest stable GitHub CLI release and
compares it with the maximum `requires_ouro` floor declared by the six Skills. An unreleased floor
fails before any Cloudflare write. A successful CLI publish explicitly dispatches this Site
workflow on current `main`; ordinary Site-only changes continue to deploy directly when the
existing release already meets the floor.

The CSP disables ambient page network access. Pool data leaves the browser only when the operator
copies and pastes a generated prompt.
