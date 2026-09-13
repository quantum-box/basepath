use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("db.sqlite")).unwrap();
    service.initialize(false).unwrap();
    (dir, service)
}

fn call(
    service: &Service,
    actor: &str,
    method: &str,
    path: &str,
    body: Value,
    key: &str,
) -> Result<Value> {
    service.handle(
        &Actor {
            id: actor.into(),
            agent: false,
        },
        method,
        path,
        &HashMap::new(),
        body,
        Some(key),
    )
}

#[test]
fn personal_workspace_only_accepts_its_owner_as_assignee() {
    let (_dir, service) = setup();
    {
        let db = service.db.lock().unwrap();
        db.execute(
            "INSERT INTO memberships VALUES('personal','someone-else','editor')",
            [],
        )
        .unwrap();
    }
    let result = call(
        &service,
        "local-owner",
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"個人の行動","kind":"action","fields":{"assignee_id":"someone-else"}}),
        "personal-assignee",
    );
    assert_eq!(result.unwrap_err().code, "VALIDATION_ERROR");
}

#[test]
fn assignment_due_and_read_notifications_are_persistent_and_idempotent() {
    let (dir, service) = setup();
    let created = call(
        &service,
        "local-owner",
        "POST",
        "/v1/workspaces/personal/items",
        json!({
            "title":"期限のある行動",
            "kind":"action",
            "due_date":"2026-09-14",
            "fields":{"assignee_id":"local-owner","priority":"urgent"}
        }),
        "create-assigned",
    )
    .unwrap();
    let replay = call(
        &service,
        "local-owner",
        "POST",
        "/v1/workspaces/personal/items",
        json!({
            "title":"期限のある行動",
            "kind":"action",
            "due_date":"2026-09-14",
            "fields":{"assignee_id":"local-owner","priority":"urgent"}
        }),
        "create-assigned",
    )
    .unwrap();
    assert_eq!(created, replay);
    let snapshot = call(
        &service,
        "local-owner",
        "GET",
        "/v1/workspaces/personal/snapshot",
        json!({}),
        "unused",
    )
    .unwrap();
    let assignment = snapshot["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|notification| notification["kind"] == "assignment")
        .unwrap();
    let notification_id = assignment["id"].as_str().unwrap();
    call(
        &service,
        "local-owner",
        "PATCH",
        &format!("/v1/workspaces/personal/notifications/{notification_id}/read"),
        json!({"read":true}),
        "read-notification",
    )
    .unwrap();
    drop(service);

    let reopened = Service::open(&dir.path().join("db.sqlite")).unwrap();
    let snapshot = call(
        &reopened,
        "local-owner",
        "GET",
        "/v1/workspaces/personal/snapshot",
        json!({}),
        "unused",
    )
    .unwrap();
    let notifications = snapshot["notifications"].as_array().unwrap();
    assert_eq!(
        notifications
            .iter()
            .filter(|notification| notification["kind"] == "assignment")
            .count(),
        1
    );
    assert!(notifications
        .iter()
        .find(|notification| notification["id"] == notification_id)
        .unwrap()["read_at"]
        .is_string());
}

#[test]
fn removed_member_cannot_receive_or_read_workspace_notifications() {
    let (_dir, service) = setup();
    {
        let db = service.db.lock().unwrap();
        db.execute(
            "INSERT INTO memberships VALUES('team','member','editor')",
            [],
        )
        .unwrap();
    }
    let item = call(
        &service,
        "local-owner",
        "POST",
        "/v1/workspaces/team/items",
        json!({"title":"共有タスク","kind":"action","fields":{"assignee_id":"member"}}),
        "assign-member",
    )
    .unwrap();
    {
        let db = service.db.lock().unwrap();
        db.execute(
            "DELETE FROM memberships WHERE workspace_id='team' AND actor='member'",
            [],
        )
        .unwrap();
    }
    assert_eq!(
        call(
            &service,
            "member",
            "GET",
            "/v1/workspaces/team/snapshot",
            json!({}),
            "unused",
        )
        .unwrap_err()
        .code,
        "NOT_FOUND"
    );
    assert!(item["fields"]["assignee_id"].is_string());
}
