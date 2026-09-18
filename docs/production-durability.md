# Production durability boundary

## Current guarantees

PathBase has two independent state classes:

| State | Current implementation | Restart | Horizontal scale |
| --- | --- | --- | --- |
| Tachyon login session | AES-256-GCM `HttpOnly` cookie | survives when the same key is configured | safe; no affinity required |
| Workspaces, memberships, documents, audit, idempotency | Tachyon-managed TiDB via `DATABASE_URL` (SQLx / MySQL protocol) | survives | safe; writers serialize per workspace inside the database |

`GET /api/health` reports what the running process actually persists to: `storage: tidb` with `storage_durability: shared-durable`, or `storage: sqlite` with `storage_durability: ephemeral-runtime` for the explicit local preview. A deployment that silently came up on the preview database is therefore visible from the health endpoint; do not treat `ephemeral-runtime` in production as durable.

Production refuses to start without a database: `DATABASE_URL` (or `PATHBASE_DATABASE_URL`) is required whenever `PATHBASE_MODE` is not `local-preview`, and an unreachable database is a startup error. There is no SQLite fallback, because `/tmp` is local to one execution environment and would silently fork the data.

Tachyon Storage/R2 is an object store for files. Copying a live SQLite database or its WAL to R2 is not a safe database: Lambda shutdown is not a commit protocol, and multiple execution environments cannot coordinate writes through object snapshots. No bucket or external database is created by this repository change.

## Session configuration and rotation

Production startup requires `PATHBASE_SESSION_KEYS`. Each entry is exactly 32 random bytes encoded as unpadded base64url. Keep it in the Cloud App secret/credential facility, never in `tachyon.yml`, source, browser variables, or build logs.

The first key encrypts new cookies; later keys decrypt old cookies. Rotate without logging everyone out:

1. Deploy `new,current` and wait at least the maximum access-token lifetime.
2. Deploy `new` only.

Removing every old key immediately invalidates existing sessions. Logout clears the browser cookie but cannot centrally revoke a copied stateless cookie; upstream Tachyon token revocation and the short access-token expiry remain the revocation boundary. PathBase intentionally does not persist a refresh token in the cookie, so a user signs in again when the access token expires.

## Storage boundary

`api/src/db.rs` is the only module that knows which backend is in use. It exposes a transaction (`Tx`) with portable `fetch_all` / `fetch_optional` / `execute`, and a `Dialect` that renders the statements the two dialects genuinely disagree about. The business services in `service.rs`, `collaboration.rs`, and `storage.rs` contain no backend conditionals, so HTTP, MCP, and the Tauri bridge all get the same contract.

Schema lives in `api/migrations/{sqlite,mysql}/*.sql` and is applied by `Db::migrate`. Because TiDB does not roll DDL back with the surrounding DML, each statement is applied on its own and `schema_migrations` records the version only after the whole file succeeded; re-running an applied migration is a no-op.

Dialect differences that were resolved rather than papered over:

- **Placeholders** — `?` everywhere; values are always bound, never interpolated.
- **Upsert / insert-ignore** — `Dialect::upsert` and `Dialect::insert_ignore` render `ON CONFLICT … DO UPDATE` or `ON DUPLICATE KEY UPDATE`, and `INSERT OR IGNORE` or `INSERT IGNORE`.
- **Insertion order** — SQLite's `rowid` ordering is replaced by an application-assigned, lexicographically sortable `seq` column. AUTO_INCREMENT is deliberately not used: TiDB allocates it per node.
- **JSON** — document bodies are `LONGTEXT` and are parsed in Rust. The previous `json_extract` index expressions and the one `json_extract` projection are gone; nothing depends on server-side JSON paths.
- **Collation** — every production table is `utf8mb4_bin`, matching SQLite's BINARY comparison, so ids and idempotency keys cannot collide case-insensitively.
- **Constraints** — no `FOREIGN KEY` or `CHECK` in either schema. TiDB does not enforce the equivalents by default, and a rule that held only in local preview would hide production bugs; roles, invitation status, and reference integrity are validated in the Rust service for both backends.
- **Savepoints** — changeset previews validate the whole batch inside a savepoint and roll back to it; `Dialect::savepoint` renders MySQL's `ROLLBACK TO SAVEPOINT` / `RELEASE SAVEPOINT` spelling.

Use an explicit `DATABASE_URL` (or `PATHBASE_DATABASE_URL`) secret, issued by Tachyon alongside the app. Do not mount SQLite on Cloud Storage FUSE or advertise a persistent disk as horizontally writable.

## Concurrency

SQLite serialized every writer with one file lock (`BEGIN IMMEDIATE`), which is what the existing business rules — `expected_version`, cycle checks, the last-owner invariant, changeset atomicity — were written against. TiDB's pessimistic transactions do not provide that for free:

- a plain `SELECT` reads the transaction's start snapshot, so a writer could decide against state another writer had already replaced; and
- row locks are only taken by statements that modify rows, so two writers could both read version 1 and both write version 2.

Two measures restore the original contract:

1. **Workspace lock.** Every write transaction takes `SELECT id FROM workspaces WHERE id=? FOR UPDATE` before reading anything. Writers of one workspace serialize; different workspaces do not block each other. Routes without a workspace segment (accepting an invitation) take the lock explicitly once the target workspace is known.
2. **Locking reads.** Inside a write transaction, every read that feeds a decision is a `FOR UPDATE` read, so it sees the latest committed row rather than the start snapshot. Read-only transactions keep their consistent snapshot.

TiDB write conflicts (9007) and lock timeouts/deadlocks (1213 / 1205) surface as `409 STORAGE_CONFLICT` rather than a 500, so a client retries rather than treating the request as malformed.

`api/tests/tidb.rs` proves this against a real TiDB with independent `Service` instances (separate connection pools): version conflicts, cross-instance idempotency replay, double-applied changesets, a rolled-back batch, the last-owner invariant, concurrent cycle creation, and a replay after membership was revoked. The fixture asserts `SELECT tidb_version()` succeeds, so a plain MySQL cannot stand in for the check.

## Staged migration and failure behavior

1. Put production in read-only/maintenance mode before export. Retain the source SQLite file.
2. Run schema migration against an empty shared database, then import in foreign-key order in one controlled job. Record row counts and stable content hashes by table.
3. Validate owner invariants, document JSON, idempotency rows, and representative read-only API projections.
4. Deploy a single canary against the shared database. Writes must fail closed if the database is unavailable; never fall back to local SQLite.
5. Expand instances only after concurrent create/update/idempotency tests pass. Keep SQLite read-only for rollback until the retention window ends.

If cutover validation fails before shared writes, point the app back to the retained SQLite source. After shared writes begin, do not reverse-copy automatically; stop writes and perform an operator-reviewed reconciliation. Backups, point-in-time recovery, connection limits, and restore drills are deployment responsibilities and must be proven in the chosen managed database.

## Local preview

`PATHBASE_MODE=local-preview` continues to use a local SQLite file and generated loopback bearer credential. It does not require `PATHBASE_SESSION_KEYS` and does not claim cloud durability.
