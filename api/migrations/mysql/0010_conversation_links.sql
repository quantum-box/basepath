-- Context metadata only; conversation contents remain in the host.
CREATE TABLE IF NOT EXISTS conversation_links(
  id VARCHAR(191) NOT NULL,
  actor VARCHAR(191) NOT NULL,
  tenant VARCHAR(191) NOT NULL,
  connection_id VARCHAR(191) NOT NULL,
  conversation_id VARCHAR(191) NOT NULL,
  workspace_id VARCHAR(191) NOT NULL,
  item_id VARCHAR(191) NULL,
  screen VARCHAR(80) NULL,
  status VARCHAR(32) NOT NULL,
  source VARCHAR(80) NOT NULL,
  source_version VARCHAR(80) NOT NULL,
  idempotency_key VARCHAR(191) NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  updated_at VARCHAR(40) NOT NULL,
  PRIMARY KEY(id),
  UNIQUE KEY conversation_links_conversation(actor, tenant, conversation_id),
  UNIQUE KEY conversation_links_idempotency(actor, tenant, idempotency_key),
  KEY conversation_links_connection(connection_id, status)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
