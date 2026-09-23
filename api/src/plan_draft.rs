//! The structured reading of one strategy conversation, kept next to — never
//! inside — the confirmed plan.
//!
//! # Why this is not a changeset
//!
//! A changeset is a diff against the confirmed plan: it has a `base_version`,
//! it expires, and approving it moves the plan. A plan draft is earlier than
//! that. It is what the conversation *sounded like*, structured — goals,
//! criteria, initiatives, milestones, actions, constraints and open questions
//! — before anyone has decided which of it becomes plan. Saving one writes no
//! item, relation, metric or record, and nothing here is an apply path.
//!
//! # What the server can and cannot know
//!
//! Basepath runs no model, so the structure is written by the AI host. The
//! contract therefore checks the claims a host could make falsely rather than
//! trying to check the structure itself:
//!
//! * **`status` is the conversation's stance, not the plan's.** `decided`
//!   means the person said so in the conversation — a draft holding it changes
//!   nothing in the plan. From an agent connection, `decided` needs a `basis`
//!   attributed to the person: an AI may report a decision, it may not be its
//!   source. `suggested` is the mirror — what only the AI proposed cannot be
//!   marked as something the person said.
//! * **`basis` carries where each node came from.** `quote`, `at`, `speaker`
//!   and `reason` are all optional because a host cannot always obtain the
//!   transcript, and an absent field is an honest answer where a fabricated
//!   one would be a lie. They are bounded so a draft stores a citation, not
//!   the conversation.
//! * **Committing detail needs a source from an agent.** A node carrying
//!   `fields` — dates, numbers, an owner, a budget — states something a person
//!   will later be asked to have decided. An agent writes such a node only
//!   with a `basis` that says where it came from.
//!
//! # Revisions
//!
//! The conversation keeps moving, so the draft does too. Every save appends a
//! numbered revision; the head document carries the latest content and its
//! number. `expected_revision` keeps a stale generation from silently
//! becoming the head, and a draft revision is never a plan version — the two
//! counters answer different questions and are never read against each other.
use crate::db::Tx;
use crate::model::{ApiError, Result};
use crate::service::{new_id, now, text, title, Actor};
use crate::storage::{get, list, put};
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// What a draft node can be. The first five are the item kinds the plan
/// already knows — a draft node of one of those kinds is something that could
/// later become an item. The last three are annotations a conversation
/// produces that are not work: a success criterion, a constraint, and a
/// question nobody has answered yet.
const NODE_KINDS: [&str; 8] = [
    "outcome",
    "idea",
    "initiative",
    "milestone",
    "action",
    "criterion",
    "constraint",
    "question",
];

/// The conversational stance of a node — what the discussion said about it,
/// not what the plan says.
///
/// * `decided` — the person stated a decision in the conversation.
/// * `considering` — raised and discussed, deliberately not decided.
/// * `hypothesis` — put forward as something to verify.
/// * `suggested` — the AI's own proposal; nobody asked for it by saying it.
/// * `question` — an open question the conversation left unanswered.
const STATUSES: [&str; 5] = [
    "decided",
    "considering",
    "hypothesis",
    "suggested",
    "question",
];

/// Kinds that may hold children through `part_of`. An action is the
/// executable leaf of a plan, and the annotation kinds annotate — none of
/// them contains anything.
const CONTAINER_KINDS: [&str; 4] = ["outcome", "idea", "initiative", "milestone"];

/// Kinds that correspond to plan items, for `contributes_to` / `depends_on`.
/// An annotation neither contributes to a goal nor blocks one.
const ITEM_KINDS: [&str; 5] = ["outcome", "idea", "initiative", "milestone", "action"];

const EDGE_TYPES: [&str; 4] = ["part_of", "contributes_to", "depends_on", "relates_to"];

/// The committing attributes a draft node may carry. Every one of them will
/// be read later as something somebody decided, which is why an agent needs a
/// `basis` to write any of them at all.
const FIELD_KEYS: [&str; 12] = [
    "due_date",
    "start_date",
    "scheduled_date",
    "scheduled_time",
    "assignee_id",
    "self_assessment",
    "target",
    "baseline",
    "unit",
    "estimate_minutes",
    "budget",
    "period",
];

/// Who a `basis` attributes the content to. `person` is what the person said;
/// `assistant` is what the AI proposed; `inference` is what the AI derived
/// from other statements rather than anything anyone said.
const ORIGINS: [&str; 3] = ["person", "assistant", "inference"];

const MAX_NODES: usize = 100;
const MAX_EDGES: usize = 200;
const MAX_REF_CHARS: usize = 64;

fn bounded(v: &Value, key: &str, max: usize) -> Result<()> {
    if !v[key].is_null() && v[key].as_str().is_none_or(|s| s.chars().count() > max) {
        return Err(ApiError::invalid(&format!(
            "{key}は{max}文字以内の文字列で指定してください"
        )));
    }
    Ok(())
}

/// The provenance attached to a node or an edge: who this came from, the
/// words it came from when they are available, when it was said, and why it
/// produced this part of the structure.
///
/// Every field is optional because a host cannot always reach the transcript
/// — and a missing `quote` or `at` is the honest answer, where an invented
/// one would become a fabricated source the person cannot check.
fn validate_basis(basis: &Value, what: &str) -> Result<()> {
    if basis.is_null() {
        return Ok(());
    }
    let object = basis
        .as_object()
        .ok_or_else(|| ApiError::invalid("basisはオブジェクトで指定してください"))?;
    for key in object.keys() {
        if ![
            "origin",
            "source_ref",
            "source_url",
            "speaker",
            "quote",
            "at",
            "reason",
            "assumptions",
        ]
        .contains(&key.as_str())
        {
            return Err(ApiError::invalid(&format!(
                "basisに使えないフィールドです: {key}"
            )));
        }
    }
    if !basis["origin"].is_null() {
        let origin = basis["origin"]
            .as_str()
            .ok_or_else(|| ApiError::invalid("basis.originは文字列で指定してください"))?;
        if !ORIGINS.contains(&origin) {
            return Err(ApiError::invalid(
                "basis.originは person / assistant / inference のいずれかです",
            ));
        }
    }
    bounded(basis, "source_ref", 191)?;
    bounded(basis, "source_url", 2048)?;
    bounded(basis, "speaker", 100)?;
    // A quotation is a citation, not the transcript. Bounded so the draft
    // keeps the span that matters rather than becoming a second copy of the
    // conversation.
    bounded(basis, "quote", 500)?;
    bounded(basis, "reason", 500)?;
    if let Some(source_url) = basis["source_url"].as_str() {
        let parsed = url::Url::parse(source_url)
            .map_err(|_| ApiError::invalid("basis.source_urlはhttp(s) URLで指定してください"))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(ApiError::invalid(
                "basis.source_urlは認証情報を含まないhttp(s) URLで指定してください",
            ));
        }
    }
    if !basis["at"].is_null() {
        let at = basis["at"]
            .as_str()
            .ok_or_else(|| ApiError::invalid("basis.atは日付文字列で指定してください"))?;
        let ok = NaiveDate::parse_from_str(at, "%Y-%m-%d").is_ok()
            || chrono::DateTime::parse_from_rfc3339(at).is_ok();
        if !ok {
            return Err(ApiError::invalid(
                "basis.atはYYYY-MM-DDかRFC3339で指定してください。分からない日時は省略します",
            ));
        }
    }
    match &basis["assumptions"] {
        Value::Array(assumptions)
            if assumptions.len() <= 10
                && assumptions.iter().all(|a| {
                    a.as_str()
                        .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 300)
                }) => {}
        Value::Null => {}
        _ => {
            return Err(ApiError::invalid(&format!(
                "{what}のbasis.assumptionsは10件・各300文字以内の文字列配列にしてください"
            )));
        }
    }
    Ok(())
}

/// Whether the basis actually attributes the content to someone or
/// something. An empty object is not a basis — it is the shape of one.
fn basis_has_content(basis: &Value) -> bool {
    ["source_ref", "source_url", "quote", "reason", "speaker"]
        .iter()
        .any(|key| basis[key].as_str().is_some_and(|s| !s.trim().is_empty()))
        || basis["assumptions"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
}

/// The committing fields a node may carry, checked the same way for every
/// agent: writing any of them is claiming a value, so it needs a basis.
fn validate_fields(fields: &Value, what: &str) -> Result<()> {
    if fields.is_null() {
        return Ok(());
    }
    let fields = fields
        .as_object()
        .ok_or_else(|| ApiError::invalid("fieldsはオブジェクトで指定してください"))?;
    for (key, value) in fields {
        if !FIELD_KEYS.contains(&key.as_str()) {
            return Err(ApiError::invalid(&format!(
                "{what}.fieldsに使えないフィールドです: {key}"
            )));
        }
        match key.as_str() {
            "due_date" | "start_date" | "scheduled_date" => {
                if !value.is_null()
                    && value
                        .as_str()
                        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                        .is_none()
                {
                    return Err(ApiError::invalid(&format!(
                        "{what}.fields.{key}はYYYY-MM-DDで指定してください"
                    )));
                }
            }
            "scheduled_time" => {
                if !value.is_null()
                    && value
                        .as_str()
                        .and_then(|s| chrono::NaiveTime::parse_from_str(s, "%H:%M").ok())
                        .is_none()
                {
                    return Err(ApiError::invalid(&format!(
                        "{what}.fields.scheduled_timeはHH:MMで指定してください"
                    )));
                }
            }
            "self_assessment" | "target" | "baseline" | "estimate_minutes" | "budget" => {
                if !value.is_null() && !value.is_number() {
                    return Err(ApiError::invalid(&format!(
                        "{what}.fields.{key}は数値で指定してください"
                    )));
                }
            }
            _ => {
                if !value.is_null() && value.as_str().is_none_or(|s| s.chars().count() > 200) {
                    return Err(ApiError::invalid(&format!(
                        "{what}.fields.{key}は200文字以内の文字列で指定してください"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Checks every node and returns ref → kind for the edge checks. Reused
/// refs are refused because a ref is how a later revision, a review note or
/// a generated operation names the same node — it must mean one thing.
fn validate_nodes(nodes: &Value, agent: bool) -> Result<HashMap<String, String>> {
    let nodes = nodes
        .as_array()
        .ok_or_else(|| ApiError::invalid("nodesを配列で指定してください"))?;
    if nodes.is_empty() || nodes.len() > MAX_NODES {
        return Err(ApiError::invalid(&format!(
            "nodesは1〜{MAX_NODES}件にしてください"
        )));
    }
    let mut kinds = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        let what = format!("nodes[{index}]");
        crate::service::only(
            node,
            &[
                "ref", "kind", "title", "detail", "status", "fields", "basis",
            ],
        )?;
        let raw_reference = node["ref"].as_str().unwrap_or("");
        let reference = raw_reference.trim();
        if reference.is_empty() || reference.chars().count() > MAX_REF_CHARS {
            return Err(ApiError::invalid(&format!(
                "{what}.refは1〜{MAX_REF_CHARS}文字の識別子が必要です"
            )));
        }
        if raw_reference != reference {
            return Err(ApiError::invalid(&format!(
                "{what}.refの前後に空白を含めないでください"
            )));
        }
        if kinds.contains_key(reference) {
            return Err(ApiError::invalid(&format!(
                "{what}.refが重複しています: {reference}"
            )));
        }
        let kind = node["kind"].as_str().unwrap_or("");
        if !NODE_KINDS.contains(&kind) {
            return Err(ApiError::invalid(&format!(
                "{what}.kindは{}のいずれかです",
                NODE_KINDS.join(" / ")
            )));
        }
        kinds.insert(reference.to_owned(), kind.to_owned());
        let status = node["status"].as_str().unwrap_or("");
        if !STATUSES.contains(&status) {
            return Err(ApiError::invalid(&format!(
                "{what}.statusは{}のいずれかです",
                STATUSES.join(" / ")
            )));
        }
        title(node, "title", 200)
            .map_err(|e| ApiError::invalid(&format!("{what}.title: {}", e.message)))?;
        bounded(node, "detail", 10000)?;
        validate_fields(&node["fields"], &what)?;
        let basis = &node["basis"];
        validate_basis(basis, &what)?;
        if agent {
            let origin = basis["origin"].as_str().unwrap_or("");
            // An AI may report that the person decided something; it may not
            // be the source of the decision. `decided` without a person behind
            // it is exactly the confusion this contract exists to prevent.
            if status == "decided" && origin != "person" {
                return Err(ApiError::invalid(&format!(
                    "{what}をdecidedにするにはbasis.origin=personが必要です。AIの提案はsuggested、検討中はconsideringで記録してください"
                )));
            }
            // The mirror image: what only the AI proposed is not something
            // the person said.
            if status == "suggested" && origin != "assistant" {
                return Err(ApiError::invalid(&format!(
                    "{what}をsuggestedにするにはbasis.origin=assistantが必要です"
                )));
            }
            // A date, a number, an owner, a budget: carried on the node, read
            // later as a commitment. An agent writes one only with where it
            // came from — otherwise it is a value the person will be asked
            // about, and the honest shape of that is a question.
            if node["fields"].as_object().is_some_and(|f| !f.is_empty())
                && !basis_has_content(basis)
            {
                return Err(ApiError::invalid(&format!(
                    "{what}に期限・数値・担当・予算などのfieldsを含めるにはbasisが必要です。書けない値は外し、questionとして本人に尋ねてください"
                )));
            }
        }
    }
    Ok(kinds)
}

fn validate_edges(edges: &Value, kinds: &HashMap<String, String>) -> Result<()> {
    let edges = match edges.as_array() {
        Some(edges) => edges,
        None if edges.is_null() => return Ok(()),
        None => return Err(ApiError::invalid("edgesを配列で指定してください")),
    };
    if edges.len() > MAX_EDGES {
        return Err(ApiError::invalid(&format!(
            "edgesは{MAX_EDGES}件以内にしてください"
        )));
    }
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut relates: HashSet<(String, String)> = HashSet::new();
    // part_of gives each node at most one parent: a draft that cannot become
    // a tree is not a structure proposal.
    let mut parent_of: HashMap<&str, &str> = HashMap::new();
    let mut directed: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for (index, edge) in edges.iter().enumerate() {
        let what = format!("edges[{index}]");
        crate::service::only(
            edge,
            &["source", "target", "type", "rationale", "basis", "position"],
        )?;
        let source = edge["source"].as_str().unwrap_or("");
        let target = edge["target"].as_str().unwrap_or("");
        let edge_type = edge["type"].as_str().unwrap_or("");
        if !EDGE_TYPES.contains(&edge_type) {
            return Err(ApiError::invalid(&format!(
                "{what}.typeは{}のいずれかです",
                EDGE_TYPES.join(" / ")
            )));
        }
        for (key, reference) in [("source", source), ("target", target)] {
            if !kinds.contains_key(reference) {
                return Err(ApiError::invalid(&format!(
                    "{what}.{key}がnodesに存在しません: {reference}"
                )));
            }
        }
        if source == target {
            return Err(ApiError::invalid(&format!("{what}は自分自身を指せません")));
        }
        if !seen.insert((source.into(), target.into(), edge_type.into())) {
            return Err(ApiError::invalid(&format!("{what}は重複しています")));
        }
        bounded(edge, "rationale", 500)?;
        validate_basis(&edge["basis"], &what)?;
        if !edge["position"].is_null()
            && !edge["position"]
                .as_i64()
                .is_some_and(|position| position >= 0)
        {
            return Err(ApiError::invalid(&format!(
                "{what}.positionは0以上の整数で指定してください"
            )));
        }
        match edge_type {
            "part_of" => {
                if parent_of.insert(source, target).is_some() {
                    return Err(ApiError::invalid(&format!(
                        "{what}: part_ofの親は1つまでです（{source}）"
                    )));
                }
                if !CONTAINER_KINDS.contains(&kinds[target].as_str()) {
                    return Err(ApiError::invalid(&format!(
                        "{what}: {}は他の項目の親になれません（{target}）",
                        kinds[target]
                    )));
                }
            }
            "contributes_to" | "depends_on" => {
                for reference in [source, target] {
                    if !ITEM_KINDS.contains(&kinds[reference].as_str()) {
                        return Err(ApiError::invalid(&format!(
                            "{what}: {edge_type}は計画項目どうしの関係です（{reference}）"
                        )));
                    }
                }
            }
            // relates_to is undirected: A↔B once, whichever way it is written.
            "relates_to" => {
                if !relates.insert((source.into(), target.into()))
                    || relates.contains(&(target.into(), source.into()))
                {
                    return Err(ApiError::invalid(&format!("{what}は重複しています")));
                }
                continue;
            }
            _ => unreachable!(),
        }
        directed
            .entry(edge_type)
            .or_default()
            .push((source, target));
    }
    // Cycles are refused per directed type, the same rule the confirmed plan
    // uses: a draft asking a goal to contain itself, or work that depends on
    // itself, has no readable meaning.
    for (edge_type, pairs) in &directed {
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
        for (source, target) in pairs {
            graph.entry(source).or_default().push(target);
        }
        let mut state: HashMap<&str, u8> = HashMap::new();
        for start in graph.keys() {
            // Iterative DFS: 0 = unvisited, 1 = on the path, 2 = done.
            let mut stack = vec![(*start, false)];
            while let Some((node, expanded)) = stack.pop() {
                if expanded {
                    state.insert(node, 2);
                    continue;
                }
                match state.get(node).copied().unwrap_or(0) {
                    1 => {
                        return Err(ApiError::invalid(&format!(
                            "{edge_type}に循環があります（{node}）"
                        )));
                    }
                    2 => continue,
                    _ => {
                        state.insert(node, 1);
                        stack.push((node, true));
                        for next in graph.get(node).into_iter().flatten() {
                            match state.get(next).copied().unwrap_or(0) {
                                0 => stack.push((*next, false)),
                                1 => {
                                    return Err(ApiError::invalid(&format!(
                                        "{edge_type}に循環があります（{next}）"
                                    )));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// One full read of the conversation: nodes, edges and assumptions, checked
/// as a unit. The rules that keep an AI's claims honest apply to agent
/// submissions; a person sketching their own plan needs no citation for their
/// own dates.
fn read_draft(body: &Value, agent: bool) -> Result<(Vec<Value>, Vec<Value>, Vec<String>)> {
    let kinds = validate_nodes(&body["nodes"], agent)?;
    validate_edges(&body["edges"], &kinds)?;
    let assumptions: Vec<String> = match &body["assumptions"] {
        Value::Null => vec![],
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && line.chars().count() <= 1000)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        ApiError::invalid(
                            "assumptionsは空でない1000文字以内の文字列で指定してください",
                        )
                    })
            })
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(ApiError::invalid(
                "assumptionsは文字列配列で指定してください",
            ))
        }
    };
    if assumptions.len() > 20 {
        return Err(ApiError::invalid("assumptionsは20件以内にしてください"));
    }
    Ok((
        body["nodes"].as_array().unwrap().clone(),
        if body["edges"].is_array() {
            body["edges"].as_array().unwrap().clone()
        } else {
            vec![]
        },
        assumptions,
    ))
}

/// The conversation a draft belongs to, verified rather than believed.
///
/// Naming a conversation linked to a *different* workspace is a conflict:
/// two businesses that happen to share words are not one plan, and the link
/// table is where that boundary is recorded. An unlinked conversation id is
/// stored as given — the host cannot always link — but it never widens what
/// the draft can reach.
async fn resolve_link(
    tx: &mut Tx,
    actor: &Actor,
    w: &str,
    conversation_id: Option<&str>,
) -> Result<Option<String>> {
    let Some(conversation_id) = conversation_id else {
        return Ok(None);
    };
    if conversation_id.is_empty()
        || conversation_id.chars().count() > crate::conversation::MAX_CONVERSATION_ID_LENGTH
    {
        return Err(ApiError::invalid(
            "conversation_idは1〜191文字で指定してください",
        ));
    }
    let connection = actor.connection.as_deref().unwrap_or("");
    let Some(link) = crate::conversation::get(tx, actor, connection, conversation_id).await? else {
        return Ok(None);
    };
    if link.workspace_id != w {
        return Err(ApiError::new(
            409,
            "CONTEXT_LINK_CONFLICT",
            "この会話は別のワークスペースにリンク済みです",
        ));
    }
    Ok(Some(link.id))
}

/// A proposal-only connection may continue or withdraw only the draft it
/// created. Workspace-level proposal access does not grant another MCP
/// client access to conversation-derived citations in a different draft.
fn ensure_agent_owns_draft(actor: &Actor, draft: &Value) -> Result<()> {
    if actor.agent
        && (draft["proposed_by"] != actor.id
            || draft["proposed_by_connection"].as_str() != actor.connection.as_deref())
    {
        return Err(ApiError::missing());
    }
    Ok(())
}

/// Saves the first revision of a new draft.
pub async fn create(tx: &mut Tx, actor: &Actor, w: &str, body: &Value) -> Result<Value> {
    crate::service::only(
        body,
        &["title", "conversation_id", "nodes", "edges", "assumptions"],
    )?;
    bounded(body, "title", 200)?;
    bounded(
        body,
        "conversation_id",
        crate::conversation::MAX_CONVERSATION_ID_LENGTH,
    )?;
    let (nodes, edges, assumptions) = read_draft(body, actor.agent)?;
    let link_id = resolve_link(tx, actor, w, body["conversation_id"].as_str()).await?;
    let stamp = now();
    let draft_title = if text(body, "title").trim().is_empty() {
        "会話の構造案".to_owned()
    } else {
        title(body, "title", 200)?
    };
    let draft = json!({
        "id": new_id("pdraft"),
        "workspace_id": w,
        "title": draft_title,
        "status": "open",
        "revision": 1,
        "conversation_id": body["conversation_id"].as_str(),
        "link_id": link_id,
        "proposed_by": actor.id,
        "proposed_by_connection": actor.connection,
        "nodes": nodes,
        "edges": edges,
        "assumptions": assumptions,
        "created_at": stamp,
        "updated_at": stamp,
    });
    save_revision(tx, w, &draft, actor).await?;
    let id = text(&draft, "id").to_owned();
    put(tx, w, "plan_drafts", &id, &draft).await?;
    Ok(draft)
}

/// Appends the next revision of an open draft — the conversation moved, and
/// the draft moved with it. `expected_revision` is required so a generation
/// that started from an older read cannot silently become the head.
pub async fn revise(tx: &mut Tx, actor: &Actor, w: &str, id: &str, body: &Value) -> Result<Value> {
    crate::service::only(
        body,
        &[
            "expected_revision",
            "title",
            "conversation_id",
            "nodes",
            "edges",
            "assumptions",
        ],
    )?;
    bounded(body, "title", 200)?;
    bounded(
        body,
        "conversation_id",
        crate::conversation::MAX_CONVERSATION_ID_LENGTH,
    )?;
    let mut draft: Value = get(tx, w, "plan_drafts", id).await?;
    ensure_agent_owns_draft(actor, &draft)?;
    if draft["status"] != "open" {
        return Err(ApiError::new(
            409,
            "VERSION_CONFLICT",
            "この構造案は取り下げ済みです",
        ));
    }
    let expected = body["expected_revision"]
        .as_i64()
        .ok_or_else(|| ApiError::new(428, "VERSION_REQUIRED", "expected_revisionが必要です"))?;
    if expected != draft["revision"].as_i64().unwrap_or(0) {
        return Err(ApiError::new(
            409,
            "VERSION_CONFLICT",
            "構造案が更新されています。最新の内容を取得してから再送してください",
        ));
    }
    let (nodes, edges, assumptions) = read_draft(body, actor.agent)?;
    // The anchor can be restated on each save — the same conversation, or a
    // later chat continuing the same business. The cross-workspace check is
    // identical either way.
    if let Some(conversation_id) = body["conversation_id"].as_str() {
        draft["link_id"] = json!(resolve_link(tx, actor, w, Some(conversation_id)).await?);
        draft["conversation_id"] = json!(conversation_id);
    }
    if !text(body, "title").is_empty() {
        draft["title"] = json!(title(body, "title", 200)?);
    }
    draft["revision"] = json!(draft["revision"].as_i64().unwrap_or(0) + 1);
    draft["nodes"] = json!(nodes);
    draft["edges"] = json!(edges);
    draft["assumptions"] = json!(assumptions);
    draft["updated_at"] = json!(now());
    save_revision(tx, w, &draft, actor).await?;
    put(tx, w, "plan_drafts", id, &draft).await?;
    Ok(draft)
}

/// Discards a draft. Revisions stay readable — what the conversation looked
/// like is still the answer to "why did we almost do this".
pub async fn withdraw(tx: &mut Tx, actor: &Actor, w: &str, id: &str) -> Result<Value> {
    let mut draft: Value = get(tx, w, "plan_drafts", id).await?;
    ensure_agent_owns_draft(actor, &draft)?;
    if draft["status"] != "open" {
        return Err(ApiError::new(
            409,
            "VERSION_CONFLICT",
            "この構造案はすでに取り下げられています",
        ));
    }
    draft["status"] = json!("withdrawn");
    draft["withdrawn_by"] = json!(actor.id);
    draft["withdrawn_at"] = json!(now());
    put(tx, w, "plan_drafts", id, &draft).await?;
    Ok(draft)
}

/// The append-only record of what the draft said, one document per save.
async fn save_revision(tx: &mut Tx, w: &str, draft: &Value, actor: &Actor) -> Result<()> {
    let revision = draft["revision"].as_i64().unwrap_or(1);
    let id = format!("{}:{revision:06}", text(draft, "id"));
    let record = json!({
        "id": id,
        "draft_id": draft["id"],
        "workspace_id": w,
        "revision": revision,
        "title": draft["title"],
        "conversation_id": draft["conversation_id"],
        "nodes": draft["nodes"],
        "edges": draft["edges"],
        "assumptions": draft["assumptions"],
        "saved_by": actor.id,
        "saved_by_connection": actor.connection,
        "created_at": now(),
    });
    put(tx, w, "plan_draft_revisions", &id, &record).await
}

/// A list row is for choosing which draft to open: it carries the shape of
/// the structure — how many nodes, how many of them still open questions —
/// without carrying the structure itself.
fn summarize(draft: &Value) -> Value {
    let nodes = draft["nodes"].as_array().map(Vec::len).unwrap_or(0);
    let open_questions = draft["nodes"]
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .filter(|node| node["status"] == "question" || node["kind"] == "question")
                .count()
        })
        .unwrap_or(0);
    json!({
        "id": draft["id"],
        "workspace_id": draft["workspace_id"],
        "title": draft["title"],
        "status": draft["status"],
        "revision": draft["revision"],
        "conversation_id": draft["conversation_id"],
        "link_id": draft["link_id"],
        "proposed_by": draft["proposed_by"],
        "proposed_by_connection": draft["proposed_by_connection"],
        "node_count": nodes,
        "edge_count": draft["edges"].as_array().map(Vec::len).unwrap_or(0),
        "open_questions": open_questions,
        "created_at": draft["created_at"],
        "updated_at": draft["updated_at"],
    })
}

pub async fn list_drafts(tx: &mut Tx, w: &str, query: &HashMap<String, String>) -> Result<Value> {
    let mut drafts: Vec<Value> = list(tx, w, "plan_drafts").await?;
    drafts.retain(|draft| {
        query
            .get("status")
            .is_none_or(|status| draft["status"] == *status)
            && query
                .get("conversation_id")
                .is_none_or(|id| draft["conversation_id"] == *id)
    });
    Ok(json!({"items": drafts.iter().map(summarize).collect::<Vec<_>>()}))
}

pub async fn get_draft(tx: &mut Tx, w: &str, id: &str) -> Result<Value> {
    get(tx, w, "plan_drafts", id).await
}

/// One saved revision, or the list of them. Revision documents are the
/// history of what the structure looked like as the conversation moved — the
/// head's `revision` number names the latest.
pub async fn revisions(tx: &mut Tx, w: &str, id: &str, revision: Option<&str>) -> Result<Value> {
    let _: Value = get(tx, w, "plan_drafts", id).await?;
    if let Some(revision) = revision {
        let n: i64 = revision
            .parse()
            .map_err(|_| ApiError::invalid("revisions番号を確認してください"))?;
        let doc: Value = get(tx, w, "plan_draft_revisions", &format!("{id}:{n:06}")).await?;
        return Ok(doc);
    }
    let mut all: Vec<Value> = list(tx, w, "plan_draft_revisions").await?;
    all.retain(|doc| doc["draft_id"] == *id);
    all.sort_by_key(|doc| doc["revision"].as_i64().unwrap_or(0));
    Ok(json!({
        "items": all
            .iter()
            .map(|doc| json!({
                "id": doc["id"],
                "draft_id": doc["draft_id"],
                "revision": doc["revision"],
                "title": doc["title"],
                "conversation_id": doc["conversation_id"],
                "node_count": doc["nodes"].as_array().map(Vec::len).unwrap_or(0),
                "edge_count": doc["edges"].as_array().map(Vec::len).unwrap_or(0),
                "saved_by": doc["saved_by"],
                "saved_by_connection": doc["saved_by_connection"],
                "created_at": doc["created_at"],
            }))
            .collect::<Vec<_>>()
    }))
}
