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
    pub icon: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub memo: String,
    #[serde(default)]
    pub next_action_id: Option<String>,
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
impl From<rusqlite::Error> for ApiError {
    fn from(_: rusqlite::Error) -> Self {
        Self::new(
            500,
            "STORAGE_ERROR",
            "保存処理に失敗しました。再試行してください",
        )
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
