//! The tenant boundary: what a person reaches when they switch tenants.
//!
//! This file exists because the answer used to be "the same thing". Selecting
//! a Tachyon tenant changed a value in the session and the Field integration's
//! filter, and nothing else: the personal workspace id was derived from the
//! user id alone, `GET /v1/workspaces` returned every membership an actor
//! held, and `authorize` asked only for a role. Two tenants, one set of data.
//!
//! So these are not tests that sharing works. They are tests that a tenant is
//! a boundary — that the same person, with the same memberships, reaches
//! different data in each tenant, and that nothing carries across:
//!
//! 1. **Identity is (tenant, actor), not actor.** The personal workspace of
//!    one person in two tenants is two workspaces.
//! 2. **A workspace id is not a capability.** Holding one from another tenant
//!    gets the same answer as holding one that does not exist — 404, not 403,
//!    because 403 would confirm it is real.
//! 3. **A membership does not travel.** Being a member in one tenant means
//!    nothing while acting in another, including for an invitation that was
//!    genuinely addressed to you.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

const ALPHA: &str = "tn_alpha";
const BETA: &str = "tn_beta";

fn acting_in(tenant: &str, id: &str) -> Actor {
    Actor {
        id: id.into(),
        tenant: tenant.into(),
        agent: false,
        connection: None,
    }
}

async fn service() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("tenants.sqlite3").to_string_lossy())
        .await
        .unwrap();
    (dir, service)
}

async fn get(s: &Service, who: &Actor, path: &str) -> Result<Value> {
    s.handle(who, "GET", path, &HashMap::new(), json!({}), None)
        .await
}

async fn post(s: &Service, who: &Actor, path: &str, body: Value, key: &str) -> Result<Value> {
    s.handle(who, "POST", path, &HashMap::new(), body, Some(key))
        .await
}

/// The id the same person's personal workspace gets in each tenant.
async fn personal(s: &Service, who: &Actor) -> String {
    s.provision_personal(who).await.unwrap();
    get(s, who, "/v1/workspaces")
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["scope"] == "個人")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The failure that started this: one person, two tenants, one set of data.
#[tokio::test]
async fn the_same_person_gets_a_separate_personal_workspace_in_each_tenant() {
    let (_dir, s) = service().await;
    let alpha = acting_in(ALPHA, "us_alice");
    let beta = acting_in(BETA, "us_alice");

    let in_alpha = personal(&s, &alpha).await;
    let in_beta = personal(&s, &beta).await;
    assert_ne!(
        in_alpha, in_beta,
        "the personal workspace must be derived from the tenant as well as the person"
    );

    // Provisioning again is still idempotent *within* a tenant: switching
    // back returns the workspace that was already there rather than a third.
    assert_eq!(personal(&s, &alpha).await, in_alpha);

    // And what was written in one tenant is not in the other.
    post(
        &s,
        &alpha,
        &format!("/v1/workspaces/{in_alpha}/items"),
        json!({"title":"アルファ社の売上目標","kind":"outcome"}),
        "item-alpha",
    )
    .await
    .unwrap();
    let items = get(&s, &beta, &format!("/v1/workspaces/{in_beta}/items"))
        .await
        .unwrap();
    assert_eq!(
        items["items"].as_array().map(Vec::len),
        Some(0),
        "an item created in one tenant must not appear in another"
    );
}

/// Every tenant's workspace list holds only that tenant's workspaces.
#[tokio::test]
async fn the_workspace_list_never_mixes_tenants() {
    let (_dir, s) = service().await;
    let alpha = acting_in(ALPHA, "us_alice");
    let beta = acting_in(BETA, "us_alice");
    personal(&s, &alpha).await;
    personal(&s, &beta).await;

    post(
        &s,
        &alpha,
        "/v1/workspaces",
        json!({"name":"アルファ営業部","scope":"チーム"}),
        "ws-alpha",
    )
    .await
    .unwrap();

    let names = |list: &Value| -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|w| w["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let in_alpha = get(&s, &alpha, "/v1/workspaces").await.unwrap();
    let in_beta = get(&s, &beta, "/v1/workspaces").await.unwrap();
    assert!(names(&in_alpha).contains(&"アルファ営業部".to_owned()));
    assert!(!names(&in_beta).contains(&"アルファ営業部".to_owned()));
    assert_eq!(
        in_beta.as_array().unwrap().len(),
        1,
        "only its own personal"
    );

    // Each workspace says which tenant it belongs to, and says its own.
    for entry in in_alpha.as_array().unwrap() {
        assert_eq!(entry["tenant_id"], ALPHA);
    }
}

/// A workspace id from another tenant is not a way in, and does not even
/// confirm that it exists.
#[tokio::test]
async fn a_workspace_id_from_another_tenant_reads_as_missing() {
    let (_dir, s) = service().await;
    let alpha = acting_in(ALPHA, "us_alice");
    let beta = acting_in(BETA, "us_alice");
    let in_alpha = personal(&s, &alpha).await;
    personal(&s, &beta).await;

    for path in [
        format!("/v1/workspaces/{in_alpha}/items"),
        format!("/v1/workspaces/{in_alpha}/snapshot"),
        format!("/v1/workspaces/{in_alpha}/members"),
    ] {
        let error = get(&s, &beta, &path).await.unwrap_err();
        assert_eq!(
            error.status, 404,
            "{path} must be indistinguishable from a workspace that does not exist"
        );
    }

    let error = post(
        &s,
        &beta,
        &format!("/v1/workspaces/{in_alpha}/items"),
        json!({"title":"横から書き込む","kind":"outcome"}),
        "cross-write",
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, 404, "a write must not reach across tenants");

    // The same id, from the tenant it belongs to, still works — so the 404
    // above is the boundary and not a broken id.
    assert!(get(&s, &alpha, &format!("/v1/workspaces/{in_alpha}/items"))
        .await
        .is_ok());
}

/// An invitation is only redeemable inside the workspace's own tenant.
///
/// Issuing one cannot check the recipient's tenants — PathBase knows which
/// Tachyon tenants the *inviter* belongs to, not the invitee. Accepting can,
/// because the person accepting is the one making the request.
#[tokio::test]
async fn an_invitation_cannot_be_accepted_from_another_tenant() {
    let (_dir, s) = service().await;
    let owner = acting_in(ALPHA, "us_alice");
    personal(&s, &owner).await;
    let shared = post(
        &s,
        &owner,
        "/v1/workspaces",
        json!({"name":"アルファ営業部","scope":"チーム"}),
        "ws",
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let version = get(&s, &owner, &format!("/v1/workspaces/{shared}/members"))
        .await
        .unwrap()["workspace"]["version"]
        .as_i64()
        .unwrap();
    let invitation = post(
        &s,
        &owner,
        &format!("/v1/workspaces/{shared}/invitations"),
        json!({"target_actor":"us_bob","role":"editor","expected_version":version}),
        "invite",
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Bob, acting in a tenant this workspace does not belong to. The
    // invitation is genuinely his, and still gets him nothing.
    let bob_elsewhere = acting_in(BETA, "us_bob");
    personal(&s, &bob_elsewhere).await;
    assert_eq!(
        get(&s, &bob_elsewhere, "/v1/invitations")
            .await
            .unwrap()
            .as_array()
            .map(Vec::len),
        Some(0),
        "a pending invitation from another tenant must not be listed"
    );
    let error = post(
        &s,
        &bob_elsewhere,
        &format!("/v1/invitations/{invitation}/accept"),
        json!({"expected_version": 1}),
        "accept-elsewhere",
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, 404);

    // The same invitation, accepted from the tenant it was issued in, works.
    let bob = acting_in(ALPHA, "us_bob");
    personal(&s, &bob).await;
    assert_eq!(
        get(&s, &bob, "/v1/invitations")
            .await
            .unwrap()
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    post(
        &s,
        &bob,
        &format!("/v1/invitations/{invitation}/accept"),
        json!({"expected_version": 1}),
        "accept",
    )
    .await
    .unwrap();
    assert!(get(&s, &bob, &format!("/v1/workspaces/{shared}/items"))
        .await
        .is_ok());

    // Membership does not travel with him back across the boundary.
    assert_eq!(
        get(
            &s,
            &bob_elsewhere,
            &format!("/v1/workspaces/{shared}/items")
        )
        .await
        .unwrap_err()
        .status,
        404
    );
}

/// Acting without having chosen a tenant reaches nothing, rather than
/// reaching everything.
#[tokio::test]
async fn an_actor_with_no_tenant_reaches_nothing() {
    let (_dir, s) = service().await;
    let alpha = acting_in(ALPHA, "us_alice");
    let in_alpha = personal(&s, &alpha).await;

    let nobody = acting_in("", "us_alice");
    assert_eq!(
        get(&s, &nobody, "/v1/workspaces")
            .await
            .unwrap()
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        get(&s, &nobody, &format!("/v1/workspaces/{in_alpha}/items"))
            .await
            .unwrap_err()
            .status,
        404
    );
    // And it cannot create one either: a workspace with no tenant would be a
    // row nothing could ever reach.
    assert_eq!(
        s.provision_personal(&nobody).await.unwrap_err().code,
        "TENANT_SELECTION_REQUIRED"
    );
}

/// What happens to a database that predates the boundary.
///
/// Migration 0007 discards the workspaces it finds rather than attributing
/// them to a tenant it would have to guess (user decision, 2026-09-19). That
/// is a destructive step, so it is worth showing rather than describing: a
/// database built at the previous schema version comes back with the column,
/// with none of the old rows, and — importantly — with nothing left behind in
/// the tables that referenced them.
#[tokio::test]
async fn upgrading_a_database_from_before_the_boundary_discards_its_workspaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite3");
    let url = format!("sqlite://{}?mode=rwc", path.to_string_lossy());

    // A database at the schema this change was written against, holding a
    // workspace that belongs to nobody in particular.
    {
        use sqlx::Executor;
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        for script in [
            include_str!("../migrations/sqlite/0001_initial.sql"),
            include_str!("../migrations/sqlite/0002_identity.sql"),
            include_str!("../migrations/sqlite/0003_mcp_connections.sql"),
            include_str!("../migrations/sqlite/0004_audit_connection.sql"),
            include_str!("../migrations/sqlite/0005_oauth_clients.sql"),
            include_str!("../migrations/sqlite/0006_sessions.sql"),
        ] {
            for statement in script.split(";\n") {
                let sql: String = statement
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("--"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !sql.trim().is_empty() {
                    pool.execute(sql.trim()).await.unwrap();
                }
            }
        }
        pool.execute(
            "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL)",
        )
        .await
        .unwrap();
        for version in 1..=6 {
            sqlx::query("INSERT INTO schema_migrations VALUES(?,'legacy','2026-09-13T00:00:00Z')")
                .bind(version)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO workspaces(id,body,seq) VALUES('ws_legacy','{\"id\":\"ws_legacy\",\"name\":\"旧データ\",\"scope\":\"チーム\",\"timezone\":\"Asia/Tokyo\",\"role\":\"owner\",\"local\":false,\"version\":1}','1')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO memberships VALUES('ws_legacy','us_alice','owner')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO documents VALUES('ws_legacy','items','item_1','{}','1')")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    // Opening it runs 0007.
    let service = Service::open(&url).await.unwrap();
    let alice = acting_in(ALPHA, "us_alice");
    assert_eq!(
        get(&service, &alice, "/v1/workspaces")
            .await
            .unwrap()
            .as_array()
            .map(Vec::len),
        Some(0),
        "a workspace with no tenant must not survive the upgrade"
    );
    assert_eq!(
        get(&service, &alice, "/v1/workspaces/ws_legacy/items")
            .await
            .unwrap_err()
            .status,
        404
    );

    // The rows that pointed at it are gone too, rather than dangling.
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    for table in ["workspaces", "memberships", "documents"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "{table} still holds rows from before the boundary"
        );
    }
    pool.close().await;

    // And the database is usable afterwards: a new workspace gets a tenant.
    service.provision_personal(&alice).await.unwrap();
    let fresh = get(&service, &alice, "/v1/workspaces").await.unwrap();
    assert_eq!(fresh.as_array().unwrap().len(), 1);
    assert_eq!(fresh[0]["tenant_id"], ALPHA);
}
