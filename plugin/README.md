# Host packages

Everything in a distributable package that is specific to one AI host, and
nothing that is not. The workflows themselves live once in [`../skills`](../skills)
and are copied in at build time by `scripts/build-plugin.mjs`.

| Directory | Host | Contents |
| --- | --- | --- |
| `chatgpt/` | ChatGPT | `plugin.json`, `mcp.json`, `assets/` |

Adding a host means adding a directory here, not editing a skill. If a change
you are about to make would apply equally to another host, it belongs in
`../skills`.

Build with `npm run build:plugin`, which writes `dist/plugin/<host>/`.
Nothing here contains a secret: the MCP endpoint issues no token to a package,
and the OAuth client is registered dynamically per connection by the host
itself.
