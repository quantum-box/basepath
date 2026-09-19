//! Portable storage boundary for the PathBase business services.
//!
//! The same Rust service code runs over two SQLx backends:
//!
//! * **MySQL / TiDB** — the shared production database. Several Lambda
//!   execution environments write to it concurrently, so every mutation has to
//!   rely on database transactions rather than a process-local `Mutex`.
//! * **SQLite** — the explicit `local-preview` mode (browser preview, Tauri
//!   debug window, tests). It is never a production fallback.
//!
//! Callers write one portable SQL string. Everything that genuinely differs
//! between the two dialects (upserts, ignore-on-conflict, savepoints, the
//! insertion-order column) is produced by [`Dialect`], so the business code
//! contains no backend conditionals.
use crate::model::{ApiError, Result};
use sqlx::{Column, MySqlPool, Row as _, SqlitePool, TypeInfo};
use std::time::Duration;

/// Which SQL dialect the current connection speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Sqlite,
    MySql,
}

impl Dialect {
    /// `INSERT` that silently keeps the existing row on a primary-key clash.
    pub fn insert_ignore(self, table: &str, columns: &[&str]) -> String {
        let names = columns.join(",");
        let holes = vec!["?"; columns.len()].join(",");
        match self {
            Dialect::Sqlite => format!("INSERT OR IGNORE INTO {table}({names}) VALUES({holes})"),
            Dialect::MySql => format!("INSERT IGNORE INTO {table}({names}) VALUES({holes})"),
        }
    }

    /// `INSERT` that overwrites `replace` when `keys` already identify a row.
    /// Columns outside both lists keep their original value, so an update does
    /// not disturb the insertion-order column.
    pub fn upsert(self, table: &str, columns: &[&str], keys: &[&str], replace: &[&str]) -> String {
        let names = columns.join(",");
        let holes = vec!["?"; columns.len()].join(",");
        match self {
            Dialect::Sqlite => {
                let set: Vec<_> = replace
                    .iter()
                    .map(|c| format!("{c}=excluded.{c}"))
                    .collect();
                format!(
                    "INSERT INTO {table}({names}) VALUES({holes}) ON CONFLICT({}) DO UPDATE SET {}",
                    keys.join(","),
                    set.join(",")
                )
            }
            Dialect::MySql => {
                let set: Vec<_> = replace.iter().map(|c| format!("{c}=VALUES({c})")).collect();
                format!(
                    "INSERT INTO {table}({names}) VALUES({holes}) ON DUPLICATE KEY UPDATE {}",
                    set.join(",")
                )
            }
        }
    }

    fn savepoint(self, name: &str) -> [String; 3] {
        match self {
            Dialect::Sqlite => [
                format!("SAVEPOINT {name}"),
                format!("ROLLBACK TO {name}"),
                format!("RELEASE {name}"),
            ],
            Dialect::MySql => [
                format!("SAVEPOINT {name}"),
                format!("ROLLBACK TO SAVEPOINT {name}"),
                format!("RELEASE SAVEPOINT {name}"),
            ],
        }
    }
}

/// A bind value. Every query parameter crosses the backend boundary as one of
/// these, so call sites never name a driver-specific argument type.
#[derive(Clone, Debug)]
pub enum Param {
    Text(String),
    Int(i64),
    Null,
}

impl From<&str> for Param {
    fn from(value: &str) -> Self {
        Param::Text(value.to_owned())
    }
}
impl From<&&str> for Param {
    fn from(value: &&str) -> Self {
        Param::Text((*value).to_owned())
    }
}
impl From<&String> for Param {
    fn from(value: &String) -> Self {
        Param::Text(value.clone())
    }
}
impl From<String> for Param {
    fn from(value: String) -> Self {
        Param::Text(value)
    }
}
impl From<i64> for Param {
    fn from(value: i64) -> Self {
        Param::Int(value)
    }
}
impl From<bool> for Param {
    fn from(value: bool) -> Self {
        Param::Int(value as i64)
    }
}
impl<T: Into<Param>> From<Option<T>> for Param {
    fn from(value: Option<T>) -> Self {
        value.map_or(Param::Null, Into::into)
    }
}

/// `params![a, b, c]` builds the bind list for [`Tx`] queries.
#[macro_export]
macro_rules! params {
    ($($value:expr),* $(,)?) => {
        [$($crate::db::Param::from($value)),*]
    };
}

enum RowKind {
    Sqlite(sqlx::sqlite::SqliteRow),
    MySql(sqlx::mysql::MySqlRow),
}

/// One result row, read by column index with an explicit expected type.
pub struct Row(RowKind);

impl Row {
    pub fn text(&self, index: usize) -> Result<String> {
        self.opt_text(index)?
            .ok_or_else(|| ApiError::new(500, "STORAGE_ERROR", "保存内容を読み取れませんでした"))
    }
    pub fn opt_text(&self, index: usize) -> Result<Option<String>> {
        match &self.0 {
            RowKind::Sqlite(row) => row.try_get(index).map_err(decode_error),
            RowKind::MySql(row) => row.try_get(index).map_err(decode_error),
        }
    }
    pub fn int(&self, index: usize) -> Result<i64> {
        match &self.0 {
            RowKind::Sqlite(row) => row.try_get(index).map_err(decode_error),
            // COUNT/EXISTS come back as unsigned or decimal on some servers.
            RowKind::MySql(row) => row
                .try_get::<i64, _>(index)
                .or_else(|_| row.try_get::<u64, _>(index).map(|v| v as i64))
                .map_err(decode_error),
        }
    }
    pub fn bool(&self, index: usize) -> Result<bool> {
        Ok(self.int(index)? != 0)
    }
    /// Column type name, used only by diagnostics and tests.
    pub fn column_type(&self, index: usize) -> String {
        match &self.0 {
            RowKind::Sqlite(row) => row.column(index).type_info().name().to_owned(),
            RowKind::MySql(row) => row.column(index).type_info().name().to_owned(),
        }
    }
}

/// Reports the TLS mode a DSN would connect with, for tests.
pub fn ssl_mode_for_test(url: &str) -> Result<String> {
    Ok(format!("{:?}", mysql_options(url)?.get_ssl_mode()))
}

/// Exposes [`redact`] so a test can assert on it directly.
pub fn redact_for_test(message: &str) -> String {
    redact(message)
}

/// Removes anything that looks like connection credentials from a message.
///
/// Driver errors are useful in an operator's hands and dangerous in a log, so
/// the password in a DSN is replaced before the text goes anywhere.
pub(crate) fn redact(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(scheme) = rest.find("://") {
        let (head, tail) = rest.split_at(scheme + 3);
        out.push_str(head);
        // `user:password@host` — keep the user, drop the secret.
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(tail.len());
        let (authority, remainder) = tail.split_at(end);
        match (authority.find('@'), authority.find(':')) {
            (Some(at), Some(colon)) if colon < at => {
                out.push_str(&authority[..colon]);
                out.push_str(":***");
                out.push_str(&authority[at..]);
            }
            _ => out.push_str(authority),
        }
        rest = remainder;
    }
    out.push_str(rest);
    out
}

fn decode_error(error: sqlx::Error) -> ApiError {
    ApiError::new(
        500,
        "STORAGE_ERROR",
        &redact(&format!("保存内容を読み取れませんでした: {error}")),
    )
}

// TiDB write conflict (9007) and MySQL deadlock/lock timeout (1213 / 1205)
// mean another execution environment won the race, not that the request was
// malformed.
const CONFLICT_CODES: [&str; 3] = ["9007", "1213", "1205"];

pub(crate) fn storage_error(error: sqlx::Error) -> ApiError {
    // Surface an integrity violation as a conflict; the business rules above
    // this layer already produce their own 409s for the cases they check.
    if let sqlx::Error::Database(database) = &error {
        if database.is_unique_violation()
            || database.is_foreign_key_violation()
            || database
                .code()
                .is_some_and(|code| CONFLICT_CODES.contains(&code.as_ref()))
        {
            return ApiError::new(
                409,
                "STORAGE_CONFLICT",
                "他の操作と競合しました。最新の内容を確認してください",
            );
        }
    }
    ApiError::new(
        500,
        "STORAGE_ERROR",
        &redact(&format!(
            "保存処理に失敗しました。再試行してください: {error}"
        )),
    )
}

enum TxKind {
    Sqlite(sqlx::Transaction<'static, sqlx::Sqlite>),
    MySql(sqlx::Transaction<'static, sqlx::MySql>),
}

/// An open database transaction. Every read and write the business services
/// perform for one request goes through the same `Tx`, so the change, its
/// approval record, its audit row, and its idempotency result commit together.
pub struct Tx {
    kind: TxKind,
    dialect: Dialect,
    write: bool,
}

// Every statement in this crate is a compile-time literal or a string built
// from literals by `Dialect`; values always arrive as bound parameters and are
// never interpolated into the statement.
macro_rules! bind_all {
    ($sql:expr, $params:expr) => {{
        let mut query = sqlx::query($sql);
        for param in $params {
            query = match param {
                Param::Text(value) => query.bind(value.clone()),
                Param::Int(value) => query.bind(*value),
                Param::Null => query.bind(Option::<String>::None),
            };
        }
        query
    }};
}

impl Tx {
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Suffix that turns a read into a locking read.
    ///
    /// A TiDB pessimistic transaction answers a plain `SELECT` from the
    /// snapshot taken when the transaction began, so a writer that waited on
    /// [`Tx::lock_workspace`] would still decide against the state it saw
    /// before the other writer committed. `FOR UPDATE` reads the latest
    /// committed row instead. SQLite needs nothing: `BEGIN IMMEDIATE` already
    /// excluded the other writer before the first read.
    pub fn lock_reads(&self) -> &'static str {
        if self.write && self.dialect == Dialect::MySql {
            " FOR UPDATE"
        } else {
            ""
        }
    }

    pub async fn fetch_all(&mut self, sql: &str, params: &[Param]) -> Result<Vec<Row>> {
        match &mut self.kind {
            TxKind::Sqlite(tx) => Ok(bind_all!(sql, params)
                .fetch_all(&mut **tx)
                .await
                .map_err(storage_error)?
                .into_iter()
                .map(|row| Row(RowKind::Sqlite(row)))
                .collect()),
            TxKind::MySql(tx) => Ok(bind_all!(sql, params)
                .fetch_all(&mut **tx)
                .await
                .map_err(storage_error)?
                .into_iter()
                .map(|row| Row(RowKind::MySql(row)))
                .collect()),
        }
    }

    pub async fn fetch_optional(&mut self, sql: &str, params: &[Param]) -> Result<Option<Row>> {
        match &mut self.kind {
            TxKind::Sqlite(tx) => Ok(bind_all!(sql, params)
                .fetch_optional(&mut **tx)
                .await
                .map_err(storage_error)?
                .map(|row| Row(RowKind::Sqlite(row)))),
            TxKind::MySql(tx) => Ok(bind_all!(sql, params)
                .fetch_optional(&mut **tx)
                .await
                .map_err(storage_error)?
                .map(|row| Row(RowKind::MySql(row)))),
        }
    }

    pub async fn fetch_one(&mut self, sql: &str, params: &[Param]) -> Result<Row> {
        self.fetch_optional(sql, params)
            .await?
            .ok_or_else(ApiError::missing)
    }

    /// Returns the number of rows the statement changed.
    pub async fn execute(&mut self, sql: &str, params: &[Param]) -> Result<u64> {
        match &mut self.kind {
            TxKind::Sqlite(tx) => Ok(bind_all!(sql, params)
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?
                .rows_affected()),
            TxKind::MySql(tx) => Ok(bind_all!(sql, params)
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?
                .rows_affected()),
        }
    }

    /// Serializes concurrent writers of one workspace.
    ///
    /// SQLite already holds the single write lock from `BEGIN IMMEDIATE`, so
    /// read-then-write sequences are safe there. TiDB's pessimistic
    /// transactions only lock rows that a statement actually modifies, so two
    /// execution environments could both read version 1 and both write
    /// version 2. Taking the workspace row exclusively before any read gives
    /// the same serialization the business rules were written against.
    pub async fn lock_workspace(&mut self, workspace: &str) -> Result<()> {
        if self.dialect == Dialect::MySql && !workspace.is_empty() {
            self.fetch_optional(
                "SELECT id FROM workspaces WHERE id=? FOR UPDATE",
                &crate::params![workspace],
            )
            .await?;
        }
        Ok(())
    }

    pub async fn savepoint(&mut self, name: &str) -> Result<()> {
        let [open, _, _] = self.dialect.savepoint(name);
        self.execute(&open, &[]).await?;
        Ok(())
    }
    /// Undo everything since the savepoint and release it, leaving the
    /// surrounding transaction usable.
    pub async fn rollback_to_savepoint(&mut self, name: &str) -> Result<()> {
        let [_, rollback, release] = self.dialect.savepoint(name);
        self.execute(&rollback, &[]).await?;
        self.execute(&release, &[]).await?;
        Ok(())
    }

    pub async fn commit(self) -> Result<()> {
        match self.kind {
            TxKind::Sqlite(tx) => tx.commit().await.map_err(storage_error),
            TxKind::MySql(tx) => tx.commit().await.map_err(storage_error),
        }
    }
}

#[derive(Clone)]
enum PoolKind {
    Sqlite(SqlitePool),
    /// The pool, and the options to open a connection *outside* it.
    ///
    /// The migration lock has to be held by one session for the whole run, and
    /// taking that session from the pool would spend one of the very few
    /// connections an execution environment has — with the default of two, a
    /// migrating cold start would then starve its own transactions.
    MySql(MySqlPool, Box<sqlx::mysql::MySqlConnectOptions>),
}

/// Advisory lock prefix held for the whole migration run; the database name
/// is appended so deployments sharing a cluster do not serialize.
const MIGRATION_LOCK: &str = "pathbase:migrate";

/// The migration lock, and the one connection that holds it.
///
/// A MySQL advisory lock belongs to the session that took it. Keeping the
/// connection here is what makes the release actually release: handing the
/// name back to the pool would run `RELEASE_LOCK` on whichever connection came
/// up, which is usually not the one holding it.
struct MigrationLock {
    name: String,
    connection: sqlx::MySqlConnection,
}

impl MigrationLock {
    /// Releases the lock, and says so if it could not.
    ///
    /// `SELECT RELEASE_LOCK(?)`, not `DO RELEASE_LOCK(?)`: the `DO` form is
    /// accepted but observably does not release the lock when sqlx sends it as
    /// a prepared statement, which left every migrating process holding the
    /// lock for the life of its pooled connection. A failure here is not fatal
    /// — the lock expires with the session — but it makes the next process
    /// wait, so it is worth seeing in the logs rather than swallowing.
    async fn release(mut self) {
        let released = sqlx::query_scalar::<_, i64>("SELECT RELEASE_LOCK(?)")
            .bind(&self.name)
            .fetch_one(&mut self.connection)
            .await;
        if !matches!(released, Ok(1)) {
            eprintln!(
                "warning: the migration lock was not released ({released:?}); \
                 the next process to open this database may wait for it"
            );
        }
        // Closing the session releases anything still held, whatever happened
        // above.
        let _ = sqlx::Connection::close(self.connection).await;
    }
}
/// Primary key of the single `database_identity` row.
const IDENTITY_ROW: &str = "singleton";

/// A connection pool plus the dialect its statements must be written in.
#[derive(Clone)]
pub struct Db {
    pool: PoolKind,
    dialect: Dialect,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the pool: its options carry the database password.
        f.debug_struct("Db")
            .field("dialect", &self.dialect)
            .finish()
    }
}

const MIGRATIONS: &[(i64, &str, &str, &str)] = &[
    (
        1,
        "initial schema",
        include_str!("../migrations/sqlite/0001_initial.sql"),
        include_str!("../migrations/mysql/0001_initial.sql"),
    ),
    (
        2,
        "database identity",
        include_str!("../migrations/sqlite/0002_identity.sql"),
        include_str!("../migrations/mysql/0002_identity.sql"),
    ),
    (
        3,
        "mcp connections",
        include_str!("../migrations/sqlite/0003_mcp_connections.sql"),
        include_str!("../migrations/mysql/0003_mcp_connections.sql"),
    ),
    (
        4,
        "audit connection",
        include_str!("../migrations/sqlite/0004_audit_connection.sql"),
        include_str!("../migrations/mysql/0004_audit_connection.sql"),
    ),
    (
        5,
        "oauth clients and grants",
        include_str!("../migrations/sqlite/0005_oauth_clients.sql"),
        include_str!("../migrations/mysql/0005_oauth_clients.sql"),
    ),
    (
        6,
        "browser sessions",
        include_str!("../migrations/sqlite/0006_sessions.sql"),
        include_str!("../migrations/mysql/0006_sessions.sql"),
    ),
];

/// The schema version this build expects. Readiness compares against it, so a
/// candidate whose migration did not run cannot be promoted.
pub fn expected_schema_version() -> i64 {
    MIGRATIONS
        .iter()
        .map(|(version, ..)| *version)
        .max()
        .unwrap_or(0)
}

/// Value used when no deployment label is configured: local development, and
/// a deployment whose manifest overlay has not been applied yet.
pub const UNLABELLED_ENVIRONMENT: &str = "local-preview";

/// Which deployment this process believes it is serving. Production and each
/// per-PR preview declare their own value in the Cloud App manifest.
pub fn configured_environment() -> String {
    std::env::var("PATHBASE_DB_ENVIRONMENT")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| UNLABELLED_ENVIRONMENT.into())
}

fn setting(key: &str, fallback: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn mysql_options(url: &str) -> Result<sqlx::mysql::MySqlConnectOptions> {
    use sqlx::mysql::{MySqlConnectOptions, MySqlSslMode};
    use std::str::FromStr;
    let options = MySqlConnectOptions::from_str(url).map_err(|error| {
        ApiError::new(
            500,
            "STORAGE_ERROR",
            &redact(&format!("DSNを解釈できません: {error}")),
        )
    })?;
    // A DSN that names its own TLS mode wins. Tachyon issues the managed
    // Cloud App DSN with `?ssl-mode=REQUIRED` because TiDB Serverless requires
    // TLS, and this process must not quietly weaken it.
    let declared = url.contains("ssl-mode=") || url.contains("sslmode=");
    let configured = std::env::var("PATHBASE_DB_SSL_MODE")
        .ok()
        .filter(|value| !value.trim().is_empty());
    if declared && configured.is_none() {
        return Ok(options);
    }
    // Otherwise `preferred` keeps TLS on wherever the server offers it,
    // without breaking a local cluster that serves none.
    let mode = configured.unwrap_or_else(|| "preferred".into());
    let mode = match mode.trim().to_ascii_lowercase().as_str() {
        "disabled" => MySqlSslMode::Disabled,
        "preferred" => MySqlSslMode::Preferred,
        "required" => MySqlSslMode::Required,
        "verify_ca" | "verify-ca" => MySqlSslMode::VerifyCa,
        "verify_identity" | "verify-identity" => MySqlSslMode::VerifyIdentity,
        other => {
            return Err(ApiError::new(
                500,
                "STORAGE_ERROR",
                &format!("PATHBASE_DB_SSL_MODE '{other}' は使用できません"),
            ))
        }
    };
    Ok(options.ssl_mode(mode))
}

/// What a readiness probe needs to decide whether this process may serve.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SchemaStatus {
    pub storage: &'static str,
    pub durability: &'static str,
    pub schema_version: i64,
    pub expected_schema_version: i64,
    pub environment: String,
    pub database_environment: Option<String>,
}

impl SchemaStatus {
    pub fn is_ready(&self) -> bool {
        self.schema_version == self.expected_schema_version
            && self.database_environment.is_some()
            && !self.environment_conflicts()
    }

    /// Whether this process still has to write its deployment label.
    pub fn needs_claim(&self) -> bool {
        match self.database_environment.as_deref() {
            None => true,
            Some(recorded) => {
                recorded != self.environment && self.environment != UNLABELLED_ENVIRONMENT
            }
        }
    }

    /// True only when two *labelled* deployments disagree.
    ///
    /// An unlabelled side is a missing configuration, not a mix-up: a process
    /// started before its manifest overlay was applied must not take the
    /// deployment down, and it must not repurpose a database either.
    pub fn environment_conflicts(&self) -> bool {
        let Some(recorded) = self.database_environment.as_deref() else {
            return false;
        };
        recorded != self.environment
            && recorded != UNLABELLED_ENVIRONMENT
            && self.environment != UNLABELLED_ENVIRONMENT
    }
}

impl Db {
    /// Opens the database named by a URL.
    ///
    /// `mysql://…` selects TiDB; `sqlite://…` or a bare filesystem path
    /// selects the local preview database.
    pub async fn connect(url: &str) -> Result<Self> {
        if url.starts_with("mysql://") || url.starts_with("mariadb://") {
            let options = mysql_options(url)?;
            let pool = sqlx::mysql::MySqlPoolOptions::new()
                // One Lambda execution environment serves one request at a
                // time, so a small pool is enough; the ceiling that matters is
                // `max_connections × reserved concurrency` against the TiDB
                // connection limit. Raise it deliberately, not by default.
                .max_connections(setting("PATHBASE_DB_MAX_CONNECTIONS", 2) as u32)
                // A frozen execution environment holds no connection open.
                .min_connections(0)
                .acquire_timeout(Duration::from_secs(setting(
                    "PATHBASE_DB_CONNECT_TIMEOUT_SECS",
                    10,
                )))
                .idle_timeout(Duration::from_secs(setting(
                    "PATHBASE_DB_IDLE_TIMEOUT_SECS",
                    300,
                )))
                // Recycle so a failed-over TiDB node cannot be pinned forever.
                .max_lifetime(Duration::from_secs(setting(
                    "PATHBASE_DB_MAX_LIFETIME_SECS",
                    600,
                )))
                // Lambda freeze/thaw can leave a connection the server has
                // already dropped; check before handing it to a request.
                .test_before_acquire(true)
                .connect_with(options)
                .await
                .map_err(|error| {
                    ApiError::new(
                        503,
                        "DATABASE_UNAVAILABLE",
                        &redact(&format!("業務データベースへ接続できません: {error}")),
                    )
                })?;
            return Ok(Self {
                pool: PoolKind::MySql(pool, Box::new(mysql_options(url)?)),
                dialect: Dialect::MySql,
            });
        }
        let options = sqlite_options(url)?;
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(options)
            .await
            .map_err(|error| {
                ApiError::new(
                    500,
                    "STORAGE_ERROR",
                    &format!("ローカル保存先を開けません: {error}"),
                )
            })?;
        Ok(Self {
            pool: PoolKind::Sqlite(pool),
            dialect: Dialect::Sqlite,
        })
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// A transaction for read-only work.
    pub async fn begin_read(&self) -> Result<Tx> {
        match &self.pool {
            PoolKind::Sqlite(pool) => Ok(Tx {
                kind: TxKind::Sqlite(pool.begin().await.map_err(storage_error)?),
                dialect: self.dialect,
                write: false,
            }),
            PoolKind::MySql(pool, _) => Ok(Tx {
                kind: TxKind::MySql(pool.begin().await.map_err(storage_error)?),
                dialect: self.dialect,
                write: false,
            }),
        }
    }

    /// A transaction that will write. SQLite takes its write lock immediately
    /// so a read-then-write request cannot fail late with SQLITE_BUSY.
    pub async fn begin_write(&self) -> Result<Tx> {
        match &self.pool {
            PoolKind::Sqlite(pool) => Ok(Tx {
                kind: TxKind::Sqlite(
                    pool.begin_with("BEGIN IMMEDIATE")
                        .await
                        .map_err(storage_error)?,
                ),
                dialect: self.dialect,
                write: true,
            }),
            PoolKind::MySql(pool, _) => Ok(Tx {
                kind: TxKind::MySql(pool.begin().await.map_err(storage_error)?),
                dialect: self.dialect,
                write: true,
            }),
        }
    }

    /// Applies every migration this build knows about, then claims the
    /// database for the configured deployment environment.
    ///
    /// DDL is **not** rolled back with the surrounding DML on TiDB, so each
    /// statement is applied on its own and the version is recorded only after
    /// the whole file succeeded. Re-running an applied migration is a no-op.
    ///
    /// Several Lambda execution environments can start at once, so the whole
    /// run is held under a database-wide advisory lock; concurrent starters
    /// wait and then find the work already done.
    pub async fn migrate(&self) -> Result<()> {
        // The common case is a warm schema: skip the lock round trips when
        // there is nothing to apply. A missing table makes the probe fail,
        // which is itself the signal to run.
        if let Ok(status) = self.schema_status().await {
            if status.is_ready() && !status.needs_claim() {
                return Ok(());
            }
        }
        let lock = self.lock_migrations().await?;
        let result = self.migrate_locked().await;
        if let Some(lock) = lock {
            lock.release().await;
        }
        result
    }

    /// Advisory lock name.
    ///
    /// MySQL advisory locks are server-wide, not per database, so the name
    /// carries the database: two deployments sharing one TiDB cluster (a
    /// per-PR preview and production) must not serialize against each other.
    async fn migration_lock_name(connection: &mut sqlx::MySqlConnection) -> Result<String> {
        let database: Option<String> = sqlx::query_scalar("SELECT DATABASE()")
            .fetch_one(&mut *connection)
            .await
            .map_err(storage_error)?;
        let database = database.unwrap_or_default();
        // MySQL caps lock names at 64 characters.
        Ok(format!("{MIGRATION_LOCK}:{database}")
            .chars()
            .take(64)
            .collect())
    }

    async fn lock_migrations(&self) -> Result<Option<MigrationLock>> {
        let PoolKind::MySql(_, options) = &self.pool else {
            return Ok(None);
        };
        // The lock is held by the *session* that took it, so acquiring and
        // releasing must happen on one connection. Taking it from the pool
        // twice can land on two, and then the release quietly does nothing
        // while the original connection keeps the lock for as long as the pool
        // keeps it warm — long enough for the next process to start, wait the
        // full timeout, and fail its readiness check.
        let mut connection = <sqlx::MySqlConnection as sqlx::Connection>::connect_with(options)
            .await
            .map_err(storage_error)?;
        let name = Self::migration_lock_name(&mut connection).await?;
        let wait = setting("PATHBASE_DB_MIGRATION_LOCK_SECS", 60) as i64;
        let mut last = None;
        // Heavy contention makes TiDB answer GET_LOCK with a retryable
        // pessimistic-lock error rather than a plain "not acquired".
        for attempt in 0..5 {
            match sqlx::query_scalar::<_, i64>("SELECT GET_LOCK(?, ?)")
                .bind(&name)
                .bind(wait)
                .fetch_one(&mut connection)
                .await
            {
                Ok(1) => return Ok(Some(MigrationLock { name, connection })),
                Ok(_) => {
                    return Err(ApiError::new(
                        503,
                        "MIGRATION_LOCK_TIMEOUT",
                        "他のインスタンスがmigrationを実行中です",
                    ))
                }
                Err(error) => {
                    last = Some(error);
                    tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
                }
            }
        }
        Err(storage_error(
            last.expect("a failed attempt records its error"),
        ))
    }

    async fn migrate_locked(&self) -> Result<()> {
        let mut tx = self.begin_write().await?;
        let create = match self.dialect {
            Dialect::Sqlite => {
                "CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL)"
            }
            Dialect::MySql => {
                "CREATE TABLE IF NOT EXISTS schema_migrations(version BIGINT NOT NULL PRIMARY KEY, name VARCHAR(191) NOT NULL, applied_at VARCHAR(40) NOT NULL) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin"
            }
        };
        tx.execute(create, &[]).await?;
        tx.commit().await?;

        for (version, name, sqlite, mysql) in MIGRATIONS {
            let mut tx = self.begin_write().await?;
            let applied = tx
                .fetch_optional(
                    "SELECT version FROM schema_migrations WHERE version=?",
                    &crate::params![*version],
                )
                .await?
                .is_some();
            tx.commit().await?;
            if applied {
                continue;
            }
            let script = match self.dialect {
                Dialect::Sqlite => sqlite,
                Dialect::MySql => mysql,
            };
            for statement in script.split(";\n").filter_map(strip_comments) {
                let mut tx = self.begin_write().await?;
                tx.execute(&statement, &[]).await?;
                tx.commit().await?;
            }
            let mut tx = self.begin_write().await?;
            tx.execute(
                &self
                    .dialect
                    .insert_ignore("schema_migrations", &["version", "name", "applied_at"]),
                &crate::params![*version, *name, crate::service::now()],
            )
            .await?;
            tx.commit().await?;
        }
        self.claim_environment().await
    }

    /// Records the deployment this database serves, or refuses if another one
    /// already claimed it.
    async fn claim_environment(&self) -> Result<()> {
        let environment = configured_environment();
        let mut tx = self.begin_write().await?;
        let recorded = tx
            .fetch_optional(
                &format!(
                    "SELECT environment FROM database_identity WHERE id=?{}",
                    tx.lock_reads()
                ),
                &crate::params![IDENTITY_ROW],
            )
            .await?
            .map(|row| row.text(0))
            .transpose()?;
        match recorded {
            // Two labelled deployments disagreeing is a real mix-up.
            Some(existing)
                if existing != environment
                    && existing != UNLABELLED_ENVIRONMENT
                    && environment != UNLABELLED_ENVIRONMENT =>
            {
                return Err(ApiError::new(
                    500,
                    "DATABASE_ENVIRONMENT_MISMATCH",
                    &format!(
                        "このデータベースは environment '{existing}' のものです。\
                         '{environment}' として使用できません"
                    ),
                ));
            }
            // The database was claimed before its manifest overlay existed;
            // adopt the label now rather than failing the rollout.
            Some(existing) if existing == UNLABELLED_ENVIRONMENT && existing != environment => {
                tx.execute(
                    "UPDATE database_identity SET environment=?,claimed_at=? WHERE id=?",
                    &crate::params![environment, crate::service::now(), IDENTITY_ROW],
                )
                .await?;
            }
            Some(_) => {}
            None => {
                tx.execute(
                    &self
                        .dialect
                        .insert_ignore("database_identity", &["id", "environment", "claimed_at"]),
                    &crate::params![IDENTITY_ROW, environment, crate::service::now()],
                )
                .await?;
            }
        }
        tx.commit().await
    }

    /// Reads what a readiness probe needs: reachability, applied schema, and
    /// whether this database belongs to the deployment asking.
    pub async fn schema_status(&self) -> Result<SchemaStatus> {
        let mut tx = self.begin_read().await?;
        let schema_version = tx
            .fetch_optional("SELECT MAX(version) FROM schema_migrations", &[])
            .await?
            .map(|row| row.int(0))
            .transpose()
            .unwrap_or(None)
            .unwrap_or(0);
        let database_environment = tx
            .fetch_optional(
                "SELECT environment FROM database_identity WHERE id=?",
                &crate::params![IDENTITY_ROW],
            )
            .await?
            .map(|row| row.text(0))
            .transpose()?;
        tx.commit().await?;
        Ok(SchemaStatus {
            storage: match self.dialect {
                Dialect::MySql => "tidb",
                Dialect::Sqlite => "sqlite",
            },
            durability: match self.dialect {
                Dialect::MySql => "shared-durable",
                Dialect::Sqlite => "ephemeral-runtime",
            },
            schema_version,
            expected_schema_version: expected_schema_version(),
            environment: configured_environment(),
            database_environment,
        })
    }

    pub async fn close(&self) {
        match &self.pool {
            PoolKind::Sqlite(pool) => pool.close().await,
            PoolKind::MySql(pool, _) => pool.close().await,
        }
    }
}

/// Drops `--` comment lines and returns `None` when nothing executable is left.
fn strip_comments(statement: &str) -> Option<String> {
    let sql = statement
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let sql = sql.trim().to_owned();
    (!sql.is_empty()).then_some(sql)
}

fn sqlite_options(url: &str) -> Result<sqlx::sqlite::SqliteConnectOptions> {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
    use std::str::FromStr;
    let options = if url.starts_with("sqlite:") {
        SqliteConnectOptions::from_str(url)
            .map_err(|error| ApiError::new(500, "STORAGE_ERROR", &error.to_string()))?
    } else {
        let path = std::path::Path::new(url);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|_| ApiError::new(500, "STORAGE_ERROR", "保存先を作成できません"))?;
        }
        SqliteConnectOptions::new().filename(path)
    };
    Ok(options
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5)))
}

/// Resolves the database for the current runtime mode.
///
/// Production never falls back to a local file: an unset or unreachable
/// `DATABASE_URL` is a startup error, because a silent SQLite fallback would
/// accept writes that the next execution environment cannot see.
pub async fn connect_from_env(local_preview: bool) -> Result<Db> {
    let configured = ["PATHBASE_DATABASE_URL", "DATABASE_URL"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty());
    match (configured, local_preview) {
        (Some(url), _) => Db::connect(&url).await,
        (None, true) => {
            let path =
                std::env::var("PATHBASE_DB").unwrap_or_else(|_| "data/pathbase.sqlite3".into());
            Db::connect(&path).await
        }
        (None, false) => Err(ApiError::new(
            500,
            "DATABASE_NOT_CONFIGURED",
            "DATABASE_URL が設定されていません。本番ではローカルSQLiteへ切り替えません",
        )),
    }
}
