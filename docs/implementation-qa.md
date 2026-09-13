# Implementation verification — 2026-09-12

The existing dashboard layout, palette, typefaces, sidebar, hero illustration and modal styling were retained. The UI now reads and writes the Rust service instead of resetting in-memory sample arrays. Domain details are added in the existing panel and modal structure.

Automated checks:

- TypeScript and production Vite build, including the three required Sites artifacts.
- Four unchanged Sites worker/package tests.
- 14 application tests: restart persistence, concurrent edits, concurrent cycle creation, idempotency, graph rules, workspace isolation/viewer access, completion/outcome separation, habit occurrences, measurement corrections and units, template atomicity, change approval/stale previews, and atomic export/import.
- Seven OIDC/Tachyon/Field tests using local mocks: PKCE/state/nonce/audience/expiry, canonical identity, refresh serialization, callback/authZ separation, logout, tenant isolation and 401/403 distinction, task deduplication, observation retries and missing values, HTTP errors and auth mode boundaries.
- One real child-process MCP stdio test: initialization, 13 tools, resources, prompts, proposal-only behavior, rejected unapproved apply and successful human-approved apply.
- One real child-process remote MCP test: Bearer authentication, Streamable HTTP initialization and session reuse, repeated calls, input and workspace-boundary errors, rejected unapproved apply and successful human-approved apply.
- Rust clippy with warnings denied and native Tauri cargo check.

Browser checks against the running local Rust API:

- Create an action, reload, complete it and verify one completion history entry after a repeated completion. The existing goal's self-assessment remains unchanged.
- Create a title-only goal: no fabricated date or progress value. It appears as unscheduled in the timeline.
- Create a goal metric, see “未計測,” then record 2/5 books with a source and date and see 40%.
- Save a memo, reload, and confirm the saved text remains.
- Inspect the dashboard at the normal desktop viewport and at 390px width. Responsive sections remain within the page and the graph retains pan/zoom controls.

All browser-created test items are identifiable by the “動作確認：” prefix in the local preview database. They are archived after verification rather than deleting their histories.

Real Tachyon client registration and Field environment settings were not provided. These checks verify the implementation and adapters, not successful production login, live Field access, shared invitations, external notifications, or deployment. See integration-contracts.md and README.md for those boundaries.

## Shared workspace implementation — 2026-09-12

Added workspace creation and renaming, targeted Tachyon invitation inboxes, acceptance/decline/revocation, owner/editor/viewer management and self-service departure. The original layout is retained; workspace selectors now use IDs and names rather than assuming one workspace per scope. Shared activities identify their actual author. Viewer controls do not submit writes, and the API rechecks membership inside the write transaction before an idempotent replay. Personal and local preview workspaces cannot be shared.

Five focused Rust integration tests pass: cross-workspace privacy and restart persistence; membership revocation through a second database connection and cached request replay; viewer restrictions and departure; owner transfer and stale configuration; targeted, expired, declined and revoked invitations; agent restrictions; and local/personal sharing boundaries. The existing broad Rust checks were not rerun locally, in accordance with the requested minimum-check policy. CI retains the full Rust suite and native compile check.

The invitation lifecycle is tested against the common Rust service using distinct canonical actor identities. Live multi-user Tachyon acceptance remains unverified until the deployment's OIDC configuration is available. Invitations appear inside PathBase; no email or Slack delivery is claimed.

## Live connection preflight — 2026-09-13

Added a redacted `npm run preflight` check for the live Tachyon / Field setup. It validates the configured mode and callback contract, loads OIDC Discovery, and confirms that intentionally unauthenticated requests reach and are rejected by the Tachyon verification and Field tenant boundaries. The report lists missing variable names but never prints client secrets, tokens, client or tenant identifiers.

The focused integration test passes against local OIDC, Tachyon, and Field mocks. The production build and the four unchanged Sites packaging tests also pass. With no local `.env`, the command exits unsuccessfully and reports all missing Tachyon and Field setting names as intended. Successful real-user login, Field authorization, expiry, and 401 / 403 behavior still require the deployment credentials and remain assigned to the live acceptance step.

## Live authenticated check — 2026-09-13

Production Tachyon login and tenant selection succeeded. Requesting the Field tenant list reached Field, but Field rejected the current upstream token with HTTP 401. PathBase previously reused the application-level `UNAUTHENTICATED` code for this upstream response, which incorrectly cleared the valid PathBase session and returned the user to the login screen. Field 401 responses now use `FIELD_AUTH_REJECTED`, keep the PathBase session intact, and show an actionable tenant/permission error. A focused integration assertion covers the distinction. Successful Field data access remains blocked on the upstream token/permission configuration.

## Secret-free acceptance E2E — 2026-09-14

The acceptance suite now exercises the browser-facing Tachyon and Field boundaries without user credentials or cloud secrets. It verifies the unauthenticated login screen, explicit tenant selection and URL state, a rejected tenant selection (403), selection persistence across reload, return-to-login logout, and that both Field upstream 401 and permission 403 errors remain visible without clearing the valid PathBase session. The Field 401 uses `FIELD_AUTH_REJECTED`; only PathBase/Tachyon `UNAUTHENTICATED` moves the UI to login.

The existing browser tests continue to run against the real Rust HTTP API and an isolated temporary SQLite database for the principal persistence flows: title-only goal plus memo across a fresh browser context, idempotent action completion across reload, and workspace selection/data isolation across reload. Rust integration tests remain the source of truth for cookie, PKCE/OIDC, tenant membership, application RBAC, and Field header/tenant contracts; browser route fixtures cover only the UI response to those already-tested API contracts.

No production token, password, client secret, tenant identifier, or developer database is read by these tests. Successful production Field reads still require an authorized real user and remain a live acceptance item rather than a CI claim.

## Production connection recheck — 2026-09-14

The shared-platform Cloud App has the Tachyon OAuth client, Field endpoint and canonical Field platform/root context configured. Its production session key is stored as a secret app environment variable, and the manifest now references that server-side secret without containing key material. Builds from `main` complete successfully.

The live URL cannot yet be used for an authenticated Tachyon/Field acceptance pass: every current Cloud Run deployment fails in the provider with `404 Not Found`, and `https://pathbase.txcloud.app/api/health` consequently returns the routing-layer response `No route for: pathbase`. This is a deployment-provider failure after a successful image build, not evidence of a Tachyon or Field authorization result. Do not mark live Field access verified until a deployment has a public URL, `/api/health` returns 200, and an authenticated user can select a current Tachyon tenant and read an authorized Field resource.
