# Production durability boundary

## Current guarantees

PathBase has two independent state classes:

| State | Current implementation | Restart | Horizontal scale |
| --- | --- | --- | --- |
| Tachyon login session | Opaque `HttpOnly` cookie + AES-256-GCM sealed session row in shared TiDB | survives replacement and renews access tokens for up to 12 hours | safe; no affinity required |
| Workspaces, memberships, documents, audit, idempotency | Tachyon-managed TiDB via `DATABASE_URL` (SQLx / MySQL protocol) | survives | safe; writers serialize per workspace inside the database |

`GET /api/health` reports what the running process actually persists to: `storage: tidb` with `storage_durability: shared-durable`, or `storage: sqlite` with `storage_durability: ephemeral-runtime` for the explicit local preview. A deployment that silently came up on the preview database is therefore visible from the health endpoint; do not treat `ephemeral-runtime` in production as durable.

Production refuses to start without a database: `DATABASE_URL` (or `PATHBASE_DATABASE_URL`) is required whenever `PATHBASE_MODE` is not `local-preview`, and an unreachable database is a startup error. There is no SQLite fallback, because `/tmp` is local to one execution environment and would silently fork the data.

## Provisioning and the deployment gate

`tachyon.yml` declares the database on the `pathbase-api` app only; the static Worker never receives a database secret:

```yaml
provisionedDatabase:
  provider: tidb
  engine: mysql
  envVar: DATABASE_URL
```

Tachyon issues the database, the SQL user, its grant, and the DSN secret, and injects the DSN as `DATABASE_URL`. The manifest names no secret path and carries no DSN. Re-applying the same manifest does not re-provision or rotate anything.

`environments.preview.provisionedDatabase` gives every pull request its own database and credentials, so PR A, PR B, and production are separated in data and permissions. `previewSharesProductionDatabase` is deliberately never declared: ADR-0049 makes PR-scoped isolation the default and refuses a preview that would resolve the production DSN.

> The canonical manifest key is being renamed to `spec.database` (PLT-4812). Until that ships in the deployed IaC, this manifest uses `provisionedDatabase`, the key the platform accepts today. The rename is a key rename only and must not re-provision the database or rotate its credentials.

### Environment claim

`PATHBASE_DB_ENVIRONMENT` is declared per environment (`production` / `preview`) and never in the base list. The first process to migrate writes it into `database_identity`; a process configured for a different value refuses to migrate and reports `environment_mismatch` from readiness. A production build pointed at a preview DSN — or the reverse — therefore fails closed instead of writing to the wrong database.

The rule applies only when **both** sides carry a label. A missing label (`local-preview`, the value when the variable is unset) is a configuration gap, not a mix-up: a preview build only *plans* the manifest, so a preview deployment can start before its overlay env var has ever been applied. Such a database is adopted by the next start that does carry a label, rather than taking the deployment down. Two labelled deployments disagreeing is always refused.

### Migration

Migrations run when the API process opens the database, inside the app's own network. That is the only place the managed Cloud App TiDB is reachable from: it is PrivateLink-only, so a command hook on the shared build runner cannot reach it, and a `migration.lambdaInvoke` hook runs *before* the candidate is deployed, which would execute the previously deployed code against the new database. Simultaneous cold starts are serialized with an advisory lock (`GET_LOCK`). MySQL advisory locks are server-wide rather than per database, so the lock name carries the database name: a per-PR preview and production sharing one TiDB cluster do not serialize against each other. A process whose schema is already current skips the lock entirely.

`pathbase-api --migrate` applies the schema and exits, printing the resulting status, for an operator or a future platform-side gate.

### Readiness

`readinessProof` points at `/health/ready`, which reaches the database and reports the applied schema:

```json
{"status":"ready","schema":"current","schema_version":2,"expected_schema_version":2,
 "storage":"tidb","storage_durability":"shared-durable",
 "environment":"production","database_environment":"production","reason":"ok"}
```

It returns 503 with a `reason` of `database_unreachable`, `schema_out_of_date`, or `environment_mismatch` otherwise. A candidate whose migration failed therefore never becomes the active deployment, and the previously deployed version keeps serving. A static 200 or a hard-coded `storage_durability` string is explicitly not accepted as evidence.

### Connection pool

One Lambda execution environment serves one request at a time, so the pool is small; the ceiling that matters is `PATHBASE_DB_MAX_CONNECTIONS × reserved concurrency` against the TiDB connection limit. Defaults, all overridable by environment variable:

| Setting | Default | Why |
| --- | --- | --- |
| `PATHBASE_DB_MAX_CONNECTIONS` | 2 | Raise deliberately; it multiplies by Lambda concurrency |
| `PATHBASE_DB_CONNECT_TIMEOUT_SECS` | 10 | Fail the request rather than hang the invocation |
| `PATHBASE_DB_IDLE_TIMEOUT_SECS` | 300 | A frozen execution environment holds nothing open |
| `PATHBASE_DB_MAX_LIFETIME_SECS` | 600 | Recycle so a failed-over TiDB node is not pinned |
| `PATHBASE_DB_MIGRATION_LOCK_SECS` | 60 | How long a cold start waits for another one's migration |
| `PATHBASE_DB_SSL_MODE` | `preferred` | See below |

`test_before_acquire` is on: Lambda freeze/thaw can leave a connection the server has already dropped.

**TLS.** A DSN that names its own mode wins. Tachyon issues the managed Cloud App DSN as `mysql://…?ssl-mode=REQUIRED` because TiDB Serverless requires TLS, and this process must not quietly weaken it. `PATHBASE_DB_SSL_MODE` is only consulted when the DSN says nothing — where it defaults to `preferred`, so a local cluster without TLS still works — or when an operator sets it deliberately, in which case it overrides the DSN. `disabled`, `preferred`, `required`, `verify_ca` and `verify_identity` are accepted; anything else is a startup error.

## Monitoring

These are the signals that mean "the database boundary is failing", and what each one looks like. None of them carries a credential: every message that could contain a DSN passes through a redaction step that replaces the password (`api/tests/durability.rs` asserts it), and no business content is put into an error message.

| Signal | Where it shows | What it means |
| --- | --- | --- |
| Connection failure | startup exits with `DATABASE_UNAVAILABLE`; `/health/ready` returns 503 `database_unreachable` | The database is unreachable or refuses the credential. The candidate is not promoted; the previous deployment keeps serving. |
| Pool exhaustion | requests fail with `STORAGE_ERROR` after `PATHBASE_DB_CONNECT_TIMEOUT_SECS` | More concurrent work than `PATHBASE_DB_MAX_CONNECTIONS`. The request fails rather than holding the invocation open. |
| Migration failure | startup exits; `/health/ready` returns 503 `schema_out_of_date` | The schema this build needs is not applied. |
| Migration lock timeout | `MIGRATION_LOCK_TIMEOUT` | Another instance has been migrating for longer than `PATHBASE_DB_MIGRATION_LOCK_SECS`. |
| Wrong deployment | `/health/ready` returns 503 `environment_mismatch` | This process was pointed at a database another deployment claimed. |
| Write conflict | `409 STORAGE_CONFLICT` | Another writer won the race. The client retries; it is not a server fault. |
| Authorization refusal | `403` / `404` from the service | Ordinary access control, not a storage problem. Worth watching for rate, not for individual events. |

## Backups

What is **verified**:

- PathBase owns a logical export/restore path — `pathbase-api --migrate-from` — and it is exercised on every CI run of the TiDB job: a database is copied into a separate, isolated database, row counts and a content digest are compared, business invariants are re-checked, and the restored copy then serves a read and a write (`api/tests/migration.rs`).
- A migration into a non-empty target is refused, and a source that fails its integrity checks is refused before anything is written.

What is **not verified here**, and must not be presented as a guarantee:

- The managed data plane is a TiDB Cloud Serverless cluster (`tachyon-cloud-apps`) with its public endpoint disabled, reached over PrivateLink. Its backups are the vendor's, not something this repository or Tachyon's Terraform declares: there is no `Backup` or `BackupSchedule` resource in the platform repository.
- Retention, point-in-time recovery, and who may trigger a restore are therefore properties of that TiDB Cloud cluster and of the account that owns it. They have **not** been confirmed from the console, so this document does not promise a PITR window.
- A restore performed by the vendor has not been rehearsed. The rehearsal above restores from a live database, not from a vendor backup artifact.

The next step to close this gap is to read the retention and PITR settings of the `tachyon-cloud-apps` cluster in the TiDB Cloud console, record them here, and rehearse a vendor restore into a throwaway database using the same comparison.

## Migration and cutover

`docs/runbook-tidb-cutover.md` is the operator procedure: inventory, stop writes, snapshot, dry run, migrate, verify, canary, resume — and what to do when it goes wrong. It also records why production needed no data migration.

Tachyon Storage/R2 is an object store for files. Copying a live SQLite database or its WAL to R2 is not a safe database: Lambda shutdown is not a commit protocol, and multiple execution environments cannot coordinate writes through object snapshots. No bucket or external database is created by this repository change.

## Session configuration and rotation

Production startup requires `PATHBASE_SESSION_KEYS`. Each entry is exactly 32 random bytes encoded as unpadded base64url. Keep it in the Cloud App secret/credential facility, never in `tachyon.yml`, source, browser variables, or build logs.

The first key encrypts new session envelopes; later keys decrypt old envelopes. Rotate without logging everyone out:

1. Deploy `new,current` and wait at least the maximum access-token lifetime.
2. Deploy `new` only.

Removing every old key immediately invalidates existing sessions. Logout deletes the shared session row and clears the browser cookie, so copied cookies stop working too. PathBase keeps the rotating refresh token only in that sealed row, renews the access token near expiry, and ends the session after 12 hours or when the upstream rejects renewal. A no-database test configuration intentionally falls back to a sealed cookie without renewal; production Lambda always attaches the shared database.

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

## What has actually been verified

Against a real TiDB, on every CI run of the `Shared TiDB behaviour` job:

| Claim | Where |
| --- | --- |
| Two independent instances (separate pools) keep `expected_version`, the last-owner rule, cycle refusal, changeset atomicity, and cross-instance idempotency | `api/tests/tidb.rs` |
| Simultaneous cold starts migrate exactly once | `api/tests/tidb.rs` |
| State and audit history survive a redeploy, and a third instance sees another's write | `api/tests/durability.rs` |
| A database outage fails loudly, and the retry after recovery neither duplicates nor loses the write | `api/tests/durability.rs` |
| Pool exhaustion fails the request instead of hanging the invocation | `api/tests/durability.rs` |
| No message carries a connection password; a DSN's `ssl-mode` is never weakened | `api/tests/durability.rs` |
| A migration preserves counts, content digest, business invariants, history, and continues versioning | `api/tests/migration.rs` |
| A restore into an isolated database matches the original and still serves reads and writes | `api/tests/migration.rs` |

Observed once in the real deployment, not in CI: a candidate that could not reach a database failed its readiness proof and the previously deployed version kept serving (2026-09-18, `dep_01m2t9gsrttb6wvvh3gvkv3z2a`).

Not verified: the managed cluster's own backup retention and point-in-time recovery — see [Backups](#backups).

## Local preview

`PATHBASE_MODE=local-preview` continues to use a local SQLite file and generated loopback bearer credential. It does not require `PATHBASE_SESSION_KEYS` and does not claim cloud durability.
