-- PathBase production schema (MySQL wire protocol / TiDB).
--
-- utf8mb4_bin keeps string comparison byte-exact, matching SQLite's default
-- BINARY collation: workspace ids, actor ids, and idempotency keys must not
-- collide case-insensitively.
--
-- JSON bodies are stored as LONGTEXT rather than the JSON type. The service
-- serializes and parses them in Rust, and a text column keeps the schema
-- identical in meaning across both backends.
--
-- `seq` is an application-assigned, lexicographically sortable creation
-- stamp. It replaces SQLite's `rowid` ordering without depending on
-- AUTO_INCREMENT allocation, which is only monotonic per TiDB node.
CREATE TABLE IF NOT EXISTS workspaces(
  id VARCHAR(191) NOT NULL,
  body LONGTEXT NOT NULL,
  seq VARCHAR(64) NOT NULL,
  PRIMARY KEY(id),
  KEY workspaces_seq (seq)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
CREATE TABLE IF NOT EXISTS memberships(
  workspace_id VARCHAR(191) NOT NULL,
  actor VARCHAR(191) NOT NULL,
  role VARCHAR(32) NOT NULL,
  PRIMARY KEY(workspace_id, actor),
  KEY memberships_actor (actor)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
CREATE TABLE IF NOT EXISTS documents(
  workspace_id VARCHAR(191) NOT NULL,
  collection VARCHAR(64) NOT NULL,
  id VARCHAR(191) NOT NULL,
  body LONGTEXT NOT NULL,
  seq VARCHAR(64) NOT NULL,
  PRIMARY KEY(workspace_id, collection, id),
  KEY documents_order (workspace_id, collection, seq)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
CREATE TABLE IF NOT EXISTS idempotency(
  actor VARCHAR(191) NOT NULL,
  workspace_id VARCHAR(191) NOT NULL,
  `key` VARCHAR(200) NOT NULL,
  fingerprint VARCHAR(128) NOT NULL,
  response LONGTEXT NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  PRIMARY KEY(actor, workspace_id, `key`)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
CREATE TABLE IF NOT EXISTS settings(
  actor VARCHAR(191) NOT NULL,
  body LONGTEXT NOT NULL,
  PRIMARY KEY(actor)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
CREATE TABLE IF NOT EXISTS audit(
  id VARCHAR(191) NOT NULL,
  workspace_id VARCHAR(191) NOT NULL,
  actor VARCHAR(191) NOT NULL,
  origin VARCHAR(32) NOT NULL,
  command VARCHAR(512) NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  seq VARCHAR(64) NOT NULL,
  PRIMARY KEY(id),
  KEY audit_workspace (workspace_id, seq)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
CREATE TABLE IF NOT EXISTS invitations(
  id VARCHAR(191) NOT NULL,
  workspace_id VARCHAR(191) NOT NULL,
  target_actor VARCHAR(191) NOT NULL,
  role VARCHAR(32) NOT NULL,
  status VARCHAR(32) NOT NULL,
  created_by VARCHAR(191) NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  expires_at VARCHAR(40) NOT NULL,
  version BIGINT NOT NULL,
  PRIMARY KEY(id),
  KEY invitations_recipient (target_actor, status),
  KEY invitations_workspace (workspace_id, status)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin
