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

fn decode_error(error: sqlx::Error) -> ApiError {
    ApiError::new(
        500,
        "STORAGE_ERROR",
        &format!("保存内容を読み取れませんでした: {error}"),
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
        &format!("保存処理に失敗しました。再試行してください: {error}"),
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
// from literals by `Dialect`; values always arrive as bound parameters. sqlx
// still needs the promise explicitly because the string is not `'static`.
macro_rules! bind_all {
    ($sql:expr, $params:expr) => {{
        let mut query = sqlx::query(sqlx::AssertSqlSafe($sql.to_owned()));
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
    MySql(MySqlPool),
}

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

const MIGRATIONS: &[(i64, &str, &str, &str)] = &[(
    1,
    "initial schema",
    include_str!("../migrations/sqlite/0001_initial.sql"),
    include_str!("../migrations/mysql/0001_initial.sql"),
)];

impl Db {
    /// Opens the database named by a URL.
    ///
    /// `mysql://…` selects TiDB; `sqlite://…` or a bare filesystem path
    /// selects the local preview database.
    pub async fn connect(url: &str) -> Result<Self> {
        if url.starts_with("mysql://") || url.starts_with("mariadb://") {
            let pool = sqlx::mysql::MySqlPoolOptions::new()
                // A Lambda execution environment handles one request at a time,
                // and TiDB is shared by many of them.
                .max_connections(
                    std::env::var("PATHBASE_DB_MAX_CONNECTIONS")
                        .ok()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(4),
                )
                .acquire_timeout(Duration::from_secs(
                    std::env::var("PATHBASE_DB_CONNECT_TIMEOUT_SECS")
                        .ok()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(10),
                ))
                .max_lifetime(Duration::from_secs(600))
                .connect(url)
                .await
                .map_err(|error| {
                    ApiError::new(
                        503,
                        "DATABASE_UNAVAILABLE",
                        &format!("業務データベースへ接続できません: {error}"),
                    )
                })?;
            return Ok(Self {
                pool: PoolKind::MySql(pool),
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
            PoolKind::MySql(pool) => Ok(Tx {
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
            PoolKind::MySql(pool) => Ok(Tx {
                kind: TxKind::MySql(pool.begin().await.map_err(storage_error)?),
                dialect: self.dialect,
                write: true,
            }),
        }
    }

    /// Applies every migration this build knows about.
    ///
    /// DDL is **not** rolled back with the surrounding DML on TiDB, so each
    /// statement is applied on its own and the version is recorded only after
    /// the whole file succeeded. Re-running an applied migration is a no-op.
    pub async fn migrate(&self) -> Result<()> {
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
        Ok(())
    }

    pub async fn close(&self) {
        match &self.pool {
            PoolKind::Sqlite(pool) => pool.close().await,
            PoolKind::MySql(pool) => pool.close().await,
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
