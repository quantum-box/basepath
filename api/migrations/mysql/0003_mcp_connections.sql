-- One row per (user, MCP client) delegation.
--
-- A valid access token proves *who* is calling. It does not prove that the
-- person agreed to let this AI client read their plan or propose changes to
-- it. That agreement is this row: created on first contact as `pending` with
-- no scopes, moved to `active` only by an explicit action in PathBase, and
-- `revoked` when the person disconnects. It lives in the shared database so
-- every Lambda execution environment sees the same answer, and so a
-- disconnect takes effect immediately rather than when a process restarts.
CREATE TABLE IF NOT EXISTS mcp_connections(
  id VARCHAR(191) NOT NULL,
  actor VARCHAR(191) NOT NULL,
  client_id VARCHAR(191) NOT NULL,
  client_name VARCHAR(191) NOT NULL,
  scopes VARCHAR(512) NOT NULL,
  status VARCHAR(32) NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  updated_at VARCHAR(40) NOT NULL,
  last_used_at VARCHAR(40) NOT NULL,
  version BIGINT NOT NULL,
  PRIMARY KEY(id),
  UNIQUE KEY mcp_connections_owner (actor, client_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin
