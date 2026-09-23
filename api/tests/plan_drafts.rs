//! What a strategy conversation becomes when it is structured, and what the
//! structure is refused for.
//!
//! Basepath runs no model: the AI host does the reading, and this file pins
//! the contract it writes under. The guarantees that matter are mostly
//! negative ones — a draft never enters the confirmed plan, an AI's claim to
//! a decision needs the person behind it, a committing value needs a source,
//! and the shape stays a tree the person can actually review.
use pathbase_api::{
    conversation::{self, LinkInput},
    model::Result,
    service::{Actor, Service, LOCAL_TENANT},
};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn setup() -> (tempfile::TempDir, Service, String) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("drafts.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    let who = person("us_alice");
    let workspace = call(
        &service,
        &who,
        "POST",
        "/v1/workspaces",
        json!({"name":"事業A","scope":"組織"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (dir, service, workspace)
}

fn person(id: &str) -> Actor {
    Actor::person(id, LOCAL_TENANT)
}

fn agent(id: &str) -> Actor {
    Actor {
        id: id.into(),
        tenant: LOCAL_TENANT.into(),
        agent: true,
        connection: Some("conn_test".into()),
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

fn node(r: &str, kind: &str, title: &str, status: &str) -> Value {
    json!({"ref": r, "kind": kind, "title": title, "status": status})
}

fn said(r: &str, kind: &str, title: &str, quote: &str) -> Value {
    json!({
        "ref": r,
        "kind": kind,
        "title": title,
        "status": "decided",
        "basis": {"origin": "person", "speaker": "本人", "quote": quote},
    })
}

async fn draft(service: &Service, who: &Actor, w: &str, body: Value) -> Result<Value> {
    call(
        service,
        who,
        "POST",
        &format!("/v1/workspaces/{w}/plan-drafts"),
        body,
    )
    .await
}

/// The acceptance example: 「海外売上を30%にしたい。まず台湾で検証しよう。
/// 代理店経由で、今月は候補に話を聞こう」 structured as deep as the
/// conversation justifies — and nowhere deeper.
#[tokio::test]
async fn a_conversation_becomes_a_reviewable_structure() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    let draft = draft(
        &service,
        &ai,
        &w,
        json!({
            "title": "海外展開の会話",
            "conversation_id": "chat-abc",
            "nodes": [
                said("g", "outcome", "海外売上を30%にする", "海外売上を30%にしたい"),
                said("m", "initiative", "台湾で市場検証する", "まず台湾で検証しよう"),
                json!({"ref":"s","kind":"initiative","title":"代理店経由の販売方法を検証する","status":"hypothesis","basis":{"origin":"inference","reason":"「代理店経由で」は販売方法の検証課題と解釈した"}}),
                json!({"ref":"a","kind":"action","title":"代理店候補に話を聞く","status":"decided","fields":{"due_date":"2026-10-31"},"basis":{"origin":"person","quote":"今月は候補に話を聞こう","reason":"「今月」を会話時点の月末と解釈"}}),
                node("q", "question", "代理店候補はどこか", "question"),
            ],
            "edges": [
                {"source":"m","target":"g","type":"part_of"},
                {"source":"s","target":"m","type":"part_of"},
                {"source":"a","target":"s","type":"part_of"},
                {"source":"q","target":"g","type":"relates_to"},
            ],
            "assumptions": ["「今月」は会話時点の月と解釈した"],
        }),
    )
    .await
    .unwrap();
    assert_eq!(draft["status"], "open");
    assert_eq!(draft["revision"], 1);
    assert_eq!(draft["conversation_id"], "chat-abc");
    assert_eq!(draft["nodes"].as_array().unwrap().len(), 5);
    assert_eq!(draft["edges"].as_array().unwrap().len(), 4);
    assert_eq!(draft["proposed_by_connection"], "conn_test");

    // Nothing entered the confirmed plan — not an item, not a relation, not
    // a change set. A draft is a reading, not a write.
    for col in ["items", "relations", "records", "changesets"] {
        let listed = query(&service, &ai, &format!("/v1/workspaces/{w}/{col}"))
            .await
            .unwrap();
        assert_eq!(
            listed["items"].as_array().unwrap().len(),
            0,
            "{col} must stay empty"
        );
    }
    let graph = query(&service, &ai, &format!("/v1/workspaces/{w}/graph"))
        .await
        .unwrap();
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 0);
}

/// `decided` is a claim about what the person said, so an agent needs the
/// person behind it — `basis.origin: person`. The AI's own reasoning cannot
/// be the source of a human decision.
#[tokio::test]
async fn an_ai_reports_a_decision_but_cannot_be_one() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");

    // No basis at all: refused.
    let missing = draft(
        &service,
        &ai,
        &w,
        json!({"nodes": [node("g", "outcome", "目標", "decided")]}),
    )
    .await;
    assert_eq!(missing.unwrap_err().code, "VALIDATION_ERROR");

    // The AI as the origin of a decision: refused.
    let own = draft(
        &service,
        &ai,
        &w,
        json!({"nodes": [{"ref":"g","kind":"outcome","title":"目標","status":"decided","basis":{"origin":"assistant","reason":"重要そうなので決定とした"}}]}),
    )
    .await;
    assert_eq!(own.unwrap_err().code, "VALIDATION_ERROR");

    // Reported as the person's words: accepted.
    let reported = draft(
        &service,
        &ai,
        &w,
        json!({"nodes": [said("g", "outcome", "目標", "こう決めた")]}),
    )
    .await;
    assert!(reported.is_ok(), "{:?}", reported.err());
}

/// The mirror rule: what only the AI proposed must not read as something the
/// person said.
#[tokio::test]
async fn an_ai_suggestion_is_not_the_persons_words() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    let wrong_origin = draft(
        &service,
        &ai,
        &w,
        json!({"nodes": [{"ref":"x","kind":"initiative","title":"新規施策","status":"suggested","basis":{"origin":"person","quote":"やりたいと言った"}}]}),
    )
    .await;
    assert_eq!(wrong_origin.unwrap_err().code, "VALIDATION_ERROR");

    let own = draft(
        &service,
        &ai,
        &w,
        json!({"nodes": [{"ref":"x","kind":"initiative","title":"新規施策","status":"suggested","basis":{"origin":"assistant","reason":"目標に対する打ち手として提案"}}]}),
    )
    .await;
    assert!(own.is_ok(), "{:?}", own.err());
}

/// A date, a number, an owner or a budget on a node will be read later as
/// something somebody decided. An agent writes one only with where it came
/// from — otherwise the honest shape is a question, not a value.
#[tokio::test]
async fn committing_values_from_an_ai_need_a_source() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    for fields in [
        json!({"due_date": "2026-12-31"}),
        json!({"target": 30, "unit": "%"}),
        json!({"budget": 3000000}),
    ] {
        let refused = draft(
            &service,
            &ai,
            &w,
            json!({"nodes": [{"ref":"x","kind":"action","title":"行動","status":"considering","fields":fields,"basis":{"origin":"inference"}}]}),
        )
        .await;
        assert_eq!(
            refused.unwrap_err().code,
            "VALIDATION_ERROR",
            "{fields} without a basis must be refused"
        );
    }
    // The same value with the span it came from is the contract working.
    let sourced = draft(
        &service,
        &ai,
        &w,
        json!({"nodes": [{"ref":"x","kind":"action","title":"行動","status":"considering","fields":{"due_date":"2026-12-31"},"basis":{"origin":"person","quote":"年末までにやりたい"}}]}),
    )
    .await;
    assert!(sourced.is_ok(), "{:?}", sourced.err());
}

/// A person sketching their own plan in the app needs no citation for their
/// own words — they are the source.
#[tokio::test]
async fn a_person_writes_their_own_draft_without_citations() {
    let (_dir, service, w) = setup().await;
    let owner = person("us_alice");
    let mine = draft(
        &service,
        &owner,
        &w,
        json!({"nodes": [
            {"ref":"g","kind":"outcome","title":"利益3億","status":"decided","fields":{"target":3,"unit":"億円"}},
            node("a", "action", "候補に連絡する", "considering"),
        ]}),
    )
    .await;
    assert!(mine.is_ok(), "{:?}", mine.err());
}

/// An unknown timestamp or an origin outside the contract is refused: the
/// honest value for a citation the host does not have is an absent field.
#[tokio::test]
async fn evidence_is_bounded_and_never_invented() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    let cases = [
        // A timestamp that is not a timestamp.
        json!({"origin":"person","quote":"言った","at":"昨日あたり"}),
        // An origin the contract does not know.
        json!({"origin":"model","quote":"言った"}),
        // A quotation longer than the span it cites (501 chars).
        json!({"origin":"person","quote":"x".repeat(501)}),
        // An unknown basis key.
        json!({"origin":"person","quote":"言った","confidence":0.9}),
    ];
    for basis in cases {
        let refused = draft(
            &service,
            &ai,
            &w,
            json!({"nodes": [{"ref":"g","kind":"outcome","title":"目標","status":"decided","basis":basis}]}),
        )
        .await;
        assert_eq!(
            refused.unwrap_err().code,
            "VALIDATION_ERROR",
            "{basis} must be refused"
        );
    }
    // The spans a host actually has are accepted.
    let cited = draft(
        &service,
        &ai,
        &w,
        json!({"nodes": [{"ref":"g","kind":"outcome","title":"目標","status":"decided","basis":{"origin":"person","speaker":"本人","quote":"30%にしたい","at":"2026-10-01T09:30:00Z","reason":"発言より","assumptions":["時区はJSTと解釈"]}}]}),
    )
    .await;
    assert!(cited.is_ok(), "{:?}", cited.err());
}

/// The structure is a tree with relations, and the rules the confirmed plan
/// keeps are the ones a draft is checked against.
#[tokio::test]
async fn the_shape_stays_reviewable() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    let two_nodes = json!([
        node("a", "outcome", "甲", "considering"),
        node("b", "outcome", "乙", "considering")
    ]);

    // A goal cannot contain itself, at any distance.
    let cyclic = draft(
        &service,
        &ai,
        &w,
        json!({"nodes": two_nodes, "edges": [
            {"source":"a","target":"b","type":"part_of"},
            {"source":"b","target":"a","type":"part_of"},
        ]}),
    )
    .await;
    assert_eq!(cyclic.unwrap_err().code, "VALIDATION_ERROR");

    // One node, one parent: a draft that cannot become a tree is not a
    // structure proposal.
    let two_parents = draft(&service, &ai, &w, json!({
        "nodes": [node("c","initiative","子","considering"), node("p","outcome","親1","considering"), node("q","outcome","親2","considering")],
        "edges": [
            {"source":"c","target":"p","type":"part_of"},
            {"source":"c","target":"q","type":"part_of"},
        ],
    })).await;
    assert_eq!(two_parents.unwrap_err().code, "VALIDATION_ERROR");

    // An action is the executable leaf — it holds nothing beneath it.
    let action_parent = draft(&service, &ai, &w, json!({
        "nodes": [node("a","action","行動","considering"), node("b","initiative","施策","considering")],
        "edges": [{"source":"b","target":"a","type":"part_of"}],
    })).await;
    assert_eq!(action_parent.unwrap_err().code, "VALIDATION_ERROR");

    // Containment is not contribution and is not dependency: a question can
    // sit under a goal but cannot be depended on.
    let depends_on_question = draft(&service, &ai, &w, json!({
        "nodes": [node("a","action","行動","considering"), node("q","question","未解決の問い","question")],
        "edges": [{"source":"a","target":"q","type":"depends_on"}],
    })).await;
    assert_eq!(depends_on_question.unwrap_err().code, "VALIDATION_ERROR");

    // relates_to is undirected: A↔B once.
    let mirrored = draft(
        &service,
        &ai,
        &w,
        json!({
            "nodes": two_nodes,
            "edges": [
                {"source":"a","target":"b","type":"relates_to"},
                {"source":"b","target":"a","type":"relates_to"},
            ],
        }),
    )
    .await;
    assert_eq!(mirrored.unwrap_err().code, "VALIDATION_ERROR");

    // An edge must name nodes that exist.
    let dangling = draft(
        &service,
        &ai,
        &w,
        json!({
            "nodes": [node("a","outcome","甲","considering")],
            "edges": [{"source":"a","target":"ghost","type":"part_of"}],
        }),
    )
    .await;
    assert_eq!(dangling.unwrap_err().code, "VALIDATION_ERROR");
}

/// The conversation keeps moving and the draft moves with it — under its own
/// counter, not the plan's.
#[tokio::test]
async fn the_draft_revises_without_touching_the_plan_version() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");

    // The confirmed plan's version fingerprint, before anything is drafted.
    let before = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/changesets/preview"),
        json!({"operations":[{"method":"POST","path":format!("/v1/workspaces/{w}/items"),"body":{"kind":"action","title":"指標用"}}]}),
    )
    .await
    .unwrap()["base_version"]
        .as_str()
        .unwrap()
        .to_owned();

    let created = draft(
        &service,
        &ai,
        &w,
        json!({"conversation_id":"chat-x","nodes":[said("g","outcome","目標","言った")]}),
    )
    .await
    .unwrap();
    let id = created["id"].as_str().unwrap().to_owned();

    // A revision without the counter is required-but-missing, not a conflict.
    let unversioned = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/plan-drafts/{id}/revisions"),
        json!({"nodes":[said("g","outcome","目標2","言った")]}),
    )
    .await;
    assert_eq!(unversioned.unwrap_err().code, "VERSION_REQUIRED");

    // A stale generation cannot silently become the head.
    let stale = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/plan-drafts/{id}/revisions"),
        json!({"expected_revision":5,"nodes":[said("g","outcome","目標2","言った")]}),
    )
    .await;
    assert_eq!(stale.unwrap_err().code, "VERSION_CONFLICT");

    // The conversation continued: a negation lands and the draft records it.
    let revised = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/plan-drafts/{id}/revisions"),
        json!({
            "expected_revision": 1,
            "conversation_id": "chat-x",
            "nodes": [
                said("g","outcome","海外売上30%","海外売上を30%にしたい"),
                {"ref":"old","kind":"initiative","title":"直販モデル","status":"question","basis":{"origin":"person","quote":"やはり直販はやめよう","reason":"取り下げられた案"}},
            ],
        }),
    )
    .await
    .unwrap();
    assert_eq!(revised["revision"], 2);
    assert_eq!(revised["nodes"].as_array().unwrap().len(), 2);

    // Every save is still there, so what the structure said before is the
    // answer to how it moved.
    let history = query(
        &service,
        &ai,
        &format!("/v1/workspaces/{w}/plan-drafts/{id}/revisions"),
    )
    .await
    .unwrap();
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    let first = query(
        &service,
        &ai,
        &format!("/v1/workspaces/{w}/plan-drafts/{id}/revisions/1"),
    )
    .await
    .unwrap();
    assert_eq!(first["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(first["revision"], 1);

    // And the plan's version fingerprint is exactly what it was: the draft's
    // counter and the plan's counter answer different questions.
    let after = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/changesets/preview"),
        json!({"operations":[{"method":"POST","path":format!("/v1/workspaces/{w}/items"),"body":{"kind":"action","title":"指標用"}}]}),
    )
    .await
    .unwrap()["base_version"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(before, after);
    // The preview created no plan item and the drafts created none either.
    let items = query(&service, &ai, &format!("/v1/workspaces/{w}/items"))
        .await
        .unwrap();
    assert_eq!(items["items"].as_array().unwrap().len(), 0);
}

/// A linked conversation belongs to one workspace. Naming it from another is
/// a conflict, because two businesses that share words are not one plan.
#[tokio::test]
async fn a_conversation_linked_elsewhere_is_a_conflict() {
    let (_dir, service, w) = setup().await;
    let who = person("us_alice");
    let other_workspace = call(
        &service,
        &who,
        "POST",
        "/v1/workspaces",
        json!({"name":"事業B","scope":"組織"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // The AI connection linked its conversation to 事業A.
    {
        let mut tx = service.db.begin_write().await.unwrap();
        conversation::upsert(
            &mut tx,
            &agent("us_alice"),
            LinkInput {
                connection_id: "conn_test",
                conversation_id: "chat-linked",
                workspace_id: &w,
                item_id: None,
                screen: None,
                idempotency_key: "link-1",
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    // Drafting it into 事業A works and records the link.
    let anchored = draft(
        &service,
        &agent("us_alice"),
        &w,
        json!({"conversation_id":"chat-linked","nodes":[said("g","outcome","目標","言った")]}),
    )
    .await
    .unwrap();
    assert!(anchored["link_id"].is_string());

    // Drafting the same conversation into 事業B is a conflict.
    let conflict = draft(
        &service,
        &agent("us_alice"),
        &other_workspace,
        json!({"conversation_id":"chat-linked","nodes":[said("g","outcome","目標","言った")]}),
    )
    .await;
    assert_eq!(conflict.unwrap_err().code, "CONTEXT_LINK_CONFLICT");
}

/// Withdrawing is the honest end of a draft — the reading is kept, the plan
/// is untouched, and a second withdrawal has nothing left to say.
#[tokio::test]
async fn a_withdrawn_draft_keeps_its_history() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    let created = draft(
        &service,
        &ai,
        &w,
        json!({"nodes":[said("g","outcome","目標","言った")]}),
    )
    .await
    .unwrap();
    let id = created["id"].as_str().unwrap().to_owned();

    let withdrawn = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/plan-drafts/{id}/withdraw"),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(withdrawn["status"], "withdrawn");

    // What was read stays readable.
    let revision = query(
        &service,
        &ai,
        &format!("/v1/workspaces/{w}/plan-drafts/{id}/revisions/1"),
    )
    .await
    .unwrap();
    assert_eq!(revision["draft_id"], json!(id));

    // But it no longer moves.
    for result in [
        call(
            &service,
            &ai,
            "POST",
            &format!("/v1/workspaces/{w}/plan-drafts/{id}/revisions"),
            json!({"expected_revision":1,"nodes":[said("g","outcome","目標","言った")]}),
        )
        .await,
        call(
            &service,
            &ai,
            "POST",
            &format!("/v1/workspaces/{w}/plan-drafts/{id}/withdraw"),
            json!({}),
        )
        .await,
    ] {
        assert_eq!(result.unwrap_err().code, "VERSION_CONFLICT");
    }
}

/// The list is for choosing which draft to open: it carries the shape of
/// each structure without the structure.
#[tokio::test]
async fn the_list_summarizes_without_carrying_the_structure() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    draft(
        &service,
        &ai,
        &w,
        json!({"conversation_id":"chat-1","nodes":[
            said("g","outcome","目標","言った"),
            node("q","question","まだ決まっていないこと","question"),
        ]}),
    )
    .await
    .unwrap();
    draft(
        &service,
        &ai,
        &w,
        json!({"conversation_id":"chat-2","nodes":[said("g","outcome","別の会話の目標","言った")]}),
    )
    .await
    .unwrap();

    let all = query(&service, &ai, &format!("/v1/workspaces/{w}/plan-drafts"))
        .await
        .unwrap();
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
    let summary = &all["items"][0];
    assert!(summary.get("nodes").is_none());
    assert!(summary["node_count"].is_number());
    assert_eq!(summary["open_questions"], 1);

    let filtered = service
        .handle(
            &ai,
            "GET",
            &format!("/v1/workspaces/{w}/plan-drafts"),
            &HashMap::from([("conversation_id".to_owned(), "chat-2".to_owned())]),
            json!({}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(filtered["items"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["items"][0]["conversation_id"], "chat-2");
}

/// Drafts live inside the same walls as everything else: a draft in one
/// workspace is invisible to a person who is not a member of it.
#[tokio::test]
async fn a_draft_stays_inside_its_workspace() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    let created = draft(
        &service,
        &ai,
        &w,
        json!({"nodes":[said("g","outcome","目標","言った")]}),
    )
    .await
    .unwrap();
    let id = created["id"].as_str().unwrap().to_owned();

    // Another person is not in the workspace, so the draft — like the
    // workspace itself — does not exist for them.
    let outsider = person("us_bob");
    let invisible = query(
        &service,
        &outsider,
        &format!("/v1/workspaces/{w}/plan-drafts/{id}"),
    )
    .await;
    assert_eq!(invisible.unwrap_err().status, 404);

    // And the same person's other workspace lists only its own drafts.
    let other = call(
        &service,
        &person("us_alice"),
        "POST",
        "/v1/workspaces",
        json!({"name":"事業B","scope":"組織"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let listed = query(
        &service,
        &ai,
        &format!("/v1/workspaces/{other}/plan-drafts"),
    )
    .await
    .unwrap();
    assert_eq!(listed["items"].as_array().unwrap().len(), 0);
}

/// An agent can draft — drafting is proposing — but it still cannot write the
/// plan itself, and a person approving nothing is asked for nothing.
#[tokio::test]
async fn an_agent_proposes_but_still_cannot_write_the_plan() {
    let (_dir, service, w) = setup().await;
    let ai = agent("us_alice");
    let drafted = draft(
        &service,
        &ai,
        &w,
        json!({"nodes":[said("g","outcome","目標","言った")]}),
    )
    .await;
    assert!(drafted.is_ok(), "{:?}", drafted.err());

    let direct = call(
        &service,
        &ai,
        "POST",
        &format!("/v1/workspaces/{w}/items"),
        json!({"kind":"action","title":"直接書き込み"}),
    )
    .await;
    assert_eq!(direct.unwrap_err().code, "APPROVAL_REQUIRED");
}
