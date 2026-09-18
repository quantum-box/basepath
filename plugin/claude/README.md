# Basepath for Claude

Two ways in, and they are not the same thing:

**A custom connector** (claude.ai and Claude Desktop). Settings → Connectors →
Add custom connector, with `https://pathbase-v2.txcloud.app/api/mcp`. Nothing
is installed: Claude discovers the endpoint, registers itself, and sends you to
Basepath to authorize it. This is the path that renders the plan, the week and
the weekly review **inside the conversation**, because those surfaces support
MCP Apps.

**This directory** — a plugin, for Claude Code. It declares the same MCP server
and carries the same skills. Claude Code is a terminal: it runs the tools and
reads the instructions, and it does **not** render MCP Apps. Nothing here
promises otherwise.

The workflows are not in this directory in the repository. They live once in
[`../../skills`](../../skills) and are copied in by
`scripts/build-plugin.mjs`, the same way they are for every other host — and
the MCP server serves the same files over the Skills extension, so a host that
supports it needs no package at all.

See [docs/claude-connector.md](../../docs/claude-connector.md) for the full
procedure, what each surface can do, and what has actually been verified.
