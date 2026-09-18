-- Which MCP delegation an audited action arrived through.
--
-- "Who did this?" was already answerable; "from which connection?" was not.
-- An action taken in the browser has no connection, which is itself the
-- answer.
ALTER TABLE audit ADD COLUMN connection VARCHAR(191) NOT NULL DEFAULT ''
