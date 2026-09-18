//! What has to stay true across redeploys, cold starts, and a database outage.
//!
//! Needs a real TiDB (`PATHBASE_TEST_DATABASE_URL`); see `tests/tidb.rs`.
use pathbase_api::{
    db::{redact_for_test, Db},
    migrate::inventory,
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn fresh_database() -> Option<String> {
    let base = std::env::var("PATHBASE_TEST_DATABASE_URL").ok()?;
    let base = base.trim().to_owned();
    if base.is_empty() {
        return None;
    }
    let admin = sqlx::MySqlPool::connect(&base).await.unwrap();
    let name = format!("pathbase_dur_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE `{name}`"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    Some(match base.rsplit_once('/') {
        Some((prefix, _)) => format!("{prefix}/{name}"),
        None => format!("{base}/{name}"),
    })
}

macro_rules! database {
    () => {
        match fresh_database().await {
            Some(url) => url,
            None => {
                eprintln!("skipped: set PATHBASE_TEST_DATABASE_URL to run the durability suite");
                return;
            }
        }
    };
}

async fn call(
    service: &Service,
    method: &str,
    path: &str,
    body: Value,
    key: Option<&str>,
) -> Result<Value> {
    service
        .handle(&Actor::local(), method, path, &HashMap::new(), body, key)
        .await
}

fn key() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A new `Service` over a new pool: what a redeploy or a cold start produces.
async fn instance(url: &str) -> Service {
    Service::open(url).await.unwrap()
}

// One test, run in order. Pool size, TLS mode and the database URL are all
// process-wide environment variables, so sibling tests in this binary would
// race each other's configuration.
#[tokio::test]
async fn shared_storage_survives_redeploys_outages_and_restores() {
    let url = database!();

    // Deployment 1 seeds and writes.
    let first = instance(&url).await;
    first.initialize(false).await.unwrap();
    let goal = call(
        &first,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"耐久性の確認","kind":"outcome"}),
        Some(&key()),
    )
    .await
    .unwrap();
    let id = goal["id"].as_str().unwrap().to_owned();
    let before = inventory(&Db::connect(&url).await.unwrap()).await.unwrap();
    // The process goes away, as a redeployed Lambda's does.
    first.db.close().await;
    drop(first);

    // Deployment 2 sees the same data and history.
    let second = instance(&url).await;
    let item = call(
        &second,
        "GET",
        &format!("/v1/workspaces/personal/items/{id}"),
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(item["title"], "耐久性の確認");
    assert_eq!(item["version"], 1);
    let audit = call(
        &second,
        "GET",
        "/v1/workspaces/personal/audit",
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert!(
        audit
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["command"]
                .as_str()
                .is_some_and(|c| c.contains("items"))),
        "the audit trail crossed the redeploy"
    );

    // A third, concurrently running instance sees deployment 2's update.
    let third = instance(&url).await;
    call(
        &second,
        "PATCH",
        &format!("/v1/workspaces/personal/items/{id}"),
        json!({"expected_version":1,"title":"別インスタンスからの更新"}),
        Some(&key()),
    )
    .await
    .unwrap();
    let seen = call(
        &third,
        "GET",
        &format!("/v1/workspaces/personal/items/{id}"),
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(seen["title"], "別インスタンスからの更新");
    assert_eq!(seen["version"], 2);

    // Nothing was lost along the way.
    let after = inventory(&Db::connect(&url).await.unwrap()).await.unwrap();
    assert!(after.total_rows >= before.total_rows);

    // --- an outage fails loudly and a retry does not duplicate ------------
    let url = database!();
    let service = instance(&url).await;
    service.initialize(false).await.unwrap();

    let idempotency_key = key();
    let body = json!({"title":"障害中に送った作成","kind":"action"});

    // The database goes away mid-flight.
    service.db.close().await;
    let error = call(
        &service,
        "POST",
        "/v1/workspaces/personal/items",
        body.clone(),
        Some(&idempotency_key),
    )
    .await
    .expect_err("a write against an unavailable database must fail, not appear to succeed");
    assert!(
        error.status >= 500,
        "an outage is a server-side failure: {error:?}"
    );
    assert!(
        !error.message.contains("://"),
        "an error must not carry the connection string: {}",
        error.message
    );

    // After recovery, retrying the same request creates the row exactly once.
    let recovered = instance(&url).await;
    let created = call(
        &recovered,
        "POST",
        "/v1/workspaces/personal/items",
        body.clone(),
        Some(&idempotency_key),
    )
    .await
    .unwrap();
    let replay = call(
        &recovered,
        "POST",
        "/v1/workspaces/personal/items",
        body,
        Some(&idempotency_key),
    )
    .await
    .unwrap();
    assert_eq!(created, replay, "the retry replays rather than duplicating");

    let items = call(
        &recovered,
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
        None,
    )
    .await
    .unwrap();
    let matching = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["title"] == "障害中に送った作成")
        .count();
    assert_eq!(matching, 1, "exactly one row survives the outage and retry");

    // --- pool exhaustion fails the request instead of hanging -------------
    // One connection and a short wait: a request that cannot get a connection
    // has to give up rather than hold the invocation open.
    let exhausted = database!();
    std::env::set_var("PATHBASE_DB_MAX_CONNECTIONS", "1");
    std::env::set_var("PATHBASE_DB_CONNECT_TIMEOUT_SECS", "1");
    let service = instance(&exhausted).await;
    service.initialize(false).await.unwrap();
    let held = service.db.begin_write().await.unwrap();
    let error = call(
        &service,
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
        None,
    )
    .await
    .expect_err("a request that cannot get a connection must fail");
    assert!(error.status >= 500, "{error:?}");
    assert!(!error.message.contains("://"), "{}", error.message);
    drop(held);
    std::env::remove_var("PATHBASE_DB_MAX_CONNECTIONS");
    std::env::remove_var("PATHBASE_DB_CONNECT_TIMEOUT_SECS");

    credentials_never_reach_a_message();
    a_dsn_that_requires_tls_is_not_weakened();
}

fn credentials_never_reach_a_message() {
    let message = redact_for_test(
        "業務データベースへ接続できません: error with mysql://pathbase:s3cr3t@10.0.0.1:4000/app",
    );
    assert!(!message.contains("s3cr3t"), "{message}");
    assert!(
        message.contains("mysql://pathbase:***@10.0.0.1:4000/app"),
        "{message}"
    );
    // A message without a DSN is unchanged.
    assert_eq!(redact_for_test("plain failure"), "plain failure");
}

fn a_dsn_that_requires_tls_is_not_weakened() {
    use pathbase_api::db::ssl_mode_for_test;
    // Tachyon issues the managed Cloud App DSN with `?ssl-mode=REQUIRED`
    // because TiDB Serverless requires TLS. This process must not quietly
    // weaken it to the local-friendly default.
    std::env::remove_var("PATHBASE_DB_SSL_MODE");
    assert_eq!(
        ssl_mode_for_test("mysql://u:p@host:4000/db?ssl-mode=REQUIRED").unwrap(),
        "Required"
    );
    assert_eq!(
        ssl_mode_for_test("mysql://u:p@host:4000/db?ssl-mode=VERIFY_IDENTITY").unwrap(),
        "VerifyIdentity"
    );
    // A DSN that says nothing gets the local-friendly default.
    assert_eq!(
        ssl_mode_for_test("mysql://u:p@host:4000/db").unwrap(),
        "Preferred"
    );
    // An explicit setting still overrides, for an operator who needs it.
    std::env::set_var("PATHBASE_DB_SSL_MODE", "verify_ca");
    assert_eq!(
        ssl_mode_for_test("mysql://u:p@host:4000/db?ssl-mode=REQUIRED").unwrap(),
        "VerifyCa"
    );
    std::env::set_var("PATHBASE_DB_SSL_MODE", "nonsense");
    assert!(ssl_mode_for_test("mysql://u:p@host:4000/db").is_err());
    std::env::remove_var("PATHBASE_DB_SSL_MODE");
}
