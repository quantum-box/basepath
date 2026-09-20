-- Keep only the canonical display name shown at consent time. The actor id
-- remains authoritative; this field is presentation data for the connection.
ALTER TABLE mcp_connections ADD COLUMN IF NOT EXISTS display_name VARCHAR(191) NOT NULL DEFAULT '';
