-- Mirrors migrations/mysql/0004_audit_connection.sql.
ALTER TABLE audit ADD COLUMN connection TEXT NOT NULL DEFAULT ''
