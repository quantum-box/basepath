use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub state: String,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub scheduled_date: Option<String>,
    #[serde(default)]
    pub scheduled_time: Option<String>,
    #[serde(default)]
    pub fields: ItemFields,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemFields {
    #[serde(default)]
    pub assignee_id: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub memo: String,
    #[serde(default)]
    pub next_action_id: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub self_assessment: Option<f64>,
    #[serde(default)]
    pub assessed_at: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub template_version: Option<i64>,
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    #[serde(default)]
    pub external_url: Option<String>,
    #[serde(default)]
    pub field_reference: Option<FieldReference>,
    /// The planning cycle this belongs to, when the workspace uses them.
    ///
    /// Optional on purpose: a workspace that has never created a cycle keeps
    /// working exactly as before, and every item in it simply has no cycle.
    #[serde(default)]
    pub cycle_id: Option<String>,
    /// How this goal's progress is derived, when it is derived at all.
    ///
    /// Unset is the default and means **no derived progress**. A number that
    /// nobody chose a method for is a number nobody can defend, and a
    /// dashboard full of those is worse than a dashboard with blanks.
    #[serde(default)]
    pub rollup: Option<String>,
    /// Someone's explicit judgement of how this goal is going.
    ///
    /// Set by a person, with a note and a timestamp, so "at risk" always has
    /// an author and a date. Absent means unknown — which is a real answer.
    #[serde(default)]
    pub health: Option<GoalHealth>,
    /// Whose goal this is: the organization, a team, or a person.
    ///
    /// Alignment is a graph inside one workspace. A goal owned by a person
    /// here is a goal they chose to put where their colleagues can see it —
    /// their private plan lives in their personal workspace and is never part
    /// of this graph. That boundary is the whole point of having two.
    #[serde(default)]
    pub owner: Option<GoalOwner>,
    /// The item this was carried over from, when it was.
    ///
    /// Carrying work into the next period copies it rather than moving it, so
    /// the period that has already been reviewed still says what was in it.
    /// This is the thread back.
    #[serde(default)]
    pub carried_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Notification {
    pub id: String,
    pub workspace_id: String,
    pub recipient: String,
    pub item_id: String,
    pub kind: String,
    pub title: String,
    pub created_at: String,
    #[serde(default)]
    pub read_at: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Recurrence {
    pub mode: String,
    pub times_per_week: u8,
    pub timezone: String,
    #[serde(default)]
    pub weekdays: Vec<u8>,
}
/// One thing a person's own Basepath remembers for them.
///
/// # Personal only
///
/// Memory exists in a personal workspace and nowhere else. It is not moved,
/// inherited or synced into a shared one, and an organization goal that a
/// personal goal contributes to gives the organization no path back here.
/// Putting something in a shared workspace is how a person shares it; a
/// product that copied their memory across on their behalf would be making
/// that decision for them.
///
/// A future organization memory is a different entity with its own storage and
/// its own ids. Sharing this one would make the boundary a convention rather
/// than a fact.
///
/// # Kinds
///
/// Separated because they behave differently and age differently:
///
/// - `fact` — something stable the person stated. Needs a source.
/// - `preference` — how they like to work, and what they avoid.
/// - `decision` — what was decided, and why.
/// - `learning` — what experience taught them.
/// - `context` — background on a project, person or topic, true for now.
/// - `episode` — something that happened, at a time.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// `verified` — the person put it here or confirmed it.
    /// `proposed` — an AI suggested it, and nobody has confirmed it yet.
    ///
    /// Never set by the caller: the route decides, because "the person said
    /// so" is exactly the claim that must not be forgeable.
    pub status: String,
    /// Where this came from, in the person's own words.
    #[serde(default)]
    pub source: String,
    /// Records, items or observations in this workspace that back it up.
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    /// When the thing happened, as distinct from when it was written down.
    #[serde(default)]
    pub observed_at: Option<String>,
    /// The window this is true in. A preference from two jobs ago is not
    /// wrong; it is no longer current, and those are different.
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
    /// How sure the proposer was, 0–1. Only a proposal has one: a thing the
    /// person stated is not accompanied by a machine's estimate of it.
    #[serde(default)]
    pub confidence: Option<f64>,
    /// The memory this corrects. Corrections append.
    #[serde(default)]
    pub supersedes_id: Option<String>,
    #[serde(default)]
    pub archived_at: Option<String>,
    /// Kept, but never handed to an AI.
    ///
    /// Some things a person wants their own Basepath to hold and no model to
    /// read. Deleting is the other option and it is theirs too; this is for
    /// what they want to keep.
    #[serde(default)]
    pub excluded_from_retrieval: bool,
    #[serde(default)]
    pub item_ids: Vec<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub people: Vec<String>,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
}

/// One update on how a goal is going.
///
/// Append-only. A check-in that turns out to be wrong is corrected by writing
/// another one that supersedes it, never by editing this: the point of a
/// check-in history is that it says what was believed at the time, and an
/// edited one says only what is believed now.
///
/// The numbers stay their own data. `observation_ids` names the measurements
/// this was written against; it does not copy their values, because a comment
/// and a measurement are different kinds of claim and a copy would let them
/// drift apart.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Checkin {
    pub id: String,
    pub workspace_id: String,
    pub item_id: String,
    /// `on_track` | `at_risk` | `off_track`, or absent when this check-in only
    /// records words.
    #[serde(default)]
    pub health: Option<String>,
    /// The author's own judgement, when they made one.
    #[serde(default)]
    pub self_assessment: Option<f64>,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub results: String,
    #[serde(default)]
    pub blockers: String,
    #[serde(default)]
    pub next_focus: String,
    #[serde(default)]
    pub observation_ids: Vec<String>,
    pub author: String,
    pub created_at: String,
    /// The check-in this corrects.
    #[serde(default)]
    pub supersedes_id: Option<String>,
}

/// Somebody's stated view of how a goal is going.
///
/// Deliberately not derived. Signals — a stale metric, a missed date, nobody
/// checking in — are facts the product can compute and show; deciding that
/// they add up to "at risk" is a judgement, and it carries the name of whoever
/// made it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalHealth {
    /// `on_track` | `at_risk` | `off_track`.
    pub status: String,
    #[serde(default)]
    pub note: String,
    pub set_at: String,
    pub set_by: String,
}

/// Who a goal belongs to.
///
/// `organization` is the workspace itself and carries no id. `team` names a
/// team within it. `person` names a member, and only that member (or an owner)
/// may change their goal — which is the separation between "the company's
/// goal", "our team's goal" and "mine".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalOwner {
    /// `organization` | `team` | `person`.
    pub kind: String,
    #[serde(default)]
    pub id: String,
}

/// A planning period: a quarter, a month, a week, or something the workspace
/// named itself.
///
/// Dates are the workspace's own local dates, inclusive at both ends, because
/// that is how a person says "this quarter". Nothing here is derived from the
/// viewer's device.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Cycle {
    pub id: String,
    pub workspace_id: String,
    /// `quarter` | `month` | `week` | `custom`.
    pub cadence: String,
    pub label: String,
    pub start_date: String,
    pub end_date: String,
    /// `planned` | `active` | `closed`. A closed period keeps its contents;
    /// closing says the work is no longer being planned, not that it is gone.
    pub status: String,
    /// The period this one continues, when it was created as the next one.
    #[serde(default)]
    pub previous_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub id: String,
    pub workspace_id: String,
    pub source_id: String,
    pub target_id: String,
    #[serde(rename = "type")]
    pub relation_type: String,
    #[serde(default)]
    pub rationale: String,
    /// When the link was made.
    ///
    /// Optional because relations recorded before this field existed have no
    /// honest answer. A timeline leaves those out rather than dating them
    /// with the moment it happened to read them.
    #[serde(default)]
    pub created_at: Option<String>,
    pub version: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub workspace_id: String,
    pub item_ids: Vec<String>,
    pub record_type: String,
    pub body: String,
    pub happened_at: String,
    pub created_at: String,
    pub author: String,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub occurrence_key: Option<String>,
    #[serde(default)]
    pub supersedes_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub id: String,
    pub workspace_id: String,
    pub item_id: String,
    pub name: String,
    pub unit: String,
    pub baseline: f64,
    pub target: f64,
    pub direction: String,
    #[serde(default)]
    pub period_start: Option<String>,
    #[serde(default)]
    pub period_end: Option<String>,
    pub version: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub id: String,
    pub workspace_id: String,
    pub metric_id: String,
    pub value: f64,
    pub unit: String,
    pub source: String,
    pub observed_at: String,
    pub created_at: String,
    #[serde(default)]
    pub supersedes_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WeeklyReview {
    pub id: String,
    pub workspace_id: String,
    pub week_start: String,
    pub week_end: String,
    pub status: String,
    #[serde(default)]
    pub learnings: String,
    #[serde(default)]
    pub challenges: String,
    #[serde(default)]
    pub next_focus: String,
    pub version: i64,
    pub revision: i64,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub finalized_at: Option<String>,
    #[serde(default)]
    pub supersedes_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub timezone: String,
    pub role: String,
    pub local: bool,
    #[serde(default = "initial_version")]
    pub version: i64,
}
fn initial_version() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Invitation {
    pub id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub target_actor: String,
    pub role: String,
    pub status: String,
    pub created_by: String,
    pub created_at: String,
    pub expires_at: String,
    pub version: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub details: Value,
    pub request_id: String,
    #[serde(skip)]
    pub status: u16,
}
pub type Result<T> = std::result::Result<T, ApiError>;
impl ApiError {
    pub fn new(status: u16, code: &str, message: &str) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            retryable: status >= 500,
            details: Value::Null,
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    pub fn invalid(message: &str) -> Self {
        Self::new(422, "VALIDATION_ERROR", message)
    }
    pub fn missing() -> Self {
        Self::new(404, "NOT_FOUND", "対象が見つからないか、アクセスできません")
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        Self::invalid(&format!("入力形式が正しくありません: {e}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldReference {
    pub tenant_id: String,
    pub platform_id: String,
    pub external_id: String,
    pub source_updated_at: String,
    pub source_status: String,
}
