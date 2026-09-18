-- Mirrors migrations/mysql/0003_mcp_connections.sql. See that file for why a
-- valid token is not by itself permission to act.
CREATE TABLE IF NOT EXISTS mcp_connections(
  id TEXT NOT NULL PRIMARY KEY,
  actor TEXT NOT NULL,
  client_id TEXT NOT NULL,
  client_name TEXT NOT NULL,
  scopes TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_used_at TEXT NOT NULL,
  version INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS mcp_connections_owner ON mcp_connections(actor, client_id)
