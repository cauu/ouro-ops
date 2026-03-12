# S0007 Frontend Prototype

Pure HTML prototype with five core pages:

1. `onboarding.html`
2. `index.html` (dashboard)
3. `deploy.html` (Deployment Wizard Step 1)
4. `deploy-step2.html`
5. `deploy-step3.html`
6. `deploy-step4.html`
7. `kes-rotate.html`
8. `kes-rotate-step2.html`
9. `kes-rotate-step3.html`
10. `kes-rotate-step4.html`
11. `upgrade-image.html`
12. `upgrade-step2.html`
13. `upgrade-step3.html`

Design direction:

- macOS-native layout language: titlebar + source list sidebar + main content + inspector
- professional, restrained, minimal
- light-only, Cardano blue as subtle accent
- single-step focused wizard for Deploy / KES Rotate / Upgrade
- Deploy Wizard is split into 4 independent views with fixed action bar
- startup flow: lightweight Welcome Window -> immersive Deploy Wizard (no sidebar)
- telemetry experience: local cached metrics first, then silent Prometheus refresh with graceful degraded hint
- safety gates: KES / Upgrade include explicit high-risk confirmation and rollback policy hints

Notes:

- Static prototype only, no business logic.
- Flow is represented by page structure and visual states.
- Inspector panel carries lightweight contextual summary.
