-- Records which deployment this database belongs to.
--
-- Production and every per-PR preview declare their own
-- `PATHBASE_DB_ENVIRONMENT`. The first process to migrate claims the database
-- for that value; a later process configured for a different one refuses to
-- migrate or serve. A production build pointed at a preview DSN (or the
-- reverse) therefore fails closed instead of writing to the wrong database.
CREATE TABLE IF NOT EXISTS database_identity(
  id VARCHAR(16) NOT NULL,
  environment VARCHAR(64) NOT NULL,
  claimed_at VARCHAR(40) NOT NULL,
  PRIMARY KEY(id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin
