# Ouro Ops Website

The onboarding website is a client-only static page. Build time injects exactly the six complete
canonical external Skills; the CLI carries no second decision copy. The page has no package-manager
or frontend-runtime dependency.

## Local Build

```sh
./web/onboarding/build.sh
```

The generated `web/onboarding/dist/index.html` must be byte-identical to the tracked
`web/onboarding/index.html`. The repository `.gitignore` excludes `dist/`.

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

The CSP disables ambient page network access. Pool data leaves the browser only when the operator
copies and pastes a generated prompt.
