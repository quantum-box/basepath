-- Mirrors migrations/mysql/0007_auto_apply.sql. See that file for why this is
-- the same act as an approval rather than an exception to it, and for what is
-- deliberately not a column here.
CREATE TABLE IF NOT EXISTS auto_apply_rules(
  id TEXT NOT NULL PRIMARY KEY,
  actor TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  allow_create INTEGER NOT NULL,
  allow_update INTEGER NOT NULL,
  allow_guarded INTEGER NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  revoked_at TEXT NULL,
  version INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS auto_apply_rules_scope
  ON auto_apply_rules(actor, workspace_id, connection_id);
ALTER TABLE mcp_connections ADD COLUMN ui_read_at TEXT NOT NULL DEFAULT '';
