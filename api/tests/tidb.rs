//! Shared-database behaviour that only a real TiDB can demonstrate.
//!
//! The SQLite preview database serializes every writer with one file lock, so
//! it cannot show whether the business contract survives two execution
//! environments updating the same workspace. These tests therefore run against
//! a real TiDB and assert it is TiDB — a plain MySQL server has different
//! locking behaviour and must not stand in for this check.
//!
//! Set `PATHBASE_TEST_DATABASE_URL` (for example
//! `mysql://root@127.0.0.1:4000/test`) to enable them; without it every test
//! skips with a printed note.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

/// A throwaway database plus a factory for independent `Service` instances.
struct Fixture {
    url: String,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let base = std::env::var("PATHBASE_TEST_DATABASE_URL").ok()?;
        let base = base.trim().to_owned();
        if base.is_empty() {
            return None;
        }
        let admin = sqlx::MySqlPool::connect(&base)
            .await
            .expect("PATHBASE_TEST_DATABASE_URL must point at a reachable server");
        // Refuse to let a plain MySQL stand in for TiDB.
        let version: String = sqlx::query_scalar("SELECT tidb_version()")
            .fetch_one(&admin)
            .await
            .expect("PATHBASE_TEST_DATABASE_URL must point at TiDB, not MySQL");
        assert!(
            version.contains("TiDB") || version.contains("Release Version"),
            "unexpected server banner: {version}"
        );
        let name = format!("pathbase_test_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE `{name}`"))
            .execute(&admin)
            .await
            .expect("the test account must be allowed to create a database");
        admin.close().await;
        let url = match base.rsplit_once('/') {
            Some((prefix, _)) => format!("{prefix}/{name}"),
            None => format!("{base}/{name}"),
        };
        Some(Self { url })
    }

    /// A `Service` with its own connection pool, standing in for a separate
    /// Lambda execution environment.
    async fn service(&self) -> Service {
        Service::open(&self.url).await.expect("open TiDB service")
    }
}

macro_rules! tidb {
    () => {
        match Fixture::new().await {
            Some(fixture) => fixture,
            None => {
                eprintln!("skipped: set PATHBASE_TEST_DATABASE_URL to run the TiDB suite");
                return;
            }
        }
    };
}

fn actor(id: &str) -> Actor {
    Actor {
        id: id.into(),
        agent: false,
        connection: None,
    }
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

async fn new_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Creates the local sample workspaces so the shared-database tests have a
/// workspace and an owner to work with.
async fn seeded(fixture: &Fixture) -> (Service, Actor) {
    let service = fixture.service().await;
    service.initialize(false).await.unwrap();
    (service, Actor::local())
}

#[tokio::test]
async fn migrations_apply_fresh_rerun_and_upgrade() {
    let fixture = tidb!();
    // Fresh apply.
    let first = fixture.service().await;
    // Re-running the same version must be a no-op rather than an error, and a
    // second process opening the same database must not duplicate rows.
    let second = fixture.service().await;
    for service in [&first, &second] {
        let mut tx = service.db.begin_read().await.unwrap();
        let versions = tx
            .fetch_all(
                "SELECT version FROM schema_migrations ORDER BY version",
                &[],
            )
            .await
            .unwrap();
        let applied: Vec<i64> = versions.iter().map(|row| row.int(0).unwrap()).collect();
        assert_eq!(
            applied,
            (1..=pathbase_api::db::expected_schema_version()).collect::<Vec<_>>(),
            "one row per applied migration"
        );
        // Every table the service writes to must exist.
        for table in [
            "database_identity",
            "workspaces",
            "memberships",
            "documents",
            "idempotency",
            "settings",
            "audit",
            "invitations",
        ] {
            tx.fetch_optional(&format!("SELECT 1 FROM `{table}` LIMIT 1"), &[])
                .await
                .unwrap_or_else(|error| panic!("missing table {table}: {}", error.message));
        }
        tx.commit().await.unwrap();
    }
    // An upgrade over a populated database keeps the data.
    first.initialize(false).await.unwrap();
    let third = fixture.service().await;
    let workspaces = call(
        &third,
        &Actor::local(),
        "GET",
        "/v1/workspaces",
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(workspaces.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn expected_version_conflict_between_independent_services() {
    let fixture = tidb!();
    let (writer, who) = seeded(&fixture).await;
    let item = call(
        &writer,
        &who,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"目標","kind":"outcome"}),
        Some(&new_key().await),
    )
    .await
    .unwrap();
    let id = item["id"].as_str().unwrap().to_owned();

    // Two separate pools, as two Lambda execution environments would be.
    let a = fixture.service().await;
    let b = fixture.service().await;
    let path = format!("/v1/workspaces/personal/items/{id}");
    let (key_a, key_b) = (new_key().await, new_key().await);
    let (left, right) = tokio::join!(
        call(
            &a,
            &who,
            "PATCH",
            &path,
            json!({"expected_version":1,"title":"A"}),
            Some(&key_a),
        ),
        call(
            &b,
            &who,
            "PATCH",
            &path,
            json!({"expected_version":1,"title":"B"}),
            Some(&key_b),
        )
    );
    let results = [left, right];
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "exactly one writer may win: {results:?}"
    );
    let error = results.iter().find_map(|r| r.as_ref().err()).unwrap();
    assert_eq!(error.status, 409);
    assert!(
        ["VERSION_CONFLICT", "STORAGE_CONFLICT"].contains(&error.code.as_str()),
        "unexpected loser code: {}",
        error.code
    );
    // The surviving state is one of the two writes at version 2, never both.
    let current = call(&a, &who, "GET", &path, json!({}), None).await.unwrap();
    assert_eq!(current["version"], 2);
    assert!(["A", "B"].contains(&current["title"].as_str().unwrap()));
}

#[tokio::test]
async fn idempotency_is_shared_across_service_instances() {
    let fixture = tidb!();
    let (writer, who) = seeded(&fixture).await;
    let a = fixture.service().await;
    let b = fixture.service().await;
    let key = new_key().await;
    let body = json!({"title":"共有キー","kind":"outcome"});

    let first = call(
        &a,
        &who,
        "POST",
        "/v1/workspaces/personal/items",
        body.clone(),
        Some(&key),
    )
    .await
    .unwrap();
    // The replay reaches a different pool and must return the stored response.
    let replay = call(
        &b,
        &who,
        "POST",
        "/v1/workspaces/personal/items",
        body,
        Some(&key),
    )
    .await
    .unwrap();
    assert_eq!(first, replay);

    // A different input under the same key is a conflict, not a second item.
    let conflict = call(
        &b,
        &who,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"別の入力","kind":"outcome"}),
        Some(&key),
    )
    .await
    .unwrap_err();
    assert_eq!(conflict.code, "IDEMPOTENCY_CONFLICT");

    let items = call(
        &writer,
        &who,
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn changeset_apply_is_atomic_and_cannot_be_applied_twice() {
    let fixture = tidb!();
    let (service, who) = seeded(&fixture).await;
    let base = "/v1/workspaces/personal";
    let goal = call(
        &service,
        &who,
        "POST",
        &format!("{base}/items"),
        json!({"title":"親","kind":"outcome"}),
        Some(&new_key().await),
    )
    .await
    .unwrap();

    let preview = call(
        &service,
        &who,
        "POST",
        &format!("{base}/changesets/preview"),
        json!({"title":"二件の追加","operations":[
            {"method":"POST","path":format!("{base}/items"),"body":{"title":"子1","kind":"action"}},
            {"method":"POST","path":format!("{base}/items"),"body":{"title":"子2","kind":"action"}}
        ]}),
        Some(&new_key().await),
    )
    .await
    .unwrap();
    let change_id = preview["id"].as_str().unwrap().to_owned();
    call(
        &service,
        &who,
        "POST",
        &format!("{base}/changesets/{change_id}/approve"),
        json!({}),
        Some(&new_key().await),
    )
    .await
    .unwrap();

    // Apply from one instance, then try to replay it from another.
    let applier = fixture.service().await;
    let other = fixture.service().await;
    call(
        &applier,
        &who,
        "POST",
        &format!("{base}/changesets/{change_id}/apply"),
        json!({}),
        Some(&new_key().await),
    )
    .await
    .unwrap();
    let second = call(
        &other,
        &who,
        "POST",
        &format!("{base}/changesets/{change_id}/apply"),
        json!({}),
        Some(&new_key().await),
    )
    .await
    .unwrap_err();
    assert_eq!(second.status, 409);

    let items = call(
        &service,
        &who,
        "GET",
        &format!("{base}/items"),
        json!({}),
        None,
    )
    .await
    .unwrap();
    // The parent plus exactly one copy of each child.
    assert_eq!(items["items"].as_array().unwrap().len(), 3);

    // A batch whose last operation is invalid must leave nothing behind.
    let before = items["items"].as_array().unwrap().len();
    let rejected = call(
        &service,
        &who,
        "POST",
        &format!("{base}/changesets/preview"),
        json!({"title":"壊れた案","operations":[
            {"method":"POST","path":format!("{base}/items"),"body":{"title":"作られてはいけない","kind":"action"}},
            {"method":"PATCH","path":format!("{base}/items/{}", goal["id"].as_str().unwrap()),"body":{"expected_version":99,"title":"衝突"}}
        ]}),
        Some(&new_key().await),
    )
    .await;
    assert!(rejected.is_err(), "the preview must reject the whole batch");
    let after = call(
        &service,
        &who,
        "GET",
        &format!("{base}/items"),
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(after["items"].as_array().unwrap().len(), before);
}

#[tokio::test]
async fn shared_workspace_never_loses_its_last_owner() {
    let fixture = tidb!();
    let service = fixture.service().await;
    let owner = actor("us_owner");
    let workspace = call(
        &service,
        &owner,
        "POST",
        "/v1/workspaces",
        json!({"name":"チームA","scope":"チーム"}),
        Some(&new_key().await),
    )
    .await
    .unwrap();
    let id = workspace["id"].as_str().unwrap().to_owned();
    let version = workspace["version"].as_i64().unwrap();

    // Two instances try to drop the only owner at the same time.
    let a = fixture.service().await;
    let b = fixture.service().await;
    let (key_a, key_b) = (new_key().await, new_key().await);
    let remove_path = format!("/v1/workspaces/{id}/members/us_owner");
    let leave_path = format!("/v1/workspaces/{id}/leave");
    let (left, right) = tokio::join!(
        call(
            &a,
            &owner,
            "DELETE",
            &remove_path,
            json!({"expected_version":version}),
            Some(&key_a),
        ),
        call(
            &b,
            &owner,
            "POST",
            &leave_path,
            json!({"expected_version":version}),
            Some(&key_b),
        )
    );
    assert!(left.is_err(), "the last owner cannot be removed: {left:?}");
    assert!(right.is_err(), "the last owner cannot leave: {right:?}");
    let members = call(
        &service,
        &owner,
        "GET",
        &format!("/v1/workspaces/{id}/members"),
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(members["members"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn concurrent_relations_cannot_commit_a_cycle() {
    let fixture = tidb!();
    let (service, who) = seeded(&fixture).await;
    let base = "/v1/workspaces/personal";
    let mut ids = Vec::new();
    for title in ["A", "B"] {
        let item = call(
            &service,
            &who,
            "POST",
            &format!("{base}/items"),
            json!({"title":title,"kind":"initiative"}),
            Some(&new_key().await),
        )
        .await
        .unwrap();
        ids.push(item["id"].as_str().unwrap().to_owned());
    }
    let a = fixture.service().await;
    let b = fixture.service().await;
    let (key_a, key_b) = (new_key().await, new_key().await);
    let relations = format!("{base}/relations");
    let forward = json!({"source_id":ids[0],"target_id":ids[1],"type":"depends_on"});
    let backward = json!({"source_id":ids[1],"target_id":ids[0],"type":"depends_on"});
    let (left, right) = tokio::join!(
        call(&a, &who, "POST", &relations, forward, Some(&key_a)),
        call(&b, &who, "POST", &relations, backward, Some(&key_b))
    );
    let results = [left, right];
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "only one direction may exist: {results:?}"
    );
    let graph = call(
        &service,
        &who,
        "GET",
        &format!("{base}/graph"),
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(graph["relations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn revoked_membership_rejects_a_replayed_request() {
    let fixture = tidb!();
    let service = fixture.service().await;
    let owner = actor("us_owner");
    let guest = actor("us_guest");
    let workspace = call(
        &service,
        &owner,
        "POST",
        "/v1/workspaces",
        json!({"name":"チームB","scope":"チーム"}),
        Some(&new_key().await),
    )
    .await
    .unwrap();
    let id = workspace["id"].as_str().unwrap().to_owned();
    let base = format!("/v1/workspaces/{id}");

    let invitation = call(
        &service,
        &owner,
        "POST",
        &format!("{base}/invitations"),
        json!({"target_actor":"us_guest","role":"editor","expected_version":workspace["version"]}),
        Some(&new_key().await),
    )
    .await
    .unwrap();
    call(
        &service,
        &guest,
        "POST",
        &format!(
            "/v1/invitations/{}/accept",
            invitation["id"].as_str().unwrap()
        ),
        json!({"expected_version":1}),
        Some(&new_key().await),
    )
    .await
    .unwrap();

    let key = new_key().await;
    let body = json!({"title":"ゲストの項目","kind":"action"});
    let created = call(
        &service,
        &guest,
        "POST",
        &format!("{base}/items"),
        body.clone(),
        Some(&key),
    )
    .await
    .unwrap();
    assert!(created["id"].is_string());

    // Revoke the guest from another instance, then replay the stored key.
    let admin = fixture.service().await;
    let current = call(
        &admin,
        &owner,
        "GET",
        &format!("{base}/members"),
        json!({}),
        None,
    )
    .await
    .unwrap();
    call(
        &admin,
        &owner,
        "DELETE",
        &format!("{base}/members/us_guest"),
        json!({"expected_version":current["workspace"]["version"]}),
        Some(&new_key().await),
    )
    .await
    .unwrap();

    let replay = call(
        &fixture.service().await,
        &guest,
        "POST",
        &format!("{base}/items"),
        body,
        Some(&key),
    )
    .await
    .unwrap_err();
    assert_eq!(
        replay.status, 404,
        "a revoked member must not replay a stored response"
    );
}

#[tokio::test]
async fn simultaneous_cold_starts_migrate_exactly_once() {
    let fixture = tidb!();
    // Several Lambda execution environments can start against a freshly
    // provisioned database at the same time. The advisory lock has to make
    // them converge instead of racing the DDL.
    let starts = (0..4)
        .map(|_| {
            let url = fixture.url.clone();
            tokio::spawn(async move { Service::open(&url).await.map(|_| ()) })
        })
        .collect::<Vec<_>>();
    for start in starts {
        start
            .await
            .unwrap()
            .expect("a simultaneous cold start failed");
    }

    let service = fixture.service().await;
    let mut tx = service.db.begin_read().await.unwrap();
    let rows = tx
        .fetch_all(
            "SELECT version FROM schema_migrations ORDER BY version",
            &[],
        )
        .await
        .unwrap();
    let versions: Vec<i64> = rows.iter().map(|row| row.int(0).unwrap()).collect();
    assert_eq!(
        versions.len(),
        versions
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "each migration must be recorded once: {versions:?}"
    );
    let identity = tx
        .fetch_all("SELECT id FROM database_identity", &[])
        .await
        .unwrap();
    assert_eq!(identity.len(), 1, "the database is claimed exactly once");
    tx.commit().await.unwrap();

    let status = service.db.schema_status().await.unwrap();
    assert!(status.is_ready(), "{status:?}");
    assert_eq!(status.storage, "tidb");
    assert_eq!(status.durability, "shared-durable");
}
