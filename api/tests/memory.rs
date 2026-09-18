//! Personal memory, and the boundary it lives inside.
//!
//! Most of this file is about two things the feature would be worse than
//! useless without:
//!
//! 1. **It is the person's, and it is personal.** Memory exists in their own
//!    workspace and has no presence in a shared one. An organization goal that
//!    a personal goal contributes to gives the organization no path back here.
//! 2. **A guess is not a fact.** An AI's suggestion is a candidate until the
//!    person confirms it, and something with no source cannot be filed as a
//!    fact at all.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service, String, Actor) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("memory.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    let who = Actor {
        id: "us_alice".into(),
        agent: false,
        connection: None,
    };
    service.provision_personal(&who).await.unwrap();
    let personal = query(&service, &who, "/v1/workspaces", &[])
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
    (dir, service, personal, who)
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

fn agent(id: &str) -> Actor {
    Actor {
        id: id.into(),
        agent: true,
        connection: Some("mcpconn_test".into()),
    }
}

async fn remember(service: &Service, who: &Actor, w: &str, body: Value) -> Result<Value> {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{w}/memories"),
        body,
    )
    .await
}

#[tokio::test]
async fn memory_exists_only_in_a_persons_own_workspace() {
    let (_dir, service, personal, who) = setup().await;
    let shared = call(
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

    remember(
        &service,
        &who,
        &personal,
        json!({"kind":"preference","title":"午前に集中したい"}),
    )
    .await
    .unwrap();

    // There is no memory in a shared workspace — not an empty list, not a
    // permission error. The route does not exist there.
    for (method, path, body) in [
        (
            "GET",
            format!("/v1/workspaces/{shared}/memories"),
            json!({}),
        ),
        (
            "POST",
            format!("/v1/workspaces/{shared}/memories"),
            json!({"kind":"fact","title":"x","source":"本人"}),
        ),
        (
            "POST",
            format!("/v1/workspaces/{shared}/memories/proposals"),
            json!({"kind":"context","title":"x"}),
        ),
    ] {
        let refused = call(&service, &who, method, &path, body).await.unwrap_err();
        assert_eq!(refused.status, 404, "{method} {path}");
    }
}

#[tokio::test]
async fn an_organisation_goal_gives_no_path_back_to_a_persons_memory() {
    let (_dir, service, personal, who) = setup().await;
    let shared = call(
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
    let company = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{shared}/items"),
        json!({"kind":"outcome","title":"会社の目標","fields":{"owner":{"kind":"organization"}}}),
    )
    .await
    .unwrap();

    // Her own goal, in her own workspace, with a memory about why she chose it.
    let mine = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{personal}/items"),
        json!({"kind":"outcome","title":"個人の目標"}),
    )
    .await
    .unwrap();
    remember(
        &service,
        &who,
        &personal,
        json!({"kind":"decision","title":"この目標を選んだ理由","body":"前職での失敗",
               "source":"本人","item_ids":[mine["id"]]}),
    )
    .await
    .unwrap();

    // The personal goal cannot even be linked to the company goal — they are
    // separate workspaces — so there is no edge to traverse.
    assert!(call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{shared}/relations"),
        json!({"source_id":mine["id"],"target_id":company["id"],"type":"contributes_to"}),
    )
    .await
    .is_err());

    // And nothing the organisation side returns mentions any of it.
    for path in [
        format!("/v1/workspaces/{shared}/alignment"),
        format!("/v1/workspaces/{shared}/dashboard"),
        format!("/v1/workspaces/{shared}/snapshot"),
    ] {
        let value = query(&service, &who, &path, &[]).await.unwrap();
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(!serialized.contains("前職での失敗"), "{path}: {serialized}");
        assert!(!serialized.contains("個人の目標"), "{path}");
        assert!(!serialized.contains("memories"), "{path}");
    }
}

#[tokio::test]
async fn an_ai_suggestion_is_a_candidate_until_the_person_says_otherwise() {
    let (_dir, service, personal, who) = setup().await;
    let ai = agent("us_alice");

    // An agent cannot write a verified memory at all.
    assert!(call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{personal}/memories"),
        json!({"kind":"fact","title":"勝手に事実","source":"推測"}),
    )
    .await
    .is_err());

    let change = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{personal}/changesets/preview"),
        json!({"title":"記憶の候補","operations":[{
            "method":"POST","path":format!("/v1/workspaces/{personal}/memories/proposals"),
            "body":{"kind":"preference","title":"午前に集中したいようだ",
                    "source":"観測: 完了の8割が午前","confidence":0.7}}]}),
    )
    .await
    .unwrap();
    assert_eq!(change["changes"][0]["collection"], "memories");
    assert_eq!(change["changes"][0]["after"]["status"], "proposed");

    let id = change["id"].as_str().unwrap();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{personal}/changesets/{id}/approve"),
        json!({"hash":change["hash"]}),
    )
    .await
    .unwrap();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{personal}/changesets/{id}/apply"),
        json!({}),
    )
    .await
    .unwrap();

    // Approving the change set stored the candidate. It did not make it into
    // something the person said.
    let listed = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[],
    )
    .await
    .unwrap();
    let stored = &listed["items"][0];
    assert_eq!(stored["status"], "proposed");
    assert_eq!(stored["confidence"], 0.7);

    // Confirming it is a separate act, and it drops the machine's estimate:
    // a thing the person has said is not accompanied by a guess about it.
    let verified = call(
        &service,
        &who,
        "POST",
        &format!(
            "/v1/workspaces/{personal}/memories/{}/verify",
            stored["id"].as_str().unwrap()
        ),
        json!({"expected_version": stored["version"]}),
    )
    .await
    .unwrap();
    assert_eq!(verified["status"], "verified");
    assert!(verified["confidence"].is_null());

    // An agent cannot confirm its own suggestion.
    assert!(call(
        &service,
        &ai,
        "POST",
        &format!(
            "/v1/workspaces/{personal}/memories/{}/verify",
            stored["id"].as_str().unwrap()
        ),
        json!({"expected_version": verified["version"]}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn a_guess_with_no_source_cannot_be_filed_as_a_fact() {
    let (_dir, service, personal, who) = setup().await;

    let refused = remember(
        &service,
        &who,
        &personal,
        json!({"kind":"fact","title":"たぶん朝型"}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "VALIDATION_ERROR");
    assert!(refused.message.contains("出典"), "{}", refused.message);

    // With a source it is a fact; without one it can still be recorded, as the
    // kind of thing it actually is.
    remember(
        &service,
        &who,
        &personal,
        json!({"kind":"fact","title":"朝型","source":"本人の申告"}),
    )
    .await
    .unwrap();
    remember(
        &service,
        &who,
        &personal,
        json!({"kind":"context","title":"たぶん朝型","body":"根拠は薄い"}),
    )
    .await
    .unwrap();

    // Evidence has to be real: a citation to nothing is worse than none.
    assert!(remember(
        &service,
        &who,
        &personal,
        json!({"kind":"fact","title":"x","evidence_ids":["rec_nope"]}),
    )
    .await
    .is_err());
    // And a person's own statement does not come with a machine's confidence.
    assert!(remember(
        &service,
        &who,
        &personal,
        json!({"kind":"preference","title":"x","confidence":0.9}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn correcting_a_memory_keeps_what_it_replaced() {
    let (_dir, service, personal, who) = setup().await;
    let first = remember(
        &service,
        &who,
        &personal,
        json!({"kind":"preference","title":"夜に集中したい","source":"本人"}),
    )
    .await
    .unwrap();
    let second = remember(
        &service,
        &who,
        &personal,
        json!({"kind":"preference","title":"午前に集中したい","source":"本人",
               "supersedes_id":first["id"]}),
    )
    .await
    .unwrap();

    let all = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
    assert!(all["superseded_ids"]
        .as_array()
        .unwrap()
        .contains(&first["id"]));

    // What still stands leaves out the one that was replaced — but the replaced
    // one is still readable, because it says what was true then.
    let current = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[("current", "true")],
    )
    .await
    .unwrap();
    let ids: Vec<&str> = current["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![second["id"].as_str().unwrap()]);

    // Archiving is not deleting either.
    call(
        &service,
        &who,
        "PATCH",
        &format!(
            "/v1/workspaces/{personal}/memories/{}",
            second["id"].as_str().unwrap()
        ),
        json!({"expected_version":1,"archived_at":"now"}),
    )
    .await
    .unwrap();
    let visible = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(visible["items"].as_array().unwrap().len(), 1);
    let including = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[("archived", "true")],
    )
    .await
    .unwrap();
    assert_eq!(including["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_preference_that_stopped_being_true_is_not_wrong() {
    let (_dir, service, personal, who) = setup().await;
    remember(
        &service,
        &who,
        &personal,
        json!({"kind":"preference","title":"前職では夜型だった","source":"本人",
               "valid_from":"2020-01-01T00:00:00Z","valid_to":"2024-12-31T00:00:00Z"}),
    )
    .await
    .unwrap();
    let now = remember(
        &service,
        &who,
        &personal,
        json!({"kind":"preference","title":"いまは朝型","source":"本人",
               "valid_from":"2025-01-01T00:00:00Z"}),
    )
    .await
    .unwrap();

    let current = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[("current", "true")],
    )
    .await
    .unwrap();
    let ids: Vec<&str> = current["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![now["id"].as_str().unwrap()]);

    // The old one is still there. It was true, and the history says so.
    let all = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn duplicates_are_reported_and_never_merged() {
    let (_dir, service, personal, who) = setup().await;
    for title in [
        "毎週金曜に振り返りをする",
        "毎週金曜に振り返りをする習慣",
        "まったく別のこと",
    ] {
        remember(
            &service,
            &who,
            &personal,
            json!({"kind":"preference","title":title,"source":"本人"}),
        )
        .await
        .unwrap();
    }

    let found = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories/duplicates"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(found["merged"], false);
    let groups = found["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0]["similar"].as_array().unwrap().len(), 1);

    // Nothing was removed by looking: both are still there, and deciding
    // which is right is the person's, not the product's.
    let all = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(all["items"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn memory_the_person_excluded_is_kept_but_never_handed_to_an_ai() {
    let (_dir, service, personal, who) = setup().await;
    let private = remember(
        &service,
        &who,
        &personal,
        json!({"kind":"context","title":"AIには渡したくない事情","source":"本人",
               "excluded_from_retrieval":true}),
    )
    .await
    .unwrap();
    remember(
        &service,
        &who,
        &personal,
        json!({"kind":"preference","title":"共有してよい好み","source":"本人"}),
    )
    .await
    .unwrap();

    // The person sees both: it is their memory, and they asked to keep it.
    let mine = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(mine["items"].as_array().unwrap().len(), 2);

    // An AI sees one, and the other does not exist as far as it can tell.
    let ai = agent("us_alice");
    let theirs = query(
        &service,
        &ai,
        &format!("/v1/workspaces/{personal}/memories"),
        &[],
    )
    .await
    .unwrap();
    let titles: Vec<&str> = theirs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["共有してよい好み"]);
    let direct = query(
        &service,
        &ai,
        &format!(
            "/v1/workspaces/{personal}/memories/{}",
            private["id"].as_str().unwrap()
        ),
        &[],
    )
    .await
    .unwrap_err();
    assert_eq!(direct.status, 404);
}

#[tokio::test]
async fn a_memory_can_be_exported_and_really_deleted() {
    let (_dir, service, personal, who) = setup().await;
    let memory = remember(
        &service,
        &who,
        &personal,
        json!({"kind":"learning","title":"消せる記憶","source":"本人"}),
    )
    .await
    .unwrap();

    let backup = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{personal}/exports"),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(backup["memories"].as_array().unwrap().len(), 1);

    call(
        &service,
        &who,
        "DELETE",
        &format!(
            "/v1/workspaces/{personal}/memories/{}",
            memory["id"].as_str().unwrap()
        ),
        json!({}),
    )
    .await
    .unwrap();

    // Gone, not hidden. Archiving is the other option and it is also theirs.
    let after = query(
        &service,
        &who,
        &format!("/v1/workspaces/{personal}/memories"),
        &[("archived", "true")],
    )
    .await
    .unwrap();
    assert!(after["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_backup_holding_memory_cannot_be_restored_into_a_shared_workspace() {
    let (_dir, service, personal, who) = setup().await;
    remember(
        &service,
        &who,
        &personal,
        json!({"kind":"decision","title":"個人的な決定","source":"本人"}),
    )
    .await
    .unwrap();
    let backup = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{personal}/exports"),
        json!({}),
    )
    .await
    .unwrap();

    let shared = call(
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

    // A file is not a way around the boundary.
    let refused = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{shared}/imports"),
        backup,
    )
    .await
    .unwrap_err();
    assert_eq!(refused.status, 404, "{refused:?}");
    let listed = query(
        &service,
        &who,
        &format!("/v1/workspaces/{shared}/items"),
        &[],
    )
    .await
    .unwrap();
    assert!(
        listed["items"].as_array().unwrap().is_empty(),
        "a refused import leaves nothing behind"
    );
}

#[tokio::test]
async fn another_person_cannot_reach_someone_elses_memory() {
    let (_dir, service, personal, who) = setup().await;
    remember(
        &service,
        &who,
        &personal,
        json!({"kind":"fact","title":"アリスの記憶","source":"本人"}),
    )
    .await
    .unwrap();

    let bob = Actor {
        id: "us_bob".into(),
        agent: false,
        connection: None,
    };
    service.provision_personal(&bob).await.unwrap();
    let refused = query(
        &service,
        &bob,
        &format!("/v1/workspaces/{personal}/memories"),
        &[],
    )
    .await
    .unwrap_err();
    assert_eq!(refused.status, 404);
    let serialized = serde_json::to_string(&refused).unwrap();
    assert!(!serialized.contains("アリス"), "{serialized}");
}
