# Host packages

Everything in a distributable package that is specific to one AI host, and
nothing that is not. The workflows themselves live once in [`../skills`](../skills)
and are copied in at build time by `scripts/build-plugin.mjs`.

| Directory | Host | Contents |
| --- | --- | --- |
| `chatgpt/` | ChatGPT | `plugin.json`, `mcp.json`, `assets/` |
| `claude/` | Claude Code | `.claude-plugin/plugin.json`, `.mcp.json` |

claude.ai and Claude Desktop need no package at all: they connect to the MCP
endpoint by URL, and the server serves the same skills over the Skills
extension. The directory above is for Claude Code, which reads them from disk.

Adding a host means adding a directory here, not editing a skill. If a change
you are about to make would apply equally to another host, it belongs in
`../skills`.

Each host's layout — where its manifest lives, which MCP transport name it
uses — is one row in `LAYOUTS` in `scripts/build-plugin.mjs`. That is the whole
host-specific surface.

Build with `npm run build:plugin`, which writes `dist/plugin/<host>/`.
Nothing here contains a secret: the MCP endpoint issues no token to a package,
and the OAuth client is registered dynamically per connection by the host
itself.
