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
        tenant: pathbase_api::service::LOCAL_TENANT.into(),
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

    // Nobody may apply what has not been approved — the guarantee the two
    // steps exist for, and the one an AI connection runs into.
    let unapproved = call(&service, &ai, "POST", &format!("{path}/apply"), json!({}))
        .await
        .unwrap_err();
    assert_eq!(unapproved.code, "APPROVAL_REQUIRED");

    // Approving the shown content works, records who and when — and writes it.
    // A person looking at the diff has decided; a second button between that
    // decision and their plan is a button they forget, and the proposal
    // expires having done nothing.
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
    assert_eq!(approved["status"], "applied");
    assert!(approved["applied_at"].is_string());
    assert_eq!(approved["applied_by"], "local-owner");
    // Approved in the browser, so the record says so rather than naming the
    // connection the proposal happened to arrive on.
    assert!(approved["applied_by_connection"].is_null());

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

    // Someone else does not get the "already applied" answer: that answer is
    // for the person whose approval it was.
    let other = Actor::person("someone-else", pathbase_api::service::LOCAL_TENANT);
    assert!(call(
        &service,
        &other,
        "POST",
        &format!("{path}/apply"),
        json!({})
    )
    .await
    .is_err());

    // The approver's own connection asking again is asking for something that
    // is already true, and is told so instead of being refused. A model that
    // relayed "承認しました" would otherwise report a failure for a change that
    // is in the plan.
    let again = call(&service, &ai, "POST", &format!("{path}/apply"), json!({}))
        .await
        .unwrap();
    assert_eq!(again["already_applied"], true);
    assert_eq!(again["changeset"]["applied_at"], approved["applied_at"]);

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
    // Someone edits the plan after the proposal was written, so the diff the
    // person is looking at is no longer the diff that would be applied.
    // Approving now writes, so this is caught at approval rather than later.
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
        &format!("{second_path}/approve"),
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
    // Proposed by the AI connection, approved in the browser — two acts by two
    // different parties, and the trail keeps them apart. The write is the
    // person's: it happened in the approval, in their session, not on the
    // connection the proposal arrived on.
    assert_eq!(find("changesets/preview")["origin"], "mcp");
    assert_eq!(find("changesets/preview")["connection"], "mcpconn_test");
    assert_eq!(find("/approve")["origin"], "ui");
    assert!(find("/approve")["connection"].is_null());
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

/// The failure this file exists to prevent, in the form it actually took.
///
/// A person approved a proposal in Basepath and left, believing they were
/// done. Nothing had been written: the approval was one step and the writing
/// was another, and the second one needed a click nobody had a reason to make.
/// Half an hour later the proposal expired and the goal had never existed.
#[tokio::test]
async fn an_approval_is_never_left_sitting_there_unwritten() {
    let (_dir, service) = setup().await;
    let ai = agent("local-owner");
    let person = Actor::local();
    let change = propose(&service, &ai, "personal", "承認したら反映される案").await;
    let path = format!(
        "/v1/workspaces/personal/changesets/{}",
        change["id"].as_str().unwrap()
    );

    let approved = call(
        &service,
        &person,
        "POST",
        &format!("{path}/approve"),
        json!({}),
    )
    .await
    .unwrap();

    // Not "approved, waiting". There is no such state to walk away from.
    assert_eq!(approved["status"], "applied");
    assert_ne!(
        approved["status"], "approved",
        "an approval that has not been written is one that can expire unnoticed"
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
    let titles: Vec<&str> = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["title"].as_str())
        .collect();
    assert_eq!(titles, vec!["承認したら反映される案"]);

    // And nothing remains that the person is expected to come back and finish.
    let listed = call(
        &service,
        &person,
        "GET",
        "/v1/workspaces/personal/changesets",
        json!({}),
    )
    .await
    .unwrap();
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] != "approved"),
        "{listed}"
    );
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
    // The same approval twice, with the same idempotency key, writes once.
    // Approving is now the write, so this is where a double click lands.
    let key = uuid::Uuid::new_v4().to_string();
    let first = service
        .handle(
            &person,
            "POST",
            &format!("{path}/approve"),
            &HashMap::new(),
            json!({}),
            Some(&key),
        )
        .await
        .unwrap();
    let replay = service
        .handle(
            &person,
            "POST",
            &format!("{path}/approve"),
            &HashMap::new(),
            json!({}),
            Some(&key),
        )
        .await
        .unwrap();
    assert_eq!(first, replay);

    // A second click that lost the key is a fresh request, and is refused
    // rather than writing again.
    let without_key = call(
        &service,
        &person,
        "POST",
        &format!("{path}/approve"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(without_key.status, 409);
    // The model asking afterwards is told it is already done, not refused.
    let asked = call(&service, &ai, "POST", &format!("{path}/apply"), json!({}))
        .await
        .unwrap();
    assert_eq!(asked["already_applied"], true);

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
