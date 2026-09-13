# Prototype Instructions

- Keep local Rust checks to the minimum needed for the change; run broader Rust validation in CI (user request, 2026-09-12).

## Durable product decisions

- Preserve the existing PathBase UI, spacing, colors, and overall layout while implementing functionality (user request, 2026-09-12).
- Implement the API and shared application rules in Rust. Keep the application contract documented in `docs/api.md`.
- Keep local development usable without cloud credentials. Clearly distinguish local storage and sample workspaces from real cloud accounts, invitations, or synchronization.

Run the local server yourself and open the preview in the browser available to this environment. Do not give the user server-start instructions when you can run it.

Before making substantial visual changes, use the Product Design plugin's `get-context` skill when the visual source is unclear or no longer matches the current goal. When the user gives durable prototype-specific design feedback, preferences, or decisions, record them in `AGENTS.md`.

When implementing from a selected generated mock, treat that image as the source of truth for layout, component anatomy, density, spacing, color, typography, visible content, and hierarchy.

Build app UI in `src/`. Keep `.openai/hosting.json`, `worker/index.js`, `scripts/prepare-sites-build.mjs`, and `tests/sites-worker.test.mjs` intact so the same local prototype can be handed to Sites. Before a Sites handoff, run `npm run build` and `npm run test:sites`; the build must leave `dist/client/index.html`, `dist/server/index.js`, and `dist/.openai/hosting.json`.

- Authentication must use Tachyon (user decision, 2026-09-12); do not create a separate account/password authority. Reuse Field API contracts wherever they fit, including canonical tenant context and domain data references. Keep authN callbacks separate from per-request authZ checks.
- Do not use Cognito Hosted UI for PathBase authentication (user decision, 2026-09-13). Collect credentials only in the PathBase sign-in form, send them to the Rust API over the same origin, and let the Rust API authenticate through Tachyon without persisting or logging the password.
- Allow a user from any Tachyon operator tenant to authenticate to PathBase through the shared Tachyon platform OAuth client (user decision, 2026-09-13). Do not use an operator-tenant membership as a PathBase login gate; enforce access with PathBase workspace memberships after authentication.
