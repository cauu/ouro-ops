# Ouro Ops Website

Static site, no frontend dependencies. `./web/onboarding/build.sh` writes:

- `dist/index.html`: `index.html` with the CLI install command (`packaging/install-bootstrap.sh`) and the skill names injected. That command runs `packaging/ouro-install.sh` from the latest GitHub Release, trusting GitHub HTTPS and the immutable Release; it is not locally verified before execution, and the installer verifies the CLI archive before installation.
- `dist/skills/<name>/SKILL.md`: exact copies of `ouro-skills/pool-ops` and `ouro-skills/kes-rotation`.

The page shows an install prompt to paste into an agent (Grok Bot, Claude Code, ...), a `pool-spec.yaml` template, and the CLI install command. The CSP denies all network access; copying is the only way anything leaves the page.

## CI/CD

`.github/workflows/site.yml` builds and tests on pull requests, and on pushes to `main` deploys `dist/` through the `ouro-ops-site` assets-only Cloudflare Worker, after checking that the latest CLI release meets the `requires_ouro` floor in `kes-rotation`. `packaging/verify-production-site.py` then checks the live page and that each published SKILL.md matches the repository.
