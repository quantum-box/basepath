-- A link is context metadata, not a plan mutation.  It lets a later MCP
-- request resolve the workspace the person was discussing without copying
-- conversation text into PathBase.
CREATE TABLE IF NOT EXISTS conversation_links(
  id TEXT NOT NULL PRIMARY KEY,
  actor TEXT NOT NULL,
  tenant TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  item_id TEXT NULL,
  screen TEXT NULL,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  source_version TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(actor, tenant, conversation_id),
  UNIQUE(actor, tenant, idempotency_key)
);
CREATE INDEX IF NOT EXISTS conversation_links_connection
  ON conversation_links(connection_id, status);
