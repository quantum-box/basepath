# Implementation verification — 2026-09-12

The existing dashboard layout, palette, typefaces, sidebar, hero illustration and modal styling were retained. The UI now reads and writes the Rust service instead of resetting in-memory sample arrays. Domain details are added in the existing panel and modal structure.

Automated checks:

- TypeScript and production Vite build, including the three required Sites artifacts.
- Four unchanged Sites worker/package tests.
- 14 application tests: restart persistence, concurrent edits, concurrent cycle creation, idempotency, graph rules, workspace isolation/viewer access, completion/outcome separation, habit occurrences, measurement corrections and units, template atomicity, change approval/stale previews, and atomic export/import.
- Seven OIDC/Tachyon/Field tests using local mocks: PKCE/state/nonce/audience/expiry, canonical identity, refresh serialization, callback/authZ separation, logout, tenant isolation and 401/403 distinction, task deduplication, observation retries and missing values, HTTP errors and auth mode boundaries.
- One real child-process MCP stdio test: initialization, 13 tools, resources, prompts, proposal-only behavior, rejected unapproved apply and successful human-approved apply.
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
