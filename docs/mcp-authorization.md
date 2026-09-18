# MCP authorization — how an AI client acts for a person

Basepath's hosted MCP endpoint is an OAuth 2.1 protected resource. This
document is the contract an AI host connects against, and the threat model the
implementation is built to.

## Three questions, kept separate

A request only proceeds when all three are answered, and each is answered by a
different mechanism on purpose.

| Question | Answered by | Where |
| --- | --- | --- |
| Who is calling? | A Bearer access token issued by the Tachyon-managed Cognito user pool, verified against the pool's JWKS, and exchanged for a canonical user id at Tachyon `/v1/me` | `api/src/auth.rs`, `api/src/mcp.rs` |
| Did the person agree to *this* connection? | A row in `mcp_connections`, created with no scopes and granted only by an explicit action in Basepath | `api/src/mcp_auth.rs` |
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
3. It runs Authorization Code + PKCE against that authorization server and gets
   an access token issued to Basepath's **MCP** OAuth client.
4. Its first authenticated call creates a `pending` connection and returns
   `403 CONNECTION_APPROVAL_REQUIRED` with a `consent_url`.
5. The person opens Basepath's settings, sees the client, chooses which of
   `pathbase.read` / `pathbase.propose` / `pathbase.apply` to allow, and saves.
6. The client works. The person can change the scopes or disconnect at any
   time, and a disconnect applies to the next request.

## What the design refuses, and why

| Attempt | Result | Why it fails |
| --- | --- | --- |
| No token | `401` + metadata challenge | The endpoint has no anonymous mode. |
| A token from another issuer, or an expired one | `401 INVALID_TOKEN` | Signature, issuer, expiry and `token_use` are verified against the pool's JWKS before anything else. |
| A token issued to the **web sign-in** client | `401 INVALID_AUDIENCE` | The MCP endpoint has its own OAuth client (`pathbase-mcp`). A browser session, however valid, is not a credential for this resource — and the reverse holds too. |
| A valid token with no approval | `403 CONNECTION_APPROVAL_REQUIRED` | A token proves identity; consent is a separate, revocable record. |
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

## MCP Apps UI

`pathbase_get_graph`, `pathbase_get_today` and `pathbase_get_week` carry
`_meta.ui.resourceUri = "ui://basepath/plan.html"`, so a host that supports MCP
Apps can preload the view before the tool is called. `resources/list` publishes
that resource with `mimeType: text/html;profile=mcp-app` and an **empty** CSP
(`connectDomains: []`, `resourceDomains: []`): the document is a single
self-contained file that loads no script, style, font or image from anywhere,
so the host's deny-by-default policy needs no exception.

The document is the empty application shell. It contains no workspace data and
no credential; it asks the host for both, and the host proxies each request to
this server, which authorizes it as usual. **Rendering is never permission.**
`_meta.ui.visibility` is deliberately never set: hiding a tool from the model is
a presentation choice, and it is not used here as a server-side authorization
device.

A host that does not support MCP Apps loses nothing. Every tool returns the
same `structuredContent` and text it always did, and the connection works
without the view.

The bundle is built by `npm run build:mcp-app` from `mcp-app/` and the shared
components in `src/shared/`, and is committed at `api/ui/mcp-app.html` because
the Lambda that serves it has no Node and no CDN. CI rebuilds it and fails if
the committed file has drifted, and the build enforces a size budget.

## Tool annotations

`annotations` describe the real effect, not a comfortable default. A change set
may contain `DELETE` operations, so `pathbase_preview_changes`,
`pathbase_propose_plan` and `pathbase_apply_changes` are marked
`destructiveHint: true` even though a person approves in between.
`pathbase_get_graph` returns at most `limit` nodes (default and maximum 200)
and reports `truncated`; a truncated graph is a slice, not the plan.

## Secrets

- The manifest never contains a client secret: both OAuth clients are public
  clients using PKCE, and `PATHBASE_MCP_CLIENT_ID` is injected from the
  registered client rather than written down.
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
  manifest, which is how the MCP client exists at all.

Not established here, and therefore not claimed:

- Whether the authorization server supports **Dynamic Client Registration**
  (RFC 7591). Hosts that insist on it may need the client registered by hand;
  the redirect URIs each host requires are added in PLT-4823 (ChatGPT) and
  PLT-4824 (Claude).
- Whether the authorization server honours the **`resource` parameter**
  (RFC 8707). Basepath performs the audience check itself against the OAuth
  client id, which does not depend on it.
- Whether the user pool can issue **custom scopes**. Basepath therefore treats
  scopes as its own grant record rather than trusting a `scope` claim; a claim,
  if present, can only narrow what the stored grant already allows.

## Verification status

`api/tests/mcp_remote.rs` runs the real binary in a real deployment mode
(`PATHBASE_MODE=tachyon`) over real HTTP, against a mock user pool, and covers
every row of the refusal table above plus two separate users.

**Not yet verified against a real host.** Connecting ChatGPT or Claude to this
endpoint, and whatever client registration each requires, is PLT-4823 and
PLT-4824. Until those are done, treat "the contract is implemented and tested"
and "a host can connect" as different claims.
