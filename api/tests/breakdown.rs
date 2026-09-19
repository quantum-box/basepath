//! Breaking a goal down until something is doable.
//!
//! What these tests hold in place:
//!
//! 1. **Depth is not a product decision.** Nothing here knows how many levels
//!    a plan has. A ten-year goal and a two-week one are both correct.
//! 2. **A move is a move.** Reparenting carries the branch with it, refuses
//!    to make a loop, and never reaches into another workspace. Archiving a
//!    parent does not take the children with it.
//! 3. **Both directions are answerable.** From a goal down to the actions
//!    that deliver it, and from an action up to why it exists — with the
//!    reasons that were actually recorded, not sentences invented now.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service, String, Actor) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("breakdown.sqlite3").to_string_lossy())
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
        tenant: pathbase_api::service::LOCAL_TENANT.into(),
        agent: false,
        connection: None,
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

/// Creates an item, optionally already placed under a parent.
async fn item(
    service: &Service,
    who: &Actor,
    w: &str,
    kind: &str,
    title: &str,
    parent: Option<&str>,
) -> String {
    let mut body = json!({"kind":kind,"title":title});
    if let Some(parent) = parent {
        body["parent_id"] = json!(parent);
    }
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

async fn link(
    service: &Service,
    who: &Actor,
    w: &str,
    source: &str,
    target: &str,
    kind: &str,
    rationale: &str,
) -> Result<Value> {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{w}/relations"),
        json!({"source_id":source,"target_id":target,"type":kind,"rationale":rationale}),
    )
    .await
}

/// Ten years down to one action: eight levels, none of which the code names.
async fn deep_chain(service: &Service, who: &Actor, w: &str) -> Vec<String> {
    let steps = [
        ("outcome", "10年: 事業を自分たちで選べる状態にする"),
        ("outcome", "年間: 収益の柱を2本にする"),
        ("outcome", "四半期: 新領域で最初の10社"),
        ("initiative", "施策: 既存顧客からの紹介経路"),
        ("milestone", "節目: 紹介が毎月5件入る"),
        ("milestone", "今月: 紹介の依頼手順を決める"),
        ("milestone", "今週: 上位20社に声をかける"),
        ("action", "行動: 3社に連絡する"),
    ];
    let mut ids = Vec::new();
    let mut parent: Option<String> = None;
    for (kind, title) in steps {
        let id = item(service, who, w, kind, title, parent.as_deref()).await;
        parent = Some(id.clone());
        ids.push(id);
    }
    ids
}

#[tokio::test]
async fn a_plan_can_be_as_deep_as_it_needs_to_be() {
    let (_dir, service, w, who) = setup().await;
    let chain = deep_chain(&service, &who, &w).await;

    // Eight levels, asked for in one call. Nothing in the schema, the route
    // or the response enumerates levels — depth is whatever the edges say.
    let tree = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{}/breakdown", chain[0]),
        &[("depth", "20")],
    )
    .await
    .unwrap();
    let nodes = tree["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), chain.len());
    assert_eq!(tree["truncated"], json!(false));
    let deepest = nodes.iter().map(|n| n["depth"].as_u64().unwrap()).max();
    assert_eq!(deepest, Some(7));

    // And it is still one chain: every node but the root has a parent.
    let rootless = nodes
        .iter()
        .filter(|node| node["parent_id"].is_null())
        .count();
    assert_eq!(rootless, 1);
}

#[tokio::test]
async fn a_deep_map_loads_a_piece_at_a_time_and_says_where_it_stopped() {
    let (_dir, service, w, who) = setup().await;
    let chain = deep_chain(&service, &who, &w).await;

    let shallow = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{}/breakdown", chain[0]),
        &[("depth", "2")],
    )
    .await
    .unwrap();
    let nodes = shallow["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    // The node at the edge is a handle, not a dead end: it says there is more
    // and it can be asked from directly.
    let edge = nodes.iter().find(|n| n["id"] == json!(chain[2])).unwrap();
    assert_eq!(edge["has_more_children"], json!(true));
    assert_eq!(edge["child_count"], json!(1));
    assert_eq!(edge["loaded_children"], json!(0));

    let next = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{}/breakdown", chain[2]),
        &[("depth", "2")],
    )
    .await
    .unwrap();
    assert_eq!(next["nodes"].as_array().unwrap().len(), 3);

    // A node budget truncates, and says so rather than quietly returning less.
    let capped = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{}/breakdown", chain[0]),
        &[("depth", "20"), ("limit", "3")],
    )
    .await
    .unwrap();
    assert_eq!(capped["truncated"], json!(true));
    assert!(capped["nodes"].as_array().unwrap().len() <= 3);
}

#[tokio::test]
async fn an_action_can_say_why_it_exists() {
    let (_dir, service, w, who) = setup().await;
    let chain = deep_chain(&service, &who, &w).await;
    let action = chain.last().unwrap();

    let why = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{action}/ancestry"),
        &[],
    )
    .await
    .unwrap();
    let ancestors = why["ancestors"].as_array().unwrap();
    assert_eq!(ancestors.len(), 7);
    // Top first, so it reads as an explanation rather than a traversal.
    assert_eq!(ancestors[0]["id"], json!(chain[0]));
    assert_eq!(why["top_level"], json!(false));

    let top = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{}/ancestry", chain[0]),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(top["ancestors"].as_array().unwrap().len(), 0);
    assert_eq!(top["top_level"], json!(true));
}

#[tokio::test]
async fn structure_and_contribution_stay_different_questions() {
    let (_dir, service, w, who) = setup().await;
    let company = item(&service, &who, &w, "outcome", "会社: 継続率を上げる", None).await;
    let team = item(
        &service,
        &who,
        &w,
        "outcome",
        "チーム: 導入支援を厚くする",
        None,
    )
    .await;
    let work = item(
        &service,
        &who,
        &w,
        "initiative",
        "施策: 導入1週間の伴走",
        Some(&team),
    )
    .await;
    link(
        &service,
        &who,
        &w,
        &work,
        &company,
        "contributes_to",
        "初月の離脱がいちばん多いため",
    )
    .await
    .unwrap();

    let why = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{work}/ancestry"),
        &[],
    )
    .await
    .unwrap();
    // It is part of the team's goal, and it contributes to the company's.
    // Two different facts, reported separately.
    assert_eq!(why["ancestors"][0]["id"], json!(team));
    assert_eq!(why["contributes_to"][0]["id"], json!(company));
    assert_eq!(
        why["contributes_to"][0]["rationale"],
        json!("初月の離脱がいちばん多いため")
    );

    // The company's structural subtree does not swallow it: contribution is
    // not containment.
    let company_tree = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{company}/breakdown"),
        &[("depth", "20")],
    )
    .await
    .unwrap();
    let ids: Vec<&str> = company_tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![company.as_str()]);
}

#[tokio::test]
async fn a_branch_can_be_moved_and_its_children_come_with_it() {
    let (_dir, service, w, who) = setup().await;
    let first = item(&service, &who, &w, "outcome", "旧: 四半期の柱", None).await;
    let second = item(&service, &who, &w, "outcome", "新: 四半期の柱", None).await;
    let branch = item(&service, &who, &w, "initiative", "施策", Some(&first)).await;
    let leaf = item(&service, &who, &w, "action", "行動", Some(&branch)).await;

    let version = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{branch}"),
        &[],
    )
    .await
    .unwrap()["version"]
        .clone();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{branch}/reparent"),
        json!({"parent_id":second,"expected_version":version,
               "rationale":"この柱のほうが近いため"}),
    )
    .await
    .unwrap();

    // The leaf was never attached to the old parent — it is attached to the
    // branch, and that has not changed.
    let moved = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{leaf}/ancestry"),
        &[],
    )
    .await
    .unwrap();
    let chain: Vec<&str> = moved["ancestors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert_eq!(chain, vec![second.as_str(), branch.as_str()]);
    // The reason for the move is on the link that moved, and the entry names
    // which end it explains.
    assert_eq!(moved["ancestors"][0]["child_id"], json!(branch));
    assert_eq!(
        moved["ancestors"][0]["rationale"],
        json!("この柱のほうが近いため")
    );

    // And the old parent has nothing left under it, rather than a copy.
    let old = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{first}/breakdown"),
        &[("depth", "20")],
    )
    .await
    .unwrap();
    assert_eq!(old["nodes"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_move_that_would_make_a_loop_is_refused_and_changes_nothing() {
    let (_dir, service, w, who) = setup().await;
    let top = item(&service, &who, &w, "outcome", "上", None).await;
    let middle = item(&service, &who, &w, "initiative", "中", Some(&top)).await;
    let bottom = item(&service, &who, &w, "milestone", "下", Some(&middle)).await;

    let version = |id: &str| {
        let service = &service;
        let who = &who;
        let w = w.clone();
        let id = id.to_owned();
        async move {
            query(service, who, &format!("/v1/workspaces/{w}/items/{id}"), &[])
                .await
                .unwrap()["version"]
                .clone()
        }
    };

    // Into its own descendant.
    let refused = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{top}/reparent"),
        json!({"parent_id":bottom,"expected_version":version(&top).await}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "CYCLE_DETECTED");

    // Into itself.
    let itself = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{middle}/reparent"),
        json!({"parent_id":middle,"expected_version":version(&middle).await}),
    )
    .await
    .unwrap_err();
    assert_eq!(itself.code, "CYCLE_DETECTED");

    // Nothing moved. A refused move must leave the plan exactly as it was.
    let still = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{bottom}/ancestry"),
        &[],
    )
    .await
    .unwrap();
    let chain: Vec<&str> = still["ancestors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert_eq!(chain, vec![top.as_str(), middle.as_str()]);
}

#[tokio::test]
async fn a_parent_in_another_workspace_is_not_reachable() {
    let (_dir, service, w, who) = setup().await;
    let mine = item(&service, &who, &w, "outcome", "こちらの目標", None).await;
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
    let theirs = item(&service, &who, &other, "outcome", "あちらの目標", None).await;

    let version = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{mine}"),
        &[],
    )
    .await
    .unwrap()["version"]
        .clone();
    let refused = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{mine}/reparent"),
        json!({"parent_id":theirs,"expected_version":version}),
    )
    .await
    .unwrap_err();
    // Not "forbidden": across the boundary the item is not there to point at.
    assert_eq!(refused.status, 404);
}

#[tokio::test]
async fn an_item_can_be_detached_without_being_deleted() {
    let (_dir, service, w, who) = setup().await;
    let parent = item(&service, &who, &w, "outcome", "親", None).await;
    let child = item(&service, &who, &w, "initiative", "子", Some(&parent)).await;
    let grandchild = item(&service, &who, &w, "action", "孫", Some(&child)).await;

    let version = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{child}"),
        &[],
    )
    .await
    .unwrap()["version"]
        .clone();
    let detached = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{child}/reparent"),
        json!({"parent_id":null,"expected_version":version,
               "rationale":"どこにも属さないまま置いておく"}),
    )
    .await
    .unwrap();
    assert_eq!(detached["top_level"], json!(true));

    // Its own branch is intact. Leaving a structure is not leaving existence.
    let tree = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{child}/breakdown"),
        &[("depth", "20")],
    )
    .await
    .unwrap();
    let ids: Vec<&str> = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![child.as_str(), grandchild.as_str()]);
}

#[tokio::test]
async fn archiving_a_parent_leaves_its_children_alone() {
    let (_dir, service, w, who) = setup().await;
    let parent = item(&service, &who, &w, "outcome", "整理する親", None).await;
    let child = item(&service, &who, &w, "initiative", "残る子", Some(&parent)).await;

    let version = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{parent}"),
        &[],
    )
    .await
    .unwrap()["version"]
        .clone();
    call(
        &service,
        &who,
        "PATCH",
        &format!("/v1/workspaces/{w}/items/{parent}"),
        json!({"archived_at":"2026-09-19T00:00:00Z","expected_version":version}),
    )
    .await
    .unwrap();

    // The child is still there, still readable, and still says what it was
    // part of. Tidying a parent is not a decision about the children.
    let survivor = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{child}"),
        &[],
    )
    .await
    .unwrap();
    assert!(survivor["archived_at"].is_null());
    let why = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{child}/ancestry"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(why["ancestors"][0]["id"], json!(parent));
}

#[tokio::test]
async fn siblings_keep_the_order_the_person_put_them_in() {
    let (_dir, service, w, who) = setup().await;
    let parent = item(&service, &who, &w, "outcome", "親", None).await;
    let first = item(&service, &who, &w, "milestone", "あ", Some(&parent)).await;
    let second = item(&service, &who, &w, "milestone", "い", Some(&parent)).await;
    let third = item(&service, &who, &w, "milestone", "う", Some(&parent)).await;

    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{parent}/children"),
        json!({"order":[third, first, second]}),
    )
    .await
    .unwrap();

    let tree = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{parent}/breakdown"),
        &[],
    )
    .await
    .unwrap();
    let ordered: Vec<&str> = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ordered,
        vec![third.as_str(), first.as_str(), second.as_str()]
    );

    // Newly added work goes to the end of an arranged list, not the top.
    let latecomer = item(&service, &who, &w, "milestone", "え", Some(&parent)).await;
    let again = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{parent}/breakdown"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        again["nodes"].as_array().unwrap().last().unwrap()["id"],
        json!(latecomer)
    );

    // Something that is not a child cannot be ordered into the list.
    let stranger = item(&service, &who, &w, "milestone", "よそ", None).await;
    let refused = call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{parent}/children"),
        json!({"order":[stranger]}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.status, 422);
}

#[tokio::test]
async fn gaps_are_reported_and_never_filled_in() {
    let (_dir, service, w, who) = setup().await;
    let untouched = item(&service, &who, &w, "outcome", "分解していない目標", None).await;
    let abstract_goal = item(&service, &who, &w, "outcome", "抽象のまま", None).await;
    let _abstract_child = item(
        &service,
        &who,
        &w,
        "initiative",
        "施策も抽象",
        Some(&abstract_goal),
    )
    .await;
    let done_properly = item(&service, &who, &w, "outcome", "降りている目標", None).await;
    let step = item(
        &service,
        &who,
        &w,
        "milestone",
        "節目",
        Some(&done_properly),
    )
    .await;
    let _action = item(&service, &who, &w, "action", "行動", Some(&step)).await;

    let found = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/breakdown/gaps"),
        &[],
    )
    .await
    .unwrap();
    let by_item = |id: &str| {
        found["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|gap| gap["item_id"] == json!(id))
            .map(|gap| gap["gap"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(by_item(&untouched), vec!["not_broken_down"]);
    // The gap that hides: it looks broken down, and nothing in it is doable.
    assert_eq!(by_item(&abstract_goal), vec!["no_action_beneath"]);
    assert!(by_item(&done_properly).is_empty());
    // Reported, not repaired — filling one in would invent a plan nobody made.
    assert_eq!(found["repaired"], json!(false));
}

#[tokio::test]
async fn a_dependency_on_something_that_went_away_is_a_gap() {
    let (_dir, service, w, who) = setup().await;
    let goal = item(&service, &who, &w, "outcome", "目標", None).await;
    let step = item(&service, &who, &w, "milestone", "節目", Some(&goal)).await;
    let action = item(&service, &who, &w, "action", "行動", Some(&step)).await;
    let blocker = item(&service, &who, &w, "action", "先にやること", Some(&step)).await;
    link(
        &service,
        &who,
        &w,
        &action,
        &blocker,
        "depends_on",
        "こちらが先に終わっている必要がある",
    )
    .await
    .unwrap();

    let clean = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/breakdown/gaps"),
        &[],
    )
    .await
    .unwrap();
    assert!(!clean.to_string().contains("dangling_dependency"));

    let version = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{blocker}"),
        &[],
    )
    .await
    .unwrap()["version"]
        .clone();
    call(
        &service,
        &who,
        "PATCH",
        &format!("/v1/workspaces/{w}/items/{blocker}"),
        json!({"archived_at":"2026-09-19T00:00:00Z","expected_version":version}),
    )
    .await
    .unwrap();

    // The waiting did not stop being true because the thing waited on was
    // tidied away. Someone has to decide what happens now.
    let found = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/breakdown/gaps"),
        &[],
    )
    .await
    .unwrap();
    let dangling: Vec<&Value> = found["gaps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|gap| gap["gap"] == json!("dangling_dependency"))
        .collect();
    assert_eq!(dangling.len(), 1);
    assert_eq!(dangling[0]["item_id"], json!(action));
    assert_eq!(dangling[0]["target_id"], json!(blocker));
}

#[tokio::test]
async fn a_move_is_recorded_where_the_rest_of_the_items_history_is() {
    let (_dir, service, w, who) = setup().await;
    let first = item(&service, &who, &w, "outcome", "はじめの親", None).await;
    let second = item(&service, &who, &w, "outcome", "つぎの親", None).await;
    let moved = item(&service, &who, &w, "initiative", "動く施策", Some(&first)).await;

    let version = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{moved}"),
        &[],
    )
    .await
    .unwrap()["version"]
        .clone();
    call(
        &service,
        &who,
        "POST",
        &format!("/v1/workspaces/{w}/items/{moved}/reparent"),
        json!({"parent_id":second,"expected_version":version,"rationale":"優先順位が変わった"}),
    )
    .await
    .unwrap();

    let timeline = query(
        &service,
        &who,
        &format!("/v1/workspaces/{w}/items/{moved}/timeline"),
        &[],
    )
    .await
    .unwrap();
    let text = timeline.to_string();
    assert!(text.contains("breakdown_change"), "{text}");
    assert!(text.contains("優先順位が変わった"), "{text}");

    // And the request itself is in the audit log, like every other change.
    let audit = query(&service, &who, &format!("/v1/workspaces/{w}/audit"), &[])
        .await
        .unwrap();
    assert!(audit.to_string().contains("reparent"));
}
