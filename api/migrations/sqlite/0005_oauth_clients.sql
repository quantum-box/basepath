-- Mirrors migrations/mysql/0005_oauth_clients.sql. See that file for why
-- Basepath issues its own MCP tokens rather than pointing hosts at Cognito.
CREATE TABLE IF NOT EXISTS mcp_clients(
  id TEXT NOT NULL PRIMARY KEY,
  name TEXT NOT NULL,
  redirect_uris TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS mcp_grants(
  id TEXT NOT NULL PRIMARY KEY,
  kind TEXT NOT NULL,
  secret_hash TEXT NOT NULL,
  client_id TEXT NOT NULL,
  actor TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  scopes TEXT NOT NULL,
  resource TEXT NOT NULL,
  redirect_uri TEXT NOT NULL,
  code_challenge TEXT NOT NULL,
  state_value TEXT NOT NULL,
  chain TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  consumed_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS mcp_grants_secret ON mcp_grants(secret_hash);
CREATE INDEX IF NOT EXISTS mcp_grants_connection ON mcp_grants(connection_id);
CREATE INDEX IF NOT EXISTS mcp_grants_chain ON mcp_grants(chain)
