//! Ranges a person decided in advance, and everything they still cannot cover.
//!
//! A range makes an apply proceed without a per-change approval, which is the
//! only place in this server that happens. So this file is written the same
//! way `approval.rs` is — from the attacker's side — and asks one question of
//! every path: could an AI reach this without the person having decided it
//! first, on Basepath's origin, with their own session?
//!
//! The four answers that must stay "no" whatever anyone configures: a deletion
//! is never in range; a date, owner or target is not in range unless they said
//! so about that exact range; another workspace or another AI client is never
//! in range; and an AI connection can neither read a range nor create one.
use pathbase_api::{
    auto_apply,
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

const CONNECTION: &str = "mcpconn_range";
const OTHER_CONNECTION: &str = "mcpconn_other";

fn person() -> Actor {
    Actor::local()
}

/// The AI, acting for that same person, through the delegation they granted.
fn ai(connection: &str) -> Actor {
    Actor {
        id: person().id,
        agent: true,
        connection: Some(connection.into()),
    }
}

async fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("range.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    // Two delegations, so "this range belongs to one AI client" is testable
    // rather than asserted.
    for (id, name) in [(CONNECTION, "ChatGPT"), (OTHER_CONNECTION, "Claude")] {
        let mut tx = service.db.begin_write().await.unwrap();
        tx.execute(
            "INSERT INTO mcp_connections(id,actor,client_id,client_name,scopes,status,\
             created_at,updated_at,last_used_at,ui_read_at,version) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?)",
            &pathbase_api::params![
                id,
                person().id,
                format!("client_{id}"),
                name,
                "pathbase.read pathbase.propose pathbase.apply",
                "active",
                "2026-09-19T00:00:00Z",
                "2026-09-19T00:00:00Z",
                "2026-09-19T00:00:00Z",
                "",
                1i64
            ],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
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

/// The person sets a range, in Basepath, on their own session.
async fn set_range(service: &Service, body: Value) -> Result<Value> {
    call(service, &person(), "POST", "/v1/mcp/auto-apply", body).await
}

fn range_for(connection: &str) -> Value {
    json!({
        "workspace_id": "personal",
        "connection_id": connection,
        "allow_create": true,
        "allow_update": false,
        "allow_guarded": false,
        "days": 7,
    })
}

async fn propose(service: &Service, who: &Actor, operations: Value) -> Value {
    call(
        service,
        who,
        "POST",
        "/v1/workspaces/personal/changesets/preview",
        json!({"title": "提案", "operations": operations}),
    )
    .await
    .unwrap()
}

async fn apply(service: &Service, who: &Actor, change: &Value) -> Result<Value> {
    call(
        service,
        who,
        "POST",
        &format!(
            "/v1/workspaces/personal/changesets/{}/apply",
            change["id"].as_str().unwrap()
        ),
        json!({}),
    )
    .await
}

fn add_action(title: &str) -> Value {
    json!([{
        "method": "POST",
        "path": "/v1/workspaces/personal/items",
        "body": {"kind": "action", "title": title},
    }])
}

async fn new_item(service: &Service, kind: &str, title: &str) -> Value {
    call(
        service,
        &person(),
        "POST",
        "/v1/workspaces/personal/items",
        json!({"kind": kind, "title": title}),
    )
    .await
    .unwrap()
}

/// A link between two goals, and the operation that would remove it.
///
/// Relations are what a re-proposed breakdown actually deletes — an item is
/// archived rather than destroyed, but "this no longer belongs under that" is
/// a real removal, and it is the kind that disappears quietly.
async fn a_link_to_remove(service: &Service) -> (Value, Value) {
    let parent = new_item(service, "outcome", "親の目標").await;
    let child = new_item(service, "initiative", "ぶら下がっている取り組み").await;
    let relation = call(
        service,
        &person(),
        "POST",
        "/v1/workspaces/personal/relations",
        json!({"source_id": child["id"], "target_id": parent["id"], "type": "part_of"}),
    )
    .await
    .unwrap();
    let removal = json!({
        "method": "DELETE",
        "path": format!("/v1/workspaces/personal/relations/{}", relation["id"].as_str().unwrap()),
        "body": {"expected_version": relation["version"]},
    });
    (relation, removal)
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_range_the_person_set_lets_a_proposal_through_and_says_so_afterwards() {
    let (_dir, service) = setup().await;
    set_range(&service, range_for(CONNECTION)).await.unwrap();

    let change = propose(&service, &ai(CONNECTION), add_action("朝の散歩")).await;
    // The proposal already knows, so the conversation can offer the right
    // control instead of a button that would fail.
    assert_eq!(change["auto_apply_eligible"], json!(true));
    assert_eq!(change["status"], "pending");

    let applied = apply(&service, &ai(CONNECTION), &change).await.unwrap();
    assert_eq!(applied["auto_applied"], json!(true));
    let stored = &applied["changeset"];
    assert_eq!(stored["status"], "applied");

    // The distinction the audit exists for: nobody approved this one. They
    // had already decided about this kind, and the row says which range.
    assert!(
        stored["approved_by"].is_null(),
        "an auto-applied change must not claim a per-change approval"
    );
    assert_eq!(stored["auto_applied"], json!(true));
    assert!(stored["auto_apply_rule"]
        .as_str()
        .unwrap()
        .starts_with("autoapply"));
    assert_eq!(stored["applied_by_connection"], CONNECTION);

    // And it is in the plan, readable afterwards like anything else.
    let items = call(
        &service,
        &person(),
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
    )
    .await
    .unwrap();
    assert!(items["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["title"] == "朝の散歩"));

    // The diff did not evaporate because it was applied without a click.
    let listed = call(
        &service,
        &person(),
        "GET",
        "/v1/workspaces/personal/changesets",
        json!({}),
    )
    .await
    .unwrap();
    let row = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == change["id"])
        .expect("an auto-applied change set stays in the list");
    assert_eq!(row["auto_applied"], json!(true));
    assert!(!row["changes"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_deletion_is_never_in_range() {
    let (_dir, service) = setup().await;
    // The widest range that can be expressed at all.
    set_range(
        &service,
        json!({
            "workspace_id": "personal",
            "connection_id": CONNECTION,
            "allow_create": true,
            "allow_update": true,
            "allow_guarded": true,
            "days": 90,
        }),
    )
    .await
    .unwrap();

    let (relation, removal) = a_link_to_remove(&service).await;
    let change = propose(&service, &ai(CONNECTION), json!([removal])).await;
    // The diff says so plainly, whatever else it says.
    assert_eq!(change["changes"][0]["effect"], "deleted");
    assert_eq!(change["auto_apply_eligible"], json!(false));

    let refused = apply(&service, &ai(CONNECTION), &change).await.unwrap_err();
    assert_eq!(refused.code, "APPROVAL_REQUIRED");

    // Still linked, and still awaiting the person.
    let links = call(
        &service,
        &person(),
        "GET",
        "/v1/workspaces/personal/relations",
        json!({}),
    )
    .await
    .unwrap();
    assert!(links["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|link| link["id"] == relation["id"]));
}

#[tokio::test]
async fn a_deletion_mixed_into_an_otherwise_covered_proposal_stops_all_of_it() {
    let (_dir, service) = setup().await;
    set_range(&service, range_for(CONNECTION)).await.unwrap();
    let (relation, removal) = a_link_to_remove(&service).await;

    let change = propose(
        &service,
        &ai(CONNECTION),
        json!([
            {"method": "POST", "path": "/v1/workspaces/personal/items",
             "body": {"kind": "action", "title": "無害な追加"}},
            removal,
        ]),
    )
    .await;

    assert_eq!(change["auto_apply_eligible"], json!(false));
    assert_eq!(
        apply(&service, &ai(CONNECTION), &change)
            .await
            .unwrap_err()
            .code,
        "APPROVAL_REQUIRED",
        "half a proposal is not a proposal the person read"
    );
    // Neither half ran.
    let items = call(
        &service,
        &person(),
        "GET",
        "/v1/workspaces/personal/items",
        json!({}),
    )
    .await
    .unwrap();
    assert!(!items["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["title"] == "無害な追加"));
    let links = call(
        &service,
        &person(),
        "GET",
        "/v1/workspaces/personal/relations",
        json!({}),
    )
    .await
    .unwrap();
    assert!(links["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|link| link["id"] == relation["id"]));
}

#[tokio::test]
async fn a_value_that_reads_as_a_commitment_needs_the_person_to_have_said_so() {
    let (_dir, service) = setup().await;
    set_range(&service, range_for(CONNECTION)).await.unwrap();

    let with_a_date = json!([{
        "method": "POST",
        "path": "/v1/workspaces/personal/items",
        "body": {"kind": "action", "title": "締切のある行動", "due_date": "2026-12-31"},
        "basis": "本人が会話で「年末までに」と言った",
    }]);
    let change = propose(&service, &ai(CONNECTION), with_a_date.clone()).await;
    assert_eq!(change["auto_apply_eligible"], json!(false));
    assert_eq!(
        apply(&service, &ai(CONNECTION), &change)
            .await
            .unwrap_err()
            .code,
        "APPROVAL_REQUIRED",
        "a date is read afterwards as something the person decided"
    );

    // Until they decide, about this range, that it may carry them.
    set_range(
        &service,
        json!({
            "workspace_id": "personal",
            "connection_id": CONNECTION,
            "allow_create": true,
            "allow_update": false,
            "allow_guarded": true,
            "days": 7,
        }),
    )
    .await
    .unwrap();
    let change = propose(&service, &ai(CONNECTION), with_a_date).await;
    assert_eq!(change["auto_apply_eligible"], json!(true));
    assert_eq!(
        apply(&service, &ai(CONNECTION), &change).await.unwrap()["auto_applied"],
        json!(true)
    );
}

#[tokio::test]
async fn an_operation_the_range_does_not_name_is_refused() {
    let (_dir, service) = setup().await;
    // Adding only. Rewriting what the person wrote is a different decision.
    set_range(&service, range_for(CONNECTION)).await.unwrap();
    let goal = new_item(&service, "outcome", "本人が書いた題").await;

    let change = propose(
        &service,
        &ai(CONNECTION),
        json!([{
            "method": "PATCH",
            "path": format!("/v1/workspaces/personal/items/{}", goal["id"].as_str().unwrap()),
            "body": {"title": "AIが書き換えた題", "expected_version": goal["version"]},
        }]),
    )
    .await;
    assert_eq!(change["auto_apply_eligible"], json!(false));
    assert_eq!(
        apply(&service, &ai(CONNECTION), &change)
            .await
            .unwrap_err()
            .code,
        "APPROVAL_REQUIRED"
    );
}

#[tokio::test]
async fn a_range_belongs_to_one_ai_client_and_one_workspace() {
    let (_dir, service) = setup().await;
    set_range(&service, range_for(CONNECTION)).await.unwrap();

    // Another delegation's proposal is not covered, even though its contents
    // would be. A range is a statement about one AI client.
    let change = propose(
        &service,
        &ai(OTHER_CONNECTION),
        add_action("別クライアント"),
    )
    .await;
    assert_eq!(change["auto_apply_eligible"], json!(false));
    assert_eq!(
        apply(&service, &ai(OTHER_CONNECTION), &change)
            .await
            .unwrap_err()
            .code,
        "APPROVAL_REQUIRED"
    );

    // Nor may the covered connection apply a proposal that arrived elsewhere.
    assert_eq!(
        apply(&service, &ai(CONNECTION), &change)
            .await
            .unwrap_err()
            .code,
        "APPROVAL_REQUIRED",
        "a proposal keeps the connection it arrived on"
    );

    // And a range needs a workspace the person is actually in.
    assert!(set_range(
        &service,
        json!({
            "workspace_id": "someone-elses",
            "connection_id": CONNECTION,
            "allow_create": true,
            "allow_update": false,
            "allow_guarded": false,
            "days": 7,
        }),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn turning_a_range_off_takes_effect_on_the_next_call() {
    let (_dir, service) = setup().await;
    let rule = set_range(&service, range_for(CONNECTION)).await.unwrap();

    // Proposed while the range was on — which changes nothing. The range is
    // read when the apply happens, not when the proposal was made.
    let change = propose(&service, &ai(CONNECTION), add_action("取り消し後")).await;
    assert_eq!(change["auto_apply_eligible"], json!(true));

    call(
        &service,
        &person(),
        "POST",
        &format!("/v1/mcp/auto-apply/{}/revoke", rule["id"].as_str().unwrap()),
        json!({}),
    )
    .await
    .unwrap();

    assert_eq!(
        apply(&service, &ai(CONNECTION), &change)
            .await
            .unwrap_err()
            .code,
        "APPROVAL_REQUIRED",
        "a revoked range must not apply a proposal made before it was revoked"
    );
    let reread = call(
        &service,
        &person(),
        "GET",
        &format!(
            "/v1/workspaces/personal/changesets/{}",
            change["id"].as_str().unwrap()
        ),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(reread["status"], "pending");
}

#[tokio::test]
async fn an_ai_connection_can_neither_read_a_range_nor_create_one() {
    let (_dir, service) = setup().await;
    set_range(&service, range_for(CONNECTION)).await.unwrap();

    // Reading gets a 404 — not a 403, which would confirm there is something
    // here and invite a second attempt.
    for path in ["/v1/mcp/auto-apply", "/v1/mcp/connections"] {
        let refused = call(&service, &ai(CONNECTION), "GET", path, json!({}))
            .await
            .unwrap_err();
        assert_eq!(refused.status, 404, "GET {path} leaked its existence");
    }

    // Writing gets the blanket refusal every agent write gets, word for word
    // and code for code — so the answer carries no information about whether
    // this particular route exists.
    let baseline = call(
        &service,
        &ai(CONNECTION),
        "POST",
        "/v1/mcp/no-such-route-at-all",
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(baseline.code, "APPROVAL_REQUIRED");
    for (path, body) in [
        ("/v1/mcp/auto-apply", range_for(CONNECTION)),
        ("/v1/mcp/auto-apply/autoapply_x/revoke", json!({})),
        ("/v1/mcp/connections/mcpconn_range/approve", json!({})),
    ] {
        let refused = call(&service, &ai(CONNECTION), "POST", path, body)
            .await
            .unwrap_err();
        assert_eq!(
            (refused.status, refused.code, refused.message),
            (
                baseline.status,
                baseline.code.clone(),
                baseline.message.clone()
            ),
            "POST {path} answered differently from an unknown route"
        );
    }

    // Nothing an agent sent changed anything: the person's range is as they
    // left it, and only they can see it.
    let listed = call(&service, &person(), "GET", "/v1/mcp/auto-apply", json!({}))
        .await
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["allow_update"], json!(false));
}

#[tokio::test]
async fn a_range_has_edges_it_cannot_be_saved_without() {
    let (_dir, service) = setup().await;
    let base = range_for(CONNECTION);

    // No expiry, and nothing beyond the ceiling: a standing permission nobody
    // revisits is one nobody remembers granting.
    for days in [0, -1, auto_apply::MAX_DAYS + 1] {
        let mut body = base.clone();
        body["days"] = json!(days);
        assert!(set_range(&service, body).await.is_err(), "days={days}");
    }

    // A range that permits nothing is a row that looks like a permission.
    let mut nothing = base.clone();
    nothing["allow_create"] = json!(false);
    nothing["allow_update"] = json!(false);
    assert!(set_range(&service, nothing).await.is_err());

    // An unknown connection, and one the person has not granted.
    let mut unknown = base.clone();
    unknown["connection_id"] = json!("mcpconn_nobody");
    assert!(set_range(&service, unknown).await.is_err());

    let mut tx = service.db.begin_write().await.unwrap();
    pathbase_api::mcp_auth::revoke(&mut tx, &person().id, CONNECTION)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        set_range(&service, base).await.is_err(),
        "a range over a revoked delegation would come back to life on reconnect"
    );
}

#[tokio::test]
async fn nothing_the_ai_sends_can_stand_in_for_a_range() {
    let (_dir, service) = setup().await;
    // No range at all.
    let change = propose(&service, &ai(CONNECTION), add_action("勝手に反映")).await;
    assert_eq!(change["auto_apply_eligible"], json!(false));
    assert_eq!(
        apply(&service, &ai(CONNECTION), &change)
            .await
            .unwrap_err()
            .code,
        "APPROVAL_REQUIRED"
    );

    // Claiming one in the proposal body does not create one.
    for smuggled in [
        json!({"title": "x", "operations": add_action("a"), "auto_apply_eligible": true}),
        json!({"title": "x", "operations": add_action("b"), "auto_applied": true}),
        json!({"title": "x", "operations": add_action("c"), "auto_apply_rule": "autoapply_1"}),
    ] {
        assert!(call(
            &service,
            &ai(CONNECTION),
            "POST",
            "/v1/workspaces/personal/changesets/preview",
            smuggled,
        )
        .await
        .is_err());
    }
}

#[tokio::test]
async fn an_expired_range_stops_working_without_anyone_doing_anything() {
    let (_dir, service) = setup().await;
    set_range(&service, range_for(CONNECTION)).await.unwrap();
    let change = propose(&service, &ai(CONNECTION), add_action("期限切れ後")).await;

    let mut tx = service.db.begin_write().await.unwrap();
    tx.execute(
        "UPDATE auto_apply_rules SET expires_at=? WHERE actor=?",
        &pathbase_api::params!["2026-09-18T00:00:00Z", person().id],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        apply(&service, &ai(CONNECTION), &change)
            .await
            .unwrap_err()
            .code,
        "APPROVAL_REQUIRED"
    );
}

#[tokio::test]
async fn the_person_approving_in_basepath_is_unchanged_by_any_of_this() {
    let (_dir, service) = setup().await;
    set_range(&service, range_for(CONNECTION)).await.unwrap();
    let change = propose(&service, &ai(CONNECTION), add_action("画面で承認")).await;

    // Approving still applies, still records who approved, and does not
    // pretend a range was involved.
    let approved = call(
        &service,
        &person(),
        "POST",
        &format!(
            "/v1/workspaces/personal/changesets/{}/approve",
            change["id"].as_str().unwrap()
        ),
        json!({"hash": change["hash"]}),
    )
    .await
    .unwrap();
    assert_eq!(approved["status"], "applied");
    assert_eq!(approved["approved_by"], person().id);
    assert!(
        approved["auto_applied"].is_null(),
        "a change the person approved must not read as auto-applied"
    );

    // A model relaying "I approved it" gets the change set, not a failure.
    let again = apply(&service, &ai(CONNECTION), &change).await.unwrap();
    assert_eq!(again["already_applied"], json!(true));
    assert!(again["results"].as_array().unwrap().is_empty());
}
