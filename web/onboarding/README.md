# Ouro Ops onboarding website

The Skill-prompt generator is a client-only static page. Its build output contains one file and has
no package-manager or frontend-runtime dependency. Build time injects exactly the six complete
canonical external Skills; the CLI does not carry a second decision copy.

## Local build

Run the build from any directory:

```sh
./web/onboarding/build.sh
```

The generated `web/onboarding/dist/index.html` must be byte-identical to the tracked
`web/onboarding/index.html`. The repository-level `.gitignore` excludes `dist/`.

## Current acceptance boundary

`.github/workflows/site.yml` builds the production-form page, proves source fidelity/copy behavior
and serves it over local HTTP in tests. The page's CSP disables ambient network access; pool data
leaves the browser only when the operator copies and pastes the prompt.

| Trigger | Result |
| --- | --- |
| Pull request touching the site, Skills or workflow | Complete local production-form validation |
| Push to `next` touching the site, Skills or workflow | Complete local production-form validation |
| Manual workflow dispatch | Complete local production-form validation |

The workflow uses no deploy secret and uploads no hosted site artifact. Production Cloudflare
deployment, custom-domain wiring and production-domain acceptance are explicitly deferred to the
next spec.

## Future production wiring

The next spec may activate the prepared assets-only Cloudflare configuration after adding explicit
production secrets/environment approval and validating the real domain. Do not infer production
deployment from a passing local-form workflow.
