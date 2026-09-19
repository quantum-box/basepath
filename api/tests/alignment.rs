//! Whose goal is this, and what does it roll up to.
//!
//! Two things are being protected. The first is the boundary: alignment is a
//! graph inside **one** workspace, and a goal in someone's personal workspace
//! is not in it and cannot be reached from it. Putting a goal in a shared
//! workspace is the act of sharing it; a company goal pointing at a private
//! one would make that choice for them.
//!
//! The second is the distinction between `part_of` and `contributes_to`. One
//! is structure, the other is contribution. Collapsing them would make "what
//! is this part of" and "what does this help" the same question.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("alignment.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    (dir, service)
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

/// A shared workspace, owned by a real person.
///
/// Not `Actor::local()`: a workspace created by the local preview owner is
/// marked local and cannot be shared, which is the point of that flag.
async fn shared(service: &Service) -> (String, Actor) {
    let owner = person("us_alice");
    service.provision_personal(&owner).await.unwrap();
    let workspace = call(
        service,
        &owner,
        "POST",
        "/v1/workspaces",
        json!({"name":"会社","scope":"組織"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (workspace, owner)
}

async fn goal(
    service: &Service,
    who: &Actor,
    workspace: &str,
    title: &str,
    owner: Value,
) -> String {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{workspace}/items"),
        json!({"kind":"outcome","title":title,"fields":{"owner":owner}}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn relate(
    service: &Service,
    who: &Actor,
    workspace: &str,
    source: &str,
    target: &str,
    kind: &str,
) -> Result<Value> {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{workspace}/relations"),
        json!({"source_id":source,"target_id":target,"type":kind}),
    )
    .await
}

#[tokio::test]
async fn a_goal_belongs_to_the_organization_a_team_or_a_person() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;

    let company = goal(
        &service,
        &owner,
        &workspace,
        "償却前利益を伸ばす",
        json!({"kind":"organization"}),
    )
    .await;
    let team = goal(
        &service,
        &owner,
        &workspace,
        "運営コストを下げる",
        json!({"kind":"team","id":"運営"}),
    )
    .await;
    let mine = goal(
        &service,
        &owner,
        &workspace,
        "発注の手戻りを減らす",
        json!({"kind":"person","id":"us_alice"}),
    )
    .await;

    // The workspace is the organization, so naming it again is refused: a
    // second place for the same fact is a second place for it to be wrong.
    assert!(call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{workspace}/items"),
        json!({"kind":"outcome","title":"x","fields":{"owner":{"kind":"organization","id":"会社"}}}),
    )
    .await
    .is_err());
    // A person who is not a member is not an owner of anything here.
    assert!(call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{workspace}/items"),
        json!({"kind":"outcome","title":"x","fields":{"owner":{"kind":"person","id":"us_stranger"}}}),
    )
    .await
    .is_err());
    for bad in [json!({"kind":"team","id":""}), json!({"kind":"department"})] {
        assert!(call(
            &service,
            &owner,
            "POST",
            &format!("/v1/workspaces/{workspace}/items"),
            json!({"kind":"outcome","title":"x","fields":{"owner":bad}}),
        )
        .await
        .is_err());
    }

    let map = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(map["goals"].as_array().unwrap().len(), 3);
    assert_eq!(map["teams"], json!(["運営"]));
    assert_eq!(map["people"], json!(["us_alice"]));
    assert_eq!(map["unowned_goals"], 0);
    // Nothing is connected yet, so all three are top-level. That is normal.
    assert_eq!(map["orphan_goals"], 3);
    assert!([company, team, mine].iter().all(|id| map["goals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["id"] == json!(id))));
}

#[tokio::test]
async fn part_of_and_contributes_to_stay_different_questions() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;
    let company = goal(
        &service,
        &owner,
        &workspace,
        "会社の目標",
        json!({"kind":"organization"}),
    )
    .await;
    let other = goal(
        &service,
        &owner,
        &workspace,
        "もう一つの会社目標",
        json!({"kind":"organization"}),
    )
    .await;
    let team = goal(
        &service,
        &owner,
        &workspace,
        "チームの目標",
        json!({"kind":"team","id":"運営"}),
    )
    .await;

    // Structure: one parent, and only one.
    relate(&service, &owner, &workspace, &team, &company, "part_of")
        .await
        .unwrap();
    assert!(
        relate(&service, &owner, &workspace, &team, &other, "part_of")
            .await
            .is_err(),
        "a second structural parent is not a thing"
    );
    // Contribution: as many as are true.
    relate(
        &service,
        &owner,
        &workspace,
        &team,
        &other,
        "contributes_to",
    )
    .await
    .unwrap();

    let map = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[],
    )
    .await
    .unwrap();
    let node = map["goals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == json!(team))
        .unwrap();
    assert_eq!(node["part_of"], json!([company]));
    assert_eq!(node["contributes_to"], json!([other]));
    assert_eq!(node["orphan"], false);

    // And the company goal can be opened from above.
    let above = map["goals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == json!(company))
        .unwrap();
    assert_eq!(above["supported_by"], json!([team]));
    assert_eq!(
        above["orphan"], true,
        "a top-level goal has nothing above it"
    );
}

#[tokio::test]
async fn alignment_cannot_close_a_loop() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;
    let a = goal(
        &service,
        &owner,
        &workspace,
        "A",
        json!({"kind":"organization"}),
    )
    .await;
    let b = goal(
        &service,
        &owner,
        &workspace,
        "B",
        json!({"kind":"team","id":"t"}),
    )
    .await;
    let c = goal(
        &service,
        &owner,
        &workspace,
        "C",
        json!({"kind":"team","id":"u"}),
    )
    .await;

    relate(&service, &owner, &workspace, &b, &a, "contributes_to")
        .await
        .unwrap();
    relate(&service, &owner, &workspace, &c, &b, "contributes_to")
        .await
        .unwrap();
    // A contributing back into what contributes to it makes "what does this
    // roll up to" unanswerable, which is the one question the map answers.
    let refused = relate(&service, &owner, &workspace, &a, &c, "contributes_to")
        .await
        .unwrap_err();
    assert_eq!(refused.code, "CYCLE_DETECTED");
    assert!(
        relate(&service, &owner, &workspace, &a, &a, "contributes_to")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_personal_plan_is_not_in_the_organisation_graph() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;
    let company = goal(
        &service,
        &owner,
        &workspace,
        "会社の目標",
        json!({"kind":"organization"}),
    )
    .await;

    // The same person's own workspace, with a goal they have not shared.
    let personal = query(&service, &owner, "/v1/workspaces", &[])
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
    let private = call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{personal}/items"),
        json!({"kind":"outcome","title":"個人の目標"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // It cannot be linked into the organisation's graph, in either direction.
    assert!(relate(
        &service,
        &owner,
        &workspace,
        &private,
        &company,
        "contributes_to"
    )
    .await
    .is_err());
    assert!(relate(
        &service,
        &owner,
        &personal,
        &private,
        &company,
        "contributes_to"
    )
    .await
    .is_err());

    // And it does not appear in the graph, or in its counts.
    let map = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[],
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&map).unwrap();
    assert!(!serialized.contains("個人の目標"), "{serialized}");
    assert!(!serialized.contains(&private));
    assert_eq!(map["goals"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn someone_elses_goal_is_theirs_to_change() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;

    // An editor joins.
    let version = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/members"),
        &[],
    )
    .await
    .unwrap()["workspace"]["version"]
        .as_i64()
        .unwrap();
    let invitation = call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{workspace}/invitations"),
        json!({"target_actor":"us_bob","role":"editor","expected_version":version}),
    )
    .await
    .unwrap();
    call(
        &service,
        &person("us_bob"),
        "POST",
        &format!(
            "/v1/invitations/{}/accept",
            invitation["id"].as_str().unwrap()
        ),
        json!({"expected_version":1}),
    )
    .await
    .unwrap();

    let bobs = goal(
        &service,
        &person("us_bob"),
        &workspace,
        "ボブの目標",
        json!({"kind":"person","id":"us_bob"}),
    )
    .await;
    let team = goal(
        &service,
        &owner,
        &workspace,
        "チームの目標",
        json!({"kind":"team","id":"運営"}),
    )
    .await;

    // A second editor cannot rewrite Bob's commitment.
    let version2 = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/members"),
        &[],
    )
    .await
    .unwrap()["workspace"]["version"]
        .as_i64()
        .unwrap();
    let invitation = call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{workspace}/invitations"),
        json!({"target_actor":"us_carol","role":"editor","expected_version":version2}),
    )
    .await
    .unwrap();
    call(
        &service,
        &person("us_carol"),
        "POST",
        &format!(
            "/v1/invitations/{}/accept",
            invitation["id"].as_str().unwrap()
        ),
        json!({"expected_version":1}),
    )
    .await
    .unwrap();
    let refused = call(
        &service,
        &person("us_carol"),
        "PATCH",
        &format!("/v1/workspaces/{workspace}/items/{bobs}"),
        json!({"expected_version":1,"title":"勝手に書き換えた"}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "GOAL_OWNER_REQUIRED");

    // Bob can, and so can the workspace owner — who could remove him anyway,
    // so refusing there would only be theatre.
    call(
        &service,
        &person("us_bob"),
        "PATCH",
        &format!("/v1/workspaces/{workspace}/items/{bobs}"),
        json!({"expected_version":1,"title":"ボブが直した"}),
    )
    .await
    .unwrap();
    call(
        &service,
        &owner,
        "PATCH",
        &format!("/v1/workspaces/{workspace}/items/{bobs}"),
        json!({"expected_version":2,"description":"オーナーの補足"}),
    )
    .await
    .unwrap();

    // A team goal is ordinary editor work.
    call(
        &service,
        &person("us_carol"),
        "PATCH",
        &format!("/v1/workspaces/{workspace}/items/{team}"),
        json!({"expected_version":1,"title":"チームで直した"}),
    )
    .await
    .unwrap();

    // Everyone in the workspace can still *see* it: a goal nobody can see
    // cannot be aligned to anything.
    let map = query(
        &service,
        &person("us_carol"),
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[],
    )
    .await
    .unwrap();
    assert!(map["goals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["id"] == json!(bobs)));
}

#[tokio::test]
async fn the_map_can_be_narrowed_to_one_owner_or_one_period() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;
    let cycle = call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{workspace}/cycles"),
        json!({"cadence":"quarter","start_date":"2026-10-01"}),
    )
    .await
    .unwrap();

    call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{workspace}/items"),
        json!({"kind":"outcome","title":"今期の会社目標",
               "fields":{"owner":{"kind":"organization"},"cycle_id":cycle["id"]}}),
    )
    .await
    .unwrap();
    goal(
        &service,
        &owner,
        &workspace,
        "期間なしのチーム目標",
        json!({"kind":"team","id":"運営"}),
    )
    .await;
    goal(
        &service,
        &owner,
        &workspace,
        "誰のものでもない目標",
        Value::Null,
    )
    .await;

    let company = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[("owner_kind", "organization")],
    )
    .await
    .unwrap();
    assert_eq!(company["goals"].as_array().unwrap().len(), 1);
    assert_eq!(company["goals"][0]["title"], "今期の会社目標");
    assert_eq!(company["goals"][0]["cycle"]["label"], "2026 Q4");

    let team = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[("owner_kind", "team"), ("owner_id", "運営")],
    )
    .await
    .unwrap();
    assert_eq!(team["goals"].as_array().unwrap().len(), 1);
    assert!(team["goals"][0]["cycle"].is_null());

    let in_cycle = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[("cycle_id", cycle["id"].as_str().unwrap())],
    )
    .await
    .unwrap();
    assert_eq!(in_cycle["goals"].as_array().unwrap().len(), 1);

    // A goal nobody owns is counted rather than hidden: it is findable.
    let all = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(all["unowned_goals"], 1);
    assert_eq!(all["goals"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn work_beneath_a_goal_is_counted_but_not_scattered_across_the_map() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;
    let company = goal(
        &service,
        &owner,
        &workspace,
        "会社の目標",
        json!({"kind":"organization"}),
    )
    .await;

    for title in ["取り組みA", "取り組みB"] {
        let initiative = call(
            &service,
            &owner,
            "POST",
            &format!("/v1/workspaces/{workspace}/items"),
            json!({"kind":"initiative","title":title}),
        )
        .await
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        relate(
            &service,
            &owner,
            &workspace,
            &initiative,
            &company,
            "part_of",
        )
        .await
        .unwrap();
    }

    let map = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[],
    )
    .await
    .unwrap();
    // Initiatives are not goals, so the map has one node, not three.
    assert_eq!(map["goals"].as_array().unwrap().len(), 1);
    assert_eq!(map["goals"][0]["descendant_work"], 2);
}

#[tokio::test]
async fn changing_an_owner_or_losing_a_member_keeps_the_history() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;
    let version = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/members"),
        &[],
    )
    .await
    .unwrap()["workspace"]["version"]
        .as_i64()
        .unwrap();
    let invitation = call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{workspace}/invitations"),
        json!({"target_actor":"us_bob","role":"editor","expected_version":version}),
    )
    .await
    .unwrap();
    call(
        &service,
        &person("us_bob"),
        "POST",
        &format!(
            "/v1/invitations/{}/accept",
            invitation["id"].as_str().unwrap()
        ),
        json!({"expected_version":1}),
    )
    .await
    .unwrap();

    let bobs = goal(
        &service,
        &person("us_bob"),
        &workspace,
        "引き継がれる目標",
        json!({"kind":"person","id":"us_bob"}),
    )
    .await;
    call(
        &service,
        &person("us_bob"),
        "POST",
        &format!("/v1/workspaces/{workspace}/records"),
        json!({"record_type":"note","body":"ボブの経緯","item_ids":[bobs.clone()],
               "happened_at":"2026-09-14T00:00:00Z"}),
    )
    .await
    .unwrap();

    // Handing it to the team.
    call(
        &service,
        &person("us_bob"),
        "PATCH",
        &format!("/v1/workspaces/{workspace}/items/{bobs}"),
        json!({"expected_version":1,"fields":{"owner":{"kind":"team","id":"運営"}}}),
    )
    .await
    .unwrap();

    // Bob leaves the workspace entirely.
    let version = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/members"),
        &[],
    )
    .await
    .unwrap()["workspace"]["version"]
        .as_i64()
        .unwrap();
    call(
        &service,
        &owner,
        "DELETE",
        &format!("/v1/workspaces/{workspace}/members/us_bob"),
        json!({"expected_version":version}),
    )
    .await
    .unwrap();

    // The goal is still there, still owned by the team, and what Bob wrote
    // about it is still readable.
    let map = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(map["goals"][0]["owner"]["kind"], "team");
    assert_eq!(map["goals"][0]["owner"]["id"], "運営");
    let records = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/records"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(records["items"][0]["body"], "ボブの経緯");
    assert_eq!(records["items"][0]["author"], "us_bob");
}

#[tokio::test]
async fn okr_builds_on_this_model_rather_than_beside_it() {
    let (_dir, service) = setup().await;
    let (workspace, owner) = shared(&service).await;
    call(
        &service,
        &owner,
        "POST",
        &format!("/v1/workspaces/{workspace}/templates/okr/apply"),
        json!({"title":"OKRの目標"}),
    )
    .await
    .unwrap();

    // The template produces ordinary items, relations and a view — there is no
    // OKR-shaped collection anywhere.
    let snapshot = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/snapshot"),
        &[],
    )
    .await
    .unwrap();
    let collections: Vec<&str> = snapshot
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert!(
        !collections.iter().any(|name| name.contains("okr")),
        "{collections:?}"
    );
    assert!(!snapshot["items"].as_array().unwrap().is_empty());

    // And the goal it created shows up on the alignment map like any other.
    let goals = call(
        &service,
        &owner,
        "PATCH",
        &format!(
            "/v1/workspaces/{workspace}/items/{}",
            snapshot["items"][0]["id"].as_str().unwrap()
        ),
        json!({"expected_version":1,"fields":{"owner":{"kind":"organization"}}}),
    )
    .await;
    assert!(goals.is_ok(), "{goals:?}");
    let map = query(
        &service,
        &owner,
        &format!("/v1/workspaces/{workspace}/alignment"),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(map["goals"][0]["owner"]["kind"], "organization");
}
