-- Basepath as the authorization server for its own MCP endpoint.
--
-- The Tachyon-managed Cognito pool authenticates people, but it cannot be the
-- authorization server an AI host talks to: its discovery document advertises
-- no `code_challenge_methods_supported`, offers no registration endpoint, and
-- its redirect URIs are fixed at deploy time. A host that mints a fresh
-- callback per connection therefore cannot register at all.
--
-- So Basepath issues its own MCP tokens. Authentication is still Cognito's —
-- a person must be signed in to Basepath to consent — but the grant, the
-- token, and the revocation are Basepath's, which is also where they belong:
-- the delegation in `mcp_connections` is the thing being granted.
CREATE TABLE IF NOT EXISTS mcp_clients(
  id VARCHAR(191) NOT NULL,
  name VARCHAR(191) NOT NULL,
  -- One absolute URI per line. Registration alone grants nothing, so this is
  -- not a trust decision; it only pins where a code may be delivered.
  redirect_uris TEXT NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  PRIMARY KEY(id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;

-- Every short-lived bearer artifact of the authorization code flow.
--
-- `kind` says which: a validated `request` awaiting the person's decision, an
-- authorization `code`, an `access` token, or a `refresh` token. They share a
-- table because they share a lifecycle — all of them are consumed, expire, and
-- die with the connection they belong to.
--
-- Only the SHA-256 of each secret is stored. A database read therefore does
-- not yield a usable token.
CREATE TABLE IF NOT EXISTS mcp_grants(
  id VARCHAR(191) NOT NULL,
  kind VARCHAR(16) NOT NULL,
  secret_hash VARCHAR(64) NOT NULL,
  client_id VARCHAR(191) NOT NULL,
  actor VARCHAR(191) NOT NULL,
  connection_id VARCHAR(191) NOT NULL,
  scopes VARCHAR(512) NOT NULL,
  resource VARCHAR(512) NOT NULL,
  redirect_uri VARCHAR(512) NOT NULL,
  code_challenge VARCHAR(191) NOT NULL,
  state_value VARCHAR(512) NOT NULL,
  -- Refresh tokens rotate. Every token descended from one authorization code
  -- shares a chain, so presenting a rotated-out token revokes the whole family
  -- rather than only itself.
  chain VARCHAR(191) NOT NULL,
  expires_at VARCHAR(40) NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  consumed_at VARCHAR(40) NOT NULL,
  PRIMARY KEY(id),
  UNIQUE KEY mcp_grants_secret (secret_hash),
  KEY mcp_grants_connection (connection_id),
  KEY mcp_grants_chain (chain)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin
