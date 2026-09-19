//! What an AI is given before it proposes a breakdown, and what it is refused.
//!
//! Basepath runs no model, so there is nothing here that generates a plan.
//! What is testable — and what actually protects the person — is the contract
//! around the generation:
//!
//! 1. **The holes are questions, not blanks to fill.** A goal with no
//!    deadline and no measure gets a list of things to ask, not defaults.
//! 2. **A committing value needs a source.** A date, a target, an owner or a
//!    self-assessment from an AI is refused unless the proposal says where it
//!    came from — because after approval it reads as the person's decision.
//! 3. **Re-breaking-down does not quietly delete.** A second proposal that
//!    forgets existing work reports it as a candidate, never as a removal.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service, String, Actor) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("copilot.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    let who = person("us_alice");
    let workspace = call(
        &service,
        &who,
        "POST",
        "/v1/workspaces",
        json!({"name":"会社","scope":"チーム"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (dir, service, workspace, who)
}

fn person(id: &str) -> Actor {
    Actor {
        id: id.into(),
        agent: false,
        connection: None,
    }
}

fn agent(id: &str) -> Actor {
    Actor {
        id: id.into(),
        agent: true,
        connection: Some("mcpconn_test".into()),
    }
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

async fn query(service: &Service, who: &Actor, path: &str) -> Result<Value> {
    service
        .handle(who, "GET", path, &HashMap::new(), json!({}), None)
        .await
}

async fn item(service: &Service, who: &Actor, w: &str, body: Value) -> String {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{w}/items"),
        body,
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// One proposal, as an AI would send it.
async fn propose(
    service: &Service,
    ai: &Actor,
    w: &str,
    operations: Value,
    assumptions: Value,
) -> Result<Value> {
    call(
        service,
        ai,
        "POST",
        &format!("/v1/workspaces/{w}/changesets/preview"),
        json!({"title":"分解案","operations":operations,"assumptions":assumptions}),
    )
    .await
}

#[tokio::test]
async fn the_brief_asks_rather_than_guesses() {
    let (_dir, service, w, who) = setup().await;
    let goal = item(&service, &who, &w, json!({"kind":"outcome","title":"海外"})).await;

    let brief = query(
        &service,
        &agent("us_alice"),
        &format!("/v1/workspaces/{w}/items/{goal}/breakdown-brief"),
    )
    .await
    .unwrap();
    let asked: Vec<&str> = brief["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| question["about"].as_str().unwrap())
        .collect();
    // No deadline, no measure, a title too short to mean one thing, and no
    // owner in a shared workspace. Four questions, not four defaults.
    assert!(asked.contains(&"due_date"), "{asked:?}");
    assert!(asked.contains(&"metric"), "{asked:?}");
    assert!(asked.contains(&"meaning"), "{asked:?}");
    assert!(asked.contains(&"owner"), "{asked:?}");
    // Each question is phrased for the person to answer, with why it matters.
    let first = &brief["questions"][0];
    assert!(!first["ask"].as_str().unwrap().is_empty());
    assert!(!first["why"].as_str().unwrap().is_empty());
    // Which context this is comes from the workspace, not from the caller.
    assert_eq!(brief["context_kind"], json!("organization"));
}

#[tokio::test]
async fn a_goal_that_says_enough_is_not_interrogated() {
    let (_dir, service, w, who) = setup().await;
    let goal = item(
        &service,
        &who,
        &w,
        json!({"kind":"outcome","title":"3年後に海外売上を30%にする",
               "description":"既存プロダクトのまま、英語圏の中堅企業へ",
               "due_date":"2029-03-31","fields":{"assignee_id":"us_alice"}}),
    )
    .await;
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/metrics"),
        json!({"item_id":goal,"name":"海外売上比率","unit":"%","baseline":8.0,"target":30.0,
               "direction":"increase"}),
    )
    .await
    .unwrap();

    let brief = query(
        &service,
        &agent("us_alice"),
        &format!("/v1/workspaces/{w}/items/{goal}/breakdown-brief"),
    )
    .await
    .unwrap();
    assert_eq!(brief["questions"].as_array().unwrap().len(), 0);
    assert_eq!(brief["metrics"][0]["name"], json!("海外売上比率"));
}

#[tokio::test]
async fn an_ai_cannot_set_a_date_without_saying_where_it_came_from() {
    let (_dir, service, w, who) = setup().await;
    let goal = item(
        &service,
        &who,
        &w,
        json!({"kind":"outcome","title":"海外売上を伸ばす"}),
    )
    .await;
    let ai = agent("us_alice");

    let refused = propose(
        &service,
        &ai,
        &w,
        json!([{"method":"POST","path":format!("/v1/workspaces/{w}/items"),
                "body":{"kind":"initiative","title":"英語サイトを整える",
                        "due_date":"2027-03-31"}}]),
        json!([]),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.status, 422);
    assert!(refused.message.contains("due_date"), "{}", refused.message);
    // And the message says what to do instead of the value.
    assert!(refused.message.contains("basis"), "{}", refused.message);

    // With a source it goes through, and the source travels with the diff so
    // the person can check it rather than take it on trust.
    let accepted = propose(
        &service,
        &ai,
        &w,
        json!([{"method":"POST","path":format!("/v1/workspaces/{w}/items"),
                "body":{"kind":"initiative","title":"英語サイトを整える",
                        "due_date":"2027-03-31"},
                "basis":"本人が「2027年度の期初までに」と話していたため"}]),
        json!(["英語圏から先に着手する前提で並べています"]),
    )
    .await
    .unwrap();
    assert_eq!(
        accepted["changes"][0]["guarded_values"],
        json!(["due_date"])
    );
    assert_eq!(
        accepted["changes"][0]["basis"],
        json!("本人が「2027年度の期初までに」と話していたため")
    );
    assert_eq!(
        accepted["assumptions"],
        json!(["英語圏から先に着手する前提で並べています"])
    );
    // Still only a proposal. Nothing was written.
    assert_eq!(accepted["status"], json!("pending"));
    let items = query(&service, &who, &format!("/v1/workspaces/{w}/items"))
        .await
        .unwrap();
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
    assert_eq!(items["items"][0]["id"], json!(goal));
}

#[tokio::test]
async fn every_committing_value_needs_the_same_thing() {
    let (_dir, service, w, who) = setup().await;
    let goal = item(
        &service,
        &who,
        &w,
        json!({"kind":"outcome","title":"継続率を上げる"}),
    )
    .await;
    let ai = agent("us_alice");

    for body in [
        json!({"kind":"action","title":"面談する","scheduled_date":"2026-10-01"}),
        json!({"kind":"action","title":"面談する","start_date":"2026-10-01"}),
        json!({"kind":"action","title":"面談する","fields":{"assignee_id":"us_bob"}}),
        json!({"kind":"action","title":"面談する","fields":{"self_assessment":60}}),
    ] {
        let refused = propose(
            &service,
            &ai,
            &w,
            json!([{"method":"POST","path":format!("/v1/workspaces/{w}/items"),"body":body}]),
            json!([]),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.status, 422, "{body}");
    }

    // A metric's target and baseline are the same kind of claim.
    let refused = propose(
        &service,
        &ai,
        &w,
        json!([{"method":"POST","path":format!("/v1/workspaces/{w}/metrics"),
                "body":{"item_id":goal,"name":"継続率","unit":"%","target":95.0}}]),
        json!([]),
    )
    .await
    .unwrap_err();
    assert!(refused.message.contains("target"), "{}", refused.message);

    // Structure itself needs no source: proposing that something is part of
    // something else is a suggestion, not a commitment about a number.
    let fine = propose(
        &service,
        &ai,
        &w,
        json!([{"method":"POST","path":format!("/v1/workspaces/{w}/items"),
                "body":{"kind":"initiative","title":"導入直後の伴走"}}]),
        json!([]),
    )
    .await;
    assert!(fine.is_ok(), "{:?}", fine.err());
}

#[tokio::test]
async fn a_person_setting_their_own_date_is_not_making_a_claim() {
    let (_dir, service, w, who) = setup().await;
    // The rule is about an AI asserting a value, not about dates. A person
    // proposing their own plan with their own deadline needs no citation.
    let fine = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/changesets/preview"),
        json!({"title":"自分の案","operations":[
            {"method":"POST","path":format!("/v1/workspaces/{w}/items"),
             "body":{"kind":"action","title":"来週やる","due_date":"2026-09-30"}}]}),
    )
    .await;
    assert!(fine.is_ok(), "{:?}", fine.err());
}

#[tokio::test]
async fn re_breaking_down_shows_what_it_would_keep_change_add_and_forget() {
    let (_dir, service, w, who) = setup().await;
    let goal = item(
        &service,
        &who,
        &w,
        json!({"kind":"outcome","title":"解約を減らす"}),
    )
    .await;
    for title in ["導入直後の伴走", "利用状況の可視化", "解約理由の聞き取り"]
    {
        item(
            &service,
            &who,
            &w,
            json!({"kind":"initiative","title":title,"parent_id":goal}),
        )
        .await;
    }

    let compared = call(
        &service,
        &agent("us_alice"),
        "POST",
        &format!("/v1/workspaces/{w}/items/{goal}/breakdown-comparison"),
        json!({"children":[
            {"title":"導入直後の伴走","kind":"initiative","rationale":"離脱は初月に集中"},
            {"title":"利用状況の可視化と通知","kind":"initiative","rationale":"兆候を早く掴む"},
            {"title":"価格プランの見直し","kind":"initiative","rationale":"値ごろ感の不一致"}
        ]}),
    )
    .await
    .unwrap();
    let verdict = |title: &str| {
        compared["comparison"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["proposed_title"] == json!(title) || row["existing_title"] == json!(title)
            })
            .map(|row| row["verdict"].as_str().unwrap().to_owned())
    };
    assert_eq!(verdict("導入直後の伴走"), Some("keep".into()));
    // Near-identical wording is a change to the same line of work, not a
    // second one: proposing both is how a plan quietly doubles.
    assert_eq!(verdict("利用状況の可視化と通知"), Some("change".into()));
    assert_eq!(verdict("価格プランの見直し"), Some("add".into()));
    // Forgotten, not deleted. Work already underway is not removed because an
    // AI did not think of it.
    assert_eq!(
        verdict("解約理由の聞き取り"),
        Some("remove_candidate".into())
    );

    assert_eq!(compared["applied"], json!(false));
    assert_eq!(compared["removed"], json!(false));
    // And nothing moved.
    let still = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{goal}/breakdown"),
    )
    .await
    .unwrap();
    assert_eq!(still["nodes"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn a_comparison_is_a_read_and_the_proposal_is_still_the_only_way_to_write() {
    let (_dir, service, w, who) = setup().await;
    let goal = item(&service, &who, &w, json!({"kind":"outcome","title":"目標"})).await;
    let ai = agent("us_alice");

    // An agent may compare — it changes nothing.
    call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/items/{goal}/breakdown-comparison"),
        json!({"children":[{"title":"候補"}]}),
    )
    .await
    .unwrap();

    // It still may not write the item the comparison suggested.
    let refused = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/items"),
        json!({"kind":"initiative","title":"候補","parent_id":goal}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "APPROVAL_REQUIRED");
}

#[tokio::test]
async fn an_expired_or_overtaken_proposal_is_not_applied() {
    let (_dir, service, w, who) = setup().await;
    let ai = agent("us_alice");
    let change = propose(
        &service,
        &ai,
        &w,
        json!([{"method":"POST","path":format!("/v1/workspaces/{w}/items"),
                "body":{"kind":"outcome","title":"提案された目標"}}]),
        json!([]),
    )
    .await
    .unwrap();
    let id = change["id"].as_str().unwrap().to_owned();

    // The plan moves on under it — the person edits it themselves while the
    // proposal sits waiting.
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items"),
        json!({"kind":"outcome","title":"先に自分で作った目標"}),
    )
    .await
    .unwrap();

    let stale = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/changesets/{id}/approve"),
        json!({"hash":change["hash"]}),
    )
    .await
    .unwrap_err();
    assert_eq!(stale.code, "VERSION_CONFLICT");
    // Nothing from the stale proposal exists.
    let items = query(&service, &who, &format!("/v1/workspaces/{w}/items"))
        .await
        .unwrap();
    assert!(!items.to_string().contains("提案された目標"));
}

#[tokio::test]
async fn the_brief_stops_at_the_workspace_it_is_asked_about() {
    let (_dir, service, w, who) = setup().await;
    let other = call(
        &service,
        &who,
        "POST",
        "/v1/workspaces",
        json!({"name":"別の会社","scope":"チーム"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let theirs = item(
        &service,
        &who,
        &other,
        json!({"kind":"outcome","title":"あちらの目標"}),
    )
    .await;

    let refused = query(
        &service,
        &agent("us_alice"),
        &format!("/v1/workspaces/{w}/items/{theirs}/breakdown-brief"),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.status, 404);
}

#[tokio::test]
async fn a_personal_brief_says_it_is_personal() {
    let (_dir, service, _w, who) = setup().await;
    service.provision_personal(&who).await.unwrap();
    let personal = query(&service, &who, "/v1/workspaces")
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["scope"] == "個人")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let goal = item(
        &service,
        &who,
        &personal,
        json!({"kind":"outcome","title":"英語で会議を進行できるようになる"}),
    )
    .await;

    let brief = query(
        &service,
        &agent("us_alice"),
        &format!("/v1/workspaces/{personal}/items/{goal}/breakdown-brief"),
    )
    .await
    .unwrap();
    assert_eq!(brief["context_kind"], json!("personal"));
    // Nobody is assigned a personal goal, so it is not asked about.
    let asked: Vec<&str> = brief["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| question["about"].as_str().unwrap())
        .collect();
    assert!(!asked.contains(&"owner"), "{asked:?}");
}

#[tokio::test]
async fn a_periods_own_dates_are_the_period_not_a_guess_about_a_goal() {
    let (_dir, service, w, _who) = setup().await;
    // "This quarter, 1 July to 30 September" is one fact, and the route
    // already refuses a quarter that does not start on one. Asking where that
    // start date "came from" would be asking where July came from.
    let proposed = propose(
        &service,
        &agent("us_alice"),
        &w,
        json!([{"method":"POST","path":format!("/v1/workspaces/{w}/cycles"),
                "body":{"cadence":"quarter","start_date":"2026-07-01"}}]),
        json!([]),
    )
    .await;
    assert!(proposed.is_ok(), "{:?}", proposed.err());
}
