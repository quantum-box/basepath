//! Moving a SQLite database into TiDB, and proving the result is usable.
//!
//! Needs a real TiDB (`PATHBASE_TEST_DATABASE_URL`); see `tests/tidb.rs`.
//! Without it every test here skips.
use pathbase_api::{
    db::Db,
    migrate::{inventory, migrate_data, validate_integrity, Check},
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

struct Tidb {
    url: String,
}

impl Tidb {
    async fn new() -> Option<Self> {
        let base = std::env::var("PATHBASE_TEST_DATABASE_URL").ok()?;
        let base = base.trim().to_owned();
        if base.is_empty() {
            return None;
        }
        Some(Self { url: base })
    }

    /// A freshly created, empty database with the schema applied.
    async fn database(&self) -> (String, Db) {
        let admin = sqlx::MySqlPool::connect(&self.url).await.unwrap();
        let name = format!("pathbase_mig_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE `{name}`"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let url = match self.url.rsplit_once('/') {
            Some((prefix, _)) => format!("{prefix}/{name}"),
            None => format!("{}/{name}", self.url),
        };
        let db = Db::connect(&url).await.unwrap();
        db.migrate().await.unwrap();
        (url, db)
    }
}

macro_rules! tidb {
    () => {
        match Tidb::new().await {
            Some(fixture) => fixture,
            None => {
                eprintln!("skipped: set PATHBASE_TEST_DATABASE_URL to run the migration suite");
                return;
            }
        }
    };
}

async fn call(
    service: &Service,
    who: &Actor,
    method: &str,
    path: &str,
    body: Value,
    key: Option<&str>,
) -> Result<Value> {
    service
        .handle(who, method, path, &HashMap::new(), body, key)
        .await
}

fn key() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn failed(checks: &[Check]) -> Vec<&Check> {
    checks.iter().filter(|check| !check.passed).collect()
}

/// A local preview database with demo data plus a few real operations, which
/// is the shape an operator would be migrating from.
async fn populated_source(directory: &std::path::Path) -> (String, Db, Service) {
    let path = directory
        .join("source.sqlite3")
        .to_string_lossy()
        .into_owned();
    let service = Service::open(&path).await.unwrap();
    service.initialize(true).await.unwrap();
    let who = Actor::local();
    let goal = call(
        &service,
        &who,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"移行する目標","kind":"outcome"}),
        Some(&key()),
    )
    .await
    .unwrap();
    call(
        &service,
        &who,
        "PATCH",
        &format!(
            "/v1/workspaces/personal/items/{}",
            goal["id"].as_str().unwrap()
        ),
        json!({"expected_version":1,"description":"更新も移行する"}),
        Some(&key()),
    )
    .await
    .unwrap();
    call(
        &service,
        &who,
        "PATCH",
        "/v1/settings",
        json!({"compact":true,"notifications":false,"timezone":"Asia/Tokyo"}),
        Some(&key()),
    )
    .await
    .unwrap();
    let db = Db::connect(&path).await.unwrap();
    (path, db, service)
}

#[tokio::test]
async fn a_dry_run_reports_without_writing() {
    let fixture = tidb!();
    let directory = tempfile::tempdir().unwrap();
    let (_path, source, _service) = populated_source(directory.path()).await;
    let (_url, target) = fixture.database().await;

    let report = migrate_data(&source, &target, true).await.unwrap();
    assert!(report.dry_run);
    assert!(report.succeeded(), "{:?}", failed(&report.checks));
    assert!(report.source.total_rows > 0);
    assert!(report.target_before.is_empty());
    assert!(report.target_after.is_none());
    // Nothing was written.
    assert!(inventory(&target).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_migration_preserves_counts_content_and_business_rules() {
    let fixture = tidb!();
    let directory = tempfile::tempdir().unwrap();
    let (_path, source, _service) = populated_source(directory.path()).await;
    let (url, target) = fixture.database().await;

    let report = migrate_data(&source, &target, false).await.unwrap();
    assert!(!report.dry_run);
    assert!(report.succeeded(), "{:?}", failed(&report.checks));

    let after = report.target_after.as_ref().unwrap();
    assert_eq!(after.total_rows, report.source.total_rows);
    assert_eq!(after.digest, report.source.digest);
    for (table, source_table) in &report.source.tables {
        assert_eq!(
            after.tables[table], *source_table,
            "{table} differs after the copy"
        );
    }

    // The migrated database is usable, not just identical: representative
    // reads and a write both work, and history survived.
    let migrated = Service::new(Db::connect(&url).await.unwrap());
    let who = Actor::local();
    let items = call(
        &migrated,
        &who,
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
        None,
    )
    .await
    .unwrap();
    let moved = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["title"] == "移行する目標")
        .expect("the migrated goal is readable");
    assert_eq!(moved["version"], 2, "the update survived the migration");
    assert_eq!(moved["description"], "更新も移行する");

    let settings = call(&migrated, &who, "GET", "/v1/settings", json!({}), None)
        .await
        .unwrap();
    assert_eq!(settings["compact"], true);

    let audit = call(
        &migrated,
        &who,
        "GET",
        "/v1/workspaces/personal/audit",
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert!(
        !audit.as_array().unwrap().is_empty(),
        "the audit trail survived the migration"
    );

    // Writing continues from the migrated state rather than restarting.
    let updated = call(
        &migrated,
        &who,
        "PATCH",
        &format!(
            "/v1/workspaces/personal/items/{}",
            moved["id"].as_str().unwrap()
        ),
        json!({"expected_version":2,"title":"移行後に更新"}),
        Some(&key()),
    )
    .await
    .unwrap();
    assert_eq!(updated["version"], 3);
}

#[tokio::test]
async fn a_migration_refuses_a_target_that_already_holds_data() {
    let fixture = tidb!();
    let directory = tempfile::tempdir().unwrap();
    let (_path, source, _service) = populated_source(directory.path()).await;
    let (url, target) = fixture.database().await;

    migrate_data(&source, &target, false).await.unwrap();
    // A second run must not merge into the live database.
    let second = migrate_data(&source, &target, false).await.unwrap();
    assert!(!second.succeeded());
    assert!(
        failed(&second.checks)
            .iter()
            .any(|check| check.name == "target_empty"),
        "{:?}",
        second.checks
    );
    // And it left the target untouched.
    let unchanged = inventory(&Db::connect(&url).await.unwrap()).await.unwrap();
    assert_eq!(unchanged.digest, second.target_before.digest);
}

#[tokio::test]
async fn an_empty_source_records_that_no_migration_is_needed() {
    let fixture = tidb!();
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join("empty.sqlite3")
        .to_string_lossy()
        .into_owned();
    let source = Db::connect(&path).await.unwrap();
    source.migrate().await.unwrap();
    let (_url, target) = fixture.database().await;

    let report = migrate_data(&source, &target, false).await.unwrap();
    assert!(report.succeeded(), "{:?}", failed(&report.checks));
    assert!(report.source.is_empty());
    assert!(
        report
            .checks
            .iter()
            .any(|check| check.name == "migration_needed" && check.passed),
        "an empty source must be recorded as evidence, not assumed"
    );
}

#[tokio::test]
async fn a_broken_reference_fails_the_migration_instead_of_shipping_it() {
    let fixture = tidb!();
    let directory = tempfile::tempdir().unwrap();
    let (path, source, _service) = populated_source(directory.path()).await;
    let (_url, target) = fixture.database().await;

    // Corrupt the source the way a partial export would.
    {
        let db = Db::connect(&path).await.unwrap();
        let mut tx = db.begin_write().await.unwrap();
        tx.execute(
            "DELETE FROM memberships WHERE workspace_id='personal' AND role='owner'",
            &[],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        db.close().await;
    }

    let report = migrate_data(&source, &target, false).await.unwrap();
    assert!(!report.succeeded());
    assert!(
        failed(&report.checks)
            .iter()
            .any(|check| check.name == "source_workspace_has_owner"),
        "{:?}",
        failed(&report.checks)
    );
    // A failing validation must not have written anything.
    assert!(inventory(&target).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_restored_copy_matches_the_original_and_still_serves() {
    let fixture = tidb!();
    let directory = tempfile::tempdir().unwrap();
    let (_path, source, _service) = populated_source(directory.path()).await;
    let (origin_url, origin) = fixture.database().await;
    migrate_data(&source, &origin, false).await.unwrap();
    let before = inventory(&origin).await.unwrap();

    // Restore into a separate, isolated database. Copying table by table is
    // the same shape as a logical restore, and lets the test assert on the
    // result rather than on the backup tool.
    let (restored_url, restored) = fixture.database().await;
    let report = migrate_data(&origin, &restored, false).await.unwrap();
    assert!(report.succeeded(), "{:?}", failed(&report.checks));

    let after = inventory(&restored).await.unwrap();
    assert_eq!(after.total_rows, before.total_rows);
    assert_eq!(after.digest, before.digest);
    assert!(
        failed(&validate_integrity(&restored).await.unwrap()).is_empty(),
        "the restored copy must satisfy the same invariants"
    );
    assert_ne!(origin_url, restored_url, "the restore is isolated");

    // Representative operations work against the restored copy.
    let service = Service::new(Db::connect(&restored_url).await.unwrap());
    let who = Actor::local();
    let snapshot = call(
        &service,
        &who,
        "GET",
        "/v1/workspaces/personal/snapshot",
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert!(!snapshot["items"].as_array().unwrap().is_empty());
    let created = call(
        &service,
        &who,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"復元後に作成","kind":"action"}),
        Some(&key()),
    )
    .await
    .unwrap();
    assert!(created["id"].is_string());
}
