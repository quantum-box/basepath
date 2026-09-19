-- Tenant becomes a data boundary, not just a step in the sign-in flow.
--
-- Until now a workspace belonged to a person and to nobody else: the personal
-- workspace id was derived from the user id alone, `GET /v1/workspaces`
-- listed every membership an actor held, and `authorize` asked only for a
-- role. Selecting a Tachyon tenant therefore changed the Field integration
-- and nothing else — the same data came back whichever tenant was chosen.
--
-- `workspaces.tenant_id` is what fixes that. Every other business table
-- reaches its tenant through `workspace_id`, so one column plus a check in
-- `authorize` is the whole boundary; adding the column to each child table
-- would create a second copy of the same fact that could disagree with it.
--
-- Existing workspaces predate the column and cannot be attributed to a
-- tenant after the fact, so they are discarded rather than adopted into a
-- guess (user decision, 2026-09-19). `settings` holds per-person display
-- preferences rather than business data and is therefore kept.
DELETE FROM idempotency;
DELETE FROM audit;
DELETE FROM invitations;
DELETE FROM documents;
DELETE FROM memberships;
DELETE FROM workspaces;
-- A delegation is to an AI client *within a tenant*. The existing rows have
-- no tenant to belong to, so the person re-consents rather than inheriting a
-- connection whose scope silently changed meaning.
-- A pre-authorized apply range names a workspace and a connection, both of
-- which are going away here. A rule that outlived them would be a standing
-- permission over rows nobody can point at any more.
DELETE FROM auto_apply_rules;
DELETE FROM mcp_grants;
DELETE FROM mcp_connections;
-- Each DDL step is conditional because the runner cannot make it atomic.
--
-- A migration's statements run in separate transactions and the version row
-- is written only after the last one, so a process that dies partway through
-- re-runs the whole script on the next start. TiDB does not roll back DDL, so
-- an unconditional `ADD COLUMN` would then fail as a duplicate and every
-- subsequent startup would stop here until somebody repaired the schema by
-- hand. This script has five DDL steps, which is five chances to land in that
-- state; `IF [NOT] EXISTS` makes a retry finish instead.
ALTER TABLE workspaces ADD COLUMN IF NOT EXISTS tenant_id VARCHAR(191) NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS workspaces_tenant ON workspaces(tenant_id);
ALTER TABLE mcp_connections ADD COLUMN IF NOT EXISTS tenant VARCHAR(191) NOT NULL DEFAULT '';
-- One connection per client *per tenant*: the same person using the same AI
-- client in two tenants is two delegations, each with its own scopes.
ALTER TABLE mcp_connections DROP INDEX IF EXISTS mcp_connections_owner;
CREATE UNIQUE INDEX IF NOT EXISTS mcp_connections_owner ON mcp_connections(actor, tenant, client_id)
