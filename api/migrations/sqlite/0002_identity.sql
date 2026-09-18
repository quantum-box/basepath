-- Mirrors migrations/mysql/0002_identity.sql. See that file for why the
-- environment claim exists.
CREATE TABLE IF NOT EXISTS database_identity(
  id TEXT NOT NULL PRIMARY KEY,
  environment TEXT NOT NULL,
  claimed_at TEXT NOT NULL
)
