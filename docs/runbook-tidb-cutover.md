# Runbook — moving PathBase business data into the managed TiDB

This runbook covers a cutover from an existing PathBase database to a new,
empty one: the SQLite→TiDB move, and any later TiDB→TiDB move (a restore
rehearsal, a cluster change). It is written to be followed by one operator and
read afterwards by someone who was not there.

**Status: rehearsed, not executed against production data.** The commands below
have been run end to end against a real TiDB in `api/tests/migration.rs` and by
hand. No production records have been migrated, because production had none —
see [Did production need a migration?](#did-production-need-a-migration).
Running this against real user data is a separate, explicitly approved action.

## What the tool does and does not do

`pathbase-api --migrate-from <source>` fills an **empty** target database from a
source database and reports what it found. It is not a merge and not a sync:

- it refuses a target that already holds business rows;
- it refuses a source that fails its own integrity checks;
- it copies `workspaces`, `memberships`, `documents`, `invitations`,
  `settings`, `audit`, `idempotency` — every table holding business state;
- it never copies `schema_migrations` or `database_identity`, which describe
  the database rather than its contents;
- it compares row counts and a content digest on both sides, then re-checks
  business invariants (owners, references, document versions, replayable
  idempotency responses) on the target.

`--dry-run` performs every read and check and writes nothing.

The target's `DATABASE_URL` (or, for a local preview, `PATHBASE_DB`) is taken
from the environment; the source is the argument. A DSN therefore never appears
in a command someone might paste into a ticket with its password intact.

## Inventory before anything else

```sh
# Inventory of the source, on its own.
PATHBASE_MODE=local-preview PATHBASE_DB=/path/to/source.sqlite3 \
  pathbase-api --inventory
```

Record the output. `total_rows`, the per-table counts, and `digest` are what
the rest of this runbook compares against. The command exits non-zero if the
database fails its integrity checks, which is a reason to stop and investigate
rather than to migrate.

> Do not assume a single file is the whole story. The pre-TiDB deployment kept
> `/tmp/pathbase.sqlite3` inside a Lambda execution environment: several
> environments could hold different data, and any of it could already be gone.
> Inventory every file you actually have, and record which ones you could not
> obtain.

## Cutover

1. **Stop writes.** Put the app in maintenance, or remove the deployment's
   ability to serve, so nothing is written to the source after the snapshot.
2. **Snapshot the source.** Copy the SQLite file (or take a logical dump of the
   source TiDB). Keep it; it is the rollback material.
3. **Inventory the snapshot** with the command above and record the digest.
4. **Dry run against the new database.**
   ```sh
   DATABASE_URL='<target DSN>' PATHBASE_DB_ENVIRONMENT=production \
     pathbase-api --migrate-from /path/to/snapshot.sqlite3 --dry-run
   ```
   Every check must pass. `target_empty` failing means the target is not new.
5. **Migrate.** Same command without `--dry-run`. The report must end with
   `content_digest` passing and every `target_*` check passing.
6. **Verify independently.** Run `--inventory` against the target and compare
   its digest with step 3's. Then exercise the app: read a goal, read the audit
   trail, make one small write, and confirm the version incremented.
7. **Canary.** Point one instance at the new database and use it. Watch the
   signals in [Monitoring](./production-durability.md#monitoring).
8. **Resume writes** only after the canary is clean.

## If something goes wrong

**Before any write reaches the new database**, point the app back at the
retained source. Nothing was lost.

**After writes have started on the new database, do not copy back
automatically.** Stop writes, take an inventory of both databases, and
reconcile by hand. An automatic reverse copy would silently drop whatever was
written after the cutover. `--migrate-from` will not do it for you: it refuses
a non-empty target.

## Did production need a migration?

No, and the reason is recorded rather than assumed:

- Before this work, `pathbase-api` was configured with
  `PATHBASE_DB=/tmp/pathbase.sqlite3`. That path lives inside a single Lambda
  execution environment; it does not survive a redeploy and is not shared
  between concurrently running environments.
- `docs/production-durability.md` and the README both stated that production
  records must not be kept until the shared database existed, and the health
  endpoint advertised `storage_durability: ephemeral-runtime` to say so.
- The managed TiDB database was created empty by Tachyon when
  `provisionedDatabase` was first applied, and the schema was created by this
  build's migrations.

There is therefore no retained production dataset to migrate. If that ever
turns out to be wrong — for example a file is recovered from a running
execution environment — this runbook is how it gets moved, and the fact that it
was recovered should be recorded here.

## Restore rehearsal

A backup you have not restored is a guess. The rehearsal is the same tool
pointed at a throwaway database:

```sh
# 1. Inventory the live database.
DATABASE_URL='<live DSN>' pathbase-api --inventory   # record digest

# 2. Restore into a new, empty database and compare.
DATABASE_URL='<restored DSN>' \
  pathbase-api --migrate-from '<live DSN>'           # digest must match

# 3. Exercise it.
DATABASE_URL='<restored DSN>' pathbase-api --inventory
```

`api/tests/migration.rs` runs exactly this shape against a real TiDB on every
CI run of the TiDB job, including reading a snapshot and writing a new item
through the restored copy.

What this rehearsal does **not** establish is the managed cluster's own
backup and point-in-time-recovery behaviour; see
[Backups](./production-durability.md#backups).
