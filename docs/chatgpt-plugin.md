# ChatGPT plugin — connecting Basepath to a conversation

What this covers: the package, the OAuth flow it depends on, how to run it in
developer mode, and — separately — what has and has not been verified.

Nothing here is published. The package can be assembled and side-loaded today;
submitting it to a public directory is a decision, not a build step, and it is
not taken in this repository.

## Why Basepath issues its own tokens

This is the part that determined the design, so it goes first.

An MCP client following the current specification discovers where to get a
token from the resource's own metadata, then reads that authorization server's
metadata. ChatGPT refuses to proceed unless that document advertises
`code_challenge_methods_supported: ["S256"]`, and it needs a way to register a
client, because it mints a fresh callback URL per connection.

The Tachyon-managed Cognito user pool — the identity provider Basepath signs
people in against — publishes this:

```
$ curl -s https://cognito-idp.ap-northeast-1.amazonaws.com/<pool>/.well-known/openid-configuration
{"authorization_endpoint": "...", "token_endpoint": "...", "issuer": "...",
 "response_types_supported": ["code","token"], "scopes_supported": ["openid","email","phone","profile"], ...}
```

No `code_challenge_methods_supported`. No `registration_endpoint`. No
`client_id_metadata_document_supported`. `/.well-known/oauth-authorization-server`
returns 400. Its redirect URIs are fixed in `tachyon.yml` at deploy time.

A host cannot complete a single step of the flow against it, and no amount of
configuration in this repository changes that. So Basepath is the authorization
server for its own MCP resource, implemented in
[`api/src/oauth.rs`](../api/src/oauth.rs).

This is not a second identity provider. A person still signs in against the
pool; the consent screen only exists behind that session. What Basepath issues
is the *delegation to one AI client*, which was already Basepath's state — the
`mcp_connections` row. The OAuth grant and the delegation are now the same act
rather than two screens someone has to find separately.

## The flow

| Step | Where | What happens |
| --- | --- | --- |
| 1 | host → `/.well-known/oauth-protected-resource/api/mcp` | resource id and its authorization server |
| 2 | host → `/.well-known/oauth-authorization-server` | endpoints, `S256`, registration |
| 3 | host → `POST /api/oauth/register` | a client id. Grants nothing |
| 4 | person → `/oauth/authorize` | the consent screen, with their Basepath session |
| 5 | Basepath → host callback | `code`, `state`, `iss` |
| 6 | host → `POST /api/oauth/token` | access + refresh token, scoped to what they granted |
| 7 | host → `POST /api/mcp` | every request, with that token |

Registration is unauthenticated, as RFC 7591 intends. It has to be — the host
has no credential yet — and it is safe because a registered client can do
nothing at all until a signed-in person authorizes it.

### What the consent screen is for

The same argument as change approval: a click inside an AI host reaches this
server as an ordinary tool call, indistinguishable from the model's. So the
screen is on Basepath's origin, behind the person's own session, and the answer
travels with the same-origin CSRF header. It is the only place a delegation can
be granted.

The person can narrow what was asked for. The server never widens beyond it,
and never beyond the scopes this resource offers.

### What a token is not

A token with every scope still approves nothing. `pathbase.apply` only allows
applying a change set the person already approved in Basepath, by digest, as
themselves. The rules in
[docs/change-approval.md](change-approval.md) are unchanged by any of this.

### Revocation

Disconnecting in settings revokes the delegation **and** every live token in the
same transaction. The next request fails; there is no window where the status
says disconnected and a token still works.

A replayed authorization code or a rotated-out refresh token revokes the whole
family descended from that code, rather than refusing one request.

## The package

```
plugin/chatgpt/        # everything specific to this host
  plugin.json          # Agent Plugins 1.0.0 manifest + extensions.com.openai
  mcp.json             # the streamable-http MCP endpoint
  assets/              # composer icon and logo
skills/                # shared, host-neutral; copied into every host package
  basepath-goal-breakdown/SKILL.md
  basepath-week-planning/SKILL.md
  basepath-record-progress/SKILL.md
  basepath-weekly-review/SKILL.md
```

```sh
npm run build:plugin     # writes dist/plugin/chatgpt/
npm run check:plugin     # validates the sources without writing (CI runs this)
npm run test:plugin
```

The skills are the workflows, and they are written once. A rule about how to
work with someone's plan does not change because the conversation is happening
in a different product, so `scripts/build-plugin.mjs` refuses a skill that names
a host: that wording belongs in `plugin/<host>/`. Claude's package is a second
directory beside `chatgpt/`, not a second copy of the rules — see
[claude-connector.md](claude-connector.md).

The MCP server also serves these same files under the
`io.modelcontextprotocol/skills` extension, so a host that supports it gets the
instructions by connecting, with no package to install or keep in step.

No credential is in the package, and there is nothing to put there: the client
id is registered per connection by the host, and the token is issued to the
person.

## Running it in developer mode

1. In ChatGPT, **Settings → Security and login → Developer Mode**.
2. **Plugins → +**, and give it the MCP server
   `https://pathbase-v2.txcloud.app/api/mcp`.
3. ChatGPT runs discovery, registers a client, and sends you to Basepath's
   consent screen. Sign in if you are not already, choose the scopes, allow.
4. The connection appears under **設定 → AIクライアントの接続** in Basepath,
   with the scopes you granted and when it was last used.
5. For the skills and the listing metadata, take the technical id from the
   browser URL (`plugin_asdk_app…`) and hand it to `@plugin-creator` with
   `dist/plugin/chatgpt/`.

To publish to a workspace rather than to yourself: **Plugins → Personal → ⋯ →
Publish** (workspace admin only).

## The in-conversation view, and which spelling ChatGPT reads

This is the part that cost a day, so it goes in full.

There are two conventions for telling a host "this tool has a view", and they
are not the same key:

| | MCP Apps (SEP-1865) | OpenAI Apps SDK |
| --- | --- | --- |
| On the tool | `_meta["ui"]["resourceUri"]` | `_meta["openai/outputTemplate"]` |
| Media type | `text/html;profile=mcp-app` | `text/html+skybridge` |
| May the view call tools | negotiated by the bridge | `_meta["openai/widgetAccessible"]` |
| Read by | claude.ai, Claude Desktop | ChatGPT |

Basepath published only the first. A ChatGPT Developer Mode connection
therefore saw no view at all — and because nothing recorded that a host had
read a UI resource, that was indistinguishable from the view being broken. On
2026-09-19 a proposal was created from ChatGPT, the conversation showed a
paragraph saying approval happens in Basepath, and it expired unread
(PLT-4943).

Both spellings are now published for the same bytes, as two listings of one
document rather than two documents. A host ignores the key it does not know, so
this is additive: nothing about the MCP Apps contract changed.

Separately, the tools that *create* a change set had no view in either
convention — only the read tools did. So even a host that renders MCP Apps
showed no diff at the moment a proposal was made. Every change tool now names
a view.

### Measuring it rather than arguing about it

Reading a `ui://` resource stamps `mcp_connections.ui_read_at`, and
**設定 → AIクライアントの接続** shows it per connection:

- 「会話内に表示あり・<日時>」 — this host rendered the view. Measured, from the
  host, on that connection.
- 「会話内の表示はまだありません」 — it has not. Either the client does not
  implement either convention, or nothing has opened a view yet.

A host reads that resource only in order to draw it, so the timestamp is the
evidence. Fill the table at the end of this file from what it says, not from
anyone's documentation, and give the host name and version you actually used.

### When a host renders nothing

It must still not be a dead end, and that is not left to the model's judgement.
Every change set a tool returns carries `approval_url` — the absolute Basepath
link — and `where_to_approve`, a sentence telling the model to show the diff and
the URL. So the worst case is prose *with a working link*, which is a person who
can finish, rather than prose alone, which is a person who cannot.

## What happens when something goes wrong

| Situation | What the person sees |
| --- | --- |
| Not signed in when the host sends them | The Basepath sign-in screen, then the consent screen for the same request. The authorization request survives the round trip |
| Declines | The host is told `access_denied`. No delegation is created |
| Scope not granted | The tool fails with `INSUFFICIENT_SCOPE` and the app says which permission to allow |
| Disconnected in settings | The next call fails. The host can reconnect, which starts a new consent |
| Authorization request expired (15 min) | "この認可リクエストは期限切れです" with no way to grant from that page |
| Access token expired (1 hour) | The host refreshes silently. Refresh tokens rotate |
| Host does not render MCP Apps | Every tool still returns the same `structuredContent` and text, including `approval_url`, so the model can hand over a working link. Nothing is lost but the in-conversation view |
| A proposal is outside every pre-set range | The app offers 「Basepathで承認する」 and shows the URL. Nothing is applied |
| A proposal is inside a range | The app offers 「この内容を反映する」, and says afterwards what it did. See [change-approval.md](change-approval.md#deciding-in-advance) |

## Verified, and not

Keeping these apart is the point of the table.

| Claim | Status | Evidence |
| --- | --- | --- |
| The authorization server behaves as specified | **CI** | `api/tests/oauth.rs` — 12 tests: registration limits, open-redirect refusal, PKCE binding, code replay, refresh rotation, per-resource audience, revocation |
| Consent, granting, declining and disconnecting work in a browser | **CI** | `tests/e2e/oauth-consent.spec.mjs` — 7 tests against the real screen |
| A client can register, consent, exchange, and call MCP over HTTP | **CI** | `api/tests/mcp_remote.rs` — real child process, real HTTP |
| The package is well-formed and carries no credential | **CI** | `tests/plugin.test.mjs`, `npm run check:plugin` |
| `WWW-Authenticate` survives API Gateway | **CI** | `tests/sites-worker.test.mjs`. Observed failing in production before this change: the header arrived as `x-amzn-remapped-www-authenticate` |
| Both view conventions are published for every tool that has one | **CI** | `api/tests/mcp_apps.rs` — `openai/outputTemplate`, `text/html+skybridge`, `openai/widgetAccessible`, and the MCP Apps keys, asserted together over a real MCP session |
| Every change tool names a view | **CI** | `api/tests/mcp_apps.rs`. This is the PLT-4943 regression: a tool that creates a proposal and cannot render it |
| A proposal renders as a diff the moment it is made, with no round trip | **CI** | `tests/e2e/mcp-app.spec.mjs` — pushed through the real AppBridge as a host does |
| A host that renders nothing still gets a working link | **CI** | `api/tests/mcp_apps.rs` and the `approval_url` on every change set |
| **Connecting from real ChatGPT** | **verified 2026-09-20** | ChatGPT Work reconnected to the production endpoint and read the sole `個人` workspace. |
| **Whether ChatGPT renders the view once both conventions are published** | **verified 2026-09-20** | ChatGPT rendered `ui://basepath/personal/plan.html` inside the conversation after `pathbase_get_graph`; the embedded view named the workspace, rendered its plan, and showed the week. |
| **The listing metadata as ChatGPT renders it** | **verified 2026-09-20** | ChatGPT's installed-plugin detail showed `Basepath`, description `目標・行動・記憶を、会話から扱う`, and plugin version `1.0.0`. |
| **Supported clients, plans, versions** | **measured 2026-09-20** | ChatGPT Work with GPT-5.6 Sol (medium effort), using Basepath plugin `1.0.0`. This is one observed configuration, not a compatibility claim for other plans or versions. |

### Real-host log

One row per actual connection. Empty rows are the honest state; do not fill
them from documentation or from the harness.

| Date | Host and version | View rendered (`ui_read_at`) | Proposal → reflected in Basepath | Notes |
| --- | --- | --- | --- | --- |
| 2026-09-19 | ChatGPT, Developer Mode | **no** | **no** — expired unread | Before this change. No view in either convention on the change tools, and only the MCP Apps spelling anywhere. PLT-4943 |
| 2026-09-20 | ChatGPT Work, GPT-5.6 Sol (medium); Basepath plugin 1.0.0 | **yes** — `ui://basepath/personal/plan.html` rendered in the conversation | **yes** — one `outcome` named `ChatGPT 実機受入` was proposed, then explicitly approved in Basepath and appeared in the personal goal map | The proposal's text diff and `Basepathで変更案を確認` link were visible in the conversation. The applied change contained one create, zero updates, and zero deletes. |
| 2026-09-20 | ChatGPT Work, GPT-5.6 Sol (medium); Basepath plugin 1.0.0 | **yes** — the conversation displayed the proposal diff and the Basepath review link | **yes** — `ChatGPT 会話内適用テスト` was approved and applied in Basepath, then a fresh conversation read returned `タイトル: ChatGPT 会話内適用テスト` and `状態: active` | This is an out-of-range change: the conversation led to Basepath for the person's approval, rather than treating an in-conversation click as evidence of approval. |

Submission to a public directory is out of scope and has not been prepared for
review. The privacy and support material a directory requires is not written.

## Operational notes

- `PATHBASE_MCP_ENABLED=1` turns the endpoint on. Without it, neither the MCP
  endpoint nor the OAuth endpoints exist.
- `PATHBASE_API_BASE_URL` is where the Rust API is reachable from outside.
  Behind the Worker that is `{PATHBASE_PUBLIC_URL}/api`, which is the default.
  The OAuth metadata publishes absolute URLs, so this cannot be guessed.
- Production and preview are different resources
  (`PATHBASE_MCP_RESOURCE`). A preview token is refused by production and the
  other way round, and the delegations live in different databases.
- Expired and consumed grants are removed after 45 days by
  `oauth::purge_expired`. They are kept that long so a replay is still
  detectable rather than merely unknown.
