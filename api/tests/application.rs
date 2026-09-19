use pathbase_api::{
    model::*,
    service::{Actor, Service},
    storage,
};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Reads the personal workspace items straight from storage.
async fn items(service: &Service) -> Vec<Item> {
    let mut tx = service.db.begin_read().await.unwrap();
    storage::list::<Item>(&mut tx, "personal", "items")
        .await
        .unwrap()
}

/// Grants a membership directly, bypassing the invitation flow, so a test can
/// set up an actor the API would not let it create.
async fn insert_membership(service: &Service, workspace: &str, actor: &str, role: &str) {
    let mut tx = service.db.begin_write().await.unwrap();
    tx.execute(
        "INSERT INTO memberships(workspace_id,actor,role) VALUES(?,?,?)",
        &pathbase_api::params![workspace, actor, role],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}
async fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(&dir.path().join("db.sqlite").to_string_lossy())
        .await
        .unwrap();
    s.initialize(false).await.unwrap();
    (dir, s)
}
async fn req(s: &Service, method: &str, path: &str, b: Value) -> Result<Value> {
    s.handle(
        &Actor::local(),
        method,
        path,
        &HashMap::new(),
        b,
        Some(&uuid::Uuid::new_v4().to_string()),
    )
    .await
}
async fn get_with_query(s: &Service, path: &str, query: &[(&str, &str)]) -> Result<Value> {
    s.handle(
        &Actor::local(),
        "GET",
        path,
        &query
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        json!({}),
        None,
    )
    .await
}
async fn item(s: &Service, kind: &str) -> Value {
    req(
        s,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"テスト項目","kind":kind}),
    )
    .await
    .unwrap()
}
fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}
async fn relation(s: &Service, source: &Value, target: &Value, kind: &str) -> Result<Value> {
    req(
        s,
        "POST",
        "/v1/workspaces/personal/relations",
        json!({"source_id":id(source),"target_id":id(target),"type":kind}),
    )
    .await
}
#[tokio::test]
async fn weekly_review_aggregates_evidence_and_preserves_finalized_corrections() {
    let (_d, s) = setup().await;
    let goal = item(&s, "outcome").await;
    req(
        &s,
        "PATCH",
        &format!("/v1/workspaces/personal/items/{}", id(&goal)),
        json!({"expected_version":1,"fields":{"self_assessment":72}}),
    )
    .await
    .unwrap();
    let action = req(
        &s,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"週の行動","kind":"action","scheduled_date":"2026-09-14"}),
    )
    .await
    .unwrap();
    req(
        &s,
        "POST",
        &format!("/v1/workspaces/personal/actions/{}/complete", id(&action)),
        json!({"expected_version":1,"local_date":"2026-09-14","completed_at":"2026-09-14T01:00:00Z"}),
    ).await
    .unwrap();
    let metric = req(
        &s,
        "POST",
        "/v1/workspaces/personal/metrics",
        json!({"item_id":id(&goal),"name":"利用者","unit":"人","baseline":0,"target":100,"direction":"increase"}),
    ).await
    .unwrap();
    for (value, observed) in [(10, "2026-09-06T01:00:00Z"), (14, "2026-09-14T02:00:00Z")] {
        req(
            &s,
            "POST",
            "/v1/workspaces/personal/observations",
            json!({"metric_id":id(&metric),"value":value,"unit":"人","source":"test","observed_at":observed}),
        ).await
        .unwrap();
    }
    let summary = get_with_query(
        &s,
        "/v1/workspaces/personal/weekly-review",
        &[("week_start", "2026-09-14")],
    )
    .await
    .unwrap();
    assert_eq!(summary["actions"]["completed"], 1);
    assert_eq!(summary["goals"][0]["self_assessment"], 72.0);
    assert_eq!(summary["metrics"][0]["delta"], 4.0);
    assert_eq!(summary["timezone"], "Asia/Tokyo");

    let draft = req(
        &s,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":"2026-09-14","learnings":"学び","challenges":"課題","next_focus":"重点"}),
    ).await
    .unwrap();
    let finalized = req(
        &s,
        "POST",
        &format!(
            "/v1/workspaces/personal/weekly-reviews/{}/finalize",
            id(&draft)
        ),
        json!({"expected_version":draft["version"]}),
    )
    .await
    .unwrap();
    let correction = req(
        &s,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":"2026-09-14","learnings":"訂正版","challenges":"","next_focus":""}),
    )
    .await
    .unwrap();
    assert_eq!(finalized["status"], "finalized");
    assert_eq!(correction["revision"], 2);
    assert_eq!(correction["supersedes_id"], finalized["id"]);
}

#[tokio::test]
async fn weekly_review_rejects_non_monday_boundary_and_unauthorized_access() {
    let (_d, s) = setup().await;
    assert_eq!(
        get_with_query(
            &s,
            "/v1/workspaces/personal/weekly-review",
            &[("week_start", "2026-09-15")],
        )
        .await
        .unwrap_err()
        .code,
        "VALIDATION_ERROR"
    );
    let outsider = Actor {
        id: "outsider".into(),
        agent: false,
        connection: None,
    };
    let mut query = HashMap::new();
    query.insert("week_start".into(), "2026-09-14".into());
    assert_eq!(
        s.handle(
            &outsider,
            "GET",
            "/v1/workspaces/personal/weekly-review",
            &query,
            json!({}),
            None
        )
        .await
        .unwrap_err()
        .status,
        404
    );
    req(
        &s,
        "POST",
        "/v1/workspaces/team/items",
        json!({"title":"担当行動","kind":"action","scheduled_date":"2026-09-14","fields":{"assignee":"local-owner"}}),
    ).await
    .unwrap();
    let team = get_with_query(
        &s,
        "/v1/workspaces/team/weekly-review",
        &[("week_start", "2026-09-14")],
    )
    .await
    .unwrap();
    assert_eq!(team["members"][0]["actor"], "local-owner");
    assert_eq!(team["members"][0]["incomplete"], 1);
}
#[tokio::test]
async fn title_only_survives_restart() {
    let (d, s) = setup().await;
    let i = item(&s, "outcome").await;
    drop(s);
    let s = Service::open(&d.path().join("db.sqlite").to_string_lossy())
        .await
        .unwrap();
    let saved = req(
        &s,
        "GET",
        &format!("/v1/workspaces/personal/items/{}", id(&i)),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(i, saved);
    assert!(saved["fields"]["self_assessment"].is_null());
}
#[tokio::test]
async fn idempotent_completion_does_not_change_outcomes() {
    let (_d, s) = setup().await;
    let goal = item(&s, "outcome").await;
    let action = item(&s, "action").await;
    relation(&s, &action, &goal, "contributes_to")
        .await
        .unwrap();
    let path = format!("/v1/workspaces/personal/actions/{}/complete", id(&action));
    let b = json!({"expected_version":1,"local_date":"2026-09-12"});
    let first = s
        .handle(
            &Actor::local(),
            "POST",
            &path,
            &HashMap::new(),
            b.clone(),
            Some("repeat"),
        )
        .await
        .unwrap();
    let second = s
        .handle(
            &Actor::local(),
            "POST",
            &path,
            &HashMap::new(),
            b,
            Some("repeat"),
        )
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first["outcome_updated"], false);
    let snap = req(&s, "GET", "/v1/workspaces/personal/snapshot", json!({}))
        .await
        .unwrap();
    assert_eq!(snap["records"].as_array().unwrap().len(), 1);
    assert_eq!(
        snap["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == goal["id"])
            .unwrap()["state"],
        "active"
    );
}
#[tokio::test]
async fn reused_key_different_input_fails() {
    let (_d, s) = setup().await;
    let a = Actor::local();
    s.handle(
        &a,
        "POST",
        "/v1/workspaces/personal/items",
        &HashMap::new(),
        json!({"title":"A"}),
        Some("key"),
    )
    .await
    .unwrap();
    assert_eq!(
        s.handle(
            &a,
            "POST",
            "/v1/workspaces/personal/items",
            &HashMap::new(),
            json!({"title":"B"}),
            Some("key")
        )
        .await
        .unwrap_err()
        .code,
        "IDEMPOTENCY_CONFLICT"
    );
}
#[tokio::test]
async fn concurrent_updates_only_one_wins() {
    let (_d, s) = setup().await;
    let i = item(&s, "outcome").await;
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let s = s.clone();
            let path = format!("/v1/workspaces/personal/items/{}", id(&i));
            tokio::spawn(async move {
                req(
                    &s,
                    "PATCH",
                    &path,
                    json!({"expected_version":1,"title":format!("update-{n}")}),
                )
                .await
            })
        })
        .collect();
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        "VERSION_CONFLICT"
    );
}
#[tokio::test]
async fn concurrent_graph_cycle_cannot_commit() {
    let (_d, s) = setup().await;
    let a = item(&s, "initiative").await;
    let b = item(&s, "initiative").await;
    let s1 = s.clone();
    let a1 = a.clone();
    let b1 = b.clone();
    let x = tokio::spawn(async move { relation(&s1, &a1, &b1, "depends_on").await });
    let y = tokio::spawn(async move { relation(&s, &b, &a, "depends_on").await });
    let results = [x.await.unwrap(), y.await.unwrap()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        "CYCLE_DETECTED"
    );
}
#[tokio::test]
async fn graph_types_are_separate_and_symmetric_links_unique() {
    let (_d, s) = setup().await;
    let a = item(&s, "action").await;
    let b = item(&s, "outcome").await;
    let c = item(&s, "outcome").await;
    relation(&s, &a, &b, "contributes_to").await.unwrap();
    relation(&s, &a, &c, "contributes_to").await.unwrap();
    relation(&s, &a, &b, "part_of").await.unwrap();
    assert!(relation(&s, &a, &c, "part_of").await.is_err());
    relation(&s, &b, &a, "depends_on").await.unwrap();
    relation(&s, &a, &b, "relates_to").await.unwrap();
    assert_eq!(
        relation(&s, &b, &a, "relates_to").await.unwrap_err().code,
        "DUPLICATE_RELATION"
    );
}
#[tokio::test]
async fn workspace_isolation_and_viewer_denial() {
    let (_d, s) = setup().await;
    let i = item(&s, "outcome").await;
    assert_eq!(
        req(
            &s,
            "GET",
            &format!("/v1/workspaces/team/items/{}", id(&i)),
            json!({})
        )
        .await
        .unwrap_err()
        .status,
        404
    );
    let actor = Actor {
        id: "outsider".into(),
        agent: false,
        connection: None,
    };
    let calendar_query = HashMap::from([
        ("start".into(), "2026-09-01".into()),
        ("end".into(), "2026-09-07".into()),
    ]);
    assert_eq!(
        s.handle(
            &actor,
            "GET",
            "/v1/workspaces/personal/calendar",
            &calendar_query,
            json!({}),
            None
        )
        .await
        .unwrap_err()
        .status,
        404
    );
    assert_eq!(
        s.handle(
            &actor,
            "GET",
            "/v1/workspaces/personal/snapshot",
            &HashMap::new(),
            json!({}),
            None
        )
        .await
        .unwrap_err()
        .status,
        404
    );
    insert_membership(&s, "personal", "outsider", "viewer").await;
    assert!(s
        .handle(
            &actor,
            "GET",
            "/v1/workspaces/personal/snapshot",
            &HashMap::new(),
            json!({}),
            None
        )
        .await
        .is_ok());
    assert!(s
        .handle(
            &actor,
            "GET",
            "/v1/workspaces/personal/calendar",
            &calendar_query,
            json!({}),
            None
        )
        .await
        .is_ok());
    assert_eq!(
        s.handle(
            &actor,
            "POST",
            "/v1/workspaces/personal/items",
            &HashMap::new(),
            json!({"title":"x"}),
            Some("key")
        )
        .await
        .unwrap_err()
        .status,
        403
    );
}

#[tokio::test]
async fn onboarding_title_only_is_atomic_and_idempotent() {
    let (_d, s) = setup().await;
    let body = json!({
        "title":"本を読む時間をつくる",
        "purpose":"毎週、本を読む時間をつくりたい",
        "due_date":null,
        "initiative_title":"",
        "action_title":"",
        "metric":null
    });
    let first = s
        .handle(
            &Actor::local(),
            "POST",
            "/v1/workspaces/personal/onboarding/complete",
            &HashMap::new(),
            body.clone(),
            Some("onboarding-once"),
        )
        .await
        .unwrap();
    let replay = s
        .handle(
            &Actor::local(),
            "POST",
            "/v1/workspaces/personal/onboarding/complete",
            &HashMap::new(),
            body,
            Some("onboarding-once"),
        )
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert!(first["goal"]["due_date"].is_null());
    assert!(first["metric"].is_null());
    let snapshot = req(&s, "GET", "/v1/workspaces/personal/snapshot", json!({}))
        .await
        .unwrap();
    assert_eq!(snapshot["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn onboarding_builds_reviewed_graph_or_rolls_back_everything() {
    let (_d, s) = setup().await;
    let created = req(
        &s,
        "POST",
        "/v1/workspaces/personal/onboarding/complete",
        json!({
            "title":"新しい習慣",
            "purpose":"無理なく続ける",
            "due_date":"2026-12-31",
            "initiative_title":"環境を整える",
            "action_title":"道具を用意する",
            "metric":{"name":"実行回数","unit":"回","baseline":0.0,"target":12.0,"direction":"increase"}
        }),
    ).await.unwrap();
    assert_eq!(
        created["goal"]["fields"]["next_action_id"],
        created["action"]["id"]
    );
    let snapshot = req(&s, "GET", "/v1/workspaces/personal/snapshot", json!({}))
        .await
        .unwrap();
    assert_eq!(snapshot["items"].as_array().unwrap().len(), 3);
    assert_eq!(snapshot["relations"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["metrics"].as_array().unwrap().len(), 1);

    let (_d, failed) = setup().await;
    assert!(req(
        &failed,
        "POST",
        "/v1/workspaces/personal/onboarding/complete",
        json!({
            "title":"保存されない目標",
            "purpose":"",
            "due_date":null,
            "initiative_title":"途中まで作れる候補",
            "action_title":"",
            "metric":{"name":"壊れた指標","unit":"回","baseline":1.0,"target":1.0,"direction":"increase"}
        }),
    ).await.is_err());
    let snapshot = req(
        &failed,
        "GET",
        "/v1/workspaces/personal/snapshot",
        json!({}),
    )
    .await
    .unwrap();
    assert!(snapshot["items"].as_array().unwrap().is_empty());
    assert!(snapshot["relations"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn onboarding_rejects_viewers_and_non_empty_workspace_conflicts() {
    let (_d, s) = setup().await;
    insert_membership(&s, "personal", "viewer", "viewer").await;
    let body = json!({"title":"目標","purpose":"","due_date":null,"initiative_title":"","action_title":"","metric":null});
    let viewer = Actor {
        id: "viewer".into(),
        agent: false,
        connection: None,
    };
    assert_eq!(
        s.handle(
            &viewer,
            "POST",
            "/v1/workspaces/personal/onboarding/complete",
            &HashMap::new(),
            body.clone(),
            Some("viewer-onboarding")
        )
        .await
        .unwrap_err()
        .status,
        403
    );
    item(&s, "outcome").await;
    assert_eq!(
        req(
            &s,
            "POST",
            "/v1/workspaces/personal/onboarding/complete",
            body
        )
        .await
        .unwrap_err()
        .code,
        "ONBOARDING_CONFLICT"
    );
}
#[tokio::test]
async fn proposals_require_human_approval_and_reject_stale_base() {
    let (_d, s) = setup().await;
    let i = item(&s, "outcome").await;
    let agent = Actor {
        id: "local-owner".into(),
        agent: true,
        connection: None,
    };
    let ops = json!({"operations":[{"method":"PATCH","path":format!("/v1/workspaces/personal/items/{}",id(&i)),"body":{"expected_version":1,"title":"changed"}}]});
    let c = s
        .handle(
            &agent,
            "POST",
            "/v1/workspaces/personal/changesets/preview",
            &HashMap::new(),
            ops,
            Some("preview"),
        )
        .await
        .unwrap();
    let apply = format!("/v1/workspaces/personal/changesets/{}/apply", id(&c));
    assert_eq!(
        s.handle(
            &agent,
            "POST",
            &apply,
            &HashMap::new(),
            json!({}),
            Some("apply")
        )
        .await
        .unwrap_err()
        .code,
        "APPROVAL_REQUIRED"
    );
    // The plan moves under the proposal. Approving is what writes, so the
    // staleness is caught there rather than at a later step.
    req(
        &s,
        "PATCH",
        &format!("/v1/workspaces/personal/items/{}", id(&i)),
        json!({"expected_version":1,"title":"other"}),
    )
    .await
    .unwrap();
    assert_eq!(
        req(
            &s,
            "POST",
            &format!("/v1/workspaces/personal/changesets/{}/approve", id(&c)),
            json!({}),
        )
        .await
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
    // And it stays unapplicable: stale first, unapproved underneath.
    assert_eq!(
        req(&s, "POST", &apply, json!({})).await.unwrap_err().code,
        "VERSION_CONFLICT"
    );
}
#[tokio::test]
async fn approved_preview_is_atomic_and_agent_cannot_replay_approval() {
    let (_d, s) = setup().await;
    let c=req(&s,"POST","/v1/workspaces/personal/changesets/preview",json!({"operations":[{"method":"POST","path":"/v1/workspaces/personal/items","body":{"title":"new"}}]})).await.unwrap();
    let approve = format!("/v1/workspaces/personal/changesets/{}/approve", id(&c));
    let approved = s
        .handle(
            &Actor::local(),
            "POST",
            &approve,
            &HashMap::new(),
            json!({}),
            Some("approval-key"),
        )
        .await
        .unwrap();
    // Approving wrote it, in the same transaction.
    assert_eq!(approved["status"], "applied");
    let agent = Actor {
        id: "local-owner".into(),
        agent: true,
        connection: None,
    };
    assert_eq!(
        s.handle(
            &agent,
            "POST",
            &approve,
            &HashMap::new(),
            json!({}),
            Some("approval-key")
        )
        .await
        .unwrap_err()
        .status,
        403
    );
    let result = s
        .handle(
            &agent,
            "POST",
            &format!("/v1/workspaces/personal/changesets/{}/apply", id(&c)),
            &HashMap::new(),
            json!({}),
            Some("apply"),
        )
        .await
        .unwrap();
    // Already done, and said so — not done a second time.
    assert_eq!(result["already_applied"], true);
    assert!(result["results"].as_array().unwrap().is_empty());
    let items = req(&s, "GET", "/v1/workspaces/personal/items", json!({}))
        .await
        .unwrap();
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
}
#[tokio::test]
async fn habit_occurrences_do_not_complete_the_whole_habit() {
    let (_d, s) = setup().await;
    let i=req(&s,"POST","/v1/workspaces/personal/items",json!({"title":"practice","kind":"action","fields":{"recurrence":{"mode":"period_quota","times_per_week":3,"timezone":"Asia/Tokyo","weekdays":[]}}})).await.unwrap();
    for (n, d) in ["2026-09-11", "2026-09-12"].iter().enumerate() {
        let result = req(
            &s,
            "POST",
            &format!("/v1/workspaces/personal/actions/{}/complete", id(&i)),
            json!({"expected_version":n+1,"local_date":d}),
        )
        .await
        .unwrap();
        assert_eq!(result["item"]["state"], "active");
    }
    let mut q = HashMap::new();
    q.insert("local_date".into(), "2026-09-13".into());
    let today = s
        .handle(
            &Actor::local(),
            "GET",
            "/v1/workspaces/personal/today",
            &q,
            json!({}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(today["items"][0]["completed"], false);
}
#[tokio::test]
async fn calendar_respects_boundaries_timezone_rules_and_corrections() {
    let (_d, s) = setup().await;
    let habit=req(&s,"POST","/v1/workspaces/personal/items",json!({"title":"DST habit","kind":"action","start_date":"2026-03-08","due_date":"2026-03-10","fields":{"recurrence":{"mode":"fixed_schedule","times_per_week":2,"timezone":"America/New_York","weekdays":[0,6]}}})).await.unwrap();
    let path = format!("/v1/workspaces/personal/actions/{}/complete", id(&habit));
    req(&s,"POST",&path,json!({"expected_version":1,"local_date":"2026-03-08","completed_at":"2026-03-08T01:30:00-05:00"})).await.unwrap();
    req(
        &s,
        "POST",
        &format!("/v1/workspaces/personal/actions/{}/skip", id(&habit)),
        json!({"expected_version":2,"local_date":"2026-03-08"}),
    )
    .await
    .unwrap();
    let mut query = HashMap::new();
    query.insert("start".into(), "2026-03-07".into());
    query.insert("end".into(), "2026-03-10".into());
    query.insert("timezone".into(), "America/New_York".into());
    let calendar = s
        .handle(
            &Actor::local(),
            "GET",
            "/v1/workspaces/personal/calendar",
            &query,
            json!({}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(calendar["days"].as_array().unwrap().len(), 4);
    let march_eighth = &calendar["days"][1]["entries"];
    assert!(march_eighth
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["label"] == "habit" && entry["status"] == "skip"));
    assert!(calendar["days"][3]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["label"] == "due"));
    query.insert("timezone".into(), "Mars/Olympus".into());
    assert_eq!(
        s.handle(
            &Actor::local(),
            "GET",
            "/v1/workspaces/personal/calendar",
            &query,
            json!({}),
            None
        )
        .await
        .unwrap_err()
        .code,
        "VALIDATION_ERROR"
    );
}
#[tokio::test]
async fn observation_units_corrections_and_missing_measurement() {
    let (_d, s) = setup().await;
    let i = item(&s, "outcome").await;
    let m=req(&s,"POST","/v1/workspaces/personal/metrics",json!({"item_id":id(&i),"name":"Time","unit":"minutes","baseline":60,"target":30,"direction":"decrease"})).await.unwrap();
    let input = json!({"metric_id":id(&m),"unit":"minutes","value":45,"source":"timer"});
    let o = req(
        &s,
        "POST",
        "/v1/workspaces/personal/observations",
        input.clone(),
    )
    .await
    .unwrap();
    let mut correction = input.clone();
    correction["supersedes_id"] = o["id"].clone();
    correction["value"] = json!(44);
    req(
        &s,
        "POST",
        "/v1/workspaces/personal/observations",
        correction.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        req(
            &s,
            "POST",
            "/v1/workspaces/personal/observations",
            correction
        )
        .await
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
    let mut wrong = input;
    wrong["unit"] = json!("hours");
    assert_eq!(
        req(&s, "POST", "/v1/workspaces/personal/observations", wrong)
            .await
            .unwrap_err()
            .code,
        "INVALID_UNIT"
    );
}
#[tokio::test]
async fn json_roundtrip_and_failed_import_roll_back() {
    let (_d, s) = setup().await;
    let a = item(&s, "outcome").await;
    let b = item(&s, "action").await;
    relation(&s, &b, &a, "contributes_to").await.unwrap();
    req(
        &s,
        "POST",
        "/v1/workspaces/personal/records",
        json!({"item_ids":[id(&a)],"body":"learning","record_type":"review"}),
    )
    .await
    .unwrap();
    let backup = req(&s, "POST", "/v1/workspaces/personal/exports", json!({}))
        .await
        .unwrap();
    let (_d2, t) = setup().await;
    req(
        &t,
        "POST",
        "/v1/workspaces/personal/imports",
        backup.clone(),
    )
    .await
    .unwrap();
    let out = req(&t, "POST", "/v1/workspaces/personal/exports", json!({}))
        .await
        .unwrap();
    for col in [
        "items",
        "relations",
        "records",
        "metrics",
        "observations",
        "views",
    ] {
        assert_eq!(backup[col], out[col]);
    }
    assert_eq!(
        req(
            &t,
            "POST",
            "/v1/workspaces/personal/imports",
            backup.clone()
        )
        .await
        .unwrap_err()
        .code,
        "IMPORT_CONFLICT"
    );
    let (_d3, u) = setup().await;
    let mut broken = backup;
    broken["relations"][0]["target_id"] = json!("missing");
    assert!(req(&u, "POST", "/v1/workspaces/personal/imports", broken)
        .await
        .is_err());
    let mut tx = u.db.begin_read().await.unwrap();
    assert!(storage::list::<Item>(&mut tx, "personal", "items")
        .await
        .unwrap()
        .is_empty());
}
#[tokio::test]
async fn templates_and_linked_create_are_atomic() {
    let (_d, s) = setup().await;
    for template in ["free", "okr", "project", "learning", "habit"] {
        req(
            &s,
            "POST",
            &format!("/v1/workspaces/personal/templates/{template}/apply"),
            json!({"title":template}),
        )
        .await
        .unwrap();
    }
    let before = items(&s).await.len();
    assert!(req(
        &s,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"child","parent_id":"missing"})
    )
    .await
    .is_err());
    assert_eq!(items(&s).await.len(), before);
}
#[tokio::test]
async fn sample_data_deserializes() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(&dir.path().join("db").to_string_lossy())
        .await
        .unwrap();
    s.initialize(true).await.unwrap();
    let mut tx = s.db.begin_read().await.unwrap();
    for w in ["personal", "team", "organization"] {
        assert!(!storage::list::<Item>(&mut tx, w, "items")
            .await
            .unwrap()
            .is_empty());
        storage::list::<Record>(&mut tx, w, "records")
            .await
            .unwrap();
    }
}
