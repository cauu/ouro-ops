# Ouro Ops onboarding website

The public onboarding and skill-prompt generator is a client-only static page. Its release artifact
contains one file and has no package-manager or frontend-runtime dependency.

## Local build

Run the build from any directory:

```sh
./web/onboarding/build.sh
```

The generated `web/onboarding/dist/index.html` must be byte-identical to the tracked
`web/onboarding/index.html`. The repository-level `.gitignore` excludes `dist/`.

## Deployment

The site is hosted as an assets-only Cloudflare Worker named `ouro-ops-site` and deployed by
`.github/workflows/site.yml`.

| Trigger | Result |
| --- | --- |
| Pull request touching the site or workflow | Build check for every PR |
| Same-repository pull request | Build plus a Cloudflare preview URL posted to the PR |
| Push to `main` touching the site or workflow | Production deployment through the `production` environment |

Fork pull requests never receive deployment credentials. Do not change the workflow to
`pull_request_target`.

### One-time wiring

1. In Cloudflare, create an API token with **Workers Scripts: Edit** and copy the account ID.
2. In the GitHub repository Actions secrets, add `CLOUDFLARE_API_TOKEN` and
   `CLOUDFLARE_ACCOUNT_ID`.
3. Create a GitHub environment named `production`; add required reviewers if production should
   require a manual approval.
4. Merge a site change to `main` to create/deploy the `ouro-ops-site` Worker.
5. In the Worker settings, attach the production custom domain when one is available.

Rollback is a normal revert on `main`; the push redeploys the reverted static artifact.
