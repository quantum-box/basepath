//! The dashboard, and the four numbers it must never merge.
//!
//! This is where a goal product usually starts lying. Four tickets shipped
//! becomes "40% toward the revenue target", the number goes on a screen, and
//! it gets defended for a quarter. So the tests here are mostly about what the
//! dashboard refuses to say:
//!
//! - a rate over nothing is not 0%
//! - a goal with no metric is not 0%
//! - a goal with no stated rollup method has no derived progress at all
//! - signals are facts; deciding they mean "at risk" is a judgement with an
//!   author and a date
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service, String, Actor) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("dashboard.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    let owner = Actor {
        id: "us_alice".into(),
        agent: false,
        connection: None,
    };
    service.provision_personal(&owner).await.unwrap();
    let workspace = call(
        &service,
        &owner,
        "POST",
        "/v1/workspaces",
        json!({"name":"会社","scope":"チーム"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (dir, service, workspace, owner)
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

async fn query(
    service: &Service,
    who: &Actor,
    path: &str,
    params: &[(&str, &str)],
) -> Result<Value> {
    let map: HashMap<String, String> = params
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    service
        .handle(who, "GET", path, &map, json!({}), None)
        .await
}

async fn goal(service: &Service, who: &Actor, w: &str, title: &str, fields: Value) -> String {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{w}/items"),
        json!({"kind":"outcome","title":title,"fields":fields}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn metric(
    service: &Service,
    who: &Actor,
    w: &str,
    item: &str,
    name: &str,
    spec: Value,
) -> String {
    let mut body = json!({"item_id":item,"name":name,"unit":"件"});
    for (key, value) in spec.as_object().unwrap() {
        body[key] = value.clone();
    }
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{w}/metrics"),
        body,
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn observe(
    service: &Service,
    who: &Actor,
    w: &str,
    metric: &str,
    value: f64,
    at: &str,
) -> Value {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{w}/observations"),
        json!({"metric_id":metric,"value":value,"unit":"件","source":"手入力","observed_at":at}),
    )
    .await
    .unwrap()
}

fn row<'a>(board: &'a Value, id: &str) -> &'a Value {
    board["goals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(id))
        .expect("the goal should be on the dashboard")
}

#[tokio::test]
async fn a_goal_with_no_stated_method_reports_no_derived_progress() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "方法を決めていない目標", json!({})).await;
    let measure = metric(
        &service,
        &who,
        &w,
        &id,
        "件数",
        json!({"direction":"increase","baseline":0,"target":100}),
    )
    .await;
    observe(&service, &who, &w, &measure, 50.0, "2026-09-14T00:00:00Z").await;

    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    let goal = row(&board, &id);
    // The measurement is there and can be seen; what is absent is a number
    // claiming to be the goal's progress, because nobody said how to derive
    // one. A figure whose method is unstated cannot be argued with.
    assert!(goal["metric_progress"]["method"].is_null());
    assert!(goal["metric_progress"]["value"].is_null());
    assert_eq!(goal["metric_progress"]["metrics"][0]["progress"], 50.0);
    assert_eq!(board["without_rollup_method"], 1);
}

#[tokio::test]
async fn a_goal_with_no_metric_is_not_zero_percent() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(
        &service,
        &who,
        &w,
        "指標のない目標",
        json!({"rollup":"metric_average"}),
    )
    .await;

    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    let goal = row(&board, &id);
    // A method with nothing to apply it to produces nothing, not zero.
    assert!(goal["metric_progress"]["value"].is_null());
    assert_eq!(goal["metric_progress"]["counted"], 0);
    // And no action was planned, so there is no completion rate either.
    assert_eq!(goal["action_completion"]["total"], 0);
    assert!(goal["action_completion"]["rate"].is_null());
    // Nor a self-assessment nobody made.
    assert!(goal["self_assessment"].is_null());
    assert!(goal["health"].is_null());
}

#[tokio::test]
async fn the_four_numbers_stay_four_numbers() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(
        &service,
        &who,
        &w,
        "四つが並ぶ目標",
        json!({"rollup":"metric_average","self_assessment":30}),
    )
    .await;
    let measure = metric(
        &service,
        &who,
        &w,
        &id,
        "売上",
        json!({"direction":"increase","baseline":0,"target":100}),
    )
    .await;
    observe(&service, &who, &w, &measure, 80.0, "2026-09-14T00:00:00Z").await;

    // Two actions under it, one done.
    for (title, state) in [("やった行動", "done"), ("まだの行動", "active")] {
        let action = call(
            &service,
            &who,
            "POST",
            &format!("/v1/workspaces/{w}/items"),
            json!({"kind":"action","title":title}),
        )
        .await
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        if state == "done" {
            // Completion goes through the completion route, not a state edit:
            // that is what makes the record evidence rather than a claim.
            call(
                &service,
                &who,
                "POST",
                &format!("/v1/workspaces/{w}/actions/{action}/complete"),
                json!({"expected_version":1,"local_date":"2026-09-14",
                       "completed_at":"2026-09-14T01:00:00Z"}),
            )
            .await
            .unwrap();
        }
        call(
            &service,
            &who,
            "POST",
            &format!("/v1/workspaces/{w}/relations"),
            json!({"source_id":action,"target_id":id,"type":"part_of"}),
        )
        .await
        .unwrap();
    }
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/health"),
        json!({"status":"at_risk","note":"人手が足りない","expected_version":1}),
    )
    .await
    .unwrap();

    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    let goal = row(&board, &id);
    // Four separate facts, four separate fields, all different numbers.
    assert_eq!(goal["action_completion"]["rate"], 50.0);
    assert_eq!(goal["metric_progress"]["value"], 80.0);
    assert_eq!(goal["self_assessment"], 30.0);
    assert_eq!(goal["health"]["status"], "at_risk");
    // The method is named, so the 80 can be argued with.
    assert_eq!(goal["metric_progress"]["method"], "metric_average");
    assert_eq!(goal["metric_progress"]["source"], "metrics");
    assert_eq!(goal["metric_progress"]["counted"], 1);
}

#[tokio::test]
async fn health_carries_the_name_of_whoever_decided_it() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "状況を記録する目標", json!({})).await;

    let after = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/health"),
        json!({"status":"off_track","note":"外部要因","expected_version":1}),
    )
    .await
    .unwrap();
    assert_eq!(after["fields"]["health"]["status"], "off_track");
    assert_eq!(after["fields"]["health"]["set_by"], "us_alice");
    assert!(after["fields"]["health"]["set_at"].as_str().unwrap().len() > 10);

    // The author and the time are the server's to stamp: a client cannot sign
    // a judgement as someone else, or backdate one.
    assert!(call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/health"),
        json!({"status":"on_track","set_by":"us_bob","expected_version":2}),
    )
    .await
    .is_err());
    assert!(call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/health"),
        json!({"status":"絶好調","expected_version":2}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn signals_are_facts_and_the_suggestion_never_becomes_the_health() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(
        &service,
        &who,
        &w,
        "兆候のある目標",
        json!({"rollup":"metric_average"}),
    )
    .await;
    call(
        &service,
        &who,
        "PATCH",
        &format!("/v1/workspaces/{w}/items/{id}"),
        json!({"expected_version":1,"due_date":"2020-01-01"}),
    )
    .await
    .unwrap();
    metric(
        &service,
        &who,
        &w,
        &id,
        "測っていない指標",
        json!({"direction":"increase","baseline":0,"target":10}),
    )
    .await;

    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    let goal = row(&board, &id);
    assert_eq!(goal["signals"]["overdue"], true);
    assert_eq!(goal["signals"]["unmeasured_metrics"], 1);
    assert!(goal["signals"]["days_since_checkin"].is_null());

    // A suggestion, with its reasons listed.
    assert_eq!(goal["suggested_health"]["status"], "off_track");
    assert!(
        goal["suggested_health"]["reasons"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    // And the health itself is still unknown, because nobody has said.
    assert!(goal["health"].is_null());
    assert_eq!(board["by_health"]["unknown"], 1);
    assert_eq!(board["by_health"]["off_track"], 0);
}

#[tokio::test]
async fn a_rollup_says_which_method_it_used_and_what_it_left_out() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(
        &service,
        &who,
        &w,
        "複数指標の目標",
        json!({"rollup":"metric_worst"}),
    )
    .await;
    let a = metric(
        &service,
        &who,
        &w,
        &id,
        "進んでいる指標",
        json!({"direction":"increase","baseline":0,"target":100}),
    )
    .await;
    let b = metric(
        &service,
        &who,
        &w,
        &id,
        "遅れている指標",
        json!({"direction":"increase","baseline":0,"target":100}),
    )
    .await;
    metric(
        &service,
        &who,
        &w,
        &id,
        "測っていない指標",
        json!({"direction":"increase","baseline":0,"target":100}),
    )
    .await;
    observe(&service, &who, &w, &a, 90.0, "2026-09-14T00:00:00Z").await;
    observe(&service, &who, &w, &b, 20.0, "2026-09-14T00:00:00Z").await;

    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    let goal = row(&board, &id);
    // The worst, not the average: a goal is not on track because two of its
    // three measures are.
    assert_eq!(goal["metric_progress"]["value"], 20.0);
    assert_eq!(goal["metric_progress"]["counted"], 2);
    // And it says what it could not count, rather than quietly averaging two.
    assert_eq!(goal["metric_progress"]["missing"], 1);

    // Switching the method changes the number, and says so.
    call(
        &service,
        &who,
        "PATCH",
        &format!("/v1/workspaces/{w}/items/{id}"),
        json!({"expected_version":1,"fields":{"rollup":"metric_average"}}),
    )
    .await
    .unwrap();
    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(row(&board, &id)["metric_progress"]["value"], 55.0);
    assert_eq!(
        row(&board, &id)["metric_progress"]["method"],
        "metric_average"
    );

    // An invented method is refused rather than silently ignored.
    assert!(call(
        &service,
        &who,
        "PATCH",
        &format!("/v1/workspaces/{w}/items/{id}"),
        json!({"expected_version":2,"fields":{"rollup":"vibes"}}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn a_parent_rolls_up_its_children_only_when_it_was_told_to() {
    let (_dir, service, w, who) = setup().await;
    let parent = goal(&service, &who, &w, "上位の目標", json!({})).await;
    for (title, value) in [("子A", 40.0), ("子B", 80.0)] {
        let child = goal(
            &service,
            &who,
            &w,
            title,
            json!({"rollup":"metric_average"}),
        )
        .await;
        let measure = metric(
            &service,
            &who,
            &w,
            &child,
            "指標",
            json!({"direction":"increase","baseline":0,"target":100}),
        )
        .await;
        observe(&service, &who, &w, &measure, value, "2026-09-14T00:00:00Z").await;
        call(
            &service,
            &who,
            "POST",
            &format!("/v1/workspaces/{w}/relations"),
            json!({"source_id":child,"target_id":parent,"type":"part_of"}),
        )
        .await
        .unwrap();
    }

    // No method on the parent: no number, even though its children have them.
    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    assert!(row(&board, &parent)["metric_progress"]["value"].is_null());

    call(
        &service,
        &who,
        "PATCH",
        &format!("/v1/workspaces/{w}/items/{parent}"),
        json!({"expected_version":1,"fields":{"rollup":"children_average"}}),
    )
    .await
    .unwrap();
    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    let rolled = row(&board, &parent);
    assert_eq!(rolled["metric_progress"]["value"], 60.0);
    assert_eq!(rolled["metric_progress"]["source"], "children");
    assert_eq!(rolled["metric_progress"]["counted"], 2);
}

#[tokio::test]
async fn a_corrected_observation_replaces_the_value_and_keeps_the_history() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(
        &service,
        &who,
        &w,
        "訂正のある目標",
        json!({"rollup":"metric_average"}),
    )
    .await;
    let measure = metric(
        &service,
        &who,
        &w,
        &id,
        "指標",
        json!({"direction":"increase","baseline":0,"target":100}),
    )
    .await;
    let first = observe(&service, &who, &w, &measure, 90.0, "2026-09-14T00:00:00Z").await;

    // The 90 was wrong. A correction is appended, not written over.
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/observations"),
        json!({"metric_id":measure,"value":40,"unit":"件","source":"再集計",
               "observed_at":"2026-09-15T00:00:00Z","supersedes_id":first["id"]}),
    )
    .await
    .unwrap();

    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    let goal = row(&board, &id);
    assert_eq!(goal["metric_progress"]["value"], 40.0);
    assert_eq!(goal["metric_progress"]["metrics"][0]["latest"], 40.0);

    // What was believed before is still readable, which is why a correction is
    // recorded rather than an edit.
    let observations = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/observations"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(observations["items"].as_array().unwrap().len(), 2);
    assert!(observations["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["value"] == 90.0));
}

#[tokio::test]
async fn every_number_can_be_followed_back_to_what_produced_it() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(
        &service,
        &who,
        &w,
        "根拠のある目標",
        json!({"rollup":"metric_average"}),
    )
    .await;
    let measure = metric(
        &service,
        &who,
        &w,
        &id,
        "指標",
        json!({"direction":"decrease","baseline":100,"target":50}),
    )
    .await;
    let observation = observe(&service, &who, &w, &measure, 75.0, "2026-09-14T00:00:00Z").await;

    let board = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    let shown = &row(&board, &id)["metric_progress"]["metrics"][0];
    // Halfway from 100 down to 50.
    assert_eq!(shown["progress"], 50.0);
    assert_eq!(shown["metric_id"], measure);
    assert_eq!(shown["latest_observation_id"], observation["id"]);
    assert_eq!(shown["observed_at"], "2026-09-14T00:00:00Z");
    assert_eq!(shown["baseline"], 100.0);
    assert_eq!(shown["target"], 50.0);
    assert_eq!(shown["direction"], "decrease");
}

#[tokio::test]
async fn a_workspace_you_are_not_in_reveals_nothing_not_even_a_count() {
    let (_dir, service, w, who) = setup().await;
    goal(&service, &who, &w, "他人には見えない目標", json!({})).await;

    let stranger = Actor {
        id: "us_stranger".into(),
        agent: false,
        connection: None,
    };
    let refused = query(
        &service,
        &stranger,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap_err();
    // 404, not 403: whether this workspace exists is itself not their business.
    assert_eq!(refused.status, 404);
    let serialized = serde_json::to_string(&refused).unwrap();
    assert!(!serialized.contains("目標"), "{serialized}");
}

#[tokio::test]
async fn the_dashboard_narrows_by_owner_and_by_period() {
    let (_dir, service, w, who) = setup().await;
    let cycle = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/cycles"),
        json!({"cadence":"quarter","start_date":"2026-10-01"}),
    )
    .await
    .unwrap();
    goal(
        &service,
        &who,
        &w,
        "今期の会社目標",
        json!({"owner":{"kind":"organization"},"cycle_id":cycle["id"]}),
    )
    .await;
    goal(
        &service,
        &who,
        &w,
        "チームの目標",
        json!({"owner":{"kind":"team","id":"運営"}}),
    )
    .await;

    let company = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[("owner_kind", "organization")],
    )
    .await
    .unwrap();
    assert_eq!(company["goal_count"], 1);
    assert_eq!(company["goals"][0]["title"], "今期の会社目標");
    assert_eq!(company["goals"][0]["cycle"]["label"], "2026 Q4");

    let in_cycle = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[("cycle_id", cycle["id"].as_str().unwrap())],
    )
    .await
    .unwrap();
    assert_eq!(in_cycle["goal_count"], 1);

    // There is no single "how is the organisation doing" figure: the summary
    // is counts by stated health, which is what is actually known.
    let all = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/dashboard"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(all["goal_count"], 2);
    assert_eq!(all["by_health"]["unknown"], 2);
    assert!(all.get("overall_progress").is_none());
}
