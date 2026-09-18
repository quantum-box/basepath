# Claude — connecting Basepath as a custom connector

Two ways in, and they are not the same thing. Which one someone uses decides
what they can see, so this document separates them before anything else.

| Surface | How it connects | In-conversation UI | Approving a change |
| --- | --- | --- | --- |
| claude.ai (web) | Custom connector, by URL | **Yes** — MCP Apps | In Basepath, via the link the app shows |
| Claude Desktop | Custom connector, by URL | **Yes** — MCP Apps | Same |
| Claude Code (CLI) | `plugin/claude/` or `.mcp.json` | **No** — it is a terminal | Same, by opening the URL |

The UI column is not a preference. MCP Apps renders in hosts that implement the
extension; the published
[client matrix](https://modelcontextprotocol.io/extensions/client-matrix) lists
claude.ai and Claude Desktop and does not list Claude Code. Nothing here
promises the CLI a rendered plan, and the tools return the same
`structuredContent` and text everywhere, so the CLI loses the view and nothing
else.

Approving is the same everywhere for the reason it always is: a click inside an
AI host reaches this server as an ordinary tool call, indistinguishable from
the model's. Approval happens on Basepath's origin, with the person's session.
See [change-approval.md](change-approval.md).

## Connecting claude.ai or Claude Desktop

1. **Settings → Connectors → Add custom connector.**
2. URL: `https://pathbase-v2.txcloud.app/api/mcp`
3. Claude reads the protected-resource metadata, then Basepath's authorization
   server metadata, registers itself (RFC 7591), and opens Basepath's consent
   screen.
4. Sign in if you are not already. The authorization request survives the
   round trip, so you land back on the same consent screen.
5. Choose the permissions. They can be narrowed later, and narrowing them
   narrows tokens that were already issued.
6. Done. Ask about your goals, your week, or last week's review.

Nothing is installed and nothing is configured by hand. The whole OAuth
contract, and why Basepath rather than the identity provider issues the tokens,
is in [chatgpt-plugin.md](chatgpt-plugin.md#why-basepath-issues-its-own-tokens)
— it is the same endpoint and the same flow for every host.

### Updating, removing, reconnecting

- **Update** — there is nothing to update. The connector is a URL; the server
  is deployed.
- **Remove** — remove the connector in Claude, *and* disconnect it in Basepath
  under 設定 → AIクライアントの接続. Removing it in Claude stops Claude calling;
  disconnecting in Basepath makes the tokens stop working, in the same
  transaction as the status change.
- **Reconnect** — add the connector again. It registers a new client and asks
  for consent again. A previously disconnected delegation is re-granted by that
  screen rather than silently revived.

## Claude Code

`plugin/claude/` is a plugin: `.claude-plugin/plugin.json`, `.mcp.json`, and the
shared skills. Build it with `npm run build:plugin`, which writes
`dist/plugin/claude/`.

It declares the same remote MCP server, so the OAuth flow is the same and the
CLI opens a browser for the consent screen. What it does not do is render the
plan in the terminal.

A stdio connection to a local binary also exists (`npm run api:mcp`) and is for
local development against a local database. It is not this, and connecting that
way is not evidence that the hosted connector works.

## The skills

The workflows — goal breakdown, week planning, recording progress, the weekly
review — are written once, in [`skills/`](../skills), and reach a host three
ways without being copied:

1. **Over MCP.** The server declares `io.modelcontextprotocol/skills` and serves
   `skills/list`, `skills/get` and the files through `resources/read` with
   `skill://` URIs, each with a SHA-256 digest and byte size the host verifies.
   A host that supports the extension needs no package at all.
2. **As ordinary resources.** They are in `resources/list` too, so a host
   without the extension can still read them.
3. **In a package**, for hosts that read skills from disk.

`api/tests/skills_over_mcp.rs` asserts the served bytes equal the repository's
files, and `tests/plugin.test.mjs` asserts both packages ship the same bytes and
that the server embeds them. Three delivery paths, one source; if they ever
diverge, CI says so rather than a person getting different instructions
depending on how they connected.

`scripts/build-plugin.mjs` refuses a skill that names a host. That wording
belongs in `plugin/<host>/`, which is the only host-specific surface: a manifest
path, an MCP transport name, and artwork.

## Two people, one connector

Each person authorizes separately and gets their own delegation. There is no
fixed actor and no shared token: the package contains no client id and no
credential, the client id is registered per connection, and the access token is
bound to the person who consented.

`api/tests/oauth.rs` covers this directly — two people using the same registered
client get separate delegations, and neither inherits the other's scopes.

## When something goes wrong

| Situation | What happens |
| --- | --- |
| Declines consent | Claude is told `access_denied`. No delegation is created |
| Permission not granted | The tool fails with `INSUFFICIENT_SCOPE`, and the app names the permission to allow |
| Disconnected in Basepath | The next call fails. Reconnecting starts a fresh consent |
| Access token expired (1 hour) | Refreshed silently; refresh tokens rotate |
| Authorization request expired (15 min) | The consent page says so and grants nothing |
| Host does not render MCP Apps | Every tool returns the same data as text. Only the view is missing |
| A workspace the person cannot see | `404`. Naming a workspace in the arguments never grants access |

## Verified, and not

| Claim | Status | Evidence |
| --- | --- | --- |
| The skills are discoverable, verifiable and readable over a real MCP session | **CI** | `api/tests/skills_over_mcp.rs` — 4 tests over stdio against the real binary |
| The served skills are byte-identical to the repository's and to both packages | **CI** | `api/tests/skills_over_mcp.rs`, `tests/plugin.test.mjs` |
| The Claude package is well-formed and carries no credential | **CI** | `tests/plugin.test.mjs`, `npm run check:plugin` |
| The in-conversation app uses no host's private API | **CI** | `tests/plugin.test.mjs` |
| Two people using one client get separate delegations | **CI** | `api/tests/oauth.rs` |
| OAuth discovery and the `WWW-Authenticate` challenge work in production | **verified** | `curl` against `https://pathbase-v2.txcloud.app`, 2026-09-19 |
| **Connecting from real claude.ai or Claude Desktop** | **not verified** | Not performed |
| **MCP Apps rendering in Claude** | **not verified** | Same |
| **Supported plans and versions** | **not measured** | Fill in from an actual connection; do not copy from documentation |

A custom connector is not a directory listing. Submitting Basepath to any public
connector directory is a separate decision and has not been prepared for or
taken.
