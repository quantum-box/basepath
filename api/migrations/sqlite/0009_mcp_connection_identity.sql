-- Keep the minimal identity the person saw when this AI connection was
-- approved. The canonical actor id remains the authority; this is only a
-- display label for consent/MCP responses and may be refreshed on re-consent.
ALTER TABLE mcp_connections ADD COLUMN display_name TEXT NOT NULL DEFAULT '';
