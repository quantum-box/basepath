//! The weekly review, from the server's side.
//!
//! Two things are being protected here, and they are easy to lose:
//!
//! 1. **The numbers say what was measured, and nothing more.** An unmeasured
//!    metric stays null; it does not become 0, and it does not borrow the
//!    previous week. Completing actions is not achieving a goal, so the two
//!    never merge into one score.
//! 2. **A review is finished when the person says so.** An AI may draft the
//!    text, but the draft reaches the plan only through a change set the
//!    person approves, and finalizing the week is not something a proposal can
//!    do at all.
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
    let service = Service::open(&dir.path().join("weekly.sqlite3").to_string_lossy())
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

async fn summary(service: &Service, who: &Actor, week: &str) -> Value {
    let mut query = HashMap::new();
    query.insert("week_start".to_string(), week.to_string());
    service
        .handle(
            who,
            "GET",
            "/v1/workspaces/personal/weekly-review",
            &query,
            json!({}),
            None,
        )
        .await
        .unwrap()
}

/// The week the fixtures below sit in. A Monday, because the server requires one.
const WEEK: &str = "2026-09-14";

#[tokio::test]
async fn a_week_with_nothing_in_it_reports_nothing_rather_than_zero() {
    let (_dir, service) = setup().await;
    let person = Actor::local();
    let view = summary(&service, &person, WEEK).await;

    assert_eq!(view["week_start"], WEEK);
    assert_eq!(view["week_end"], "2026-09-20");
    assert_eq!(view["actions"]["total"], 0);
    assert!(view["actions"]["items"].as_array().unwrap().is_empty());
    assert!(view["metrics"].as_array().unwrap().is_empty());
    assert!(view["review"].is_null(), "no review has been written yet");
    // A personal workspace has no member breakdown to show.
    assert!(view["members"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_week_must_start_on_monday_in_the_workspaces_own_calendar() {
    let (_dir, service) = setup().await;
    let person = Actor::local();
    let mut query = HashMap::new();
    query.insert("week_start".to_string(), "2026-09-15".to_string());
    let refused = service
        .handle(
            &person,
            "GET",
            "/v1/workspaces/personal/weekly-review",
            &query,
            json!({}),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, "VALIDATION_ERROR");

    // The boundary is inclusive on both ends: Monday and the following Sunday.
    let view = summary(&service, &person, WEEK).await;
    assert_eq!(view["week_start"], "2026-09-14");
    assert_eq!(view["week_end"], "2026-09-20");
}

#[tokio::test]
async fn an_unmeasured_metric_is_not_a_measured_zero() {
    let (_dir, service) = setup().await;
    let person = Actor::local();
    let goal = call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"体力をつける","kind":"outcome"}),
    )
    .await
    .unwrap();
    let goal_id = goal["id"].as_str().unwrap().to_owned();
    call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/metrics",
        json!({"item_id":goal_id,"name":"走行距離","unit":"km","direction":"increase","baseline":0,"target":50}),
    )
    .await
    .unwrap();

    let view = summary(&service, &person, WEEK).await;
    let metric = &view["metrics"][0];
    assert!(
        metric["latest"].is_null(),
        "never observed, so never a number"
    );
    assert!(metric["previous"].is_null());
    assert!(metric["delta"].is_null(), "a delta needs two measurements");
    assert_eq!(metric["status"], "unmeasured");

    // A goal nobody has assessed is unassessed, not scored 0.
    assert!(view["goals"][0]["self_assessment"].is_null());
    // And its assessment is reported apart from action completion.
    assert_eq!(view["actions"]["total"], 0);
}

#[tokio::test]
async fn an_ai_can_draft_the_review_but_only_the_person_saves_it() {
    let (_dir, service) = setup().await;
    let person = Actor::local();
    let ai = agent("local-owner");

    // Writing the review directly is refused: this connection proposes.
    let refused = call(
        &service,
        &ai,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":WEEK,"learnings":"AIが直接書いた","challenges":"","next_focus":""}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "APPROVAL_REQUIRED");

    let change = call(
        &service,
        &ai,
        "POST",
        "/v1/workspaces/personal/changesets/preview",
        json!({"title":"週次レビュー案","operations":[{
            "method":"POST",
            "path":"/v1/workspaces/personal/weekly-reviews/draft",
            "body":{"week_start":WEEK,"learnings":"観測: 3回完了","challenges":"推測: 移動時間が原因","next_focus":"質問: 朝に動かせますか"}
        }]}),
    )
    .await
    .unwrap();
    let id = change["id"].as_str().unwrap().to_owned();

    // The diff names the week and shows the text that would be saved.
    let row = &change["changes"][0];
    assert_eq!(row["collection"], "weekly_reviews");
    assert_eq!(row["title"], format!("{WEEK}の週次レビュー"));
    assert!(
        row["before"].is_null(),
        "nothing was written for this week yet"
    );
    assert_eq!(row["after"]["learnings"], "観測: 3回完了");
    assert_eq!(row["after"]["status"], "draft");

    // Previewing wrote nothing.
    assert!(summary(&service, &person, WEEK).await["review"].is_null());

    // The person approves on their own surface, then it applies.
    call(
        &service,
        &person,
        "POST",
        &format!("/v1/workspaces/personal/changesets/{id}/approve"),
        json!({"hash":change["hash"]}),
    )
    .await
    .unwrap();
    call(
        &service,
        &person,
        "POST",
        &format!("/v1/workspaces/personal/changesets/{id}/apply"),
        json!({}),
    )
    .await
    .unwrap();

    let view = summary(&service, &person, WEEK).await;
    assert_eq!(view["review"]["learnings"], "観測: 3回完了");
    // Approving the text is not declaring the week reviewed.
    assert_eq!(view["review"]["status"], "draft");
}

#[tokio::test]
async fn a_proposal_can_never_finalize_a_week() {
    let (_dir, service) = setup().await;
    let person = Actor::local();
    let ai = agent("local-owner");

    let saved = call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":WEEK,"learnings":"自分で書いた","challenges":"","next_focus":""}),
    )
    .await
    .unwrap();
    let review_id = saved["id"].as_str().unwrap().to_owned();

    for operation in [
        json!({"method":"POST","path":format!("/v1/workspaces/personal/weekly-reviews/{review_id}/finalize"),"body":{"expected_version":1}}),
        json!({"method":"PATCH","path":format!("/v1/workspaces/personal/weekly-reviews/{review_id}"),"body":{"status":"finalized"}}),
    ] {
        let refused = call(
            &service,
            &ai,
            "POST",
            "/v1/workspaces/personal/changesets/preview",
            json!({"title":"確定させたい","operations":[operation]}),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, "VALIDATION_ERROR", "{refused:?}");
    }

    assert_eq!(
        summary(&service, &person, WEEK).await["review"]["status"],
        "draft",
        "the week is still the person's to finish"
    );
}

#[tokio::test]
async fn a_proposal_written_against_an_older_draft_is_refused_on_apply() {
    let (_dir, service) = setup().await;
    let person = Actor::local();
    let ai = agent("local-owner");

    let first = call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":WEEK,"learnings":"最初の下書き","challenges":"","next_focus":""}),
    )
    .await
    .unwrap();
    assert_eq!(first["version"], 1);

    let change = call(
        &service,
        &ai,
        "POST",
        "/v1/workspaces/personal/changesets/preview",
        json!({"title":"AIの加筆","operations":[{
            "method":"POST",
            "path":"/v1/workspaces/personal/weekly-reviews/draft",
            "body":{"week_start":WEEK,"learnings":"AIの加筆","challenges":"","next_focus":"","expected_version":1}
        }]}),
    )
    .await
    .unwrap();
    let id = change["id"].as_str().unwrap().to_owned();
    // The proposal shows the person's own words as what it would replace.
    assert_eq!(change["changes"][0]["before"]["learnings"], "最初の下書き");

    // Meanwhile the person keeps writing.
    call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":WEEK,"learnings":"自分で書き直した","challenges":"","next_focus":"","expected_version":1}),
    )
    .await
    .unwrap();

    call(
        &service,
        &person,
        "POST",
        &format!("/v1/workspaces/personal/changesets/{id}/approve"),
        json!({"hash":change["hash"]}),
    )
    .await
    .unwrap();
    let refused = call(
        &service,
        &person,
        "POST",
        &format!("/v1/workspaces/personal/changesets/{id}/apply"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "VERSION_CONFLICT", "{refused:?}");

    // The person's text survived the refusal.
    assert_eq!(
        summary(&service, &person, WEEK).await["review"]["learnings"],
        "自分で書き直した"
    );
}

#[tokio::test]
async fn a_correction_keeps_what_the_earlier_revision_said() {
    let (_dir, service) = setup().await;
    let person = Actor::local();

    let draft = call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":WEEK,"learnings":"第1版","challenges":"","next_focus":""}),
    )
    .await
    .unwrap();
    let first_id = draft["id"].as_str().unwrap().to_owned();
    call(
        &service,
        &person,
        "POST",
        &format!("/v1/workspaces/personal/weekly-reviews/{first_id}/finalize"),
        json!({"expected_version":1}),
    )
    .await
    .unwrap();

    // Correcting starts a new revision rather than editing the finalized one.
    let corrected = call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":WEEK,"learnings":"第2版","challenges":"","next_focus":""}),
    )
    .await
    .unwrap();
    assert_eq!(corrected["revision"], 2);
    assert_eq!(corrected["supersedes_id"], first_id);

    let view = summary(&service, &person, WEEK).await;
    let history = view["history"].as_array().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["learnings"], "第1版");
    assert_eq!(history[0]["status"], "finalized");
    assert_eq!(view["review"]["learnings"], "第2版");
    assert_eq!(view["review"]["status"], "draft");
}

#[tokio::test]
async fn a_review_with_nothing_written_cannot_be_declared_finished() {
    let (_dir, service) = setup().await;
    let person = Actor::local();
    let draft = call(
        &service,
        &person,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":WEEK,"learnings":"   ","challenges":"","next_focus":""}),
    )
    .await
    .unwrap();
    let id = draft["id"].as_str().unwrap().to_owned();
    let refused = call(
        &service,
        &person,
        "POST",
        &format!("/v1/workspaces/personal/weekly-reviews/{id}/finalize"),
        json!({"expected_version":1}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "VALIDATION_ERROR");
}
