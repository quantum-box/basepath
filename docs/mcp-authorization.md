# MCP authorization — how an AI client acts for a person

Basepath's hosted MCP endpoint is an OAuth 2.1 protected resource. This
document is the contract an AI host connects against, and the threat model the
implementation is built to.

## Three questions, kept separate

A request only proceeds when all three are answered, and each is answered by a
different mechanism on purpose.

| Question | Answered by | Where |
| --- | --- | --- |
| Who is calling? | A Bearer access token Basepath issued, after the person signed in against the Tachyon-managed Cognito user pool and granted this client on Basepath's own origin | `api/src/oauth.rs`, `api/src/mcp.rs` |
| Did the person agree to *this* connection? | A row in `mcp_connections`, granted only by that consent screen and revocable at any time | `api/src/mcp_auth.rs` |
| May this actor touch this workspace? | The existing workspace membership check, re-read on every operation | `api/src/storage.rs` |

A granted scope is still not an approved change. Everything an MCP client
writes is a proposal; a person reviews the specific changeset in Basepath and
approves it there. `pathbase.apply` only allows applying a changeset that was
already approved, by the person who approved it.

## Connecting

1. The client calls the MCP endpoint without a token and gets
   `401` with `WWW-Authenticate: Bearer realm="pathbase", resource_metadata="…"`.
2. It fetches `/.well-known/oauth-protected-resource/api/mcp` (RFC 9728), which
   names the authorization server, the resource identifier, and the scopes this
   resource understands.
3. The authorization server is **Basepath itself**, at
   `/.well-known/oauth-authorization-server`. It is not the Cognito user pool:
   the pool's discovery document advertises no PKCE method and no registration
   endpoint, and its redirect URIs are fixed at deploy time, so a host that
   mints a callback per connection cannot use it at all.
   [docs/chatgpt-plugin.md](chatgpt-plugin.md) has the observed evidence.
4. The client registers itself (RFC 7591) and runs Authorization Code + PKCE.
   Registration grants nothing; it only creates an id that a person can later
   authorize.
5. The person lands on Basepath's consent screen, signed in, on Basepath's own
   origin. They choose which of `pathbase.read` / `pathbase.propose` /
   `pathbase.apply` / `pathbase.context` to allow. The context scope is only
   for creating or explicitly relinking a conversation-to-workspace pointer;
   it does not grant plan writes. That decision is the `mcp_connections` row.
6. The code is exchanged for an access token and a refresh token, both bound to
   this resource and this delegation. The person can narrow the scopes or
   disconnect at any time in settings, and either takes effect on the next
   request.

Basepath is not a second identity provider. People authenticate against the
pool exactly as before; what Basepath issues is the delegation to one AI
client, which was always its own state to hold.

## What the design refuses, and why

| Attempt | Result | Why it fails |
| --- | --- | --- |
| No token | `401` + metadata challenge | The endpoint has no anonymous mode. |
| A token this server did not issue, or an expired one | `401 INVALID_TOKEN` | Only Basepath's own MCP tokens are accepted, looked up by digest and checked for expiry and consumption. |
| An identity-provider token, including the browser's own | `401 INVALID_TOKEN` | A pool token proves who someone is. It says nothing about which AI client they allowed, so it is not a credential for this resource — and an MCP token is never a browser session. |
| A token issued for another resource (a preview, say) | `401 INVALID_AUDIENCE` | Each token records the resource it was granted for, and the check is against this deployment's own. |
| A replayed authorization code, or a rotated-out refresh token | `400 invalid_grant`, whole family revoked | A single-use secret appearing twice means someone else has a copy; refusing only that request would leave the copy working. |
| An authorization request for an unregistered `redirect_uri` | `400`, no redirect | Reporting the error to the supplied URI would make the endpoint an open redirector. |
| A connection the person declined | no delegation at all | Declining consumes the request and returns `access_denied` to the client. |
| A tool outside the granted scopes | `403 INSUFFICIENT_SCOPE` | Scope is checked before the tool's arguments are even validated, so an unauthorized caller learns nothing about the tool. |
| A disconnected client reusing a still-valid token | `403 CONNECTION_REVOKED` | The delegation is read from the shared database on every request, not cached in a process. |
| Naming someone else's workspace in the arguments | `404` | Workspace membership is re-checked per operation; arguments never confer access. |
| Applying a changeset the person did not approve | `403 APPROVAL_REQUIRED` | Unchanged business rule: an agent's own claim of approval is not approval. |
| Asking Basepath to manage its own delegation | `404` | The connection routes refuse an agent actor outright. |
| Passing a flag that says "approved" | ignored | Approval is a stored, versioned record tied to the changeset's digest and the approving person. |

## Why the delegation lives in the database

Several Lambda execution environments serve the same endpoint, and any of them
can be cold. A grant or a disconnect held in process memory would apply to one
of them and not the others, and would come back after a redeploy. The row in
`mcp_connections` is the single answer every execution environment reads, and a
disconnect is therefore immediate rather than eventual.

## Transport

The endpoint is **stateless** Streamable HTTP with JSON responses
(`stateful_mode: false`, `json_response: true`).

It runs on Lambda: consecutive requests from one client reach different
execution environments, any of which can be cold. A session pinned to one
process would work until it did not, so there is no session. Every request
carries its own access token and is answered on its own.

What that means for a client:

- `initialize` returns capabilities and **no** `Mcp-Session-Id`. There is
  nothing to send back on later requests.
- Responses are `application/json`, not `text/event-stream`. There is no SSE
  framing to parse.
- `GET` on the endpoint is refused: there is no server-initiated stream to
  open. The endpoint does not advertise a transport feature it does not have.
- `Host` is validated against the deployment's own hostnames, which stops a
  DNS-rebinding attempt from reaching the tools. `Origin` is validated when
  `PATHBASE_MCP_ALLOWED_ORIGINS` is set; non-browser clients send none.
- Every response carries `Cache-Control: no-store`, and the Cloudflare Worker
  in front forwards the request and response unchanged (including
  `Authorization` and `Mcp-Protocol-Version`).

State that has to survive lives in the shared database: the delegation, the
plan, the change sets, the audit trail. A redeploy or a cold start loses
nothing, because there is nothing in a process worth keeping.

## MCP Apps plan-tree contract

The MCP server publishes one reusable resource, `ui://basepath/plan-v3.html`.
The previous `ui://basepath/plan-v2.html` URI remains readable as a legacy
alias, but new tool metadata advertises v3 so hosts do not reuse a cached
widget that predates conversation proposals.
Plan-reading tools carry `_meta.ui.resourceUri` and the ChatGPT compatibility
alias `openai/outputTemplate`; the widget renders the selected workspace's
goal tree from the tool result. It does not publish separate personal and
organization screens.

The server still returns `structuredContent` plus a text content block from
`tools/call`, so hosts without embedded UI remain usable. Workspace identifiers,
personal/organization scope, `part_of` relations, truncation markers, and change
approval links remain explicit in the response. Authorization is enforced on
every tool call and is never delegated to a renderer or to the model's summary.

## Tool annotations

`annotations` describe the real effect, not a comfortable default. A change set
may contain `DELETE` operations, so `pathbase_preview_changes`,
`pathbase_propose_plan` and `pathbase_apply_changes` are marked
`destructiveHint: true` even though a person approves in between.
`pathbase_get_graph` returns at most `limit` nodes (default and maximum 200)
and reports `truncated`; a truncated graph is a slice, not the plan.

## Secrets

- There is no client secret anywhere: Basepath registers public clients only
  (`token_endpoint_auth_method: none`), and PKCE is required. A stolen
  registration is not a credential.
- Only the SHA-256 of each code and token is stored. Reading the database
  yields nothing a client could present.
- The distributable plugin package contains no client id either: the host
  registers one per connection. An embedded id would be shared by everyone who
  installed the package.
- Access tokens are never written into a tool result, `structuredContent`, an
  error message, or the connection record. `api/tests/mcp_remote.rs` asserts
  the token does not appear in a response body.
- The browser session cookie is never handed to an MCP client, and an MCP token
  is never accepted as a browser session.

## Production and preview are different resources

`PATHBASE_MCP_RESOURCE` differs per environment, and each environment has its
own database. A connection approved against a preview grants nothing in
production: the approval record is in the preview database, and the resource
identifier the client requested a token for is not production's.

## What Tachyon provides, and what it does not

Verified in this repository:

- Access tokens are issued by the Cognito user pool Tachyon manages, and
  Basepath verifies them against the pool's JWKS (issuer, signature, expiry,
  `token_use`, `client_id`).
- Canonical user identity comes from Tachyon `/v1/me`; Basepath does not mint
  identities.
- `useTachyonUserPool` OAuth2Clients can be registered from the Cloud App
  manifest, which is how browser sign-in works.

Measured, and the reason for the design above:

- The pool's discovery document has **no `code_challenge_methods_supported`**,
  **no `registration_endpoint`**, and **no
  `client_id_metadata_document_supported`**, and
  `/.well-known/oauth-authorization-server` returns `400`. A host following the
  MCP authorization specification cannot use it as an authorization server.
- Its redirect URIs are declared in `tachyon.yml` and fixed at deploy time,
  while hosts mint a callback per connection.

Consequently Basepath does not depend on the pool for **Dynamic Client
Registration**, the **`resource` parameter**, or **custom scopes**: it
implements all three itself, and scope is its own grant record rather than a
claim inside a token.

## Verification status

`api/tests/mcp_remote.rs` runs the real binary in a real deployment mode
(`PATHBASE_MODE=tachyon`) over real HTTP, against a mock user pool, and covers
every row of the refusal table above plus two separate users.

`api/tests/oauth.rs` covers the authorization server itself — registration
limits, open-redirect refusal, PKCE binding, code replay, refresh rotation,
per-resource audience, and revocation — and
`tests/e2e/oauth-consent.spec.mjs` drives the consent screen in a browser.

ChatGPT Work (GPT-5.6 Sol, Basepath plugin 1.0.0) connected to the production
endpoint on 2026-09-20 and completed the documented read/propose/approve flow.
The MCP Apps tree presentation still needs a fresh real-host check after the
widget update in this change. Claude has not been verified. Treat
"the contract is implemented and tested" and "the updated widget is rendered
by every host" as different claims.
