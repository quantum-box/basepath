# Production durability boundary

## Current guarantees

PathBase has two independent state classes:

| State | Current implementation | Restart | Horizontal scale |
| --- | --- | --- | --- |
| Tachyon login session | AES-256-GCM `HttpOnly` cookie | survives when the same key is configured | safe; no affinity required |
| Workspaces, memberships, documents, audit, idempotency | `/app/data/pathbase.sqlite3` in the Cloud Run container | **may be lost** | **not safe**; instances diverge |

`GET /api/health` deliberately reports `storage_durability: ephemeral-container`. Do not use a successful health check as evidence that application records are durable.

Tachyon Storage/R2 is an object store for files. Copying a live SQLite database or its WAL to R2 is not a safe database: Cloud Run shutdown is not a commit protocol, and multiple instances cannot coordinate writes through object snapshots. No bucket or external database is created by this repository change.

## Session configuration and rotation

Production startup requires `PATHBASE_SESSION_KEYS`. Each entry is exactly 32 random bytes encoded as unpadded base64url. Keep it in the Cloud App secret/credential facility, never in `tachyon.yml`, source, browser variables, or build logs.

The first key encrypts new cookies; later keys decrypt old cookies. Rotate without logging everyone out:

1. Deploy `new,current` and wait at least the maximum access-token lifetime.
2. Deploy `new` only.

Removing every old key immediately invalidates existing sessions. Logout clears the browser cookie but cannot centrally revoke a copied stateless cookie; upstream Tachyon token revocation and the short access-token expiry remain the revocation boundary. PathBase intentionally does not persist a refresh token in the cookie, so a user signs in again when the access token expires.

## Database migration target

Before enabling production writes, replace the concrete `rusqlite::Connection` behind `Service` with a transactional shared SQL adapter (managed PostgreSQL is the expected shape) while retaining the service and HTTP contracts. The migration must preserve:

- primary and foreign keys, invitation constraints, and owner invariants;
- atomic changesets and optimistic `expected_version` checks;
- the `(actor, workspace_id, key)` idempotency uniqueness boundary;
- JSON document fields and the indexes currently expressed with SQLite `json_extract`;
- UTC timestamp semantics and the audit log.

Use an explicit `PATHBASE_DATABASE_URL` secret for the shared adapter. Do not mount SQLite on Cloud Storage FUSE or advertise a persistent disk as horizontally writable.

## Staged migration and failure behavior

1. Put production in read-only/maintenance mode before export. Retain the source SQLite file.
2. Run schema migration against an empty shared database, then import in foreign-key order in one controlled job. Record row counts and stable content hashes by table.
3. Validate owner invariants, document JSON, idempotency rows, and representative read-only API projections.
4. Deploy a single canary against the shared database. Writes must fail closed if the database is unavailable; never fall back to local SQLite.
5. Expand instances only after concurrent create/update/idempotency tests pass. Keep SQLite read-only for rollback until the retention window ends.

If cutover validation fails before shared writes, point the app back to the retained SQLite source. After shared writes begin, do not reverse-copy automatically; stop writes and perform an operator-reviewed reconciliation. Backups, point-in-time recovery, connection limits, and restore drills are deployment responsibilities and must be proven in the chosen managed database.

## Local preview

`PATHBASE_MODE=local-preview` continues to use a local SQLite file and generated loopback bearer credential. It does not require `PATHBASE_SESSION_KEYS` and does not claim cloud durability.
