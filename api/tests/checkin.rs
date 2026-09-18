//! Check-ins, and the history they leave behind.
//!
//! A check-in history is only worth having if it says what was believed at the
//! time. So corrections append rather than overwrite, the author and the time
//! are the server's to stamp, and a goal's current health is a projection of
//! the newest check-in rather than a separate thing that can drift from it.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service, String, Actor) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("checkin.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    let who = Actor {
        id: "us_alice".into(),
        agent: false,
        connection: None,
    };
    service.provision_personal(&who).await.unwrap();
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

async fn goal(service: &Service, who: &Actor, w: &str, title: &str) -> String {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{w}/items"),
        json!({"kind":"outcome","title":title}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_correction_appends_and_the_original_stays_readable() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "訂正される目標").await;

    let first = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"on_track","comment":"順調です","self_assessment":70}),
    )
    .await
    .unwrap();
    assert_eq!(first["author"], "us_alice");
    assert!(first["created_at"].as_str().unwrap().len() > 10);

    // It turns out that was wrong. The correction is a new record.
    let second = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"off_track","comment":"見込み違いでした","self_assessment":20,
               "supersedes_id":first["id"]}),
    )
    .await
    .unwrap();

    let history = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    assert_eq!(history["standing_id"], second["id"]);
    // What was believed at the time is still there, in its own words.
    assert!(history["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["comment"] == "順調です" && entry["health"] == "on_track"));

    // And the goal now carries the corrected judgement.
    let item = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(item["fields"]["health"]["status"], "off_track");
    assert_eq!(item["fields"]["self_assessment"], 20.0);
}

#[tokio::test]
async fn the_author_and_the_time_are_not_the_clients_to_supply() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "記名される目標").await;

    for forged in [
        json!({"health":"on_track","author":"us_bob"}),
        json!({"health":"on_track","created_at":"2020-01-01T00:00:00Z"}),
        json!({"health":"on_track","id":"checkin_chosen"}),
    ] {
        assert!(
            call(
                &service,
                &who,
                "POST",
                &format!("/v1/workspaces/{w}/items/{id}/checkins"),
                forged.clone(),
            )
            .await
            .is_err(),
            "{forged}"
        );
    }

    let written = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"at_risk"}),
    )
    .await
    .unwrap();
    assert_eq!(written["author"], "us_alice");
}

#[tokio::test]
async fn a_check_in_that_says_nothing_is_refused() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "空のチェックイン").await;
    let refused = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"comment":"   "}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "VALIDATION_ERROR");

    // And a status this model does not have is not recorded as one.
    assert!(call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"絶好調"}),
    )
    .await
    .is_err());
    // Nor an assessment outside the scale it is on.
    assert!(call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"self_assessment":180}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn numbers_and_words_stay_separate_data() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "指標のある目標").await;
    let metric = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/metrics"),
        json!({"item_id":id,"name":"売上","unit":"万円","direction":"increase",
               "baseline":0,"target":100}),
    )
    .await
    .unwrap();
    let observation = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/observations"),
        json!({"metric_id":metric["id"],"value":40,"unit":"万円","source":"集計",
               "observed_at":"2026-09-14T00:00:00Z"}),
    )
    .await
    .unwrap();

    let checkin = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"at_risk","comment":"数字は伸びているが想定より遅い",
               "observation_ids":[observation["id"]]}),
    )
    .await
    .unwrap();
    // The check-in points at the measurement; it does not copy the number,
    // because a copy would let the comment and the measurement drift apart.
    assert_eq!(checkin["observation_ids"][0], observation["id"]);
    assert!(checkin.get("value").is_none());

    // A check-in cannot point at a measurement that does not exist here.
    assert!(call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"on_track","observation_ids":["obs_nope"]}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn the_timeline_replays_what_was_believed_at_a_moment() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "履歴のある目標").await;

    let early = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"on_track","comment":"順調"}),
    )
    .await
    .unwrap();
    let between = chrono::Utc::now().to_rfc3339();
    // Far enough apart that the two check-ins do not share a timestamp.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"off_track","comment":"問題が出た"}),
    )
    .await
    .unwrap();

    let now = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}/timeline"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(now["state"]["health"], "off_track");
    assert!(now["events"].as_array().unwrap().len() >= 3);

    // Replayed to before the second one: what was believed then.
    let earlier = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}/timeline"),
        &[("as_of", &between)],
    )
    .await
    .unwrap();
    assert_eq!(earlier["state"]["health"], "on_track");
    assert_eq!(earlier["state"]["checkin_id"], early["id"]);
    assert_eq!(earlier["state"]["checked_in_by"], "us_alice");
    // And the later event is not in a history of that moment.
    let summaries: Vec<&str> = earlier["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["summary"].as_str())
        .collect();
    assert!(!summaries
        .iter()
        .any(|summary| summary.contains("off_track")));

    // Before anything was said, nothing was known — which is a real answer.
    let beginning = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}/timeline"),
        &[("as_of", "2000-01-01T00:00:00Z")],
    )
    .await
    .unwrap();
    assert!(beginning["state"]["health"].is_null());
    assert!(beginning["events"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn the_timeline_carries_observations_records_and_alignment() {
    let (_dir, service, w, who) = setup().await;
    let parent = goal(&service, &who, &w, "上位の目標").await;
    let id = goal(&service, &who, &w, "つながる目標").await;
    let metric = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/metrics"),
        json!({"item_id":id,"name":"件数","unit":"件","direction":"increase",
               "baseline":0,"target":10}),
    )
    .await
    .unwrap();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/observations"),
        json!({"metric_id":metric["id"],"value":4,"unit":"件","source":"集計",
               "observed_at":"2026-09-14T00:00:00Z"}),
    )
    .await
    .unwrap();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/records"),
        json!({"record_type":"note","body":"経緯のメモ","item_ids":[id.clone()],
               "happened_at":"2026-09-15T00:00:00Z"}),
    )
    .await
    .unwrap();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/relations"),
        json!({"source_id":id,"target_id":parent,"type":"part_of"}),
    )
    .await
    .unwrap();

    let timeline = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}/timeline"),
        &[],
    )
    .await
    .unwrap();
    let kinds: Vec<&str> = timeline["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"created"), "{kinds:?}");
    assert!(kinds.contains(&"observation"), "{kinds:?}");
    assert!(kinds.contains(&"record_note"), "{kinds:?}");
    assert!(kinds.contains(&"alignment_changed"), "{kinds:?}");
    // In order, so it reads as a history.
    let times: Vec<&str> = timeline["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["at"].as_str())
        .collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    assert_eq!(times, sorted);
}

#[tokio::test]
async fn the_review_queue_keeps_silence_and_warning_apart() {
    let (_dir, service, w, who) = setup().await;
    let quiet = goal(&service, &who, &w, "誰も何も言っていない目標").await;
    let trouble = goal(&service, &who, &w, "問題があると言われた目標").await;
    let fine = goal(&service, &who, &w, "順調と言われた目標").await;

    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{trouble}/checkins"),
        json!({"health":"at_risk","blockers":"人手","next_focus":"採用"}),
    )
    .await
    .unwrap();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{fine}/checkins"),
        json!({"health":"on_track"}),
    )
    .await
    .unwrap();

    let review = query(&service, &who, &format!("/v1/workspaces/{w}/review"), &[])
        .await
        .unwrap();
    // Silence is its own list. It is not a warning, and merging the two would
    // make every new goal look like a problem.
    let never: Vec<&str> = review["never_checked_in"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect();
    assert_eq!(never, vec![quiet.as_str()]);

    let at_risk: Vec<&str> = review["at_risk"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect();
    assert_eq!(at_risk, vec![trouble.as_str()]);
    assert_eq!(review["at_risk"][0]["blockers"], "人手");
    assert_eq!(review["at_risk"][0]["next_focus"], "採用");

    // Both of the ones that were spoken about are recent, so neither is stale.
    assert!(review["stale"].as_array().unwrap().is_empty());
    assert_eq!(review["recently_updated"].as_array().unwrap().len(), 2);
    assert_eq!(review["stale_days"], 14);
}

#[tokio::test]
async fn an_ai_drafts_a_check_in_and_a_person_approves_the_words() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "AIが下書きする目標").await;
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"on_track","comment":"本人の前回"}),
    )
    .await
    .unwrap();

    let ai = Actor {
        id: "us_alice".into(),
        agent: true,
        connection: Some("mcpconn_test".into()),
    };
    // Writing one directly is refused: this connection proposes.
    assert!(call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"at_risk"}),
    )
    .await
    .is_err());

    let change = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/changesets/preview"),
        json!({"title":"チェックイン案","operations":[{
            "method":"POST","path":format!("/v1/workspaces/{w}/items/{id}/checkins"),
            "body":{"health":"at_risk","comment":"観測: 2週間更新なし。推測: 優先度が下がった可能性。質問: 継続しますか"}}]}),
    )
    .await
    .unwrap();
    // The diff shows the words, next to the person's own last ones.
    assert_eq!(change["changes"][0]["collection"], "checkins");
    assert_eq!(change["changes"][0]["before"]["comment"], "本人の前回");
    assert_eq!(change["changes"][0]["after"]["health"], "at_risk");

    // And nothing has changed until they approve it.
    let item = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(item["fields"]["health"]["status"], "on_track");

    let change_id = change["id"].as_str().unwrap();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/changesets/{change_id}/approve"),
        json!({"hash":change["hash"]}),
    )
    .await
    .unwrap();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/changesets/{change_id}/apply"),
        json!({}),
    )
    .await
    .unwrap();

    let after = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(after["fields"]["health"]["status"], "at_risk");
    // Applied as the person, so the record carries their name rather than the
    // model's — they are the one who decided.
    let history = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(history["items"][0]["author"], "us_alice");
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn check_ins_survive_an_export_and_restore() {
    let (_dir, service, w, who) = setup().await;
    let id = goal(&service, &who, &w, "履歴つきの目標").await;
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{id}/checkins"),
        json!({"health":"at_risk","comment":"残す言葉"}),
    )
    .await
    .unwrap();

    let backup = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/exports"),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(backup["checkins"].as_array().unwrap().len(), 1);

    let restored = call(
        &service,
        &who,
        "POST",
        "/v1/workspaces",
        json!({"name":"復元先","scope":"チーム"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{restored}/imports"),
        backup.clone(),
    )
    .await
    .unwrap();

    let history = query(
        &service,
        &who,
        &format!("/v1/workspaces/{restored}/items/{id}/checkins"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(history["items"][0]["comment"], "残す言葉");
    assert_eq!(history["items"][0]["author"], "us_alice");
}
