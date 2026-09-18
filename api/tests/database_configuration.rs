//! Startup rules for choosing a database.
//!
//! These run in their own test binary because they mutate process environment
//! variables; a single `#[tokio::test]` keeps them serialized.
use pathbase_api::db::{connect_from_env, Db, Dialect};

#[tokio::test]
async fn production_requires_an_explicit_database_and_never_falls_back_to_sqlite() {
    for key in ["PATHBASE_DATABASE_URL", "DATABASE_URL", "PATHBASE_DB"] {
        std::env::remove_var(key);
    }

    // Production: an unset database is a startup error, not a local file.
    let error = connect_from_env(false).await.unwrap_err();
    assert_eq!(error.code, "DATABASE_NOT_CONFIGURED");

    // Even with a local SQLite path present, production must not use it.
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("PATHBASE_DB", dir.path().join("fallback.sqlite3"));
    let error = connect_from_env(false).await.unwrap_err();
    assert_eq!(error.code, "DATABASE_NOT_CONFIGURED");
    assert!(
        !dir.path().join("fallback.sqlite3").exists(),
        "production must not create a local database"
    );

    // Local preview may use it, and says so.
    let db = connect_from_env(true).await.unwrap();
    assert_eq!(db.dialect(), Dialect::Sqlite);
    db.close().await;

    // An unreachable production database fails loudly rather than degrading.
    std::env::set_var("PATHBASE_DB_CONNECT_TIMEOUT_SECS", "1");
    std::env::set_var(
        "DATABASE_URL",
        "mysql://pathbase@127.0.0.1:1/pathbase_unreachable",
    );
    let error = connect_from_env(false).await.unwrap_err();
    assert_eq!(error.code, "DATABASE_UNAVAILABLE");
    std::env::remove_var("DATABASE_URL");
    std::env::remove_var("PATHBASE_DB");
    std::env::remove_var("PATHBASE_DB_CONNECT_TIMEOUT_SECS");
}

#[tokio::test]
async fn dialects_render_their_own_upsert_and_ignore_statements() {
    let columns = ["workspace_id", "collection", "id", "body", "seq"];
    let keys = ["workspace_id", "collection", "id"];

    let sqlite = Dialect::Sqlite.upsert("documents", &columns, &keys, &["body"]);
    assert!(
        sqlite.contains("ON CONFLICT(workspace_id,collection,id) DO UPDATE SET body=excluded.body")
    );
    assert!(
        !sqlite.contains("seq="),
        "an update must not disturb the insertion-order column"
    );

    let mysql = Dialect::MySql.upsert("documents", &columns, &keys, &["body"]);
    assert!(mysql.contains("ON DUPLICATE KEY UPDATE body=VALUES(body)"));
    assert!(!mysql.contains("seq=VALUES(seq)"));

    assert!(Dialect::Sqlite
        .insert_ignore("memberships", &["workspace_id", "actor", "role"])
        .starts_with("INSERT OR IGNORE INTO memberships"));
    assert!(Dialect::MySql
        .insert_ignore("memberships", &["workspace_id", "actor", "role"])
        .starts_with("INSERT IGNORE INTO memberships"));
}

#[tokio::test]
async fn a_local_preview_database_reports_the_sqlite_dialect() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&dir.path().join("preview.sqlite3").to_string_lossy())
        .await
        .unwrap();
    db.migrate().await.unwrap();
    // Re-running migrations on an existing database is a no-op.
    db.migrate().await.unwrap();
    let mut tx = db.begin_read().await.unwrap();
    let rows = tx
        .fetch_all("SELECT version FROM schema_migrations", &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    tx.commit().await.unwrap();
    db.close().await;
}
