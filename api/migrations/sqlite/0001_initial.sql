-- PathBase local-preview schema (SQLite).
-- Mirrors migrations/mysql/0001_initial.sql. Keep the two in step: the same
-- Rust service code runs over both, so a column that exists in only one of
-- them is a bug, not a dialect difference.
--
-- Deliberately no FOREIGN KEY or CHECK constraints: TiDB does not enforce the
-- equivalents by default, and a rule that holds only in local preview hides
-- production bugs. Membership roles, invitation status, and reference
-- integrity are validated in the Rust service for both backends.
CREATE TABLE IF NOT EXISTS workspaces(
  id TEXT NOT NULL PRIMARY KEY,
  body TEXT NOT NULL,
  seq TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS workspaces_seq ON workspaces(seq);
CREATE TABLE IF NOT EXISTS memberships(
  workspace_id TEXT NOT NULL,
  actor TEXT NOT NULL,
  role TEXT NOT NULL,
  PRIMARY KEY(workspace_id, actor)
);
CREATE INDEX IF NOT EXISTS memberships_actor ON memberships(actor);
CREATE TABLE IF NOT EXISTS documents(
  workspace_id TEXT NOT NULL,
  collection TEXT NOT NULL,
  id TEXT NOT NULL,
  body TEXT NOT NULL,
  seq TEXT NOT NULL,
  PRIMARY KEY(workspace_id, collection, id)
);
CREATE INDEX IF NOT EXISTS documents_order ON documents(workspace_id, collection, seq);
CREATE TABLE IF NOT EXISTS idempotency(
  actor TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  `key` TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  response TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(actor, workspace_id, `key`)
);
CREATE TABLE IF NOT EXISTS settings(
  actor TEXT NOT NULL PRIMARY KEY,
  body TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS audit(
  id TEXT NOT NULL PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  actor TEXT NOT NULL,
  origin TEXT NOT NULL,
  command TEXT NOT NULL,
  created_at TEXT NOT NULL,
  seq TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS audit_workspace ON audit(workspace_id, seq);
CREATE TABLE IF NOT EXISTS invitations(
  id TEXT NOT NULL PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  target_actor TEXT NOT NULL,
  role TEXT NOT NULL,
  status TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  version INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS invitations_recipient ON invitations(target_actor, status);
CREATE INDEX IF NOT EXISTS invitations_workspace ON invitations(workspace_id, status)
