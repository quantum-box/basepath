-- A conversation link may be retried with more than one host-generated key.
-- Keep every key owned by the same conversation instead of overwriting the
-- link's legacy single-key column and releasing the old key.
CREATE TABLE IF NOT EXISTS conversation_link_idempotency(
  id VARCHAR(191) NOT NULL,
  actor VARCHAR(191) NOT NULL,
  tenant VARCHAR(191) NOT NULL,
  connection_id VARCHAR(191) NOT NULL,
  idempotency_key VARBINARY(200) NOT NULL,
  conversation_id VARCHAR(191) NOT NULL,
  link_id VARCHAR(191) NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  PRIMARY KEY(id),
  UNIQUE KEY conversation_link_idempotency_key(actor, tenant, connection_id, idempotency_key),
  KEY conversation_link_idempotency_link(link_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
INSERT IGNORE INTO conversation_link_idempotency
  (id,actor,tenant,connection_id,idempotency_key,conversation_id,link_id,created_at)
SELECT CONCAT('clinkkey_', id), actor, tenant, connection_id, idempotency_key,
       conversation_id, id, created_at
FROM conversation_links;
