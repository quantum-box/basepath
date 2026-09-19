-- A range the person decided in advance may be applied without asking again.
--
-- This is not an exception to "only a person approves". It is the same act,
-- moved earlier. A row here is written on Basepath's own origin, with the
-- person's own session and the same-origin CSRF header — exactly the evidence
-- an approval carries — and it says which proposals from which AI connection
-- they have already decided about. An AI connection can neither read this
-- table nor write to it; the route that manages it refuses an agent actor
-- before it reads anything.
--
-- What is deliberately absent matters as much as what is here:
--
--   * There is no `allow_delete` column. Deleting is not a checkbox somebody
--     left unticked, it is outside what this table can express at all, so no
--     future screen can offer it by accident.
--   * There is no "all workspaces" and no "any connection". Both are NOT NULL,
--     because a range with no edges is not a range — it is turning the
--     proposal model off, and then the proposals were pointless.
--   * `expires_at` is NOT NULL. An indefinite standing permission is one
--     nobody revisits.
CREATE TABLE IF NOT EXISTS auto_apply_rules(
  id VARCHAR(191) NOT NULL,
  -- Whose plan this is about. Always the person, never an agent actor.
  actor VARCHAR(191) NOT NULL,
  -- One workspace. The personal/organization boundary is not crossed by a
  -- convenience setting.
  workspace_id VARCHAR(191) NOT NULL,
  -- Which AI client. `mcp_connections.id` — the record of who they delegated
  -- to — so the range sits beside the delegation it narrows.
  connection_id VARCHAR(191) NOT NULL,
  -- Adding is the safe half of a breakdown: a goal that should not be there
  -- is visible and removable. These are separate because "let it add work"
  -- and "let it rewrite what I wrote" are different decisions.
  allow_create TINYINT(1) NOT NULL,
  allow_update TINYINT(1) NOT NULL,
  -- Dates, owners, targets, baselines and self-assessments read afterwards as
  -- something the person decided. Off unless they say otherwise, explicitly,
  -- on this row.
  allow_guarded TINYINT(1) NOT NULL,
  expires_at VARCHAR(40) NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  updated_at VARCHAR(40) NOT NULL,
  -- Set when the person turns it off. Kept rather than deleted so a change
  -- set that was auto-applied can still name the range it was applied under.
  revoked_at VARCHAR(40) NULL,
  version BIGINT NOT NULL,
  PRIMARY KEY(id),
  UNIQUE KEY auto_apply_rules_scope (actor, workspace_id, connection_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;

-- When a host last rendered the in-conversation view on this connection.
--
-- "Does this host draw MCP Apps?" was unanswerable from the audit trail,
-- because reading a resource is not a write and nothing recorded it. Two
-- people lost an afternoon to that question. It is one column: the host reads
-- the `ui://` resource only in order to render it, so a timestamp here is the
-- difference between "the host does not support it" and "it supported it and
-- something else went wrong" — measured, per host, rather than assumed.
ALTER TABLE mcp_connections ADD COLUMN ui_read_at VARCHAR(40) NOT NULL DEFAULT '';
