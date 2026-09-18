use crate::{
    db::{Db, Tx},
    model::*,
    params,
    storage::*,
};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// The business service. It owns a connection pool, not a connection: several
/// Lambda execution environments run this code against the same TiDB database,
/// so mutual exclusion has to come from database transactions.
#[derive(Clone, Debug)]
pub struct Service {
    pub db: Db,
}
#[derive(Clone, Debug)]
pub struct Actor {
    pub id: String,
    pub agent: bool,
    /// The MCP delegation this request arrived through, when it did.
    ///
    /// Recorded in the audit trail so "which connection proposed this?" has an
    /// answer. It is never an authorization input: an agent is an agent
    /// whichever connection it came from.
    pub connection: Option<String>,
}
impl Actor {
    pub fn local() -> Self {
        Self {
            id: "local-owner".into(),
            agent: false,
            connection: None,
        }
    }
    /// A person acting through the browser.
    pub fn person(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            agent: false,
            connection: None,
        }
    }
}
pub fn now() -> String {
    Utc::now().to_rfc3339()
}
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4())
}
pub(crate) fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub(crate) fn title(v: &Value, key: &str, max: usize) -> Result<String> {
    let s = text(v, key).trim();
    if s.is_empty() || s.chars().count() > max {
        return Err(ApiError::invalid(&format!(
            "{key}は1〜{max}文字で入力してください"
        )));
    }
    Ok(s.into())
}
pub(crate) fn only(v: &Value, fields: &[&str]) -> Result<()> {
    let obj = v
        .as_object()
        .ok_or_else(|| ApiError::invalid("JSONオブジェクトを指定してください"))?;
    if let Some(k) = obj.keys().find(|k| !fields.contains(&k.as_str())) {
        return Err(ApiError::invalid(&format!(
            "変更できないフィールドです: {k}"
        )));
    }
    Ok(())
}
fn date(v: &Option<String>) -> Result<()> {
    if let Some(d) = v {
        if NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() || d.len() != 10 {
            return Err(ApiError::invalid("日付はYYYY-MM-DDで指定してください"));
        }
    }
    Ok(())
}
fn timestamp(s: &str) -> Result<()> {
    DateTime::parse_from_rfc3339(s)
        .map(|_| ())
        .map_err(|_| ApiError::invalid("日時はタイムゾーンを含むRFC3339で指定してください"))
}
fn review_week(body_or_query: &HashMap<String, String>) -> Result<(NaiveDate, NaiveDate)> {
    let raw = body_or_query
        .get("week_start")
        .ok_or_else(|| ApiError::invalid("week_startが必要です"))?;
    let start = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| ApiError::invalid("week_startはYYYY-MM-DDで指定してください"))?;
    if start.weekday().num_days_from_monday() != 0 {
        return Err(ApiError::invalid("week_startは月曜日を指定してください"));
    }
    Ok((start, start + chrono::Duration::days(6)))
}

/// The span and name of a planning period.
///
/// A cadence implies its own length — that is what choosing one means — so the
/// end date is derived rather than asked for. Nothing else is: item dates are
/// never touched by creating a period, and `custom` takes both ends from the
/// person because there is nothing to derive them from.
///
/// `quarter`, `month` and `week` require an aligned start. A workspace whose
/// quarters do not follow the calendar year uses `custom` and says what its
/// periods are, rather than having this guess.
fn cycle_span(
    cadence: &str,
    start: &str,
    end: Option<&str>,
    label: &str,
) -> Result<(String, String)> {
    let from = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .map_err(|_| ApiError::invalid("開始日はYYYY-MM-DDで指定してください"))?;
    let (last, derived) = match cadence {
        "quarter" => {
            if from.day() != 1 || !matches!(from.month(), 1 | 4 | 7 | 10) {
                return Err(ApiError::invalid(
                    "四半期は1月・4月・7月・10月の1日から始めてください。会計年度が違う場合はcustomを使用します",
                ));
            }
            let end = from + chrono::Months::new(3) - chrono::Duration::days(1);
            (
                end,
                format!("{} Q{}", from.year(), (from.month() - 1) / 3 + 1),
            )
        }
        "month" => {
            if from.day() != 1 {
                return Err(ApiError::invalid("月次は月の1日から始めてください"));
            }
            let end = from + chrono::Months::new(1) - chrono::Duration::days(1);
            (end, format!("{}-{:02}", from.year(), from.month()))
        }
        "week" => {
            if from.weekday().num_days_from_monday() != 0 {
                return Err(ApiError::invalid("週次は月曜日から始めてください"));
            }
            let iso = from.iso_week();
            (
                from + chrono::Duration::days(6),
                format!("{}-W{:02}", iso.year(), iso.week()),
            )
        }
        "custom" => {
            let raw = end.ok_or_else(|| ApiError::invalid("customの期間には終了日が必要です"))?;
            let parsed = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                .map_err(|_| ApiError::invalid("終了日はYYYY-MM-DDで指定してください"))?;
            if parsed < from {
                return Err(ApiError::invalid("終了日は開始日以降にしてください"));
            }
            if label.trim().is_empty() {
                return Err(ApiError::invalid("customの期間には名前が必要です"));
            }
            (parsed, label.trim().to_owned())
        }
        _ => {
            return Err(ApiError::invalid(
                "cadenceはquarter / month / week / customのいずれかです",
            ))
        }
    };
    // A derived end for a named cadence is not negotiable: accepting a
    // different one would make the label a lie.
    if cadence != "custom" {
        if let Some(raw) = end.filter(|value| !value.is_empty()) {
            if raw != last.to_string() {
                return Err(ApiError::invalid(
                    "この期間の終了日はcadenceから決まります。別の期間にはcustomを使用します",
                ));
            }
        }
    }
    Ok((
        last.to_string(),
        if label.trim().is_empty() {
            derived
        } else {
            label.trim().chars().take(120).collect()
        },
    ))
}

/// Today, in the workspace's own timezone.
fn workspace_today(workspace: &Workspace) -> NaiveDate {
    workspace
        .timezone
        .parse::<chrono_tz::Tz>()
        .map(|zone| Utc::now().with_timezone(&zone).date_naive())
        .unwrap_or_else(|_| Utc::now().date_naive())
}

/// The alignment graph: whose goals these are, and what they roll up to.
///
/// One workspace, one graph. A goal in someone's personal workspace is not in
/// here and cannot be — the two workspaces are separate security boundaries,
/// and a person's private plan being visible because a company goal happens to
/// point at it would defeat having them. Putting a goal in a shared workspace
/// *is* the act of sharing it.
///
/// `part_of` and `contributes_to` mean different things and are kept apart:
/// the first is structure (one parent, this is part of that), the second is
/// contribution (many, this helps that along). Collapsing them would make
/// "what is this part of" and "what does this help" the same question, and
/// they are not.
async fn alignment_graph(tx: &mut Tx, w: &str, query: &HashMap<String, String>) -> Result<Value> {
    let items: Vec<Item> = list(tx, w, "items").await?;
    let relations: Vec<Relation> = list(tx, w, "relations").await?;
    let cycles: Vec<Cycle> = list(tx, w, "cycles").await?;

    let wanted_kind = query.get("owner_kind").map(String::as_str);
    let wanted_id = query.get("owner_id").map(String::as_str);
    let wanted_cycle = query.get("cycle_id").map(String::as_str);

    // Goals, not every item: alignment is about outcomes and the milestones
    // that mark them. Actions hang off initiatives and are reached by
    // drilling in, not by being scattered across the map.
    let goals: Vec<&Item> = items
        .iter()
        .filter(|item| {
            item.archived_at.is_none() && ["outcome", "milestone"].contains(&item.kind.as_str())
        })
        .collect();

    let matches = |item: &Item| -> bool {
        let owner = item.fields.owner.as_ref();
        wanted_kind.is_none_or(|kind| owner.is_some_and(|owner| owner.kind == kind))
            && wanted_id.is_none_or(|id| owner.is_some_and(|owner| owner.id == id))
            && wanted_cycle.is_none_or(|cycle| item.fields.cycle_id.as_deref() == Some(cycle))
    };

    let edge_of = |item: &Item, kind: &str| -> Vec<String> {
        relations
            .iter()
            .filter(|relation| relation.relation_type == kind && relation.source_id == item.id)
            .map(|relation| relation.target_id.clone())
            .collect()
    };

    let nodes: Vec<Value> = goals
        .iter()
        .filter(|item| matches(item))
        .map(|item| {
            let part_of = edge_of(item, "part_of");
            let contributes_to = edge_of(item, "contributes_to");
            // What rolls up into this, so a company goal can be opened.
            let supported_by: Vec<&str> = relations
                .iter()
                .filter(|relation| {
                    ["part_of", "contributes_to"].contains(&relation.relation_type.as_str())
                        && relation.target_id == item.id
                })
                .map(|relation| relation.source_id.as_str())
                .collect();
            // Initiatives and actions under this goal, for drilling in.
            let beneath = items
                .iter()
                .filter(|other| {
                    other.archived_at.is_none()
                        && ["initiative", "action"].contains(&other.kind.as_str())
                        && relations.iter().any(|relation| {
                            relation.source_id == other.id
                                && relation.target_id == item.id
                                && ["part_of", "contributes_to"]
                                    .contains(&relation.relation_type.as_str())
                        })
                })
                .count();
            let cycle = item
                .fields
                .cycle_id
                .as_ref()
                .and_then(|id| cycles.iter().find(|cycle| &cycle.id == id));
            json!({
                "id": item.id,
                "title": item.title,
                "kind": item.kind,
                "state": item.state,
                "owner": item.fields.owner,
                "cycle": cycle,
                "self_assessment": item.fields.self_assessment,
                "assessed_at": item.fields.assessed_at,
                "due_date": item.due_date,
                // Kept separate on purpose; see this function's comment.
                "part_of": part_of,
                "contributes_to": contributes_to,
                "supported_by": supported_by,
                "descendant_work": beneath,
                // A goal connected to nothing above it. Not wrong — a company
                // goal is supposed to be one — but worth being able to find.
                "orphan": part_of.is_empty() && contributes_to.is_empty(),
            })
        })
        .collect();

    // Who appears in this workspace at all, so a surface can offer the
    // switches without inventing owners that do not exist.
    let mut teams: Vec<&str> = goals
        .iter()
        .filter_map(|item| item.fields.owner.as_ref())
        .filter(|owner| owner.kind == "team")
        .map(|owner| owner.id.as_str())
        .collect();
    teams.sort_unstable();
    teams.dedup();
    let mut people: Vec<&str> = goals
        .iter()
        .filter_map(|item| item.fields.owner.as_ref())
        .filter(|owner| owner.kind == "person")
        .map(|owner| owner.id.as_str())
        .collect();
    people.sort_unstable();
    people.dedup();

    let unowned = goals
        .iter()
        .filter(|item| item.fields.owner.is_none())
        .count();

    Ok(json!({
        "workspace_id": w,
        "goals": nodes,
        "teams": teams,
        "people": people,
        "unowned_goals": unowned,
        "orphan_goals": nodes.iter().filter(|node| node["orphan"] == true).count(),
    }))
}

/// Everything a surface needs to show "which period is this, and what is in
/// it" — the same answer for the web app and for a conversation.
async fn planning_context(tx: &mut Tx, w: &str, query: &HashMap<String, String>) -> Result<Value> {
    let sql = format!("SELECT body FROM workspaces WHERE id=?{}", tx.lock_reads());
    let workspace: Workspace = serde_json::from_str(
        &tx.fetch_optional(&sql, &params![w])
            .await?
            .ok_or_else(ApiError::missing)?
            .text(0)?,
    )?;
    let today = workspace_today(&workspace).to_string();

    let mut cycles: Vec<Cycle> = list(tx, w, "cycles").await?;
    cycles.sort_by(|a, b| a.start_date.cmp(&b.start_date).then(a.id.cmp(&b.id)));

    // "Current" is the period today falls in. A workspace with no periods has
    // no current one, and that is not an error — it is most workspaces.
    let current = query
        .get("cycle_id")
        .and_then(|id| cycles.iter().position(|cycle| &cycle.id == id))
        .or_else(|| {
            cycles
                .iter()
                .position(|cycle| cycle.start_date <= today && today <= cycle.end_date)
        });
    let previous = current.and_then(|index| index.checked_sub(1)).or_else(|| {
        // No current period: the newest one that has already ended.
        cycles
            .iter()
            .rposition(|cycle| cycle.end_date < today)
            .filter(|_| current.is_none())
    });
    let next = match current {
        Some(index) => cycles.get(index + 1).map(|_| index + 1),
        None => cycles.iter().position(|cycle| cycle.start_date > today),
    };

    let items: Vec<Item> = list(tx, w, "items").await?;
    let count_in = |cycle: &Cycle| {
        items
            .iter()
            .filter(|item| {
                item.archived_at.is_none() && item.fields.cycle_id.as_deref() == Some(&cycle.id)
            })
            .count()
    };
    let describe = |index: Option<usize>| -> Value {
        match index.and_then(|index| cycles.get(index)) {
            Some(cycle) => json!({"cycle":cycle,"item_count":count_in(cycle)}),
            None => Value::Null,
        }
    };

    // Work that belongs to no period. It is not a problem to be fixed — an
    // idea with no date is allowed to exist — but a period view would hide it.
    let unassigned = items
        .iter()
        .filter(|item| item.archived_at.is_none() && item.fields.cycle_id.is_none())
        .count();

    // What the person last said they wanted to focus on. Their words, not a
    // plan derived from them: turning this into actions is a proposal someone
    // has to approve.
    let mut reviews: Vec<WeeklyReview> = list(tx, w, "weekly_reviews").await?;
    reviews.retain(|review| review.status == "finalized");
    reviews.sort_by(|a, b| {
        a.week_start
            .cmp(&b.week_start)
            .then(a.revision.cmp(&b.revision))
    });
    let last_review = reviews.last().map(|review| {
        json!({"week_start":review.week_start,"next_focus":review.next_focus,
               "learnings":review.learnings,"challenges":review.challenges,
               "finalized_at":review.finalized_at})
    });

    Ok(json!({
        "workspace_id": w,
        "timezone": workspace.timezone,
        "today": today,
        "cycles": cycles,
        "current": describe(current),
        "previous": describe(previous),
        "next": describe(next),
        "unassigned_items": unassigned,
        "last_finalized_review": last_review,
    }))
}

fn latest_by_occurrence(records: &[Record]) -> HashMap<String, &Record> {
    let mut result = HashMap::new();
    for record in records {
        if let Some(key) = &record.occurrence_key {
            result.insert(key.clone(), record);
        }
    }
    result
}

async fn weekly_summary(tx: &mut Tx, w: &str, query: &HashMap<String, String>) -> Result<Value> {
    let (start, end) = review_week(query)?;
    let sql = format!("SELECT body FROM workspaces WHERE id=?{}", tx.lock_reads());
    let workspace: Workspace = serde_json::from_str(
        &tx.fetch_optional(&sql, &params![w])
            .await?
            .ok_or_else(ApiError::missing)?
            .text(0)?,
    )?;
    let items: Vec<Item> = list(tx, w, "items").await?;
    let records: Vec<Record> = list(tx, w, "records").await?;
    let metrics: Vec<Metric> = list(tx, w, "metrics").await?;
    let observations: Vec<Observation> = list(tx, w, "observations").await?;
    let latest_occurrences = latest_by_occurrence(&records);
    let start_s = start.to_string();
    let end_s = end.to_string();

    let mut actions = Vec::new();
    let mut member_counts: HashMap<String, [u64; 3]> = HashMap::new();
    for item in items
        .iter()
        .filter(|item| item.kind == "action" && item.archived_at.is_none())
    {
        let mut occurrences: Vec<(String, Option<&Record>)> = Vec::new();
        if let Some(recurrence) = &item.fields.recurrence {
            if recurrence.mode == "period_quota" {
                occurrences.extend(latest_occurrences.values().filter_map(|record| {
                    let key = record.occurrence_key.as_ref()?;
                    let (item_id, date) = key.rsplit_once(':')?;
                    (item_id == item.id && date >= start_s.as_str() && date <= end_s.as_str())
                        .then(|| (date.to_string(), Some(*record)))
                }));
                occurrences.sort_by(|a, b| a.0.cmp(&b.0));
                while occurrences.len() < recurrence.times_per_week as usize {
                    occurrences.push((start_s.clone(), None));
                }
            } else {
                for offset in 0..7 {
                    let day = start + chrono::Duration::days(offset);
                    let date = day.to_string();
                    if item.start_date.as_ref().is_some_and(|d| d > &date)
                        || item.due_date.as_ref().is_some_and(|d| d < &date)
                        || !recurrence
                            .weekdays
                            .contains(&(day.weekday().num_days_from_monday() as u8))
                    {
                        continue;
                    }
                    let key = format!("{}:{date}", item.id);
                    occurrences.push((date, latest_occurrences.get(&key).copied()));
                }
            }
        } else if item
            .scheduled_date
            .as_ref()
            .is_some_and(|d| d >= &start_s && d <= &end_s)
        {
            let date = item.scheduled_date.clone().unwrap();
            let key = format!("{}:{date}", item.id);
            occurrences.push((date, latest_occurrences.get(&key).copied()));
        }
        for (date, record) in occurrences {
            let status = match record.map(|r| r.record_type.as_str()) {
                Some("completion") => "completed",
                Some("skip") => "skipped",
                None if item.fields.recurrence.is_none() && item.state == "done" => "completed",
                None if item.fields.recurrence.is_none() && item.state == "abandoned" => "skipped",
                _ => "incomplete",
            };
            let evidence_id = record.map(|r| r.id.clone());
            let actor = item
                .fields
                .assignee
                .clone()
                .or_else(|| record.map(|r| r.author.clone()));
            if let Some(actor) = &actor {
                let counts = member_counts.entry(actor.clone()).or_default();
                counts[match status {
                    "completed" => 0,
                    "skipped" => 1,
                    _ => 2,
                }] += 1;
            }
            actions.push(json!({"item_id":item.id,"title":item.title,"date":date,"status":status,"record_id":evidence_id,"actor":actor}));
        }
    }
    let completed = actions
        .iter()
        .filter(|a| a["status"] == "completed")
        .count();
    let skipped = actions.iter().filter(|a| a["status"] == "skipped").count();
    let incomplete = actions.len().saturating_sub(completed + skipped);

    let goals: Vec<Value> = items.iter().filter(|item| ["outcome", "milestone"].contains(&item.kind.as_str()) && item.archived_at.is_none())
        .map(|item| json!({"item_id":item.id,"title":item.title,"self_assessment":item.fields.self_assessment,"assessed_at":item.fields.assessed_at})).collect();
    let now = Utc::now();
    let metric_values: Vec<Value> = metrics.iter().map(|metric| {
        let mut own: Vec<&Observation> = observations.iter().filter(|o| o.metric_id == metric.id && !observations.iter().any(|n| n.supersedes_id.as_deref() == Some(&o.id))).collect();
        own.sort_by(|a,b| a.observed_at.cmp(&b.observed_at));
        let latest = own.last().copied();
        let prior = latest.and_then(|l| own.iter().rev().copied().find(|o| o.observed_at < l.observed_at && o.observed_at < format!("{start_s}T23:59:59Z")));
        let stale = latest.and_then(|o| DateTime::parse_from_rfc3339(&o.observed_at).ok()).is_some_and(|d| now.signed_duration_since(d.with_timezone(&Utc)).num_days() > 14);
        json!({"metric_id":metric.id,"item_id":metric.item_id,"name":metric.name,"unit":metric.unit,"latest":latest.map(|o| o.value),"latest_observation_id":latest.map(|o| o.id.clone()),"previous":prior.map(|o| o.value),"delta":latest.zip(prior).map(|(a,b)|a.value-b.value),"status":if latest.is_none(){"unmeasured"}else if stale{"stale"}else{"current"}})
    }).collect();
    let members: Vec<Value> = if workspace.scope == "チーム" {
        let rows = tx
            .fetch_all(
                "SELECT actor,role FROM memberships WHERE workspace_id=? ORDER BY actor",
                &params![w],
            )
            .await?;
        rows.iter().map(|row| {
            let actor = row.text(0)?;
            let counts=member_counts.get(&actor).copied().unwrap_or_default();
            Ok(json!({"actor":actor,"role":row.text(1)?,"completed":counts[0],"skipped":counts[1],"incomplete":counts[2]}))
        }).collect::<Result<Vec<_>>>()?
    } else {
        vec![]
    };
    let mut reviews: Vec<WeeklyReview> = list(tx, w, "weekly_reviews").await?;
    reviews.retain(|r| r.week_start == start_s);
    reviews.sort_by_key(|r| r.revision);
    Ok(
        json!({"workspace_id":w,"timezone":workspace.timezone,"week_start":start_s,"week_end":end_s,"actions":{"total":actions.len(),"completed":completed,"skipped":skipped,"incomplete":incomplete,"items":actions},"goals":goals,"metrics":metric_values,"members":members,"review":reviews.last(),"history":reviews}),
    )
}
pub(crate) fn version(v: &Value, current: i64) -> Result<()> {
    let n = v["expected_version"]
        .as_i64()
        .ok_or_else(|| ApiError::new(428, "VERSION_REQUIRED", "expected_versionが必要です"))?;
    if n != current {
        let mut e = ApiError::new(
            409,
            "VERSION_CONFLICT",
            "別の操作で更新されています。入力を保持したまま最新の内容を確認してください",
        );
        e.details = json!({"expected_version":n,"current_version":current});
        return Err(e);
    }
    Ok(())
}
fn fingerprint(method: &str, path: &str, body: &Value) -> String {
    format!("{:x}", Sha256::digest(format!("{method}\n{path}\n{body}")))
}
async fn validate_item(tx: &mut Tx, item: &Item) -> Result<()> {
    if item.title.trim().is_empty()
        || item.title.chars().count() > 200
        || item.description.chars().count() > 10000
        || item.fields.memo.chars().count() > 20000
    {
        return Err(ApiError::invalid(
            "タイトルまたは本文の長さを確認してください",
        ));
    }
    if !["idea", "outcome", "initiative", "action", "milestone"].contains(&item.kind.as_str()) {
        return Err(ApiError::invalid("不明な項目の種類です"));
    }
    if !["draft", "active", "paused", "done", "abandoned"].contains(&item.state.as_str()) {
        return Err(ApiError::invalid("不明な状態です"));
    }
    if item
        .fields
        .priority
        .as_deref()
        .is_some_and(|priority| !["low", "medium", "high", "urgent"].contains(&priority))
    {
        return Err(ApiError::invalid("優先度を確認してください"));
    }
    if let Some(owner) = &item.fields.owner {
        match owner.kind.as_str() {
            // The workspace is the organization; naming it again would only
            // create a second place for it to be wrong.
            "organization" => {
                if !owner.id.is_empty() {
                    return Err(ApiError::invalid("組織の目標にownerのidは指定しません"));
                }
            }
            "team" => {
                if owner.id.trim().is_empty() || owner.id.chars().count() > 120 {
                    return Err(ApiError::invalid("チーム名を確認してください"));
                }
            }
            // A person's goal belongs to a member of this workspace. Anyone
            // else is either a typo or an attempt to attribute work to someone
            // who never agreed to it.
            "person" => {
                if role(tx, &item.workspace_id, &owner.id).await?.is_none() {
                    return Err(ApiError::invalid(
                        "目標の担当者は現在のワークスペースメンバーから選択してください",
                    ));
                }
            }
            _ => {
                return Err(ApiError::invalid(
                    "ownerのkindはorganization / team / personのいずれかです",
                ))
            }
        }
    }
    if let Some(assignee) = &item.fields.assignee_id {
        let member = role(tx, &item.workspace_id, assignee).await?;
        if member.is_none() {
            return Err(ApiError::invalid(
                "担当者は現在のワークスペースメンバーから選択してください",
            ));
        }
        let sql = format!("SELECT body FROM workspaces WHERE id=?{}", tx.lock_reads());
        let workspace: Workspace = serde_json::from_str(
            &tx.fetch_one(&sql, &params![&item.workspace_id])
                .await?
                .text(0)?,
        )?;
        if workspace.scope == "個人" {
            let sql = format!(
                "SELECT actor FROM memberships WHERE workspace_id=? AND role='owner' \
                 ORDER BY actor LIMIT 1{}",
                tx.lock_reads()
            );
            let owner = tx
                .fetch_one(&sql, &params![&item.workspace_id])
                .await?
                .text(0)?;
            if assignee != &owner {
                return Err(ApiError::invalid("個人領域では本人だけを担当者にできます"));
            }
        }
    }
    date(&item.start_date)?;
    date(&item.due_date)?;
    date(&item.scheduled_date)?;
    if item
        .start_date
        .as_ref()
        .zip(item.due_date.as_ref())
        .is_some_and(|(s, e)| s > e)
    {
        return Err(ApiError::invalid("終了日は開始日以降にしてください"));
    }
    if let Some(t) = &item.scheduled_time {
        if NaiveTime::parse_from_str(t, "%H:%M").is_err() || t.len() != 5 {
            return Err(ApiError::invalid("時刻はHH:MMで指定してください"));
        }
    }
    if item
        .fields
        .self_assessment
        .is_some_and(|v| !v.is_finite() || !(0.0..=100.0).contains(&v))
    {
        return Err(ApiError::invalid("自己評価は0〜100で指定してください"));
    }
    if let Some(r) = &item.fields.recurrence {
        if item.kind != "action"
            || !["period_quota", "fixed_schedule"].contains(&r.mode.as_str())
            || r.times_per_week == 0
            || r.times_per_week > 7
            || r.timezone.parse::<chrono_tz::Tz>().is_err()
            || r.weekdays.iter().any(|d| *d > 6)
            || (r.mode == "fixed_schedule" && r.weekdays.is_empty())
        {
            return Err(ApiError::invalid(
                "習慣の回数・曜日・タイムゾーンを確認してください",
            ));
        }
    }
    if let Some(u) = &item.fields.external_url {
        if !(u.is_empty() || u.starts_with("https://") || u.starts_with("http://")) {
            return Err(ApiError::invalid(
                "参考リンクはHTTPまたはHTTPSのURLにしてください",
            ));
        }
    }
    if let Some(id) = &item.fields.next_action_id {
        let next: Item = get(tx, &item.workspace_id, "items", id).await?;
        if next.kind != "action" {
            return Err(ApiError::invalid("次の一歩には行動を指定してください"));
        }
    }
    if let Some(actor) = &item.fields.assignee {
        let member = role(tx, &item.workspace_id, actor).await?.is_some();
        if !member {
            return Err(ApiError::invalid(
                "担当者は同じワークスペースのメンバーから選んでください",
            ));
        }
    }
    Ok(())
}

async fn create_notification(
    tx: &mut Tx,
    item: &Item,
    recipient: &str,
    kind: &str,
    discriminator: &str,
    title: String,
) -> Result<()> {
    let digest = format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\n{}\n{}\n{}\n{}",
            item.workspace_id, item.id, recipient, kind, discriminator
        ))
    );
    let notification = Notification {
        id: format!("notification_{}", &digest[..24]),
        workspace_id: item.workspace_id.clone(),
        recipient: recipient.into(),
        item_id: item.id.clone(),
        kind: kind.into(),
        title,
        created_at: now(),
        read_at: None,
    };
    let workspace_id = item.workspace_id.clone();
    let id = notification.id.clone();
    put_new(tx, &workspace_id, "notifications", &id, &notification).await?;
    Ok(())
}

async fn sync_due_notifications(tx: &mut Tx, workspace_id: &str, actor: &str) -> Result<()> {
    let sql = format!("SELECT body FROM workspaces WHERE id=?{}", tx.lock_reads());
    let workspace: Workspace =
        serde_json::from_str(&tx.fetch_one(&sql, &params![workspace_id]).await?.text(0)?)?;
    let timezone: chrono_tz::Tz = workspace
        .timezone
        .parse()
        .map_err(|_| ApiError::invalid("ワークスペースのタイムゾーンが不正です"))?;
    let today = Utc::now().with_timezone(&timezone).date_naive();
    for item in list::<Item>(tx, workspace_id, "items").await? {
        if item.archived_at.is_some()
            || item.state == "done"
            || item.fields.assignee_id.as_deref() != Some(actor)
        {
            continue;
        }
        let Some(raw_due) = item.due_date.as_deref() else {
            continue;
        };
        let due = NaiveDate::parse_from_str(raw_due, "%Y-%m-%d")
            .map_err(|_| ApiError::invalid("期限の日付が不正です"))?;
        let days = (due - today).num_days();
        let (kind, title) = match days {
            value if value < 0 => ("overdue", format!("期限超過: 「{}」", item.title)),
            0 => ("due_today", format!("今日が期限です: 「{}」", item.title)),
            1..=7 => ("due_soon", format!("7日以内が期限です: 「{}」", item.title)),
            _ => continue,
        };
        create_notification(tx, &item, actor, kind, raw_due, title).await?;
    }
    Ok(())
}
/// Someone else's goal is theirs.
///
/// A shared workspace is not a place where anyone may rewrite anyone's
/// commitments. A goal a person owns is changed by that person, or by a
/// workspace owner — who can already remove them, so refusing here would only
/// be theatre. Organization and team goals are ordinary editor work.
///
/// This is about *changing* it. Everyone who can see the workspace can see the
/// goal, because a goal nobody can see cannot be aligned to anything.
async fn guard_personal_goal(tx: &mut Tx, actor: &Actor, w: &str, item: &Item) -> Result<()> {
    let Some(owner) = &item.fields.owner else {
        return Ok(());
    };
    if owner.kind != "person" || owner.id == actor.id {
        return Ok(());
    }
    if role(tx, w, &actor.id).await?.as_deref() == Some("owner") {
        return Ok(());
    }
    Err(ApiError::new(
        403,
        "GOAL_OWNER_REQUIRED",
        "この目標は担当者本人か、ワークスペースのオーナーだけが変更できます",
    ))
}

async fn validate_relation(tx: &mut Tx, r: &Relation) -> Result<()> {
    let _: Item = get(tx, &r.workspace_id, "items", &r.source_id).await?;
    let _: Item = get(tx, &r.workspace_id, "items", &r.target_id).await?;
    if r.source_id == r.target_id {
        return Err(ApiError::new(
            422,
            "CYCLE_DETECTED",
            "自分自身とは接続できません",
        ));
    }
    if !["part_of", "contributes_to", "depends_on", "relates_to"]
        .contains(&r.relation_type.as_str())
    {
        return Err(ApiError::invalid("不明な関係です"));
    }
    let all: Vec<Relation> = list(tx, &r.workspace_id, "relations").await?;
    let same: Vec<_> = all
        .iter()
        .filter(|x| x.id != r.id && x.relation_type == r.relation_type)
        .collect();
    if same.iter().any(|x| {
        (x.source_id == r.source_id && x.target_id == r.target_id)
            || (r.relation_type == "relates_to"
                && x.source_id == r.target_id
                && x.target_id == r.source_id)
    }) {
        return Err(ApiError::new(
            409,
            "DUPLICATE_RELATION",
            "この関係はすでに登録されています",
        ));
    }
    if r.relation_type == "part_of" && same.iter().any(|x| x.source_id == r.source_id) {
        return Err(ApiError::invalid("整理上の親は1つまでです"));
    }
    // `contributes_to` is included: a goal that contributes to a goal that
    // contributes back to it makes "what does this roll up to" unanswerable,
    // which is the one question the alignment graph exists for.
    if ["part_of", "depends_on", "contributes_to"].contains(&r.relation_type.as_str()) {
        let mut todo = vec![r.target_id.clone()];
        let mut visited = HashSet::new();
        while let Some(node) = todo.pop() {
            if node == r.source_id {
                return Err(ApiError::new(
                    422,
                    "CYCLE_DETECTED",
                    "循環する関係は追加できません",
                ));
            }
            if visited.insert(node.clone()) {
                todo.extend(
                    same.iter()
                        .filter(|x| x.source_id == node)
                        .map(|x| x.target_id.clone()),
                );
            }
        }
    }
    Ok(())
}

impl Service {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Opens the database for the current runtime mode and applies migrations.
    pub async fn open_from_env(local_preview: bool) -> Result<Self> {
        let db = crate::db::connect_from_env(local_preview).await?;
        db.migrate().await?;
        Ok(Self { db })
    }

    /// Opens a database by URL (or local file path) and applies migrations.
    pub async fn open(url: &str) -> Result<Self> {
        let db = Db::connect(url).await?;
        db.migrate().await?;
        Ok(Self { db })
    }

    pub async fn provision_personal(&self, actor: &Actor) -> Result<()> {
        let w = format!("personal-{:x}", Sha256::digest(actor.id.as_bytes()));
        let workspace = Workspace {
            id: w.clone(),
            name: "個人".into(),
            scope: "個人".into(),
            timezone: "Asia/Tokyo".into(),
            role: "owner".into(),
            local: false,
            version: 1,
        };
        let mut tx = self.db.begin_write().await?;
        let sql = tx
            .dialect()
            .insert_ignore("workspaces", &["id", "body", "seq"]);
        tx.execute(
            &sql,
            &params![&w, serde_json::to_string(&workspace)?, sequence()],
        )
        .await?;
        let sql = tx
            .dialect()
            .insert_ignore("memberships", &["workspace_id", "actor", "role"]);
        tx.execute(&sql, &params![&w, &actor.id, "owner"]).await?;
        tx.commit().await
    }

    /// Seeds the local preview workspaces. Never runs against production.
    pub async fn initialize(&self, demo: bool) -> Result<()> {
        let mut tx = self.db.begin_write().await?;
        let empty = tx
            .fetch_one("SELECT COUNT(*) FROM workspaces", &[])
            .await?
            .int(0)?
            == 0;
        if empty {
            crate::seed::seed(&mut tx, demo).await?;
        }
        tx.commit().await
    }

    pub async fn handle(
        &self,
        actor: &Actor,
        method: &str,
        path: &str,
        query: &HashMap<String, String>,
        body: Value,
        key: Option<&str>,
    ) -> Result<Value> {
        self.handle_internal(actor, (method, path), query, body, key, None)
            .await
    }
    pub async fn handle_derived(
        &self,
        actor: &Actor,
        original_path: &str,
        original_body: &Value,
        operation: Operation,
        key: Option<&str>,
    ) -> Result<Value> {
        let fingerprint = fingerprint("POST", original_path, original_body);
        self.handle_internal(
            actor,
            (&operation.method, &operation.path),
            &HashMap::new(),
            operation.body,
            key,
            Some(fingerprint),
        )
        .await
    }
    async fn handle_internal(
        &self,
        actor: &Actor,
        route: (&str, &str),
        query: &HashMap<String, String>,
        body: Value,
        key: Option<&str>,
        fingerprint_override: Option<String>,
    ) -> Result<Value> {
        let (method, path) = route;
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let w = if parts.get(1) == Some(&"workspaces") {
            parts.get(2).copied().unwrap_or("")
        } else {
            ""
        };
        let suggestion_preview =
            parts.as_slice() == ["v1", "workspaces", w, "ai", "suggestions", "preview"];
        let notification_read = method == "PATCH"
            && parts.get(3) == Some(&"notifications")
            && parts.get(5) == Some(&"read");
        let workspace_write = method != "GET"
            && parts.as_slice() != ["v1", "workspaces", w, "leave"]
            && !notification_read
            && !suggestion_preview;
        if method == "GET" {
            let mut tx = self.db.begin_read().await?;
            if !w.is_empty() {
                authorize(&mut tx, &actor.id, w, workspace_write).await?;
            }
            if parts.get(3) == Some(&"snapshot") && !w.is_empty() {
                // Derived reminders are written inside their own transaction so
                // a read request never holds a write lock it does not need.
                drop(tx);
                let mut write = self.db.begin_write().await?;
                write.lock_workspace(w).await?;
                authorize(&mut write, &actor.id, w, false).await?;
                sync_due_notifications(&mut write, w, &actor.id).await?;
                write.commit().await?;
                tx = self.db.begin_read().await?;
                authorize(&mut tx, &actor.id, w, workspace_write).await?;
            }
            return dispatch(&mut tx, actor, method, path, query, &body).await;
        }
        let key = key
            .filter(|k| !k.is_empty() && k.len() <= 200)
            .ok_or_else(|| {
                ApiError::new(400, "IDEMPOTENCY_KEY_REQUIRED", "Idempotency-Keyが必要です")
            })?;
        // An agent may propose, withdraw a proposal, and apply one the person
        // already approved. It may not approve, and it may not write directly.
        if actor.agent
            && !(parts.get(3) == Some(&"changesets")
                && (parts.get(4) == Some(&"preview")
                    || parts.get(5) == Some(&"apply")
                    || parts.get(5) == Some(&"reject")))
        {
            return Err(ApiError::new(
                403,
                "APPROVAL_REQUIRED",
                "画面での承認が必要です",
            ));
        }
        let fp = fingerprint_override.unwrap_or_else(|| fingerprint(method, path, &body));
        let mut tx = self.db.begin_write().await?;
        // Serialize writers of this workspace before reading anything, so a
        // read-then-write sequence cannot interleave with another execution
        // environment's.
        tx.lock_workspace(w).await?;
        // Recheck inside the write transaction, including before an idempotent replay.
        if !w.is_empty() {
            authorize(&mut tx, &actor.id, w, workspace_write).await?;
        }
        crate::collaboration::authorize_route(&mut tx, actor, method, &parts).await?;
        let idempotency_actor = format!(
            "{}:{}",
            actor.id,
            if actor.agent { "agent" } else { "human" }
        );
        let sql = format!(
            "SELECT fingerprint,response FROM idempotency \
             WHERE actor=? AND workspace_id=? AND `key`=?{}",
            tx.lock_reads()
        );
        let prior = tx
            .fetch_optional(&sql, &params![&idempotency_actor, w, key])
            .await?;
        if let Some(row) = prior {
            if row.text(0)? != fp {
                return Err(ApiError::new(
                    409,
                    "IDEMPOTENCY_CONFLICT",
                    "この再送キーは別の入力で使用されています",
                ));
            }
            let response: Value = serde_json::from_str(&row.text(1)?)?;
            crate::collaboration::authorize_replay(&mut tx, method, &parts, &response).await?;
            return Ok(response);
        }
        let result = dispatch(&mut tx, actor, method, path, query, &body).await?;
        tx.execute(
            "INSERT INTO idempotency(actor,workspace_id,`key`,fingerprint,response,created_at) \
             VALUES(?,?,?,?,?,?)",
            &params![&idempotency_actor, w, key, fp, result.to_string(), now()],
        )
        .await?;
        tx.execute(
            "INSERT INTO audit(id,workspace_id,actor,origin,command,created_at,seq,connection) \
             VALUES(?,?,?,?,?,?,?,?)",
            &params![
                new_id("audit"),
                w,
                &actor.id,
                if actor.agent { "mcp" } else { "ui" },
                format!("{method} {path}"),
                now(),
                sequence(),
                actor.connection.clone().unwrap_or_default()
            ],
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }
}

/// Routes one request against an open transaction.
///
/// Boxed because plan templates, onboarding, and approved changesets re-enter
/// `dispatch` for each derived operation; a plain `async fn` would need an
/// infinitely sized future.
pub fn dispatch<'a>(
    tx: &'a mut Tx,
    actor: &'a Actor,
    method: &'a str,
    path: &'a str,
    query: &'a HashMap<String, String>,
    body: &'a Value,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(dispatch_inner(tx, actor, method, path, query, body))
}

async fn dispatch_inner(
    tx: &mut Tx,
    actor: &Actor,
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    body: &Value,
) -> Result<Value> {
    let p: Vec<_> = path.trim_matches('/').split('/').collect();
    if crate::collaboration::routes(method, &p) {
        return crate::collaboration::dispatch(tx, actor, method, &p, body).await;
    }
    match (method, p.as_slice()) {
        ("GET", ["v1", "me"]) => {
            return Ok(
                json!({"id":actor.id,"name":"やまだ はるか","mode":"local-preview","agent":actor.agent}),
            )
        }
        ("GET", ["v1", "workspaces"]) => return value(memberships(tx, &actor.id).await?),
        ("GET", ["v1", "templates"]) => return Ok(templates()),
        // MCP connections belong to the person, not to a workspace: they are
        // the record of which AI clients they let act on their behalf. An
        // agent can never manage its own delegation.
        ("GET", ["v1", "mcp", "connections"]) if !actor.agent => {
            return value(crate::mcp_auth::list_connections(tx, &actor.id).await?);
        }
        ("POST", ["v1", "mcp", "connections", id, "approve"]) if !actor.agent => {
            only(body, &["scopes", "expected_version"])?;
            let scopes: Vec<String> = serde_json::from_value(body["scopes"].clone())?;
            let expected = body["expected_version"].as_i64().ok_or_else(|| {
                ApiError::new(428, "VERSION_REQUIRED", "expected_versionが必要です")
            })?;
            return value(crate::mcp_auth::approve(tx, &actor.id, id, &scopes, expected).await?);
        }
        ("POST", ["v1", "mcp", "connections", id, "revoke"]) if !actor.agent => {
            only(body, &[])?;
            return value(crate::mcp_auth::revoke(tx, &actor.id, id).await?);
        }
        ("GET", ["v1", "settings"]) => {
            let raw = tx
                .fetch_optional(
                    "SELECT body FROM settings WHERE actor=?",
                    &params![&actor.id],
                )
                .await?
                .map(|row| row.text(0))
                .transpose()?;
            return Ok(raw
                .map(|x| serde_json::from_str(&x))
                .transpose()?
                .unwrap_or(json!({"compact":false,"notifications":true,"timezone":"Asia/Tokyo"})));
        }
        ("PATCH", ["v1", "settings"]) if !actor.agent => {
            only(body, &["compact", "notifications", "timezone"])?;
            if body["compact"].as_bool().is_none()
                || body["notifications"].as_bool().is_none()
                || text(body, "timezone").parse::<chrono_tz::Tz>().is_err()
            {
                return Err(ApiError::invalid("設定値を確認してください"));
            }
            let sql = tx
                .dialect()
                .upsert("settings", &["actor", "body"], &["actor"], &["body"]);
            tx.execute(&sql, &params![&actor.id, body.to_string()])
                .await?;
            return Ok(body.clone());
        }
        _ => {}
    }
    if p.len() < 4 || p[0] != "v1" || p[1] != "workspaces" {
        return Err(ApiError::missing());
    }
    let w = p[2];
    let col = p[3];
    let id = p.get(4).copied().unwrap_or("");
    let suffix = p.get(5).copied().unwrap_or("");
    let suggestion_preview =
        p.as_slice() == ["v1", "workspaces", w, "ai", "suggestions", "preview"];
    let notification_read = method == "PATCH" && col == "notifications" && suffix == "read";
    authorize(
        tx,
        &actor.id,
        w,
        method != "GET" && !suggestion_preview && !notification_read,
    )
    .await?;
    if actor.agent
        && method != "GET"
        && !(col == "changesets" && (id == "preview" || suffix == "apply" || suffix == "reject"))
    {
        return Err(ApiError::new(
            403,
            "APPROVAL_REQUIRED",
            "この接続は提案モードです。変更をプレビューして画面で承認してください",
        ));
    }
    match (method, col, id, suffix) {
        ("GET", "weekly-review", "", "") => weekly_summary(tx, w, query).await,
        ("POST", "weekly-reviews", "draft", "") => {
            only(
                body,
                &[
                    "week_start",
                    "learnings",
                    "challenges",
                    "next_focus",
                    "expected_version",
                ],
            )?;
            let mut week_query = HashMap::new();
            week_query.insert("week_start".into(), text(body, "week_start").into());
            let (start, end) = review_week(&week_query)?;
            for field in ["learnings", "challenges", "next_focus"] {
                if text(body, field).chars().count() > 20_000 {
                    return Err(ApiError::invalid(
                        "レビューの各入力は20000文字以内にしてください",
                    ));
                }
            }
            let mut reviews: Vec<WeeklyReview> = list(tx, w, "weekly_reviews").await?;
            reviews.retain(|r| r.week_start == start.to_string());
            reviews.sort_by_key(|r| r.revision);
            let latest = reviews.last();
            let stamp = now();
            let mut review = if let Some(old) = latest.filter(|r| r.status == "draft") {
                version(body, old.version)?;
                let mut next = old.clone();
                next.version += 1;
                next.updated_at = stamp.clone();
                next
            } else {
                WeeklyReview {
                    id: new_id("weekly_review"),
                    workspace_id: w.into(),
                    week_start: start.to_string(),
                    week_end: end.to_string(),
                    status: "draft".into(),
                    learnings: String::new(),
                    challenges: String::new(),
                    next_focus: String::new(),
                    version: 1,
                    revision: latest.map_or(1, |r| r.revision + 1),
                    author: actor.id.clone(),
                    created_at: stamp.clone(),
                    updated_at: stamp,
                    finalized_at: None,
                    supersedes_id: latest.map(|r| r.id.clone()),
                }
            };
            review.learnings = text(body, "learnings").into();
            review.challenges = text(body, "challenges").into();
            review.next_focus = text(body, "next_focus").into();
            let review_id = review.id.clone();
            put(tx, w, "weekly_reviews", &review_id, &review).await?;
            value(review)
        }
        ("POST", "weekly-reviews", id, "finalize") if !id.is_empty() => {
            only(body, &["expected_version"])?;
            let mut review: WeeklyReview = get(tx, w, "weekly_reviews", id).await?;
            version(body, review.version)?;
            if review.status != "draft" {
                return Err(ApiError::new(
                    409,
                    "VERSION_CONFLICT",
                    "このレビューはすでに確定されています",
                ));
            }
            if review.learnings.trim().is_empty()
                && review.challenges.trim().is_empty()
                && review.next_focus.trim().is_empty()
            {
                return Err(ApiError::invalid("確定前にレビューを入力してください"));
            }
            review.status = "finalized".into();
            review.version += 1;
            review.updated_at = now();
            review.finalized_at = Some(review.updated_at.clone());
            put(tx, w, "weekly_reviews", id, &review).await?;
            value(review)
        }
        ("POST", "ai", "suggestions", "preview") if !actor.agent => {
            crate::suggestions::preview(tx, w, body).await
        }
        ("GET", "snapshot", "", "") => {
            let cols = [
                "items",
                "relations",
                "records",
                "metrics",
                "observations",
                "views",
                "changesets",
                "weekly_reviews",
                "cycles",
            ];
            let mut result = json!({"workspace_id":w});
            for col in cols {
                result[col] = value(list::<Value>(tx, w, col).await?)?;
            }
            result["notifications"] = value(
                list::<Notification>(tx, w, "notifications")
                    .await?
                    .into_iter()
                    .filter(|notification| notification.recipient == actor.id)
                    .collect::<Vec<_>>(),
            )?;
            Ok(result)
        }
        ("GET", "graph", "", "") => {
            let limit = query
                .get("limit")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(200)
                .clamp(1, 200);
            let items: Vec<Item> = list(tx, w, "items").await?;
            let relations: Vec<Relation> = list(tx, w, "relations").await?;
            let items: Vec<_> = items
                .into_iter()
                .filter(|i| i.archived_at.is_none())
                .collect();
            let total = items.len();
            let items: Vec<_> = items.into_iter().take(limit).collect();
            let ids: HashSet<_> = items.iter().map(|i| i.id.as_str()).collect();
            let edges: Vec<_> = relations
                .into_iter()
                .filter(|r| {
                    ids.contains(r.source_id.as_str()) && ids.contains(r.target_id.as_str())
                })
                .collect();
            Ok(json!({"items":items,"relations":edges,"truncated":total>limit,"limit":limit}))
        }
        ("GET", "calendar", "", "") => {
            let start = query
                .get("start")
                .ok_or_else(|| ApiError::invalid("startが必要です"))?;
            let end = query
                .get("end")
                .ok_or_else(|| ApiError::invalid("endが必要です"))?;
            date(&Some(start.clone()))?;
            date(&Some(end.clone()))?;
            let start_day = NaiveDate::parse_from_str(start, "%Y-%m-%d").unwrap();
            let end_day = NaiveDate::parse_from_str(end, "%Y-%m-%d").unwrap();
            if end_day < start_day || (end_day - start_day).num_days() > 62 {
                return Err(ApiError::invalid(
                    "カレンダー期間は開始日以降62日以内にしてください",
                ));
            }
            let timezone = query
                .get("timezone")
                .map(String::as_str)
                .unwrap_or("Asia/Tokyo");
            if timezone.parse::<chrono_tz::Tz>().is_err() {
                return Err(ApiError::invalid("タイムゾーンを確認してください"));
            }
            let items: Vec<Item> = list(tx, w, "items").await?;
            let records: Vec<Record> = list(tx, w, "records").await?;
            let active: Vec<_> = items
                .iter()
                .filter(|item| item.archived_at.is_none())
                .cloned()
                .collect();
            let unscheduled: Vec<_> = active
                .iter()
                .filter(|item| {
                    item.start_date.is_none()
                        && item.due_date.is_none()
                        && item.scheduled_date.is_none()
                        && item.fields.recurrence.is_none()
                })
                .cloned()
                .collect();
            let mut days = vec![];
            let mut day = start_day;
            while day <= end_day {
                let key = day.to_string();
                let weekday = day.weekday().num_days_from_monday() as u8;
                let mut entries = vec![];
                for item in &active {
                    for (label, value) in [
                        ("start", item.start_date.as_deref()),
                        ("due", item.due_date.as_deref()),
                        ("scheduled", item.scheduled_date.as_deref()),
                    ] {
                        if value == Some(key.as_str()) {
                            entries.push(json!({"item":item,"label":label}));
                        }
                    }
                    if let Some(rule) = &item.fields.recurrence {
                        let in_range = item.start_date.as_ref().is_none_or(|value| value <= &key)
                            && item.due_date.as_ref().is_none_or(|value| value >= &key);
                        if in_range
                            && (rule.mode == "period_quota" || rule.weekdays.contains(&weekday))
                        {
                            let occurrence = format!("{}:{key}", item.id);
                            let latest = records.iter().rev().find(|record| {
                                record.occurrence_key.as_deref() == Some(&occurrence)
                            });
                            entries.push(json!({"item":item,"label":"habit","occurrence_key":occurrence,"status":latest.map(|record| record.record_type.as_str()).unwrap_or("missed")}));
                        }
                    }
                }
                days.push(json!({"date":key,"entries":entries}));
                day = day.succ_opt().unwrap();
            }
            Ok(
                json!({"start":start,"end":end,"timezone":timezone,"days":days,"unscheduled":unscheduled}),
            )
        }
        ("GET", "today", "", "") => {
            let d = query.get("local_date").cloned().unwrap_or_else(|| {
                Utc::now()
                    .with_timezone(&chrono_tz::Asia::Tokyo)
                    .date_naive()
                    .to_string()
            });
            date(&Some(d.clone()))?;
            let day = NaiveDate::parse_from_str(&d, "%Y-%m-%d").unwrap();
            let items: Vec<Item> = list(tx, w, "items").await?;
            let records: Vec<Record> = list(tx, w, "records").await?;
            let items: Vec<Value> = items
                .into_iter()
                .filter(|i| {
                    i.kind == "action"
                        && i.archived_at.is_none()
                        && i.state != "paused"
                        && i.state != "abandoned"
                        && i.state != "draft"
                        && match &i.fields.recurrence {
                            Some(r) => {
                                i.start_date.as_ref().is_none_or(|s| s <= &d)
                                    && i.due_date.as_ref().is_none_or(|e| e >= &d)
                                    && (r.mode == "period_quota"
                                        || r.weekdays.contains(
                                            &(day.weekday().num_days_from_monday() as u8),
                                        ))
                            }
                            None => i.scheduled_date.as_ref().is_none_or(|s| s == &d),
                        }
                })
                .map(|i| {
                    let occ = format!("{}:{d}", i.id);
                    let latest = records
                        .iter()
                        .rev()
                        .find(|r| r.occurrence_key.as_deref() == Some(&occ));
                    let completed = if i.fields.recurrence.is_some() {
                        latest.is_some_and(|r| r.record_type == "completion")
                    } else {
                        i.state == "done"
                    };
                    json!({"item":i,"completed":completed,"occurrence_key":occ})
                })
                .collect();
            Ok(json!({"local_date":d,"items":items}))
        }
        ("GET", "audit", "", "") => {
            let rows = tx
                .fetch_all(
                    "SELECT id,command,created_at,origin,actor,connection FROM audit \
                     WHERE workspace_id=? ORDER BY seq DESC LIMIT 100",
                    &params![w],
                )
                .await?;
            value(rows.iter().map(|r| Ok(json!({"id":r.text(0)?,"command":r.text(1)?,"created_at":r.text(2)?,"origin":r.text(3)?,"actor":r.text(4)?,"connection":r.opt_text(5)?.filter(|value| !value.is_empty())}))).collect::<Result<Vec<_>>>()?)
        }
        ("GET", col, "", "")
            if [
                "items",
                "relations",
                "records",
                "metrics",
                "observations",
                "views",
                "changesets",
                "weekly_reviews",
                "cycles",
            ]
            .contains(&col) =>
        {
            let mut items: Vec<Value> = list(tx, w, col).await?;
            items.retain(|i| {
                (col != "items"
                    || query.get("archived").is_some_and(|v| v == "true")
                    || i["archived_at"].is_null())
                    && query
                        .get("query")
                        .is_none_or(|q| text(i, "title").to_lowercase().contains(&q.to_lowercase()))
                    && query.get("kind").is_none_or(|k| text(i, "kind") == k)
                    && query.get("state").is_none_or(|k| text(i, "state") == k)
            });
            let start = if let Some(c) = query.get("cursor") {
                items
                    .iter()
                    .position(|i| text(i, "id") == c)
                    .ok_or_else(|| ApiError::invalid("カーソルが無効です"))?
                    + 1
            } else {
                0
            };
            let limit = query
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(50)
                .clamp(1, 200);
            let more = items.len() > start + limit;
            let page: Vec<_> = items.into_iter().skip(start).take(limit).collect();
            let cursor = if more {
                page.last().map(|i| text(i, "id").to_string())
            } else {
                None
            };
            Ok(json!({"items":page,"next_cursor":cursor}))
        }
        ("GET", col, id, "")
            if !id.is_empty()
                && [
                    "items",
                    "relations",
                    "records",
                    "metrics",
                    "observations",
                    "views",
                    "changesets",
                    "weekly_reviews",
                    "cycles",
                ]
                .contains(&col) =>
        {
            get(tx, w, col, id).await
        }
        ("POST", "items", "", "") => {
            only(
                body,
                &[
                    "title",
                    "kind",
                    "description",
                    "state",
                    "start_date",
                    "due_date",
                    "scheduled_date",
                    "scheduled_time",
                    "fields",
                    "parent_id",
                ],
            )?;
            let stamp = now();
            let item = Item {
                id: new_id("item"),
                workspace_id: w.into(),
                title: title(body, "title", 200)?,
                kind: body["kind"].as_str().unwrap_or("outcome").into(),
                description: text(body, "description").into(),
                state: body["state"].as_str().unwrap_or("active").into(),
                version: 1,
                created_at: stamp.clone(),
                updated_at: stamp,
                archived_at: None,
                start_date: serde_json::from_value(body["start_date"].clone())?,
                due_date: serde_json::from_value(body["due_date"].clone())?,
                scheduled_date: serde_json::from_value(body["scheduled_date"].clone())?,
                scheduled_time: serde_json::from_value(body["scheduled_time"].clone())?,
                fields: if body["fields"].is_null() {
                    ItemFields::default()
                } else {
                    serde_json::from_value(body["fields"].clone())?
                },
            };
            validate_item(tx, &item).await?;
            if let Some(reference) = &item.fields.field_reference {
                if let Some(existing) = list::<Item>(tx, w, "items").await?.into_iter().find(|i| {
                    i.fields.field_reference.as_ref().is_some_and(|r| {
                        r.tenant_id == reference.tenant_id
                            && r.external_id == reference.external_id
                            && r.platform_id == reference.platform_id
                    })
                }) {
                    return value(existing);
                }
            }
            put(tx, w, "items", &item.id, &item).await?;
            if let Some(assignee) = &item.fields.assignee_id {
                create_notification(
                    tx,
                    &item,
                    assignee,
                    "assignment",
                    &item.version.to_string(),
                    format!("「{}」の担当になりました", item.title),
                )
                .await?;
            }
            if let Some(parent) = body["parent_id"].as_str() {
                let r = Relation {
                    id: new_id("rel"),
                    workspace_id: w.into(),
                    source_id: item.id.clone(),
                    target_id: parent.into(),
                    relation_type: "part_of".into(),
                    rationale: String::new(),
                    version: 1,
                };
                validate_relation(tx, &r).await?;
                put(tx, w, "relations", &r.id, &r).await?;
            }
            value(item)
        }
        ("PATCH", "items", id, "") if !id.is_empty() => {
            only(
                body,
                &[
                    "expected_version",
                    "title",
                    "description",
                    "state",
                    "start_date",
                    "due_date",
                    "scheduled_date",
                    "scheduled_time",
                    "fields",
                    "archived_at",
                ],
            )?;
            let old: Item = get(tx, w, "items", id).await?;
            guard_personal_goal(tx, actor, w, &old).await?;
            version(body, old.version)?;
            let mut v = value(&old)?;
            for (key, val) in body.as_object().unwrap() {
                if key == "expected_version" {
                    continue;
                }
                if key == "fields" {
                    for (k, x) in val
                        .as_object()
                        .ok_or_else(|| ApiError::invalid("fieldsの形式を確認してください"))?
                    {
                        v["fields"][k] = x.clone();
                    }
                } else {
                    v[key] = val.clone();
                }
            }
            let mut item: Item = serde_json::from_value(v)?;
            if old.kind == "action" && item.state != old.state && item.state == "done" {
                return Err(ApiError::invalid(
                    "行動完了にはcomplete操作を使用してください",
                ));
            }
            if item.archived_at.is_some() {
                item.archived_at = Some(now());
            }
            if item.fields.self_assessment != old.fields.self_assessment {
                item.fields.assessed_at = Some(now());
            }
            item.title = item.title.trim().into();
            item.version += 1;
            item.updated_at = now();
            validate_item(tx, &item).await?;
            if old.fields.recurrence != item.fields.recurrence {
                /* Historical definitions stay in immutable records. */
                let rec = json!({"id":new_id("record"),"workspace_id":w,"item_ids":[id],"record_type":"recurrence_change","body":serde_json::to_string(&old.fields.recurrence)?,"happened_at":now(),"created_at":now(),"author":actor.id});
                put(tx, w, "records", text(&rec, "id"), &rec).await?;
            }
            if old.fields.assignee_id != item.fields.assignee_id {
                let record = json!({"id":new_id("record"),"workspace_id":w,"item_ids":[id],"record_type":"assignment_change","body":json!({"from":old.fields.assignee_id,"to":item.fields.assignee_id}).to_string(),"happened_at":now(),"created_at":now(),"author":actor.id});
                put(tx, w, "records", text(&record, "id"), &record).await?;
                if let Some(assignee) = &item.fields.assignee_id {
                    create_notification(
                        tx,
                        &item,
                        assignee,
                        "assignment",
                        &item.version.to_string(),
                        format!("「{}」の担当になりました", item.title),
                    )
                    .await?;
                }
            }
            if old.start_date != item.start_date || old.due_date != item.due_date {
                let record = json!({"id":new_id("record"),"workspace_id":w,"item_ids":[id],"record_type":"schedule_change","body":json!({"start_date":{"from":old.start_date,"to":item.start_date},"due_date":{"from":old.due_date,"to":item.due_date}}).to_string(),"happened_at":now(),"created_at":now(),"author":actor.id});
                put(tx, w, "records", text(&record, "id"), &record).await?;
                if let Some(assignee) = &item.fields.assignee_id {
                    create_notification(
                        tx,
                        &item,
                        assignee,
                        "due_date",
                        item.due_date.as_deref().unwrap_or("none"),
                        format!("「{}」の期限が更新されました", item.title),
                    )
                    .await?;
                }
            }
            put(tx, w, "items", id, &item).await?;
            value(item)
        }
        ("PATCH", "notifications", id, "read") if !id.is_empty() => {
            only(body, &["read"])?;
            let mut notification: Notification = get(tx, w, "notifications", id).await?;
            if notification.recipient != actor.id {
                return Err(ApiError::missing());
            }
            notification.read_at = if body["read"].as_bool() == Some(false) {
                None
            } else {
                Some(now())
            };
            put(tx, w, "notifications", id, &notification).await?;
            value(notification)
        }
        ("POST", "actions", id, "complete" | "reopen" | "skip") => {
            only(
                body,
                &["expected_version", "local_date", "completed_at", "note"],
            )?;
            let mut item: Item = get(tx, w, "items", id).await?;
            version(body, item.version)?;
            if item.kind != "action"
                || item.archived_at.is_some()
                || item.state == "paused"
                || (item.state == "abandoned" && suffix != "reopen")
            {
                return Err(ApiError::invalid("この行動は記録できる状態ではありません"));
            }
            let d = body["local_date"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("実施するローカル日付が必要です"))?;
            date(&Some(d.into()))?;
            if let Some(r) = &item.fields.recurrence {
                let day = NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap();
                if item.start_date.as_ref().is_some_and(|s| s.as_str() > d)
                    || item.due_date.as_ref().is_some_and(|e| e.as_str() < d)
                    || (r.mode == "fixed_schedule"
                        && !r
                            .weekdays
                            .contains(&(day.weekday().num_days_from_monday() as u8)))
                {
                    return Err(ApiError::invalid("この日は習慣の予定対象外です"));
                }
            }
            let occurrence = format!("{id}:{d}");
            let records: Vec<Record> = list(tx, w, "records").await?;
            let prior = records
                .iter()
                .rev()
                .find(|r| r.occurrence_key.as_deref() == Some(&occurrence));
            let kind = match suffix {
                "complete" => "completion",
                "skip" => "skip",
                _ => "reopen",
            };
            if prior.is_some_and(|r| r.record_type == kind) {
                return Ok(json!({"item":item,"record":prior,"outcome_updated":false}));
            }
            if item.fields.recurrence.is_none() {
                item.state = if suffix == "complete" {
                    "done"
                } else if suffix == "skip" {
                    "abandoned"
                } else {
                    "active"
                }
                .into();
            }
            item.version += 1;
            item.updated_at = now();
            let happened = body["completed_at"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(now);
            timestamp(&happened)?;
            let rec = Record {
                id: new_id("record"),
                workspace_id: w.into(),
                item_ids: vec![id.into()],
                record_type: kind.into(),
                body: text(body, "note").into(),
                happened_at: happened,
                created_at: now(),
                author: actor.id.clone(),
                decision: None,
                occurrence_key: Some(occurrence),
                supersedes_id: prior.map(|r| r.id.clone()),
            };
            put(tx, w, "items", id, &item).await?;
            put(tx, w, "records", &rec.id, &rec).await?;
            if suffix == "complete" {
                if let Some(assignee) = item.fields.assignee_id.clone() {
                    create_notification(
                        tx,
                        &item,
                        &assignee,
                        "completion",
                        rec.occurrence_key.as_deref().unwrap_or(&rec.id),
                        format!("「{}」が完了しました", item.title),
                    )
                    .await?;
                }
            }
            Ok(json!({"item":item,"record":rec,"outcome_updated":false}))
        }
        ("POST", "relations", "", "") => {
            only(body, &["source_id", "target_id", "type", "rationale"])?;
            let r = Relation {
                id: new_id("rel"),
                workspace_id: w.into(),
                source_id: title(body, "source_id", 200)?,
                target_id: title(body, "target_id", 200)?,
                relation_type: title(body, "type", 40)?,
                rationale: text(body, "rationale").into(),
                version: 1,
            };
            validate_relation(tx, &r).await?;
            put(tx, w, col, &r.id, &r).await?;
            value(r)
        }
        ("DELETE", "relations", id, "") => {
            let r: Relation = get(tx, w, col, id).await?;
            version(body, r.version)?;
            remove(tx, w, col, id).await?;
            Ok(json!({"id":id,"deleted":true}))
        }
        ("POST", "records", "", "") => {
            only(
                body,
                &[
                    "item_ids",
                    "record_type",
                    "body",
                    "happened_at",
                    "decision",
                    "supersedes_id",
                ],
            )?;
            let ids: Vec<String> =
                serde_json::from_value(body.get("item_ids").cloned().unwrap_or(json!([])))?;
            for id in &ids {
                let _: Item = get(tx, w, "items", id).await?;
            }
            let kind = body["record_type"].as_str().unwrap_or("note");
            if !["note", "review", "learning", "checkin"].contains(&kind) {
                return Err(ApiError::invalid("この記録の種類は使用できません"));
            }
            let supersedes: Option<String> = serde_json::from_value(body["supersedes_id"].clone())?;
            if let Some(id) = &supersedes {
                let old: Record = get(tx, w, col, id).await?;
                if old.record_type != kind {
                    return Err(ApiError::invalid("同じ種類の記録だけ訂正できます"));
                }
            }
            let happened = body["happened_at"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(now);
            timestamp(&happened)?;
            let r = Record {
                id: new_id("record"),
                workspace_id: w.into(),
                item_ids: ids,
                record_type: kind.into(),
                body: title(body, "body", 20000)?,
                happened_at: happened,
                created_at: now(),
                author: actor.id.clone(),
                decision: serde_json::from_value(body["decision"].clone())?,
                occurrence_key: None,
                supersedes_id: supersedes,
            };
            put(tx, w, col, &r.id, &r).await?;
            value(r)
        }
        ("POST", "metrics", "", "") => {
            only(
                body,
                &[
                    "item_id",
                    "name",
                    "unit",
                    "baseline",
                    "target",
                    "direction",
                    "period_start",
                    "period_end",
                ],
            )?;
            let m = Metric {
                id: new_id("metric"),
                workspace_id: w.into(),
                item_id: title(body, "item_id", 200)?,
                name: title(body, "name", 200)?,
                unit: title(body, "unit", 40)?,
                baseline: body["baseline"]
                    .as_f64()
                    .ok_or_else(|| ApiError::invalid("基準値が必要です"))?,
                target: body["target"]
                    .as_f64()
                    .ok_or_else(|| ApiError::invalid("目標値が必要です"))?,
                direction: title(body, "direction", 20)?,
                period_start: serde_json::from_value(body["period_start"].clone())?,
                period_end: serde_json::from_value(body["period_end"].clone())?,
                version: 1,
            };
            let _: Item = get(tx, w, "items", &m.item_id).await?;
            validate_metric(&m)?;
            put(tx, w, col, &m.id, &m).await?;
            value(m)
        }
        ("POST", "observations", "", "") => {
            only(
                body,
                &[
                    "metric_id",
                    "value",
                    "unit",
                    "source",
                    "observed_at",
                    "supersedes_id",
                ],
            )?;
            let metric: Metric = get(tx, w, "metrics", text(body, "metric_id")).await?;
            if text(body, "unit") != metric.unit {
                return Err(ApiError::new(
                    422,
                    "INVALID_UNIT",
                    "指標の単位と一致しません",
                ));
            }
            let observed = body["observed_at"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(now);
            timestamp(&observed)?;
            let supersedes: Option<String> = serde_json::from_value(body["supersedes_id"].clone())?;
            if let Some(id) = &supersedes {
                let old: Observation = get(tx, w, col, id).await?;
                if old.metric_id != metric.id {
                    return Err(ApiError::invalid("同じ指標の観測だけ訂正できます"));
                }
                if list::<Observation>(tx, w, col)
                    .await?
                    .iter()
                    .any(|o| o.supersedes_id.as_ref() == Some(id))
                {
                    return Err(ApiError::new(
                        409,
                        "VERSION_CONFLICT",
                        "この観測はすでに訂正されています",
                    ));
                }
            }
            let o = Observation {
                id: new_id("obs"),
                workspace_id: w.into(),
                metric_id: metric.id,
                value: body["value"]
                    .as_f64()
                    .ok_or_else(|| ApiError::invalid("観測値を入力してください"))?,
                unit: metric.unit,
                source: title(body, "source", 500)?,
                observed_at: observed,
                created_at: now(),
                supersedes_id: supersedes,
            };
            put(tx, w, col, &o.id, &o).await?;
            value(o)
        }
        ("POST", "views", "", "") => {
            only(body, &["name", "type", "filters"])?;
            if !["list", "map", "timeline", "okr", "today"].contains(&text(body, "type")) {
                return Err(ApiError::invalid("不明なビューです"));
            }
            let v = json!({"id":new_id("view"),"workspace_id":w,"name":title(body,"name",200)?,"type":body["type"],"filters":body.get("filters").cloned().unwrap_or(json!({})),"version":1});
            put(tx, w, col, text(&v, "id"), &v).await?;
            Ok(v)
        }
        ("POST", "views", id, "query") => {
            let view: Value = get(tx, w, "views", id).await?;
            let filters: HashMap<String, String> = serde_json::from_value(view["filters"].clone())?;
            dispatch(
                tx,
                actor,
                "GET",
                &format!("/v1/workspaces/{w}/items"),
                &filters,
                &Value::Null,
            )
            .await
        }
        ("GET", "planning", "", "") => planning_context(tx, w, query).await,
        ("GET", "alignment", "", "") => alignment_graph(tx, w, query).await,
        ("POST", "cycles", "", "") => {
            only(
                body,
                &["cadence", "start_date", "end_date", "label", "previous_id"],
            )?;
            let cadence = text(body, "cadence");
            let start = text(body, "start_date");
            let (end, label) = cycle_span(
                cadence,
                start,
                body["end_date"].as_str(),
                text(body, "label"),
            )?;
            // Periods do not overlap: "which period is this" has to have one
            // answer, and a person choosing between two overlapping quarters
            // is being asked a question the product invented.
            for existing in list::<Cycle>(tx, w, "cycles").await? {
                if start <= existing.end_date.as_str() && existing.start_date <= end {
                    return Err(ApiError::new(
                        409,
                        "CYCLE_OVERLAP",
                        &format!("{}と期間が重なっています", existing.label),
                    ));
                }
            }
            let previous = body["previous_id"].as_str().filter(|id| !id.is_empty());
            if let Some(previous) = previous {
                let _: Cycle = get(tx, w, "cycles", previous).await?;
            }
            let stamp = now();
            let cycle = Cycle {
                id: new_id("cycle"),
                workspace_id: w.into(),
                cadence: cadence.into(),
                label,
                start_date: start.into(),
                end_date: end,
                status: "planned".into(),
                previous_id: previous.map(str::to_owned),
                created_at: stamp.clone(),
                updated_at: stamp,
                version: 1,
            };
            put(tx, w, "cycles", &cycle.id, &cycle).await?;
            value(cycle)
        }
        ("PATCH", "cycles", id, "") if !id.is_empty() => {
            only(body, &["label", "status", "expected_version"])?;
            let mut cycle: Cycle = get(tx, w, "cycles", id).await?;
            version(body, cycle.version)?;
            if let Some(label) = body["label"].as_str() {
                if label.trim().is_empty() {
                    return Err(ApiError::invalid("期間の名前を入力してください"));
                }
                cycle.label = label.trim().chars().take(120).collect();
            }
            if let Some(status) = body["status"].as_str() {
                if !["planned", "active", "closed"].contains(&status) {
                    return Err(ApiError::invalid(
                        "状態はplanned / active / closedのいずれかです",
                    ));
                }
                cycle.status = status.into();
            }
            cycle.version += 1;
            cycle.updated_at = now();
            put(tx, w, "cycles", id, &cycle).await?;
            value(cycle)
        }
        ("DELETE", "cycles", id, "") if !id.is_empty() => {
            let cycle: Cycle = get(tx, w, "cycles", id).await?;
            // Deleting a period must not orphan what is in it. Emptying it
            // first is a decision someone makes deliberately.
            let held = list::<Item>(tx, w, "items")
                .await?
                .into_iter()
                .filter(|item| item.fields.cycle_id.as_deref() == Some(id))
                .count();
            if held > 0 {
                return Err(ApiError::new(
                    409,
                    "CYCLE_NOT_EMPTY",
                    &format!("{held}件の項目がこの期間に属しています"),
                ));
            }
            remove(tx, w, "cycles", &cycle.id).await?;
            Ok(json!({"deleted":id}))
        }
        // Carrying work into the next period.
        //
        // The source item is not touched. The period that has already been
        // reviewed still says what was in it, and the new item points back at
        // where it came from — otherwise "we carried this over three times"
        // becomes unanswerable.
        ("POST", "cycles", id, "carry-over") if !id.is_empty() => {
            only(body, &["item_ids", "expected_version"])?;
            let target: Cycle = get(tx, w, "cycles", id).await?;
            version(body, target.version)?;
            if target.status == "closed" {
                return Err(ApiError::invalid("終了した期間へは引き継げません"));
            }
            let requested: Vec<String> = body["item_ids"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if requested.is_empty() || requested.len() > 100 {
                return Err(ApiError::invalid("引き継ぐ項目を1〜100件指定してください"));
            }
            let mut carried = Vec::new();
            for item_id in requested {
                let source: Item = get(tx, w, "items", &item_id).await?;
                if source.fields.cycle_id.as_deref() == Some(id) {
                    return Err(ApiError::invalid(&format!(
                        "{}はすでにこの期間にあります",
                        source.title
                    )));
                }
                let stamp = now();
                let mut fields = source.fields.clone();
                fields.cycle_id = Some(id.to_owned());
                fields.carried_from = Some(source.id.clone());
                // Dates belong to the period they were set for. Carrying work
                // forward does not decide when it now happens; the person does.
                let item = Item {
                    id: new_id("item"),
                    workspace_id: w.into(),
                    kind: source.kind.clone(),
                    title: source.title.clone(),
                    description: source.description.clone(),
                    state: "active".into(),
                    version: 1,
                    created_at: stamp.clone(),
                    updated_at: stamp,
                    archived_at: None,
                    start_date: None,
                    due_date: None,
                    scheduled_date: None,
                    scheduled_time: None,
                    fields,
                };
                put(tx, w, "items", &item.id, &item).await?;
                carried.push(item);
            }
            Ok(json!({"cycle":target,"items":carried}))
        }
        ("POST", "templates", id, "apply") => apply_template(tx, actor, w, id, body).await,
        ("POST", "onboarding", "complete", "") => complete_onboarding(tx, actor, w, body).await,
        ("POST", "changesets", "preview", "") => preview(tx, actor, w, body).await,
        // Approval is a person's act, and only a person's.
        //
        // An MCP client's call — whether the model made it or a button in the
        // app did — arrives on the same connection with the same token, so the
        // server cannot tell them apart. It therefore does not try: an agent
        // actor is refused here, and approval happens in Basepath's own origin
        // with the person's own session. `_meta.ui.visibility`, a header, or an
        // `approved` flag the caller sets are not evidence of anything.
        ("POST", "changesets", id, "approve") if !actor.agent => {
            only(body, &["hash"])?;
            let mut c: Value = get(tx, w, col, id).await?;
            validate_preview(tx, w, &c).await?;
            if c["status"] != "pending" {
                return Err(ApiError::new(
                    409,
                    "VERSION_CONFLICT",
                    "この変更案は承認待ちではありません",
                ));
            }
            // The person approves the content they were shown. If the caller
            // names a digest, it has to be that content's.
            if let Some(shown) = body["hash"].as_str() {
                if shown != text(&c, "hash") {
                    return Err(ApiError::new(
                        409,
                        "CHANGESET_SUPERSEDED",
                        "表示していた内容と異なります。最新の変更案を確認してください",
                    ));
                }
            }
            c["approved_by"] = json!(actor.id);
            c["approved_at"] = json!(now());
            c["status"] = json!("approved");
            c["approved_hash"] = c["hash"].clone();
            put(tx, w, col, id, &c).await?;
            Ok(c)
        }
        // Rejecting only discards a proposal, so an agent may do it: nothing
        // is applied, and the person can always propose again.
        ("POST", "changesets", id, "reject") => {
            only(body, &[])?;
            let mut c: Value = get(tx, w, col, id).await?;
            if !["pending", "approved"].contains(&text(&c, "status")) {
                return Err(ApiError::new(
                    409,
                    "VERSION_CONFLICT",
                    "この変更案はすでに処理されています",
                ));
            }
            c["status"] = json!("rejected");
            c["rejected_by"] = json!(actor.id);
            c["rejected_at"] = json!(now());
            // A rejected proposal can never be applied, approval or not.
            c["approved_hash"] = Value::Null;
            put(tx, w, col, id, &c).await?;
            Ok(c)
        }
        ("POST", "changesets", id, "apply") => {
            only(body, &[])?;
            let mut c: Value = get(tx, w, col, id).await?;
            validate_preview(tx, w, &c).await?;
            // Applying is allowed for the person who approved this exact
            // content, whichever surface they are on. It is never allowed
            // because a caller says it was approved.
            if c["status"] != "approved"
                || c["approved_hash"].is_null()
                || c["approved_hash"] != c["hash"]
                || c["approved_by"] != actor.id
            {
                return Err(ApiError::new(
                    403,
                    "APPROVAL_REQUIRED",
                    "画面での差分確認と承認が必要です",
                ));
            }
            let ops: Vec<Operation> = serde_json::from_value(c["operations"].clone())?;
            let human = Actor {
                id: actor.id.clone(),
                agent: false,
                connection: actor.connection.clone(),
            };
            let mut output = vec![];
            for op in ops {
                output.push(
                    dispatch(tx, &human, &op.method, &op.path, &HashMap::new(), &op.body).await?,
                );
            }
            c["status"] = json!("applied");
            c["applied_at"] = json!(now());
            c["applied_by"] = json!(actor.id);
            c["applied_by_connection"] = json!(actor.connection);
            put(tx, w, col, id, &c).await?;
            Ok(json!({"changeset":c,"results":output}))
        }
        ("POST", "exports", "", "") if !actor.agent => {
            only(body, &[])?;
            let mut backup = json!({"schema_version":1,"exported_at":now(),"workspace_id":w});
            for col in [
                "items",
                "relations",
                "records",
                "metrics",
                "observations",
                "views",
                "weekly_reviews",
                "cycles",
            ] {
                backup[col] = value(list::<Value>(tx, w, col).await?)?;
            }
            Ok(backup)
        }
        ("POST", "imports", "", "") if !actor.agent => import(tx, w, body).await,
        _ => Err(ApiError::missing()),
    }
}

async fn complete_onboarding(tx: &mut Tx, actor: &Actor, w: &str, body: &Value) -> Result<Value> {
    only(
        body,
        &[
            "title",
            "purpose",
            "due_date",
            "initiative_title",
            "action_title",
            "metric",
        ],
    )?;
    let title = title(body, "title", 200)?;
    let purpose = text(body, "purpose").trim();
    if purpose.chars().count() > 10_000 {
        return Err(ApiError::invalid(
            "達成したいことは10000文字以内で入力してください",
        ));
    }
    let due_date: Option<String> = serde_json::from_value(body["due_date"].clone())?;
    date(&due_date)?;
    if list::<Item>(tx, w, "items")
        .await?
        .iter()
        .any(|item| item.archived_at.is_none())
    {
        return Err(ApiError::new(
            409,
            "ONBOARDING_CONFLICT",
            "別の操作で項目が作成されました。入力を保持したまま最新の内容を確認してください",
        ));
    }

    let base = format!("/v1/workspaces/{w}");
    let mut goal = dispatch(
        tx,
        actor,
        "POST",
        &format!("{base}/items"),
        &HashMap::new(),
        &json!({
            "title": title,
            "description": purpose,
            "kind": "outcome",
            "due_date": due_date,
            "fields": { "icon": "target" }
        }),
    )
    .await?;

    let initiative_title = text(body, "initiative_title").trim();
    let action_title = text(body, "action_title").trim();
    let mut initiative = Value::Null;
    let mut action = Value::Null;
    if !initiative_title.is_empty() {
        if initiative_title.chars().count() > 200 {
            return Err(ApiError::invalid("取り組みは200文字以内で入力してください"));
        }
        initiative = dispatch(
            tx,
            actor,
            "POST",
            &format!("{base}/items"),
            &HashMap::new(),
            &json!({"title":initiative_title,"kind":"initiative","parent_id":goal["id"],"fields":{"icon":"flag"}}),
        ).await?;
    }
    if !action_title.is_empty() {
        if action_title.chars().count() > 200 {
            return Err(ApiError::invalid("次の一歩は200文字以内で入力してください"));
        }
        let parent = initiative["id"].as_str().or_else(|| goal["id"].as_str());
        action = dispatch(
            tx,
            actor,
            "POST",
            &format!("{base}/items"),
            &HashMap::new(),
            &json!({"title":action_title,"kind":"action","parent_id":parent}),
        )
        .await?;
        goal = dispatch(
            tx,
            actor,
            "PATCH",
            &format!("{base}/items/{}", text(&goal, "id")),
            &HashMap::new(),
            &json!({"expected_version":goal["version"],"fields":{"next_action_id":action["id"]}}),
        )
        .await?;
    }

    let mut metric = Value::Null;
    if !body["metric"].is_null() {
        let candidate = &body["metric"];
        only(
            candidate,
            &["name", "unit", "baseline", "target", "direction"],
        )?;
        let mut metric_body = candidate.clone();
        metric_body["item_id"] = goal["id"].clone();
        metric = dispatch(
            tx,
            actor,
            "POST",
            &format!("{base}/metrics"),
            &HashMap::new(),
            &metric_body,
        )
        .await?;
    }
    Ok(json!({"goal":goal,"initiative":initiative,"action":action,"metric":metric}))
}
fn validate_metric(m: &Metric) -> Result<()> {
    date(&m.period_start)?;
    date(&m.period_end)?;
    if !["increase", "decrease", "threshold"].contains(&m.direction.as_str())
        || (m.direction == "increase" && m.target <= m.baseline)
        || (m.direction == "decrease" && m.target >= m.baseline)
        || m.period_start
            .as_ref()
            .zip(m.period_end.as_ref())
            .is_some_and(|(s, e)| s > e)
    {
        return Err(ApiError::invalid(
            "評価方向・基準値・目標値・期間を確認してください",
        ));
    }
    Ok(())
}
async fn workspace_version(tx: &mut Tx, w: &str) -> Result<String> {
    let mut parts = String::new();
    for col in ["items", "relations", "metrics", "views"] {
        parts.push_str(&serde_json::to_string(&list::<Value>(tx, w, col).await?)?);
    }
    let sql = format!(
        "SELECT actor,role FROM memberships WHERE workspace_id=? ORDER BY actor{}",
        tx.lock_reads()
    );
    let rows = tx.fetch_all(&sql, &params![w]).await?;
    for row in &rows {
        parts.push_str(&format!("{:?}", (row.text(0)?, row.text(1)?)));
    }
    Ok(format!("{:x}", Sha256::digest(parts)))
}
async fn preview(tx: &mut Tx, actor: &Actor, w: &str, b: &Value) -> Result<Value> {
    only(b, &["operations", "title"])?;
    let ops: Vec<Operation> = serde_json::from_value(b["operations"].clone())?;
    if ops.is_empty() || ops.len() > 100 {
        return Err(ApiError::invalid("変更は1〜100操作にしてください"));
    }
    for op in &ops {
        let p: Vec<_> = op.path.trim_matches('/').split('/').collect();
        // A weekly review draft may be proposed; finalizing one may not. A
        // person approving a change set is agreeing to the text, not declaring
        // the week reviewed — that stays an act they perform in Basepath.
        let weekly_draft =
            p.len() == 5 && p[3] == "weekly-reviews" && p[4] == "draft" && op.method == "POST";
        if p.len() < 4
            || p[0] != "v1"
            || p[1] != "workspaces"
            || p[2] != w
            || !(weekly_draft
                || [
                    "items",
                    "relations",
                    "records",
                    "metrics",
                    "observations",
                    "actions",
                    "templates",
                    "views",
                    "cycles",
                ]
                .contains(&p[3]))
            || !["POST", "PATCH", "DELETE"].contains(&op.method.as_str())
        {
            return Err(ApiError::invalid(
                "同じワークスペースの計画操作だけ提案できます",
            ));
        }
    }
    // Validate the complete batch without changing the live plan, and record
    // what each operation would do while the effect is observable.
    //
    // The diff is captured here, inside the savepoint, rather than being
    // re-derived later: only here is both the state before an operation and
    // the state after it available, in order, without touching the live plan.
    tx.savepoint("preview_validation").await?;
    let human = Actor {
        id: actor.id.clone(),
        agent: false,
        connection: actor.connection.clone(),
    };
    let mut validation = Ok(());
    let mut changes = Vec::new();
    for op in &ops {
        match describe_operation(tx, &human, w, op).await {
            Ok(change) => changes.push(change),
            Err(error) => {
                validation = Err(error);
                break;
            }
        }
    }
    tx.rollback_to_savepoint("preview_validation").await?;
    validation?;
    let c = json!({"id":new_id("change"),"workspace_id":w,"title":b["title"].as_str().unwrap_or("計画の変更案"),"operations":ops,"changes":changes,"status":"pending","actor":actor.id,"proposed_by_connection":actor.connection,"hash":fingerprint("PREVIEW",w,&b["operations"]),"base_version":workspace_version(tx,w).await?,"created_at":now(),"expires_at":(Utc::now()+chrono::Duration::minutes(30)).to_rfc3339()});
    put(tx, w, "changesets", text(&c, "id"), &c).await?;
    Ok(c)
}

/// The newest revision recorded for the week containing `week_start`.
///
/// Returns `None` rather than failing when the week cannot be parsed: the
/// operation itself is about to run and will reject a bad week with the real
/// message.
async fn latest_weekly_review(tx: &mut Tx, w: &str, week_start: &str) -> Result<Option<Value>> {
    let mut query = HashMap::new();
    query.insert("week_start".to_string(), week_start.to_string());
    let Ok((start, _)) = review_week(&query) else {
        return Ok(None);
    };
    let mut reviews: Vec<WeeklyReview> = list(tx, w, "weekly_reviews").await?;
    reviews.retain(|review| review.week_start == start.to_string());
    reviews.sort_by_key(|review| review.revision);
    Ok(match reviews.last() {
        Some(review) => Some(serde_json::to_value(review)?),
        None => None,
    })
}

/// Runs one proposed operation and records what it did.
///
/// The caller is inside a savepoint that will be rolled back, so this is a
/// dry run: the returned description is what a person will be shown, and the
/// plan is untouched.
async fn describe_operation(tx: &mut Tx, human: &Actor, w: &str, op: &Operation) -> Result<Value> {
    let parts: Vec<_> = op.path.trim_matches('/').split('/').collect();
    let collection = parts.get(3).copied().unwrap_or("");
    // `actions` operate on an item; a weekly review draft is addressed by its
    // week rather than by a row id; everything else names its own collection.
    let (target_collection, target_id) = match collection {
        "actions" => ("items", parts.get(4).copied().unwrap_or("")),
        "weekly-reviews" => ("weekly_reviews", ""),
        _ => (collection, parts.get(4).copied().unwrap_or("")),
    };
    // What the operation would replace. For a review draft that is the newest
    // revision recorded for the week it names, so the person sees their own
    // words next to the proposed ones instead of an unexplained creation.
    let before: Option<Value> = if collection == "weekly-reviews" {
        latest_weekly_review(tx, w, text(&op.body, "week_start")).await?
    } else if target_id.is_empty() {
        None
    } else {
        get(tx, w, target_collection, target_id).await.ok()
    };

    let result = dispatch(tx, human, &op.method, &op.path, &HashMap::new(), &op.body).await?;

    // The identifier of what the operation actually touched: for a create it
    // only exists in the result.
    let id = if target_id.is_empty() {
        result["id"].as_str().unwrap_or("").to_owned()
    } else {
        target_id.to_owned()
    };
    let after: Option<Value> = if id.is_empty() {
        None
    } else {
        get(tx, w, target_collection, &id).await.ok()
    };
    let effect = match (&before, &after) {
        (None, Some(_)) => "created",
        (Some(_), None) => "deleted",
        (Some(_), Some(_)) => "updated",
        (None, None) => "unknown",
    };
    let title = after
        .as_ref()
        .or(before.as_ref())
        .and_then(|value| {
            value["title"]
                .as_str()
                .map(str::to_owned)
                // A weekly review has no title; the week it covers is what
                // identifies it to a person.
                .or_else(|| {
                    value["week_start"]
                        .as_str()
                        .map(|week| format!("{week}の週次レビュー"))
                })
        })
        .unwrap_or_default();
    Ok(json!({
        "method": op.method,
        "path": op.path,
        "collection": target_collection,
        "id": id,
        "title": title,
        "effect": effect,
        "before": before,
        "after": after,
    }))
}
async fn validate_preview(tx: &mut Tx, w: &str, c: &Value) -> Result<()> {
    if text(c, "expires_at") < now().as_str()
        || text(c, "base_version") != workspace_version(tx, w).await?
    {
        return Err(ApiError::new(
            409,
            "VERSION_CONFLICT",
            "計画または権限が変わったか、有効期限が切れました。再プレビューしてください",
        ));
    }
    Ok(())
}
pub fn templates() -> Value {
    json!([
        {"id":"free","title":"自由形式","version":1,"preview":["目標を1つ作成。数値・期限・親は任意。"]},
        {"id":"okr","title":"OKR","version":1,"preview":["目標を1つ作成","成果指標を自分で設定できるOKRビューを追加"]},
        {"id":"project","title":"プロジェクト","version":1,"preview":["目標を1つ作成","計画を整理する取り組みと、最初の行動を追加"]},
        {"id":"learning","title":"学習計画","version":1,"preview":["目標を1つ作成","練習の取り組みと、週3回の習慣を追加"]},
        {"id":"habit","title":"習慣づくり","version":1,"preview":["目標を1つ作成","週3回取り組む習慣を追加。完了は各日ごとに記録。"]}
    ])
}
async fn apply_template(tx: &mut Tx, a: &Actor, w: &str, id: &str, b: &Value) -> Result<Value> {
    only(b, &["title", "description", "start_date", "due_date"])?;
    if !["free", "okr", "project", "learning", "habit"].contains(&id) {
        return Err(ApiError::missing());
    }
    let mut b = b.clone();
    b["fields"] = json!({"template":id,"template_version":1,"icon":match id{"learning"=>"graduation","project"=>"folder","habit"=>"repeat",_=>"target"}});
    let base = format!("/v1/workspaces/{w}");
    let mut root = dispatch(tx, a, "POST", &format!("{base}/items"), &HashMap::new(), &b).await?;
    if ["project", "learning", "habit"].contains(&id) {
        let child = dispatch(
            tx,
            a,
            "POST",
            &format!("{base}/items"),
            &HashMap::new(),
            &json!({"title":if id=="project"{"計画を整理する"}else{"少しずつ練習する"},"kind":"initiative","fields":{"icon":"flag"}}),
        ).await?;
        dispatch(
            tx,
            a,
            "POST",
            &format!("{base}/relations"),
            &HashMap::new(),
            &json!({"source_id":child["id"],"target_id":root["id"],"type":"part_of"}),
        )
        .await?;
        let fields = if id == "project" {
            json!({})
        } else {
            json!({"recurrence":{"mode":"period_quota","times_per_week":3,"timezone":"Asia/Tokyo","weekdays":[]}})
        };
        let action = dispatch(
            tx,
            a,
            "POST",
            &format!("{base}/items"),
            &HashMap::new(),
            &json!({"kind":"action","title":if id=="project"{"最初の一歩を書き出す"}else{"今日の練習をする"},"fields":fields}),
        ).await?;
        dispatch(
            tx,
            a,
            "POST",
            &format!("{base}/relations"),
            &HashMap::new(),
            &json!({"source_id":action["id"],"target_id":child["id"],"type":"part_of"}),
        )
        .await?;
        root = dispatch(
            tx,
            a,
            "PATCH",
            &format!("{base}/items/{}", text(&root, "id")),
            &HashMap::new(),
            &json!({"expected_version":1,"fields":{"next_action_id":action["id"]}}),
        )
        .await?;
    }
    if id == "okr" {
        dispatch(
            tx,
            a,
            "POST",
            &format!("{base}/views"),
            &HashMap::new(),
            &json!({"name":"OKR","type":"okr","filters":{"kind":"outcome"}}),
        )
        .await?;
    }
    Ok(root)
}
async fn import(tx: &mut Tx, w: &str, b: &Value) -> Result<Value> {
    only(
        b,
        &[
            "schema_version",
            "exported_at",
            "workspace_id",
            "items",
            "relations",
            "records",
            "metrics",
            "observations",
            "views",
            "weekly_reviews",
            "cycles",
        ],
    )?;
    if b["schema_version"] != 1 {
        return Err(ApiError::invalid("対応していないバックアップ形式です"));
    }
    let cols = [
        "items",
        "relations",
        "records",
        "metrics",
        "observations",
        "views",
        "weekly_reviews",
        "cycles",
    ];
    let mut count = 0;
    for col in cols {
        let Some(docs) = b[col].as_array() else {
            // Collections added after the backup format existed are optional:
            // an older export simply has none of them.
            if ["weekly_reviews", "cycles"].contains(&col) {
                continue;
            }
            return Err(ApiError::invalid("バックアップに必要な一覧がありません"));
        };
        if docs.len() > 10000 {
            return Err(ApiError::invalid("一度に取り込める件数を超えています"));
        }
        for doc in docs {
            let id = title(doc, "id", 200)?;
            if text(doc, "workspace_id") != text(b, "workspace_id") {
                return Err(ApiError::invalid(
                    "バックアップ内のワークスペースが不整合です",
                ));
            }
            if exists(tx, w, col, &id).await? {
                return Err(ApiError::new(
                    409,
                    "IMPORT_CONFLICT",
                    "同じIDのデータがあります。既存データは変更していません",
                ));
            }
            let mut doc = doc.clone();
            doc["workspace_id"] = json!(w);
            put(tx, w, col, &id, &doc).await?;
            count += 1;
        }
    }
    // Validate after insertion so references can resolve independent of file order; outer transaction rolls all back on failure.
    for i in list::<Item>(tx, w, "items").await? {
        validate_item(tx, &i).await?;
    }
    for r in list::<Relation>(tx, w, "relations").await? {
        validate_relation(tx, &r).await?;
    }
    for m in list::<Metric>(tx, w, "metrics").await? {
        let _: Item = get(tx, w, "items", &m.item_id).await?;
        validate_metric(&m)?;
    }
    for r in list::<Record>(tx, w, "records").await? {
        for id in r.item_ids {
            let _: Item = get(tx, w, "items", &id).await?;
        }
        timestamp(&r.happened_at)?;
    }
    for o in list::<Observation>(tx, w, "observations").await? {
        let m: Metric = get(tx, w, "metrics", &o.metric_id).await?;
        if o.unit != m.unit {
            return Err(ApiError::invalid("観測の単位が一致しません"));
        }
        timestamp(&o.observed_at)?;
        if let Some(id) = o.supersedes_id {
            let _: Observation = get(tx, w, "observations", &id).await?;
        }
    }
    for cycle in list::<Cycle>(tx, w, "cycles").await? {
        date(&Some(cycle.start_date.clone()))?;
        date(&Some(cycle.end_date.clone()))?;
        if cycle.end_date < cycle.start_date {
            return Err(ApiError::invalid("期間の終了日が開始日より前です"));
        }
        if !["planned", "active", "closed"].contains(&cycle.status.as_str()) {
            return Err(ApiError::invalid("期間の状態が不正です"));
        }
        if let Some(id) = cycle.previous_id {
            let _: Cycle = get(tx, w, "cycles", &id).await?;
        }
    }
    for item in list::<Item>(tx, w, "items").await? {
        if let Some(id) = item.fields.cycle_id {
            let _: Cycle = get(tx, w, "cycles", &id).await?;
        }
        if let Some(id) = item.fields.carried_from {
            let _: Item = get(tx, w, "items", &id).await?;
        }
    }
    for review in list::<WeeklyReview>(tx, w, "weekly_reviews").await? {
        date(&Some(review.week_start.clone()))?;
        date(&Some(review.week_end.clone()))?;
        if !["draft", "finalized"].contains(&review.status.as_str()) {
            return Err(ApiError::invalid("週次レビューの状態が不正です"));
        }
        if let Some(id) = review.supersedes_id {
            let _: WeeklyReview = get(tx, w, "weekly_reviews", &id).await?;
        }
    }
    Ok(json!({"imported":count,"workspace_id":w}))
}
