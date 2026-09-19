//! Planning periods, and what they must not break.
//!
//! A period is a frame around work, not a container that owns it. Most of
//! these tests are about that distinction: a workspace that has never made a
//! period keeps working, carrying work forward copies rather than moves, and
//! closing a period does not erase what was in it.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("planning.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    (dir, service)
}

async fn call(service: &Service, method: &str, path: &str, body: Value) -> Result<Value> {
    service
        .handle(
            &Actor::local(),
            method,
            path,
            &HashMap::new(),
            body,
            Some(&uuid::Uuid::new_v4().to_string()),
        )
        .await
}

async fn query(service: &Service, path: &str, params: &[(&str, &str)]) -> Result<Value> {
    let map: HashMap<String, String> = params
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    service
        .handle(&Actor::local(), "GET", path, &map, json!({}), None)
        .await
}

async fn cycle(service: &Service, body: Value) -> Result<Value> {
    call(service, "POST", "/v1/workspaces/personal/cycles", body).await
}

#[tokio::test]
async fn a_cadence_decides_its_own_length_and_its_own_name() {
    let (_dir, service) = setup().await;

    let quarter = cycle(
        &service,
        json!({"cadence":"quarter","start_date":"2026-10-01"}),
    )
    .await
    .unwrap();
    assert_eq!(quarter["end_date"], "2026-12-31");
    assert_eq!(quarter["label"], "2026 Q4");
    assert_eq!(quarter["status"], "planned");

    let month = cycle(
        &service,
        json!({"cadence":"month","start_date":"2027-02-01"}),
    )
    .await
    .unwrap();
    // February, without anyone having to say how long February is.
    assert_eq!(month["end_date"], "2027-02-28");
    assert_eq!(month["label"], "2027-02");

    let week = cycle(
        &service,
        json!({"cadence":"week","start_date":"2026-09-14"}),
    )
    .await
    .unwrap();
    assert_eq!(week["end_date"], "2026-09-20");
    assert_eq!(week["label"], "2026-W38");

    // A named cadence starts where it starts. A workspace whose quarters do
    // not follow the calendar uses `custom` and says so, rather than having
    // this guess what their year looks like.
    for refused in [
        json!({"cadence":"quarter","start_date":"2026-11-01"}),
        json!({"cadence":"month","start_date":"2026-11-15"}),
        json!({"cadence":"week","start_date":"2026-09-16"}),
        json!({"cadence":"fortnight","start_date":"2026-09-14"}),
    ] {
        assert!(cycle(&service, refused.clone()).await.is_err(), "{refused}");
    }

    // And its end is not negotiable, because the label would stop being true.
    assert!(cycle(
        &service,
        json!({"cadence":"month","start_date":"2026-11-01","end_date":"2026-12-24"}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn a_custom_period_takes_both_ends_and_a_name_from_the_person() {
    let (_dir, service) = setup().await;

    // Nothing to derive, so nothing is derived.
    assert!(cycle(
        &service,
        json!({"cadence":"custom","start_date":"2026-04-01"})
    )
    .await
    .is_err());
    assert!(
        cycle(
            &service,
            json!({"cadence":"custom","start_date":"2026-04-01","end_date":"2026-06-30"}),
        )
        .await
        .is_err(),
        "a custom period needs a name"
    );
    assert!(cycle(
        &service,
        json!({"cadence":"custom","start_date":"2026-06-30","end_date":"2026-04-01","label":"逆"}),
    )
    .await
    .is_err());

    let fiscal = cycle(
        &service,
        json!({"cadence":"custom","start_date":"2026-04-01","end_date":"2026-06-30",
               "label":"FY26 Q1"}),
    )
    .await
    .unwrap();
    assert_eq!(fiscal["label"], "FY26 Q1");
    assert_eq!(fiscal["end_date"], "2026-06-30");
}

#[tokio::test]
async fn periods_do_not_overlap_so_which_one_is_this_has_one_answer() {
    let (_dir, service) = setup().await;
    cycle(
        &service,
        json!({"cadence":"quarter","start_date":"2026-10-01"}),
    )
    .await
    .unwrap();

    for overlapping in [
        json!({"cadence":"month","start_date":"2026-11-01"}),
        json!({"cadence":"custom","start_date":"2026-09-01","end_date":"2026-10-02","label":"またぐ"}),
        json!({"cadence":"custom","start_date":"2026-12-31","end_date":"2027-01-15","label":"末日に触れる"}),
    ] {
        let refused = cycle(&service, overlapping.clone()).await.unwrap_err();
        assert_eq!(refused.code, "CYCLE_OVERLAP", "{overlapping}");
    }

    // Adjacent is fine: the next quarter starts the day after this one ends.
    cycle(
        &service,
        json!({"cadence":"quarter","start_date":"2027-01-01"}),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_workspace_with_no_periods_works_exactly_as_before() {
    let (_dir, service) = setup().await;
    call(
        &service,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"kind":"outcome","title":"期間を使わない目標"}),
    )
    .await
    .unwrap();

    let planning = query(&service, "/v1/workspaces/personal/planning", &[])
        .await
        .unwrap();
    assert!(planning["cycles"].as_array().unwrap().is_empty());
    // Having no current period is the ordinary state of most workspaces, not
    // an error and not something to nag about.
    assert!(planning["current"].is_null());
    assert!(planning["previous"].is_null());
    assert!(planning["next"].is_null());
    assert_eq!(planning["unassigned_items"], 1);
    assert!(planning["today"].as_str().unwrap().len() == 10);
    assert_eq!(planning["timezone"], "Asia/Tokyo");

    // And the item is reachable the way it always was.
    let items = query(&service, "/v1/workspaces/personal/items", &[])
        .await
        .unwrap();
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_planning_context_names_the_period_around_today() {
    let (_dir, service) = setup().await;
    let today: String = query(&service, "/v1/workspaces/personal/planning", &[])
        .await
        .unwrap()["today"]
        .as_str()
        .unwrap()
        .to_owned();
    let today = chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap();

    // Three adjacent custom periods around the workspace's own today, so this
    // does not depend on when the test runs.
    let past = cycle(
        &service,
        json!({"cadence":"custom","label":"前",
               "start_date":(today - chrono::Duration::days(60)).to_string(),
               "end_date":(today - chrono::Duration::days(31)).to_string()}),
    )
    .await
    .unwrap();
    let current = cycle(
        &service,
        json!({"cadence":"custom","label":"今",
               "start_date":(today - chrono::Duration::days(30)).to_string(),
               "end_date":(today + chrono::Duration::days(30)).to_string(),
               "previous_id": past["id"]}),
    )
    .await
    .unwrap();
    let future = cycle(
        &service,
        json!({"cadence":"custom","label":"次",
               "start_date":(today + chrono::Duration::days(31)).to_string(),
               "end_date":(today + chrono::Duration::days(60)).to_string(),
               "previous_id": current["id"]}),
    )
    .await
    .unwrap();

    let planning = query(&service, "/v1/workspaces/personal/planning", &[])
        .await
        .unwrap();
    assert_eq!(planning["current"]["cycle"]["id"], current["id"]);
    assert_eq!(planning["previous"]["cycle"]["id"], past["id"]);
    assert_eq!(planning["next"]["cycle"]["id"], future["id"]);
    assert_eq!(planning["current"]["cycle"]["previous_id"], past["id"]);

    // Any period can be looked at, not only the one today falls in.
    let looking_back = query(
        &service,
        "/v1/workspaces/personal/planning",
        &[("cycle_id", past["id"].as_str().unwrap())],
    )
    .await
    .unwrap();
    assert_eq!(looking_back["current"]["cycle"]["id"], past["id"]);
    assert!(
        looking_back["previous"].is_null(),
        "nothing before the first"
    );
    assert_eq!(looking_back["next"]["cycle"]["id"], current["id"]);
}

#[tokio::test]
async fn carrying_work_forward_copies_it_and_keeps_the_thread_back() {
    let (_dir, service) = setup().await;
    let first = cycle(
        &service,
        json!({"cadence":"quarter","start_date":"2026-07-01"}),
    )
    .await
    .unwrap();
    let second = cycle(
        &service,
        json!({"cadence":"quarter","start_date":"2026-10-01","previous_id":first["id"]}),
    )
    .await
    .unwrap();

    let goal = call(
        &service,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"kind":"outcome","title":"終わらなかった目標","due_date":"2026-09-30",
               "fields":{"cycle_id":first["id"],"memo":"背景"}}),
    )
    .await
    .unwrap();

    let carried = call(
        &service,
        "POST",
        &format!(
            "/v1/workspaces/personal/cycles/{}/carry-over",
            second["id"].as_str().unwrap()
        ),
        json!({"item_ids":[goal["id"]],"expected_version":second["version"]}),
    )
    .await
    .unwrap();
    let copy = &carried["items"][0];
    assert_eq!(copy["title"], "終わらなかった目標");
    assert_eq!(copy["fields"]["cycle_id"], second["id"]);
    assert_eq!(copy["fields"]["carried_from"], goal["id"]);
    assert_eq!(copy["fields"]["memo"], "背景", "context comes with it");
    // A date that was set for the period it was set in does not silently
    // become a date in the next one.
    assert!(copy["due_date"].is_null());
    assert_ne!(copy["id"], goal["id"]);

    // The period that was already reviewed still says what was in it.
    let original: Value = call(
        &service,
        "GET",
        &format!(
            "/v1/workspaces/personal/items/{}",
            goal["id"].as_str().unwrap()
        ),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(original["fields"]["cycle_id"], first["id"]);
    assert_eq!(original["version"], 1, "the source is untouched");

    // Carrying the same thing in twice is a mistake worth naming.
    let again = call(
        &service,
        "POST",
        &format!(
            "/v1/workspaces/personal/cycles/{}/carry-over",
            second["id"].as_str().unwrap()
        ),
        json!({"item_ids":[copy["id"]],"expected_version":second["version"]}),
    )
    .await
    .unwrap_err();
    assert_eq!(again.code, "VALIDATION_ERROR");
}

#[tokio::test]
async fn closing_a_period_keeps_what_was_in_it() {
    let (_dir, service) = setup().await;
    let period = cycle(
        &service,
        json!({"cadence":"quarter","start_date":"2026-07-01"}),
    )
    .await
    .unwrap();
    let id = period["id"].as_str().unwrap().to_owned();
    call(
        &service,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"kind":"action","title":"この期間の行動","fields":{"cycle_id":id}}),
    )
    .await
    .unwrap();

    let closed = call(
        &service,
        "PATCH",
        &format!("/v1/workspaces/personal/cycles/{id}"),
        json!({"status":"closed","expected_version":1}),
    )
    .await
    .unwrap();
    assert_eq!(closed["status"], "closed");
    assert_eq!(closed["version"], 2);

    // The contents are still there, and still attributed to that period.
    let planning = query(
        &service,
        "/v1/workspaces/personal/planning",
        &[("cycle_id", &id)],
    )
    .await
    .unwrap();
    assert_eq!(planning["current"]["item_count"], 1);

    // Nothing more may be carried into a period that is over.
    let refused = call(
        &service,
        "POST",
        &format!("/v1/workspaces/personal/cycles/{id}/carry-over"),
        json!({"item_ids":["item_whatever"],"expected_version":2}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "VALIDATION_ERROR");

    // And deleting it would orphan the work, so it is refused until emptied.
    let held = call(
        &service,
        "DELETE",
        &format!("/v1/workspaces/personal/cycles/{id}"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(held.code, "CYCLE_NOT_EMPTY");
}

#[tokio::test]
async fn the_last_finalized_review_is_offered_as_the_persons_words() {
    let (_dir, service) = setup().await;
    let draft = call(
        &service,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":"2026-09-14","learnings":"観測: 3件完了",
               "challenges":"推測: 移動日に抜ける","next_focus":"朝に寄せる"}),
    )
    .await
    .unwrap();

    // A draft is not yet what they decided.
    let before = query(&service, "/v1/workspaces/personal/planning", &[])
        .await
        .unwrap();
    assert!(before["last_finalized_review"].is_null());

    call(
        &service,
        "POST",
        &format!(
            "/v1/workspaces/personal/weekly-reviews/{}/finalize",
            draft["id"].as_str().unwrap()
        ),
        json!({"expected_version":draft["version"]}),
    )
    .await
    .unwrap();

    let after = query(&service, "/v1/workspaces/personal/planning", &[])
        .await
        .unwrap();
    let review = &after["last_finalized_review"];
    assert_eq!(review["next_focus"], "朝に寄せる");
    assert_eq!(review["week_start"], "2026-09-14");
    // It is offered as text, not as a plan: nothing here has become an action.
    let items = query(&service, "/v1/workspaces/personal/items", &[])
        .await
        .unwrap();
    assert!(items["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_ai_proposes_a_period_and_a_person_approves_it() {
    let (_dir, service) = setup().await;
    let ai = Actor {
        id: "local-owner".into(),
        tenant: pathbase_api::service::LOCAL_TENANT.into(),
        agent: true,
        connection: Some("mcpconn_test".into()),
    };
    let key = uuid::Uuid::new_v4().to_string();

    // Creating a period directly is refused: this connection proposes.
    let refused = service
        .handle(
            &ai,
            "POST",
            "/v1/workspaces/personal/cycles",
            &HashMap::new(),
            json!({"cadence":"quarter","start_date":"2027-01-01"}),
            Some(&key),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, "APPROVAL_REQUIRED");

    let change = service
        .handle(
            &ai,
            "POST",
            "/v1/workspaces/personal/changesets/preview",
            &HashMap::new(),
            json!({"title":"来期の枠","operations":[{
                "method":"POST","path":"/v1/workspaces/personal/cycles",
                "body":{"cadence":"quarter","start_date":"2027-01-01"}}]}),
            Some(&uuid::Uuid::new_v4().to_string()),
        )
        .await
        .unwrap();
    assert_eq!(change["changes"][0]["effect"], "created");
    assert_eq!(change["changes"][0]["after"]["label"], "2027 Q1");

    // Previewing wrote nothing.
    let none = query(&service, "/v1/workspaces/personal/cycles", &[])
        .await
        .unwrap();
    assert!(none["items"].as_array().unwrap().is_empty());

    let id = change["id"].as_str().unwrap();
    call(
        &service,
        "POST",
        &format!("/v1/workspaces/personal/changesets/{id}/approve"),
        json!({"hash":change["hash"]}),
    )
    .await
    .unwrap();
    call(
        &service,
        "POST",
        &format!("/v1/workspaces/personal/changesets/{id}/apply"),
        json!({}),
    )
    .await
    .unwrap();

    let created = query(&service, "/v1/workspaces/personal/cycles", &[])
        .await
        .unwrap();
    assert_eq!(created["items"].as_array().unwrap().len(), 1);
    assert_eq!(created["items"][0]["label"], "2027 Q1");
}

#[tokio::test]
async fn periods_survive_an_export_and_restore() {
    let (_dir, service) = setup().await;
    let period = cycle(
        &service,
        json!({"cadence":"quarter","start_date":"2026-10-01"}),
    )
    .await
    .unwrap();
    let goal = call(
        &service,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"kind":"outcome","title":"期間つきの目標","fields":{"cycle_id":period["id"]}}),
    )
    .await
    .unwrap();

    let backup = call(
        &service,
        "POST",
        "/v1/workspaces/personal/exports",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(backup["cycles"].as_array().unwrap().len(), 1);

    // Into a workspace of its own, as a restore does.
    let restored = call(
        &service,
        "POST",
        "/v1/workspaces",
        json!({"name":"復元先","scope":"チーム"}),
    )
    .await
    .unwrap();
    let target = restored["id"].as_str().unwrap();
    let mut payload = backup.clone();
    payload["workspace_id"] = backup["workspace_id"].clone();
    call(
        &service,
        "POST",
        &format!("/v1/workspaces/{target}/imports"),
        payload,
    )
    .await
    .unwrap();

    let planning = query(&service, &format!("/v1/workspaces/{target}/planning"), &[])
        .await
        .unwrap();
    assert_eq!(planning["cycles"].as_array().unwrap().len(), 1);
    assert_eq!(planning["cycles"][0]["label"], "2026 Q4");
    // And the item is still attributed to it.
    let items = query(&service, &format!("/v1/workspaces/{target}/items"), &[])
        .await
        .unwrap();
    assert_eq!(items["items"][0]["fields"]["cycle_id"], period["id"]);
    assert_eq!(items["items"][0]["id"], goal["id"]);
}
