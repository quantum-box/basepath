/// Breaking a large goal down until something is actually doable.
///
/// # Structure and contribution are different questions
///
/// `part_of` says where something sits: one structural parent, so "what is
/// this a piece of" always has one answer. `contributes_to` says what it
/// helps: many, across branches, because real work usually serves more than
/// one aim. Merging them would make the tree unanswerable in exchange for a
/// slightly shorter graph, so they stay apart everywhere in this file.
/// `depends_on` is neither — it is ordering, and it may point sideways.
///
/// # No fixed depth
///
/// There is no level table, no `depth` column, and no enum of tiers. A person
/// with a ten-year goal and a person with a two-week one are both right, and
/// a schema that picks for them is wrong for one of them. Depth is whatever
/// the edges say it is; what this module bounds is how much is *read* at
/// once, which is a different concern.
use crate::db::Tx;
use crate::model::*;
use crate::storage::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Kinds that are meant to be broken down further.
const GOALISH: [&str; 4] = ["outcome", "initiative", "milestone", "idea"];

/// The structural edges of a workspace, indexed both ways.
struct Structure {
    items: HashMap<String, Item>,
    /// parent id → its children's part_of edges, in display order.
    children: HashMap<String, Vec<Relation>>,
    /// child id → its single part_of edge upward.
    parent: HashMap<String, Relation>,
    /// Everything that is not `part_of`, kept separate on purpose.
    other: Vec<Relation>,
}

/// Where a child sits among its siblings.
///
/// Unset sorts last rather than first: a child nobody has placed is new, and
/// new work appearing at the top of someone's plan is a small theft of
/// attention.
fn placement(relation: &Relation, item_order: &HashMap<String, usize>) -> (i64, usize, String) {
    (
        relation.position.unwrap_or(i64::MAX),
        item_order
            .get(&relation.source_id)
            .copied()
            .unwrap_or(usize::MAX),
        relation.id.clone(),
    )
}

async fn structure(tx: &mut Tx, w: &str) -> Result<Structure> {
    let items: Vec<Item> = list(tx, w, "items").await?;
    let item_order: HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.id.clone(), index))
        .collect();
    let relations: Vec<Relation> = list(tx, w, "relations").await?;
    let mut children: HashMap<String, Vec<Relation>> = HashMap::new();
    let mut parent = HashMap::new();
    let mut other = Vec::new();
    for relation in relations {
        if relation.relation_type == "part_of" {
            parent.insert(relation.source_id.clone(), relation.clone());
            children
                .entry(relation.target_id.clone())
                .or_default()
                .push(relation);
        } else {
            other.push(relation);
        }
    }
    for edges in children.values_mut() {
        edges.sort_by_key(|relation| placement(relation, &item_order));
    }
    Ok(Structure {
        items: items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect(),
        children,
        parent,
        other,
    })
}

impl Structure {
    /// Structural children that still exist and are not archived.
    ///
    /// An archived parent keeps its children; they are simply no longer
    /// reached through it. Nothing here deletes anything.
    fn live_children(&self, id: &str) -> Vec<&Relation> {
        self.children
            .get(id)
            .map(|edges| {
                edges
                    .iter()
                    .filter(|edge| {
                        self.items
                            .get(&edge.source_id)
                            .is_some_and(|item| item.archived_at.is_none())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn edges_from(&self, id: &str, kind: &str) -> Vec<&Relation> {
        self.other
            .iter()
            .filter(|edge| edge.source_id == id && edge.relation_type == kind)
            .collect()
    }

    /// Is there an action anywhere below this, however deep?
    fn reaches_an_action(&self, id: &str) -> bool {
        let mut todo = vec![id.to_owned()];
        let mut seen = HashSet::new();
        while let Some(node) = todo.pop() {
            if !seen.insert(node.clone()) {
                continue;
            }
            for edge in self.live_children(&node) {
                if self
                    .items
                    .get(&edge.source_id)
                    .is_some_and(|item| item.kind == "action")
                {
                    return true;
                }
                todo.push(edge.source_id.clone());
            }
        }
        false
    }
}

fn summary(item: &Item) -> Value {
    json!({
        "id": item.id,
        "kind": item.kind,
        "title": item.title,
        "state": item.state,
        "due_date": item.due_date,
        "archived_at": item.archived_at,
        "cycle_id": item.fields.cycle_id,
    })
}

/// The part of the breakdown under one item, to a depth the caller chooses.
///
/// Depth and node budget are both bounded, and the response says where it
/// stopped rather than silently returning less: a node with
/// `has_more_children` is a handle to ask again from, which is what makes a
/// deep map loadable a piece at a time instead of all at once.
pub async fn subtree(
    tx: &mut Tx,
    w: &str,
    root_id: &str,
    query: &HashMap<String, String>,
) -> Result<Value> {
    let depth = query
        .get("depth")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2)
        .clamp(1, 20);
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(200)
        .clamp(1, 500);

    let root: Item = get(tx, w, "items", root_id).await?;
    let s = structure(tx, w).await?;

    let mut nodes = Vec::new();
    let mut truncated = false;
    // Breadth first, so a budget that runs out loses the deepest work rather
    // than a whole branch: what is near the goal is what orients someone.
    let mut queue = vec![(root.id.clone(), 0usize)];
    let mut seen = HashSet::new();
    while let Some((id, level)) = (!queue.is_empty()).then(|| queue.remove(0)) {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(item) = s.items.get(&id) else {
            continue;
        };
        let edges = s.live_children(&id);
        let expand = level < depth && nodes.len() < limit;
        let mut loaded = 0usize;
        if expand {
            for edge in &edges {
                if nodes.len() + queue.len() >= limit {
                    truncated = true;
                    break;
                }
                queue.push((edge.source_id.clone(), level + 1));
                loaded += 1;
            }
        }
        let mut row = summary(item);
        row["depth"] = json!(level);
        // The rationale lives on the edge, because "why is this part of that"
        // is a fact about the link and not about either end of it.
        row["rationale"] = json!(s.parent.get(&id).map(|edge| edge.rationale.clone()));
        row["parent_id"] = json!(s.parent.get(&id).map(|edge| edge.target_id.clone()));
        row["child_count"] = json!(edges.len());
        row["loaded_children"] = json!(loaded);
        // True when there is more under this node than this response carries.
        // A caller asks again with this id as the root.
        row["has_more_children"] = json!(loaded < edges.len());
        row["contributes_to"] = json!(s
            .edges_from(&id, "contributes_to")
            .iter()
            .map(|edge| json!({"id": edge.target_id, "rationale": edge.rationale}))
            .collect::<Vec<_>>());
        row["depends_on"] = json!(s
            .edges_from(&id, "depends_on")
            .iter()
            .map(|edge| json!({"id": edge.target_id, "rationale": edge.rationale}))
            .collect::<Vec<_>>());
        nodes.push(row);
        if nodes.len() >= limit && !queue.is_empty() {
            truncated = true;
            break;
        }
    }

    Ok(json!({
        "root": summary(&root),
        "nodes": nodes,
        "depth": depth,
        "limit": limit,
        // Said plainly, because a map that quietly stops is a map that lies.
        "truncated": truncated,
    }))
}

/// Why this exists: the chain from here up to whatever sits at the top.
///
/// The counterpart of `subtree`. An action nobody can explain is an action
/// nobody should be doing, and the explanation is the rationale recorded on
/// each link, not a sentence generated now.
pub async fn ancestry(tx: &mut Tx, w: &str, id: &str) -> Result<Value> {
    let item: Item = get(tx, w, "items", id).await?;
    let s = structure(tx, w).await?;

    let mut chain = Vec::new();
    let mut cursor = item.id.clone();
    let mut seen = HashSet::new();
    // The write path rejects cycles, but a read that trusts that and loops
    // forever is a worse failure than one that stops.
    while let Some(edge) = s.parent.get(&cursor) {
        if !seen.insert(cursor.clone()) {
            break;
        }
        let Some(parent) = s.items.get(&edge.target_id) else {
            break;
        };
        let mut step = summary(parent);
        // The rationale belongs to the link, and a link has two ends. Naming
        // the lower one makes each entry read as a sentence — "`child_id` is
        // part of this, because `rationale`" — instead of leaving the reader
        // to guess which step the reason explains.
        step["child_id"] = json!(cursor);
        step["rationale"] = json!(edge.rationale);
        step["relation_id"] = json!(edge.id);
        chain.push(step);
        cursor = parent.id.clone();
    }
    // Top first: the chain reads as an explanation rather than a traversal.
    chain.reverse();

    Ok(json!({
        "item": summary(&item),
        "ancestors": chain,
        "contributes_to": s
            .edges_from(id, "contributes_to")
            .iter()
            .map(|edge| json!({
                "id": edge.target_id,
                "title": s.items.get(&edge.target_id).map(|item| item.title.clone()),
                "rationale": edge.rationale,
            }))
            .collect::<Vec<_>>(),
        "depends_on": s
            .edges_from(id, "depends_on")
            .iter()
            .map(|edge| json!({
                "id": edge.target_id,
                "title": s.items.get(&edge.target_id).map(|item| item.title.clone()),
                "rationale": edge.rationale,
            }))
            .collect::<Vec<_>>(),
        // Nothing above it. Normal for a top-level goal, worth noticing
        // anywhere else.
        "top_level": chain.is_empty(),
    }))
}

/// Where the breakdown is not finished.
///
/// Reported, never repaired. A gap is a question for the person — "how does
/// this actually get done" — and a machine that fills it in produces an
/// answer nobody meant.
pub async fn gaps(tx: &mut Tx, w: &str) -> Result<Value> {
    let s = structure(tx, w).await?;
    let mut found = Vec::new();
    for item in s.items.values() {
        if item.archived_at.is_some() {
            continue;
        }
        let children = s.live_children(&item.id);
        if GOALISH.contains(&item.kind.as_str()) {
            if children.is_empty() {
                found.push(json!({
                    "item_id": item.id,
                    "title": item.title,
                    "kind": item.kind,
                    "gap": "not_broken_down",
                    "detail": "この目標の下には何もありません。何によって達成されるのかが決まっていません",
                }));
            } else if !s.reaches_an_action(&item.id) {
                // Broken down, but only into more abstractions. This is the
                // gap that hides: the plan looks complete and nothing in it
                // can be started.
                found.push(json!({
                    "item_id": item.id,
                    "title": item.title,
                    "kind": item.kind,
                    "gap": "no_action_beneath",
                    "detail": "分解はされていますが、実行できる行動までは降りていません",
                }));
            }
        }
        for edge in s.edges_from(&item.id, "depends_on") {
            let target = s.items.get(&edge.target_id);
            if target.is_none_or(|target| target.archived_at.is_some()) {
                found.push(json!({
                    "item_id": item.id,
                    "title": item.title,
                    "kind": item.kind,
                    "gap": "dangling_dependency",
                    "detail": "待っている相手が見当たらないか、整理済みです",
                    "target_id": edge.target_id,
                }));
            }
        }
    }
    found.sort_by(|a, b| {
        (a["gap"].as_str(), a["item_id"].as_str()).cmp(&(b["gap"].as_str(), b["item_id"].as_str()))
    });
    Ok(json!({
        "gaps": found,
        // Said out loud so nothing downstream reads silence as approval.
        "repaired": false,
    }))
}
