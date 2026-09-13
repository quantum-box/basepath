use pathbase_api::service::{Actor, Service};
use serde_json::{json, Value};
use std::collections::HashMap;

fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("suggestions.sqlite3")).unwrap();
    service.initialize(true).unwrap();
    (dir, service)
}

fn call(
    service: &Service,
    actor: &Actor,
    method: &str,
    path: &str,
    body: Value,
    key: Option<&str>,
) -> pathbase_api::model::Result<Value> {
    service.handle(actor, method, path, &HashMap::new(), body, key)
}

fn first_goal(service: &Service, workspace: &str) -> Value {
    let snapshot = call(
        service,
        &Actor::local(),
        "GET",
        &format!("/v1/workspaces/{workspace}/snapshot"),
        json!({}),
        None,
    )
    .unwrap();
    snapshot["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| ["outcome", "idea", "milestone"].contains(&item["kind"].as_str().unwrap()))
        .unwrap()
        .clone()
}

#[test]
fn quality_fixture_returns_short_actions_and_separates_reflection_claims() {
    let (_dir, service) = setup();
    let goal = first_goal(&service, "personal");
    let body = json!({"goal_id":goal["id"],"expected_version":goal["version"]});
    let result = call(
        &service,
        &Actor::local(),
        "POST",
        "/v1/workspaces/personal/ai/suggestions/preview",
        body,
        Some("quality-fixture"),
    )
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

#[test]
fn retry_is_idempotent_and_does_not_create_domain_items() {
    let (_dir, service) = setup();
    let goal = first_goal(&service, "personal");
    let body = json!({"goal_id":goal["id"],"expected_version":goal["version"]});
    let before = call(
        &service,
        &Actor::local(),
        "GET",
        "/v1/workspaces/personal/snapshot",
        json!({}),
        None,
    )
    .unwrap();
    let a = call(
        &service,
        &Actor::local(),
        "POST",
        "/v1/workspaces/personal/ai/suggestions/preview",
        body.clone(),
        Some("same-input"),
    )
    .unwrap();
    let b = call(
        &service,
        &Actor::local(),
        "POST",
        "/v1/workspaces/personal/ai/suggestions/preview",
        body,
        Some("same-input"),
    )
    .unwrap();
    let after = call(
        &service,
        &Actor::local(),
        "GET",
        "/v1/workspaces/personal/snapshot",
        json!({}),
        None,
    )
    .unwrap();
    assert_eq!(a, b);
    assert_eq!(before["items"], after["items"]);
    assert_eq!(before["records"], after["records"]);
}

#[test]
fn permissions_and_stale_goal_version_are_enforced() {
    let (_dir, service) = setup();
    let goal = first_goal(&service, "team");
    let body = json!({"goal_id":goal["id"],"expected_version":goal["version"]});
    assert_eq!(
        call(
            &service,
            &Actor {
                id: "outsider".into(),
                agent: false
            },
            "POST",
            "/v1/workspaces/team/ai/suggestions/preview",
            body.clone(),
            Some("outside")
        )
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
            "/v1/workspaces/team/ai/suggestions/preview",
            stale,
            Some("stale")
        )
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
}

#[test]
fn adoption_still_requires_preview_approval_and_rejects_conflicts() {
    let (_dir, service) = setup();
    let goal = first_goal(&service, "personal");
    let path = "/v1/workspaces/personal/changesets/preview";
    let proposal = call(&service, &Actor::local(), "POST", path, json!({"title":"AI提案を採用","operations":[{"method":"POST","path":"/v1/workspaces/personal/items","body":{"title":"30分だけ試す","kind":"action","parent_id":goal["id"]}}]}), Some("preview-ai")).unwrap();
    let id = proposal["id"].as_str().unwrap();
    assert_eq!(
        call(
            &service,
            &Actor::local(),
            "POST",
            &format!("/v1/workspaces/personal/changesets/{id}/apply"),
            json!({}),
            Some("early-apply")
        )
        .unwrap_err()
        .code,
        "APPROVAL_REQUIRED"
    );
    call(
        &service,
        &Actor::local(),
        "POST",
        &format!("/v1/workspaces/personal/changesets/{id}/approve"),
        json!({}),
        Some("approve-ai"),
    )
    .unwrap();
    call(
        &service,
        &Actor::local(),
        "PATCH",
        &format!(
            "/v1/workspaces/personal/items/{}",
            goal["id"].as_str().unwrap()
        ),
        json!({"expected_version":goal["version"],"title":"changed elsewhere"}),
        Some("conflict"),
    )
    .unwrap();
    assert_eq!(
        call(
            &service,
            &Actor::local(),
            "POST",
            &format!("/v1/workspaces/personal/changesets/{id}/apply"),
            json!({}),
            Some("stale-apply")
        )
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
}

#[test]
fn expired_approved_proposal_is_rejected() {
    let (_dir, service) = setup();
    let proposal = call(
        &service,
        &Actor::local(),
        "POST",
        "/v1/workspaces/personal/changesets/preview",
        json!({"title":"期限テスト","operations":[{"method":"POST","path":"/v1/workspaces/personal/items","body":{"title":"短い行動","kind":"action"}}]}),
        Some("expiry-preview"),
    )
    .unwrap();
    let id = proposal["id"].as_str().unwrap();
    call(
        &service,
        &Actor::local(),
        "POST",
        &format!("/v1/workspaces/personal/changesets/{id}/approve"),
        json!({}),
        Some("expiry-approve"),
    )
    .unwrap();
    {
        let db = service.db.lock().unwrap();
        let mut change: Value =
            pathbase_api::storage::get(&db, "personal", "changesets", id).unwrap();
        change["expires_at"] = json!("2000-01-01T00:00:00+00:00");
        pathbase_api::storage::put(&db, "personal", "changesets", id, &change).unwrap();
    }
    assert_eq!(
        call(
            &service,
            &Actor::local(),
            "POST",
            &format!("/v1/workspaces/personal/changesets/{id}/apply"),
            json!({}),
            Some("expired-apply")
        )
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
}
