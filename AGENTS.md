# Prototype Instructions

- Keep local Rust checks to the minimum needed for the change; run broader Rust validation in CI (user request, 2026-09-12).
- Tachyon's `cargo_lambda` builder compiles `pathbase-api` with **rustc 1.88.0**, which is older than the 1.95.0 this repo pins for local work and GitHub Actions. A dependency whose `rust-version` exceeds 1.88 fails the Cloud App build (`error: rustc 1.88.0 is not supported by the following packages`) after GitHub CI has already gone green. Before adding or upgrading a Rust dependency, check it with `cargo +1.88.0 check --locked --manifest-path api/Cargo.toml --bin lambda-pathbase-api` (observed 2026-09-18 with sqlx 0.9, which requires 1.94; the repo therefore stays on sqlx 0.8).

## Durable product decisions

- Preserve the existing PathBase UI, spacing, colors, and overall layout while implementing functionality (user request, 2026-09-12).
- The goal map is a multi-level tree built from `part_of` relations. Show every level expanded by default (branches may be folded per node), and give the dashboard map the full dashboard width so the whole tree is visible (user request, 2026-09-16).
- The home view should keep the goal map as the primary content: show the template chooser only until the workspace has a goal, omit the always-visible explanatory root node, keep goal cards compact, and group less-frequent sidebar destinations under a collapsed 「その他」 menu (user feedback, 2026-09-21).
- Local demo data is a golf-course scenario: the whole goal tree lives in the organization workspace under the top goal 「償却前利益3億円」 (user decision, 2026-09-16).
- Implement the API and shared application rules in Rust. Keep the application contract documented in `docs/api.md`.
- Keep local development usable without cloud credentials. Clearly distinguish local storage and sample workspaces from real cloud accounts, invitations, or synchronization.

Run the local server yourself and open the preview in the browser available to this environment. Do not give the user server-start instructions when you can run it.

Before making substantial visual changes, use the Product Design plugin's `get-context` skill when the visual source is unclear or no longer matches the current goal. When the user gives durable prototype-specific design feedback, preferences, or decisions, record them in `AGENTS.md`.

When implementing from a selected generated mock, treat that image as the source of truth for layout, component anatomy, density, spacing, color, typography, visible content, and hierarchy.

Build app UI in `src/`. Keep `.openai/hosting.json`, `worker/index.js`, `scripts/prepare-sites-build.mjs`, and `tests/sites-worker.test.mjs` intact so the same local prototype can be handed to Sites. Before a Sites handoff, run `npm run build` and `npm run test:sites`; the build must leave `dist/client/index.html`, `dist/server/index.js`, and `dist/.openai/hosting.json`.

- Authentication must use Tachyon (user decision, 2026-09-12); do not create a separate account/password authority. Reuse Field API contracts wherever they fit, including canonical tenant context and domain data references. Keep authN callbacks separate from per-request authZ checks.
- Do not use Cognito Hosted UI for PathBase authentication (user decision, 2026-09-13). Collect credentials only in the PathBase sign-in form, send them to the Rust API over the same origin, and let the Rust API authenticate through Tachyon without persisting or logging the password.
- Allow a user from any Tachyon operator tenant to authenticate to PathBase through the shared Tachyon platform OAuth client (user decision, 2026-09-13). Do not use an operator-tenant membership as a PathBase login gate; enforce access with PathBase workspace memberships after authentication.
- After Tachyon authentication, require the user to explicitly select one of their current Tachyon tenants before provisioning or loading PathBase workspaces (user decision, 2026-09-13). Revalidate that selection against Tachyon membership without treating it as PathBase workspace authorization.
- The selected tenant is a data boundary, not only a step in the sign-in flow (user decision, 2026-09-19). Every workspace belongs to exactly one tenant and never moves; the personal workspace is derived from (tenant, actor) rather than the actor alone; `authorize` checks the tenant before the role and answers 404 — never 403 — for another tenant's workspace; workspace and invitation listings are per tenant; an invitation becomes membership only when its recipient is acting in the workspace's tenant; and an MCP delegation acts in the tenant it was granted in. Existing workspaces were discarded in migration 0007 rather than attributed to a guessed tenant (user decision, 2026-09-19).
- Keep the browser URL synchronized with authentication and tenant-screen transitions. Use `/login` for sign-in, `/tenants?tenant_id=...` for tenant selection, and include `tenant_id` on authenticated home URLs without treating the URL value as authorization (user decision, 2026-09-13).
