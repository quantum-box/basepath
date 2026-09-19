-- Mirrors migrations/mysql/0008_tenant_isolation.sql. See that file for why
-- the tenant lives on `workspaces` alone, and why existing rows are dropped
-- rather than attributed to a tenant after the fact.
DELETE FROM idempotency;
DELETE FROM audit;
DELETE FROM invitations;
DELETE FROM documents;
DELETE FROM memberships;
DELETE FROM workspaces;
DELETE FROM auto_apply_rules;
DELETE FROM mcp_grants;
DELETE FROM mcp_connections;
-- The index steps are conditional; the two ADD COLUMNs cannot be, because
-- SQLite has no `IF NOT EXISTS` for them. A process that dies between the
-- committed column and the version row therefore leaves a database this
-- script cannot re-run. That is survivable only because SQLite here is
-- local preview — a developer's own file, replaced by deleting it — and not
-- because the window does not exist. Production is TiDB, where the MySQL
-- file spells every step conditionally.
ALTER TABLE workspaces ADD COLUMN tenant_id TEXT NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS workspaces_tenant ON workspaces(tenant_id);
ALTER TABLE mcp_connections ADD COLUMN tenant TEXT NOT NULL DEFAULT '';
DROP INDEX IF EXISTS mcp_connections_owner;
CREATE UNIQUE INDEX IF NOT EXISTS mcp_connections_owner ON mcp_connections(actor, tenant, client_id)
