/// What an AI needs before it proposes a breakdown, and what it must not
/// decide on its own.
///
/// Basepath runs no model. The AI is the host — ChatGPT, Claude, whatever
/// comes next — so "copilot" here is a **contract**, not an engine: what the
/// server hands over before a proposal, what it refuses to accept in one, and
/// what the person sees when they decide.
///
/// # The questions are the product
///
/// A goal with no metric and no deadline can be broken down into something
/// that looks complete and means nothing. The brief names those holes as
/// *questions to ask the person* rather than gaps for a model to fill, because
/// a plausible answer to "when is this due" is worse than no answer: it will
/// be read later as something the person decided.
use crate::db::Tx;
use crate::model::*;
use crate::params;
use crate::service::Actor;
use crate::storage::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Values an AI may not put in a proposal without saying where they came from.
///
/// Each of these is read later as a commitment the person made. A date nobody
/// set, a number nobody measured and an owner nobody asked all become facts
/// the moment they are approved as part of a larger change nobody reread.
pub const GUARDED: [&str; 7] = [
    "due_date",
    "start_date",
    "scheduled_date",
    "assignee_id",
    "self_assessment",
    "target",
    "baseline",
];

/// Which guarded keys an operation body mentions at all — set *or* cleared.
///
/// Separate from [`guarded_values`], and the difference is the question each
/// one answers. `guarded_values` asks "does this state a commitment that needs
/// a source?"; clearing a date states nothing, so it needs no basis. This asks
/// "does this touch a commitment?", which clearing very much does — a deadline
/// somebody removes is as consequential as one they set. A range granted for
/// adding work must not quietly carry either.
pub fn guarded_keys(body: &Value) -> Vec<&'static str> {
    GUARDED
        .into_iter()
        .filter(|key| {
            body.get(key).is_some()
                || body
                    .get("fields")
                    .and_then(|fields| fields.get(key))
                    .is_some()
        })
        .collect()
}

/// Which of the guarded values this operation body would set.
pub fn guarded_values(body: &Value) -> Vec<&'static str> {
    let mut found = Vec::new();
    for key in GUARDED {
        let direct = body.get(key);
        let nested = body.get("fields").and_then(|fields| fields.get(key));
        if [direct, nested]
            .into_iter()
            .flatten()
            .any(|value| !value.is_null())
        {
            found.push(key);
        }
    }
    found
}

fn bigrams(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    chars
        .windows(2)
        .map(|pair| pair.iter().collect::<String>())
        .collect()
}

/// How alike two titles are, 0–100. Character bigrams, so Japanese matches.
fn overlap(a: &str, b: &str) -> usize {
    let (left, right) = (bigrams(a), bigrams(b));
    if left.is_empty() || right.is_empty() {
        return usize::from(a == b) * 100;
    }
    let shared = left.intersection(&right).count();
    shared * 200 / (left.len() + right.len())
}

/// What to read, and what to ask, before proposing a breakdown of one goal.
pub async fn brief(tx: &mut Tx, actor: &Actor, w: &str, item_id: &str) -> Result<Value> {
    let item: Item = get(tx, w, "items", item_id).await?;
    // Workspaces live in their own table, not in the per-workspace document
    // store, so they are read directly rather than through `get`.
    let sql = format!("SELECT body FROM workspaces WHERE id=?{}", tx.lock_reads());
    let workspace: Workspace = serde_json::from_str(
        &tx.fetch_optional(&sql, &params![w])
            .await?
            .ok_or_else(ApiError::missing)?
            .text(0)?,
    )?;
    let relations: Vec<Relation> = list(tx, w, "relations").await?;
    let items: Vec<Item> = list(tx, w, "items").await?;
    let metrics: Vec<Metric> = list(tx, w, "metrics").await?;
    let by_id: HashMap<&str, &Item> = items.iter().map(|i| (i.id.as_str(), i)).collect();

    let children: Vec<&Item> = relations
        .iter()
        .filter(|r| r.relation_type == "part_of" && r.target_id == item.id)
        .filter_map(|r| by_id.get(r.source_id.as_str()).copied())
        .filter(|child| child.archived_at.is_none())
        .collect();

    let measures: Vec<&Metric> = metrics
        .iter()
        .filter(|metric| metric.item_id == item.id)
        .collect();

    // Things to ask, not things to guess. Each one is phrased as the question
    // rather than the missing field, because the person answers questions.
    let mut questions = Vec::new();
    if item.due_date.is_none() {
        questions.push(json!({
            "about": "due_date",
            "ask": "いつまでに達成したいですか。分解の粒度が変わります",
            "why": "期限を分解して子に配ると、誰も決めていない日付が並びます",
        }));
    }
    if measures.is_empty() {
        questions.push(json!({
            "about": "metric",
            "ask": "達成したと判断できるのは、何がどうなったときですか",
            "why": "測り方が決まっていないと、下の行動が効いたかどうかが分かりません",
        }));
    }
    if item.description.trim().is_empty() && item.title.chars().count() < 12 {
        questions.push(json!({
            "about": "meaning",
            "ask": "この目標で実際に何が変わっていてほしいですか",
            "why": "短い題だけでは、複数のまったく違う分解が同じくらい妥当に見えます",
        }));
    }
    if item.fields.assignee_id.is_none() && workspace.scope != "個人" {
        questions.push(json!({
            "about": "owner",
            "ask": "これは誰の目標ですか",
            "why": "担当が決まっていない目標の下に担当を割り当てることはできません",
        }));
    }

    Ok(json!({
        "item": {
            "id": item.id,
            "kind": item.kind,
            "title": item.title,
            "description": item.description,
            "due_date": item.due_date,
            "state": item.state,
            "cycle_id": item.fields.cycle_id,
            "assignee_id": item.fields.assignee_id,
        },
        // Derived from the workspace, never chosen by the caller. A personal
        // plan and an organization plan are different plans with different
        // rules, and asking the caller which one this is would let it choose.
        "context_kind": if workspace.scope == "個人" { "personal" } else { "organization" },
        "existing_children": children
            .iter()
            .map(|child| json!({"id":child.id,"kind":child.kind,"title":child.title,
                                "state":child.state}))
            .collect::<Vec<_>>(),
        "metrics": measures
            .iter()
            .map(|metric| json!({"id":metric.id,"name":metric.name,"unit":metric.unit,
                                 "target":metric.target,"baseline":metric.baseline}))
            .collect::<Vec<_>>(),
        "questions": questions,
        "guarded_values": GUARDED,
        "guidance": "候補には根拠のある値だけを入れてください。期限・担当・目標値・基準値を含める操作には、どこから来た値かを basis に書きます。書けないものは提案から外し、questions として本人に尋ねてください。",
        "actor": actor.id,
    }))
}

/// A proposed set of children, next to the ones that already exist.
///
/// Re-breaking-down a goal that already has work under it is where this goes
/// wrong most often: the second proposal quietly duplicates the first. The
/// comparison is reported as keep / add / change, and anything existing that
/// the proposal does not mention is a **candidate** for removal — never a
/// removal. Something already underway is not deleted because an AI did not
/// think of it.
pub async fn compare(tx: &mut Tx, w: &str, item_id: &str, proposed: &Value) -> Result<Value> {
    let _: Item = get(tx, w, "items", item_id).await?;
    let relations: Vec<Relation> = list(tx, w, "relations").await?;
    let items: Vec<Item> = list(tx, w, "items").await?;
    let by_id: HashMap<&str, &Item> = items.iter().map(|i| (i.id.as_str(), i)).collect();
    let existing: Vec<&Item> = relations
        .iter()
        .filter(|r| r.relation_type == "part_of" && r.target_id == item_id)
        .filter_map(|r| by_id.get(r.source_id.as_str()).copied())
        .filter(|child| child.archived_at.is_none())
        .collect();

    let candidates = proposed
        .as_array()
        .ok_or_else(|| ApiError::invalid("childrenに候補を配列で指定してください"))?;

    let mut rows = Vec::new();
    let mut matched: HashSet<&str> = HashSet::new();
    for candidate in candidates {
        let title = candidate["title"].as_str().unwrap_or_default().trim();
        if title.is_empty() {
            return Err(ApiError::invalid("候補にはタイトルが必要です"));
        }
        let kind = candidate["kind"].as_str().unwrap_or("initiative");
        let best = existing
            .iter()
            .filter(|child| !matched.contains(child.id.as_str()))
            .map(|child| (overlap(title, &child.title), *child))
            .max_by_key(|(score, _)| *score)
            .filter(|(score, _)| *score >= 50);
        match best {
            Some((score, child)) => {
                matched.insert(&child.id);
                let same = child.title == title && child.kind == kind;
                rows.push(json!({
                    "verdict": if same { "keep" } else { "change" },
                    "existing_id": child.id,
                    "existing_title": child.title,
                    "existing_kind": child.kind,
                    "proposed_title": title,
                    "proposed_kind": kind,
                    "rationale": candidate["rationale"],
                    "similarity": score,
                }));
            }
            None => rows.push(json!({
                "verdict": "add",
                "proposed_title": title,
                "proposed_kind": kind,
                "rationale": candidate["rationale"],
            })),
        }
    }
    for child in &existing {
        if matched.contains(child.id.as_str()) {
            continue;
        }
        rows.push(json!({
            "verdict": "remove_candidate",
            "existing_id": child.id,
            "existing_title": child.title,
            "existing_kind": child.kind,
            // Said plainly: the proposal not mentioning it is not a reason.
            "note": "この候補には含まれていません。残すか外すかは本人が決めます",
        }));
    }
    Ok(json!({
        "item_id": item_id,
        "comparison": rows,
        // Nothing here changed anything. The next step is a change set.
        "applied": false,
        "removed": false,
    }))
}
