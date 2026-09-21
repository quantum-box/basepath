# Basepath for Claude

Two ways in, and they are not the same thing:

**A custom connector** (claude.ai and Claude Desktop). Settings → Connectors →
Add custom connector, with `https://pathbase-v2.txcloud.app/api/mcp`. Nothing
is installed: Claude discovers the endpoint, registers itself, and sends you to
Basepath to authorize it. The server returns structured data and text plus one
reusable plan-tree resource for MCP Apps-capable hosts.

**This directory** — a plugin, for Claude Code. It declares the same MCP server
and carries the same skills. Claude Code is a terminal, so it uses the same
structured data and text contract rather than an embedded widget.

The workflows are not in this directory in the repository. They live once in
[`../../skills`](../../skills) and are copied in by
`scripts/build-plugin.mjs`, the same way they are for every other host — and
the MCP server serves the same files over the Skills extension, so a host that
supports it needs no package at all.

See [docs/claude-connector.md](../../docs/claude-connector.md) for the full
procedure, what each surface can do, and what has actually been verified.
