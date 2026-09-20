-- A conversation link may be retried with more than one host-generated key.
-- Keep every key owned by the same conversation instead of overwriting the
-- link's legacy single-key column and releasing the old key.
CREATE TABLE IF NOT EXISTS conversation_link_idempotency(
  id TEXT NOT NULL PRIMARY KEY,
  actor TEXT NOT NULL,
  tenant TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  link_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(actor, tenant, connection_id, idempotency_key)
);
INSERT OR IGNORE INTO conversation_link_idempotency
  (id,actor,tenant,connection_id,idempotency_key,conversation_id,link_id,created_at)
SELECT 'clinkkey_' || id, actor, tenant, connection_id, idempotency_key,
       conversation_id, id, created_at
FROM conversation_links;
