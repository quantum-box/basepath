use pathbase_api::{
    model::*,
    service::{Actor, Service},
    storage,
};
use serde_json::{json, Value};
use std::collections::HashMap;
fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(&dir.path().join("db.sqlite")).unwrap();
    s.initialize(false).unwrap();
    (dir, s)
}
fn req(s: &Service, method: &str, path: &str, b: Value) -> Result<Value> {
    s.handle(
        &Actor::local(),
        method,
        path,
        &HashMap::new(),
        b,
        Some(&uuid::Uuid::new_v4().to_string()),
    )
}
fn get_with_query(s: &Service, path: &str, query: &[(&str, &str)]) -> Result<Value> {
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
}
fn item(s: &Service, kind: &str) -> Value {
    req(
        s,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"テスト項目","kind":kind}),
    )
    .unwrap()
}
fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}
fn relation(s: &Service, source: &Value, target: &Value, kind: &str) -> Result<Value> {
    req(
        s,
        "POST",
        "/v1/workspaces/personal/relations",
        json!({"source_id":id(source),"target_id":id(target),"type":kind}),
    )
}
#[test]
fn weekly_review_aggregates_evidence_and_preserves_finalized_corrections() {
    let (_d, s) = setup();
    let goal = item(&s, "outcome");
    req(
        &s,
        "PATCH",
        &format!("/v1/workspaces/personal/items/{}", id(&goal)),
        json!({"expected_version":1,"fields":{"self_assessment":72}}),
    )
    .unwrap();
    let action = req(
        &s,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"週の行動","kind":"action","scheduled_date":"2026-09-14"}),
    )
    .unwrap();
    req(
        &s,
        "POST",
        &format!("/v1/workspaces/personal/actions/{}/complete", id(&action)),
        json!({"expected_version":1,"local_date":"2026-09-14","completed_at":"2026-09-14T01:00:00Z"}),
    )
    .unwrap();
    let metric = req(
        &s,
        "POST",
        "/v1/workspaces/personal/metrics",
        json!({"item_id":id(&goal),"name":"利用者","unit":"人","baseline":0,"target":100,"direction":"increase"}),
    )
    .unwrap();
    for (value, observed) in [(10, "2026-09-06T01:00:00Z"), (14, "2026-09-14T02:00:00Z")] {
        req(
            &s,
            "POST",
            "/v1/workspaces/personal/observations",
            json!({"metric_id":id(&metric),"value":value,"unit":"人","source":"test","observed_at":observed}),
        )
        .unwrap();
    }
    let summary = get_with_query(
        &s,
        "/v1/workspaces/personal/weekly-review",
        &[("week_start", "2026-09-14")],
    )
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
    )
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
    .unwrap();
    let correction = req(
        &s,
        "POST",
        "/v1/workspaces/personal/weekly-reviews/draft",
        json!({"week_start":"2026-09-14","learnings":"訂正版","challenges":"","next_focus":""}),
    )
    .unwrap();
    assert_eq!(finalized["status"], "finalized");
    assert_eq!(correction["revision"], 2);
    assert_eq!(correction["supersedes_id"], finalized["id"]);
}

#[test]
fn weekly_review_rejects_non_monday_boundary_and_unauthorized_access() {
    let (_d, s) = setup();
    assert_eq!(
        get_with_query(
            &s,
            "/v1/workspaces/personal/weekly-review",
            &[("week_start", "2026-09-15")],
        )
        .unwrap_err()
        .code,
        "VALIDATION_ERROR"
    );
    let outsider = Actor {
        id: "outsider".into(),
        agent: false,
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
        .unwrap_err()
        .status,
        404
    );
    req(
        &s,
        "POST",
        "/v1/workspaces/team/items",
        json!({"title":"担当行動","kind":"action","scheduled_date":"2026-09-14","fields":{"assignee":"local-owner"}}),
    )
    .unwrap();
    let team = get_with_query(
        &s,
        "/v1/workspaces/team/weekly-review",
        &[("week_start", "2026-09-14")],
    )
    .unwrap();
    assert_eq!(team["members"][0]["actor"], "local-owner");
    assert_eq!(team["members"][0]["incomplete"], 1);
}
#[test]
fn title_only_survives_restart() {
    let (d, s) = setup();
    let i = item(&s, "outcome");
    drop(s);
    let s = Service::open(&d.path().join("db.sqlite")).unwrap();
    let saved = req(
        &s,
        "GET",
        &format!("/v1/workspaces/personal/items/{}", id(&i)),
        json!({}),
    )
    .unwrap();
    assert_eq!(i, saved);
    assert!(saved["fields"]["self_assessment"].is_null());
}
#[test]
fn idempotent_completion_does_not_change_outcomes() {
    let (_d, s) = setup();
    let goal = item(&s, "outcome");
    let action = item(&s, "action");
    relation(&s, &action, &goal, "contributes_to").unwrap();
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
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first["outcome_updated"], false);
    let snap = req(&s, "GET", "/v1/workspaces/personal/snapshot", json!({})).unwrap();
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
#[test]
fn reused_key_different_input_fails() {
    let (_d, s) = setup();
    let a = Actor::local();
    s.handle(
        &a,
        "POST",
        "/v1/workspaces/personal/items",
        &HashMap::new(),
        json!({"title":"A"}),
        Some("key"),
    )
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
        .unwrap_err()
        .code,
        "IDEMPOTENCY_CONFLICT"
    );
}
#[test]
fn concurrent_updates_only_one_wins() {
    let (_d, s) = setup();
    let i = item(&s, "outcome");
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let s = s.clone();
            let path = format!("/v1/workspaces/personal/items/{}", id(&i));
            std::thread::spawn(move || {
                req(
                    &s,
                    "PATCH",
                    &path,
                    json!({"expected_version":1,"title":format!("update-{n}")}),
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        "VERSION_CONFLICT"
    );
}
#[test]
fn concurrent_graph_cycle_cannot_commit() {
    let (_d, s) = setup();
    let a = item(&s, "initiative");
    let b = item(&s, "initiative");
    let s1 = s.clone();
    let a1 = a.clone();
    let b1 = b.clone();
    let x = std::thread::spawn(move || relation(&s1, &a1, &b1, "depends_on"));
    let y = std::thread::spawn(move || relation(&s, &b, &a, "depends_on"));
    let results = [x.join().unwrap(), y.join().unwrap()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        "CYCLE_DETECTED"
    );
}
#[test]
fn graph_types_are_separate_and_symmetric_links_unique() {
    let (_d, s) = setup();
    let a = item(&s, "action");
    let b = item(&s, "outcome");
    let c = item(&s, "outcome");
    relation(&s, &a, &b, "contributes_to").unwrap();
    relation(&s, &a, &c, "contributes_to").unwrap();
    relation(&s, &a, &b, "part_of").unwrap();
    assert!(relation(&s, &a, &c, "part_of").is_err());
    relation(&s, &b, &a, "depends_on").unwrap();
    relation(&s, &a, &b, "relates_to").unwrap();
    assert_eq!(
        relation(&s, &b, &a, "relates_to").unwrap_err().code,
        "DUPLICATE_RELATION"
    );
}
#[test]
fn workspace_isolation_and_viewer_denial() {
    let (_d, s) = setup();
    let i = item(&s, "outcome");
    assert_eq!(
        req(
            &s,
            "GET",
            &format!("/v1/workspaces/team/items/{}", id(&i)),
            json!({})
        )
        .unwrap_err()
        .status,
        404
    );
    let actor = Actor {
        id: "outsider".into(),
        agent: false,
    };
    assert_eq!(
        s.handle(
            &actor,
            "GET",
            "/v1/workspaces/personal/snapshot",
            &HashMap::new(),
            json!({}),
            None
        )
        .unwrap_err()
        .status,
        404
    );
    s.db.lock()
        .unwrap()
        .execute(
            "INSERT INTO memberships VALUES('personal','outsider','viewer')",
            [],
        )
        .unwrap();
    assert!(s
        .handle(
            &actor,
            "GET",
            "/v1/workspaces/personal/snapshot",
            &HashMap::new(),
            json!({}),
            None
        )
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
        .unwrap_err()
        .status,
        403
    );
}
#[test]
fn proposals_require_human_approval_and_reject_stale_base() {
    let (_d, s) = setup();
    let i = item(&s, "outcome");
    let agent = Actor {
        id: "local-owner".into(),
        agent: true,
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
        .unwrap_err()
        .code,
        "APPROVAL_REQUIRED"
    );
    req(
        &s,
        "POST",
        &format!("/v1/workspaces/personal/changesets/{}/approve", id(&c)),
        json!({}),
    )
    .unwrap();
    req(
        &s,
        "PATCH",
        &format!("/v1/workspaces/personal/items/{}", id(&i)),
        json!({"expected_version":1,"title":"other"}),
    )
    .unwrap();
    assert_eq!(
        req(&s, "POST", &apply, json!({})).unwrap_err().code,
        "VERSION_CONFLICT"
    );
}
#[test]
fn approved_preview_is_atomic_and_agent_cannot_replay_approval() {
    let (_d, s) = setup();
    let c=req(&s,"POST","/v1/workspaces/personal/changesets/preview",json!({"operations":[{"method":"POST","path":"/v1/workspaces/personal/items","body":{"title":"new"}}]})).unwrap();
    let approve = format!("/v1/workspaces/personal/changesets/{}/approve", id(&c));
    s.handle(
        &Actor::local(),
        "POST",
        &approve,
        &HashMap::new(),
        json!({}),
        Some("approval-key"),
    )
    .unwrap();
    let agent = Actor {
        id: "local-owner".into(),
        agent: true,
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
        .unwrap();
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
}
#[test]
fn habit_occurrences_do_not_complete_the_whole_habit() {
    let (_d, s) = setup();
    let i=req(&s,"POST","/v1/workspaces/personal/items",json!({"title":"practice","kind":"action","fields":{"recurrence":{"mode":"period_quota","times_per_week":3,"timezone":"Asia/Tokyo","weekdays":[]}}})).unwrap();
    for (n, d) in ["2026-09-11", "2026-09-12"].iter().enumerate() {
        let result = req(
            &s,
            "POST",
            &format!("/v1/workspaces/personal/actions/{}/complete", id(&i)),
            json!({"expected_version":n+1,"local_date":d}),
        )
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
        .unwrap();
    assert_eq!(today["items"][0]["completed"], false);
}
#[test]
fn observation_units_corrections_and_missing_measurement() {
    let (_d, s) = setup();
    let i = item(&s, "outcome");
    let m=req(&s,"POST","/v1/workspaces/personal/metrics",json!({"item_id":id(&i),"name":"Time","unit":"minutes","baseline":60,"target":30,"direction":"decrease"})).unwrap();
    let input = json!({"metric_id":id(&m),"unit":"minutes","value":45,"source":"timer"});
    let o = req(
        &s,
        "POST",
        "/v1/workspaces/personal/observations",
        input.clone(),
    )
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
    .unwrap();
    assert_eq!(
        req(
            &s,
            "POST",
            "/v1/workspaces/personal/observations",
            correction
        )
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
    let mut wrong = input;
    wrong["unit"] = json!("hours");
    assert_eq!(
        req(&s, "POST", "/v1/workspaces/personal/observations", wrong)
            .unwrap_err()
            .code,
        "INVALID_UNIT"
    );
}
#[test]
fn json_roundtrip_and_failed_import_roll_back() {
    let (_d, s) = setup();
    let a = item(&s, "outcome");
    let b = item(&s, "action");
    relation(&s, &b, &a, "contributes_to").unwrap();
    req(
        &s,
        "POST",
        "/v1/workspaces/personal/records",
        json!({"item_ids":[id(&a)],"body":"learning","record_type":"review"}),
    )
    .unwrap();
    let backup = req(&s, "POST", "/v1/workspaces/personal/exports", json!({})).unwrap();
    let (_d2, t) = setup();
    req(
        &t,
        "POST",
        "/v1/workspaces/personal/imports",
        backup.clone(),
    )
    .unwrap();
    let out = req(&t, "POST", "/v1/workspaces/personal/exports", json!({})).unwrap();
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
        .unwrap_err()
        .code,
        "IMPORT_CONFLICT"
    );
    let (_d3, u) = setup();
    let mut broken = backup;
    broken["relations"][0]["target_id"] = json!("missing");
    assert!(req(&u, "POST", "/v1/workspaces/personal/imports", broken).is_err());
    assert!(
        storage::list::<Item>(&u.db.lock().unwrap(), "personal", "items")
            .unwrap()
            .is_empty()
    );
}
#[test]
fn templates_and_linked_create_are_atomic() {
    let (_d, s) = setup();
    for template in ["free", "okr", "project", "learning", "habit"] {
        req(
            &s,
            "POST",
            &format!("/v1/workspaces/personal/templates/{template}/apply"),
            json!({"title":template}),
        )
        .unwrap();
    }
    let before = storage::list::<Item>(&s.db.lock().unwrap(), "personal", "items")
        .unwrap()
        .len();
    assert!(req(
        &s,
        "POST",
        "/v1/workspaces/personal/items",
        json!({"title":"child","parent_id":"missing"})
    )
    .is_err());
    assert_eq!(
        storage::list::<Item>(&s.db.lock().unwrap(), "personal", "items")
            .unwrap()
            .len(),
        before
    );
}
#[test]
fn sample_data_deserializes() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(&dir.path().join("db")).unwrap();
    s.initialize(true).unwrap();
    let db = s.db.lock().unwrap();
    for w in ["personal", "team", "organization"] {
        assert!(!storage::list::<Item>(&db, w, "items").unwrap().is_empty());
        storage::list::<Record>(&db, w, "records").unwrap();
    }
}
