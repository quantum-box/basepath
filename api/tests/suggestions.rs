use pathbase_api::service::{Actor, Service};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("suggestions.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(true).await.unwrap();
    (dir, service)
}

async fn call(
    service: &Service,
    actor: &Actor,
    method: &str,
    path: &str,
    body: Value,
    key: Option<&str>,
) -> pathbase_api::model::Result<Value> {
    service
        .handle(actor, method, path, &HashMap::new(), body, key)
        .await
}

async fn first_goal(service: &Service, workspace: &str) -> Value {
    let snapshot = call(
        service,
        &Actor::local(),
        "GET",
        &format!("/v1/workspaces/{workspace}/snapshot"),
        json!({}),
        None,
    )
    .await
    .unwrap();
    snapshot["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| ["outcome", "idea", "milestone"].contains(&item["kind"].as_str().unwrap()))
        .unwrap()
        .clone()
}

#[tokio::test]
async fn quality_fixture_returns_short_actions_and_separates_reflection_claims() {
    let (_dir, service) = setup().await;
    let goal = first_goal(&service, "organization").await;
    let body = json!({"goal_id":goal["id"],"expected_version":goal["version"]});
    let result = call(
        &service,
        &Actor::local(),
        "POST",
        "/v1/workspaces/organization/ai/suggestions/preview",
        body,
        Some("quality-fixture"),
    )
    .await
    .unwrap();
    let suggestions = result["suggestions"].as_array().unwrap();
    assert!(suggestions.len() >= 2);
    assert!(suggestions
        .iter()
        .all(|item| item["duration_minutes"].as_i64().unwrap() <= 30));
    assert!(suggestions
        .iter()
        .all(|item| item["evidence"]["goal"]["id"] == goal["id"]));
    assert!(result["reflection"]["fact"].is_array());
    assert!(result["reflection"]["inference"].is_array());
    assert!(result["reflection"]["questions"].is_array());
}

#[tokio::test]
async fn retry_is_idempotent_and_does_not_create_domain_items() {
    let (_dir, service) = setup().await;
    let goal = first_goal(&service, "organization").await;
    let body = json!({"goal_id":goal["id"],"expected_version":goal["version"]});
    let before = call(
        &service,
        &Actor::local(),
        "GET",
        "/v1/workspaces/organization/snapshot",
        json!({}),
        None,
    )
    .await
    .unwrap();
    let a = call(
        &service,
        &Actor::local(),
        "POST",
        "/v1/workspaces/organization/ai/suggestions/preview",
        body.clone(),
        Some("same-input"),
    )
    .await
    .unwrap();
    let b = call(
        &service,
        &Actor::local(),
        "POST",
        "/v1/workspaces/organization/ai/suggestions/preview",
        body,
        Some("same-input"),
    )
    .await
    .unwrap();
    let after = call(
        &service,
        &Actor::local(),
        "GET",
        "/v1/workspaces/organization/snapshot",
        json!({}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(a, b);
    assert_eq!(before["items"], after["items"]);
    assert_eq!(before["records"], after["records"]);
}

#[tokio::test]
async fn permissions_and_stale_goal_version_are_enforced() {
    let (_dir, service) = setup().await;
    let goal = first_goal(&service, "organization").await;
    let body = json!({"goal_id":goal["id"],"expected_version":goal["version"]});
    assert_eq!(
        call(
            &service,
            &Actor {
                id: "outsider".into(),
                agent: false,
                connection: None
            },
            "POST",
            "/v1/workspaces/organization/ai/suggestions/preview",
            body.clone(),
            Some("outside")
        )
        .await
        .unwrap_err()
        .code,
        "NOT_FOUND"
    );
    let stale = json!({"goal_id":goal["id"],"expected_version":0});
    assert_eq!(
        call(
            &service,
            &Actor::local(),
            "POST",
            "/v1/workspaces/organization/ai/suggestions/preview",
            stale,
            Some("stale")
        )
        .await
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
}

#[tokio::test]
async fn adoption_still_requires_preview_approval_and_rejects_conflicts() {
    let (_dir, service) = setup().await;
    let goal = first_goal(&service, "organization").await;
    let path = "/v1/workspaces/organization/changesets/preview";
    let proposal = call(&service, &Actor::local(), "POST", path, json!({"title":"AI提案を採用","operations":[{"method":"POST","path":"/v1/workspaces/organization/items","body":{"title":"30分だけ試す","kind":"action","parent_id":goal["id"]}}]}), Some("preview-ai")).await.unwrap();
    let id = proposal["id"].as_str().unwrap();
    assert_eq!(
        call(
            &service,
            &Actor::local(),
            "POST",
            &format!("/v1/workspaces/organization/changesets/{id}/apply"),
            json!({}),
            Some("early-apply")
        )
        .await
        .unwrap_err()
        .code,
        "APPROVAL_REQUIRED"
    );
    // The goal the proposal hangs off moves while the proposal is waiting.
    call(
        &service,
        &Actor::local(),
        "PATCH",
        &format!(
            "/v1/workspaces/organization/items/{}",
            goal["id"].as_str().unwrap()
        ),
        json!({"expected_version":goal["version"],"title":"changed elsewhere"}),
        Some("conflict"),
    )
    .await
    .unwrap();
    // Approving writes, so a proposal that no longer matches the plan is
    // refused at the approval rather than after it.
    assert_eq!(
        call(
            &service,
            &Actor::local(),
            "POST",
            &format!("/v1/workspaces/organization/changesets/{id}/approve"),
            json!({}),
            Some("stale-approve")
        )
        .await
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
}

#[tokio::test]
async fn an_expired_proposal_is_rejected() {
    let (_dir, service) = setup().await;
    let proposal = call(
        &service,
        &Actor::local(),
        "POST",
        "/v1/workspaces/organization/changesets/preview",
        json!({"title":"期限テスト","operations":[{"method":"POST","path":"/v1/workspaces/organization/items","body":{"title":"短い行動","kind":"action"}}]}),
        Some("expiry-preview"),
    ).await
    .unwrap();
    let id = proposal["id"].as_str().unwrap();
    {
        // Expire the changeset without waiting 30 minutes.
        let mut tx = service.db.begin_write().await.unwrap();
        let mut change: Value =
            pathbase_api::storage::get(&mut tx, "organization", "changesets", id)
                .await
                .unwrap();
        change["expires_at"] = json!("2000-01-01T00:00:00+00:00");
        pathbase_api::storage::put(&mut tx, "organization", "changesets", id, &change)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    // Neither door opens on an expired proposal: it has to be made again.
    for (step, key) in [("approve", "expired-approve"), ("apply", "expired-apply")] {
        assert_eq!(
            call(
                &service,
                &Actor::local(),
                "POST",
                &format!("/v1/workspaces/organization/changesets/{id}/{step}"),
                json!({}),
                Some(key)
            )
            .await
            .unwrap_err()
            .code,
            "VERSION_CONFLICT",
            "{step}"
        );
    }
}
