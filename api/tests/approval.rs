//! What must never be accepted as a person's approval.
//!
//! The change-set flow is the one place an AI's output turns into the person's
//! plan, so this file is written from the attacker's side: every way a caller
//! might try to make a proposal look approved, and the refusal it gets.
//!
//! The central fact the design rests on: an MCP client's call reaches the
//! server on the same connection with the same access token whether the model
//! made it or a button in the app did. The server cannot tell them apart, so
//! it never tries — approval is a person's act in Basepath's own origin, with
//! their own session.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

fn agent(id: &str) -> Actor {
    Actor {
        id: id.into(),
        agent: true,
        connection: Some("mcpconn_test".into()),
    }
}

async fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("approval.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    (dir, service)
}

async fn call(
    service: &Service,
    who: &Actor,
    method: &str,
    path: &str,
    body: Value,
) -> Result<Value> {
    service
        .handle(
            who,
            method,
            path,
            &HashMap::new(),
            body,
            Some(&uuid::Uuid::new_v4().to_string()),
        )
        .await
}

/// A proposal from an AI client, as the model would make it.
async fn propose(service: &Service, who: &Actor, workspace: &str, title: &str) -> Value {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{workspace}/changesets/preview"),
        json!({"title":title,"operations":[{
            "method":"POST",
            "path":format!("/v1/workspaces/{workspace}/items"),
            "body":{"kind":"action","title":title}
        }]}),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_proposal_shows_what_it_would_do_without_doing_it() {
    let (_dir, service) = setup().await;
    let person = Actor::local();

    let goal = call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"既存の目標","kind":"outcome"}),
    )
    .await
    .unwrap();
    let goal_id = goal["id"].as_str().unwrap().to_owned();

    let change = call(
        &service,
        &agent("local-owner"),
        "POST",
        "/v1/workspaces/personal/changesets/preview",
        json!({"title":"三種類の変更","operations":[
            {"method":"POST","path":"/v1/workspaces/personal/items","body":{"kind":"action","title":"新しい行動"}},
            {"method":"PATCH","path":format!("/v1/workspaces/personal/items/{goal_id}"),"body":{"expected_version":1,"title":"名前を変えた目標"}}
        ]}),
    )
    .await
    .unwrap();

    // The diff names the workspace, the effect, and the before/after state.
    assert_eq!(change["workspace_id"], "personal");
    assert_eq!(change["status"], "pending");
    assert!(change["expires_at"].is_string(), "an expiry is shown");
    assert_eq!(change["proposed_by_connection"], "mcpconn_test");
    let changes = change["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0]["effect"], "created");
    assert_eq!(changes[0]["title"], "新しい行動");
    assert!(changes[0]["before"].is_null());
    assert_eq!(changes[1]["effect"], "updated");
    assert_eq!(changes[1]["before"]["title"], "既存の目標");
    assert_eq!(changes[1]["after"]["title"], "名前を変えた目標");

    // And the plan is untouched: previewing is not doing.
    let items = call(
        &service,
        &person,
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
    )
    .await
    .unwrap();
    let titles: Vec<&str> = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["既存の目標"]);
}

#[tokio::test]
async fn nothing_an_ai_can_say_counts_as_approval() {
    let (_dir, service) = setup().await;
    let ai = agent("local-owner");
    let change = propose(&service, &ai, "personal", "AIの提案").await;
    let id = change["id"].as_str().unwrap().to_owned();
    let path = format!("/v1/workspaces/personal/changesets/{id}");

    // 1. Applying an unapproved change set.
    let refused = call(&service, &ai, "POST", &format!("{path}/apply"), json!({}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "APPROVAL_REQUIRED");

    // 2. Approving as the agent itself.
    let refused = call(&service, &ai, "POST", &format!("{path}/approve"), json!({}))
        .await
        .unwrap_err();
    assert_eq!(refused.status, 403, "{refused:?}");

    // 3. Claiming approval in the request body.
    for body in [
        json!({"approved": true}),
        json!({"approved_by": "local-owner"}),
        json!({"status": "approved"}),
        json!({"approved_hash": change["hash"]}),
    ] {
        let refused = call(&service, &ai, "POST", &format!("{path}/apply"), body)
            .await
            .unwrap_err();
        assert!(
            ["APPROVAL_REQUIRED", "VALIDATION_ERROR"].contains(&refused.code.as_str()),
            "{refused:?}"
        );
    }

    // 4. Editing the stored change set directly through the document API.
    let refused = call(
        &service,
        &ai,
        "PATCH",
        &path,
        json!({"status":"approved","approved_by":"local-owner"}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.status, 403, "{refused:?}");

    // Nothing was created by any of that.
    let items = call(
        &service,
        &Actor::local(),
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
    )
    .await
    .unwrap();
    assert!(items["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn approval_belongs_to_the_person_the_content_and_the_workspace() {
    let (_dir, service) = setup().await;
    let ai = agent("local-owner");
    let person = Actor::local();
    let change = propose(&service, &ai, "personal", "承認の帰属").await;
    let id = change["id"].as_str().unwrap().to_owned();
    let path = format!("/v1/workspaces/personal/changesets/{id}");

    // Approving content other than what was shown is refused.
    let stale = call(
        &service,
        &person,
        "POST",
        &format!("{path}/approve"),
        json!({"hash":"not-the-shown-digest"}),
    )
    .await
    .unwrap_err();
    assert_eq!(stale.code, "CHANGESET_SUPERSEDED");

    // Approving the shown content works, and records who and when.
    let approved = call(
        &service,
        &person,
        "POST",
        &format!("{path}/approve"),
        json!({"hash":change["hash"]}),
    )
    .await
    .unwrap();
    assert_eq!(approved["approved_by"], "local-owner");
    assert!(approved["approved_at"].is_string());

    // Someone else cannot apply what this person approved.
    let other = Actor::person("someone-else");
    let refused = call(
        &service,
        &other,
        "POST",
        &format!("{path}/apply"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert!(
        refused.status == 403 || refused.status == 404,
        "{refused:?}"
    );

    // A change-set id from another workspace is not found in this one.
    let team = call(
        &service,
        &person,
        "POST",
        &format!("/v1/workspaces/team/changesets/{id}/apply"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(team.status, 404);

    // The approver — including through their AI connection, which is the same
    // person — may apply, exactly once.
    let applied = call(&service, &ai, "POST", &format!("{path}/apply"), json!({}))
        .await
        .unwrap();
    assert_eq!(applied["changeset"]["status"], "applied");
    assert_eq!(applied["changeset"]["applied_by"], "local-owner");
    assert_eq!(
        applied["changeset"]["applied_by_connection"],
        "mcpconn_test"
    );

    // A second apply is refused. Which refusal comes first depends on what
    // moved: applying changed the plan, so the staleness check fires before
    // the approval check. Either way it does not happen twice.
    let twice = call(&service, &ai, "POST", &format!("{path}/apply"), json!({}))
        .await
        .unwrap_err();
    assert!(
        ["VERSION_CONFLICT", "APPROVAL_REQUIRED"].contains(&twice.code.as_str()),
        "a change set applies once: {twice:?}"
    );

    let items = call(
        &service,
        &person,
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_rejected_or_stale_proposal_can_never_be_applied() {
    let (_dir, service) = setup().await;
    let ai = agent("local-owner");
    let person = Actor::local();

    // --- rejection --------------------------------------------------------
    let change = propose(&service, &ai, "personal", "却下される案").await;
    let id = change["id"].as_str().unwrap().to_owned();
    let path = format!("/v1/workspaces/personal/changesets/{id}");
    call(
        &service,
        &person,
        "POST",
        &format!("{path}/approve"),
        json!({}),
    )
    .await
    .unwrap();
    // Rejecting after approval invalidates the approval.
    let rejected = call(&service, &ai, "POST", &format!("{path}/reject"), json!({}))
        .await
        .unwrap();
    assert_eq!(rejected["status"], "rejected");
    let refused = call(
        &service,
        &person,
        "POST",
        &format!("{path}/apply"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "APPROVAL_REQUIRED");
    // And it cannot be resurrected.
    let again = call(
        &service,
        &person,
        "POST",
        &format!("{path}/approve"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(again.status, 409);

    // --- the plan moved underneath ----------------------------------------
    let second = propose(&service, &ai, "personal", "古くなる案").await;
    let second_path = format!(
        "/v1/workspaces/personal/changesets/{}",
        second["id"].as_str().unwrap()
    );
    call(
        &service,
        &person,
        "POST",
        &format!("{second_path}/approve"),
        json!({}),
    )
    .await
    .unwrap();
    // Someone edits the plan after the approval.
    call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"割り込みの変更","kind":"outcome"}),
    )
    .await
    .unwrap();
    let stale = call(
        &service,
        &person,
        "POST",
        &format!("{second_path}/apply"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(
        stale.code, "VERSION_CONFLICT",
        "an approval does not survive the plan changing under it"
    );
}

#[tokio::test]
async fn an_applied_change_set_is_all_or_nothing_and_leaves_a_trail() {
    let (_dir, service) = setup().await;
    let ai = agent("local-owner");
    let person = Actor::local();

    // A batch whose second operation cannot succeed must leave the first
    // undone as well.
    let broken = call(
        &service,
        &ai,
        "POST",
        "/v1/workspaces/personal/changesets/preview",
        json!({"title":"壊れた案","operations":[
            {"method":"POST","path":"/v1/workspaces/personal/items","body":{"kind":"action","title":"作られてはいけない"}},
            {"method":"PATCH","path":"/v1/workspaces/personal/items/missing","body":{"expected_version":1,"title":"存在しない"}}
        ]}),
    )
    .await;
    assert!(broken.is_err(), "the whole batch is refused at preview");
    let items = call(
        &service,
        &person,
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
    )
    .await
    .unwrap();
    assert!(items["items"].as_array().unwrap().is_empty());

    // A good batch applies atomically and is auditable end to end.
    let change = propose(&service, &ai, "personal", "監査される案").await;
    let path = format!(
        "/v1/workspaces/personal/changesets/{}",
        change["id"].as_str().unwrap()
    );
    call(
        &service,
        &person,
        "POST",
        &format!("{path}/approve"),
        json!({}),
    )
    .await
    .unwrap();
    call(&service, &ai, "POST", &format!("{path}/apply"), json!({}))
        .await
        .unwrap();

    let audit = call(
        &service,
        &person,
        "GET",
        "/v1/workspaces/personal/audit",
        json!({}),
    )
    .await
    .unwrap();
    let entries = audit.as_array().unwrap();
    let find = |needle: &str| {
        entries
            .iter()
            .find(|entry| {
                entry["command"]
                    .as_str()
                    .is_some_and(|c| c.contains(needle))
            })
            .unwrap_or_else(|| panic!("no audit entry for {needle}"))
            .clone()
    };
    // Proposed by the AI connection, approved in the browser, applied through
    // the connection again — each one distinguishable.
    assert_eq!(find("changesets/preview")["origin"], "mcp");
    assert_eq!(find("changesets/preview")["connection"], "mcpconn_test");
    assert_eq!(find("/approve")["origin"], "ui");
    assert!(find("/approve")["connection"].is_null());
    assert_eq!(find("/apply")["origin"], "mcp");
    assert_eq!(find("/apply")["connection"], "mcpconn_test");
}

#[tokio::test]
async fn a_resent_or_double_clicked_approval_does_not_apply_twice() {
    let (_dir, service) = setup().await;
    let ai = agent("local-owner");
    let person = Actor::local();
    let change = propose(&service, &ai, "personal", "再送される案").await;
    let path = format!(
        "/v1/workspaces/personal/changesets/{}",
        change["id"].as_str().unwrap()
    );
    call(
        &service,
        &person,
        "POST",
        &format!("{path}/approve"),
        json!({}),
    )
    .await
    .unwrap();

    // The same request twice, with the same idempotency key, is one apply.
    let key = uuid::Uuid::new_v4().to_string();
    let first = service
        .handle(
            &ai,
            "POST",
            &format!("{path}/apply"),
            &HashMap::new(),
            json!({}),
            Some(&key),
        )
        .await
        .unwrap();
    let replay = service
        .handle(
            &ai,
            "POST",
            &format!("{path}/apply"),
            &HashMap::new(),
            json!({}),
            Some(&key),
        )
        .await
        .unwrap();
    assert_eq!(first, replay);

    let items = call(
        &service,
        &person,
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(
        items["items"].as_array().unwrap().len(),
        1,
        "a resend must not create the item twice"
    );
}
