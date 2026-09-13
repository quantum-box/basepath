# Tachyon / Field integration

User decisions (2026-09-12): preserve the existing UI; API in Rust; reuse Field where applicable; authentication belongs to Tachyon.

## Runtime contracts

- Tachyon canonical identity: `POST /auth/v1beta/verify` with the current bearer token and `{ token }`; the verified `user.id` identifies the user.
- Tenant discovery: `GET /get_tenants?required_action=field:ViewSalesAnalytics`, with `x-operator-id` and `x-platform-id`.
- Field tasks: `GET /v1/erp/sales-tasks?limit=50&offset=N` and `GET /v1/erp/sales-tasks/{id}`, using camelCase response fields and the user's task permissions.
- Field metrics: `GET /v1/erp/sales-contracts/metrics`. Missing observations remain missing.
- Configure the registered OIDC issuer and client explicitly. Authentication failures return 401; permission failures return 403.

## Implemented boundaries

`api/src/auth.rs` provides an OIDC authorization-code flow with S256 PKCE, one-use state bound to an HttpOnly cookie, nonce / signature / issuer / audience / expiry checks, then delegates canonical identity verification to Tachyon. No passwords or independent account database. Browser tokens stay on the Rust server; sessions are held in memory, expire after eight hours, refresh upstream tokens when possible, and require login after a server restart. Authentication is revalidated at protected requests. Login callbacks never call Field, create memberships, or infer RBAC.

The authenticated API provisions an isolated personal PathBase workspace for the verified Tachyon user. It does not attach other users' personal data or local demo data. Shared PathBase workspaces have their own owner/editor/viewer memberships. Owners invite a specific canonical Tachyon user ID for seven days; that user must authenticate and explicitly accept. A pending invitation grants no read access. Personal and local demo workspaces cannot be invited into. Only an owner may manage memberships, and at least one owner must remain. Membership revocation also invalidates outstanding invitations issued by the departing owner. These PathBase permissions remain separate from Field sales permissions; Field sales access does not grant access to arbitrary plans.

`api/src/field.rs` delegates authorized tenant discovery and narrowly selected read operations to Field. It forwards the current user's token and canonical tenant context, has a timeout, disables redirects, does not send `x-user-id`, does not use service-account fallback, and preserves 401 / 403 / missing / unavailable distinctions. Every operation rechecks the selected tenant and relies on the Field endpoint's own action authorization.

UI settings can reference Field sales tasks as personal actions and record compatible Field metrics as sourced observations. Field remains the source of truth for its sales tasks. Importing an action or completing it in PathBase does not change the upstream task. Automated bidirectional sync, external notifications and background workers belong to the specification's later integration phase.

## Deployment / preview

The standalone Rust API defaults to Tachyon authentication and fails startup if required OIDC settings are absent. `npm run dev` explicitly selects `local-preview` unless a mode is already provided. This preview is loopback-only, has a generated per-process credential, and is visibly marked as a local preview. It cannot call Field on behalf of a user.

Set the variables in `.env.example` through the deployment environment. Do not put credentials in `VITE_*` variables. Register the exact callback `PATHBASE_PUBLIC_URL/api/auth/callback` in Tachyon. Route `/api/*` to the Rust API with the `/api` prefix removed and serve the frontend from the same origin. The unchanged Sites worker is static-only; deploying the static artifact alone does not deploy the Rust API.

Native debug builds use the same Rust service over Tauri IPC with a separate local preview database. Authenticated native builds load `PATHBASE_WEB_URL` so the same Tachyon session and same-origin API work in the webview. Release builds do not fall back to the local owner when that URL is absent.

## Pending real-environment verification

Tachyon client registration, registered callback, issuer and Field deployment context have not been provided for this app. Local contract tests can verify the adapters, but do not establish successful login or live access to production Field. No existing upstream app credentials or user tokens are reused implicitly.

設定を登録したら`npm run preflight`で、値そのものを表示せずに設定形式と公開認証境界への到達性を確認する。プリフライトが成功しても利用者の権限は証明しないため、続けて実ユーザーでログインし、許可されたFieldデータ、401、403、期限切れを確認する。
