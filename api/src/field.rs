//! Narrow, source-backed Field API adapter. No arbitrary upstream paths or service-account fallback.
use crate::{
    auth::validate_url,
    model::{ApiError, Result},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
#[derive(Clone)]
pub struct FieldClient {
    client: Client,
    pub base_url: String,
    pub platform_id: String,
    pub root_operator_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldTenant {
    pub id: String,
    pub name: String,
    pub environment: String,
    pub platform_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldTask {
    pub id: String,
    pub tenant_id: String,
    pub title: String,
    pub status: String,
    pub due_at: Option<String>,
    pub updated_at: String,
}
fn upstream() -> ApiError {
    ApiError::new(
        503,
        "FIELD_UNAVAILABLE",
        "Field APIに接続できません。入力は保持されています",
    )
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
impl FieldClient {
    pub fn from_env() -> Result<Option<Self>> {
        let Some(base) = std::env::var("FIELD_API_URL").ok() else {
            return Ok(None);
        };
        let platform = std::env::var("FIELD_PLATFORM_ID")
            .map_err(|_| ApiError::invalid("FIELD_PLATFORM_ID is required"))?;
        let operator = std::env::var("FIELD_OPERATOR_ID")
            .map_err(|_| ApiError::invalid("FIELD_OPERATOR_ID is required"))?;
        Ok(Some(Self::new(base, platform, operator)?))
    }
    pub fn new(base_url: String, platform_id: String, root_operator_id: String) -> Result<Self> {
        validate_url(&base_url)?;
        if !valid_id(&platform_id) || !valid_id(&root_operator_id) {
            return Err(ApiError::invalid("Invalid Field tenant context"));
        }
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| upstream())?,
            base_url,
            platform_id,
            root_operator_id,
        })
    }
    async fn get(&self, path: &str, token: &str, operator: &str, platform: &str) -> Result<Value> {
        if token.is_empty() {
            return Err(ApiError::new(
                401,
                "UNAUTHENTICATED",
                "Tachyonへのログインが必要です",
            ));
        }
        let r = self
            .client
            .get(format!("{}{}", self.base_url.trim_end_matches('/'), path))
            .bearer_auth(token)
            .header("x-operator-id", operator)
            .header("x-platform-id", platform)
            .send()
            .await
            .map_err(|_| upstream())?;
        match r.status().as_u16() {
            401 => {
                return Err(ApiError::new(
                    401,
                    "UNAUTHENTICATED",
                    "Tachyonの認証期限が切れています",
                ))
            }
            403 => {
                return Err(ApiError::new(
                    403,
                    "FORBIDDEN",
                    "Fieldのこの情報を閲覧する権限がありません",
                ))
            }
            404 => return Err(ApiError::missing()),
            429 => {
                return Err(ApiError::new(
                    429,
                    "RATE_LIMITED",
                    "Field APIが混み合っています",
                ))
            }
            200 => {}
            _ => return Err(upstream()),
        }
        r.json().await.map_err(|_| {
            ApiError::new(
                502,
                "FIELD_INVALID_RESPONSE",
                "Field APIの応答形式を確認してください",
            )
        })
    }
    pub async fn tenants(&self, token: &str) -> Result<Vec<FieldTenant>> {
        // Field currently permits only this action for /get_tenants. Do not invent a PathBase action here.
        let v = self
            .get(
                "/get_tenants?required_action=field%3AViewSalesAnalytics",
                token,
                &self.root_operator_id,
                &self.platform_id,
            )
            .await?;
        let tenants: Vec<FieldTenant> = serde_json::from_value(v).map_err(|_| {
            ApiError::new(
                502,
                "FIELD_INVALID_RESPONSE",
                "テナント一覧の形式が不正です",
            )
        })?;
        if tenants.iter().any(|t| {
            !valid_id(&t.id)
                || !t.id.starts_with("tn_")
                || t.platform_id
                    .as_ref()
                    .is_some_and(|p| p != &self.platform_id)
        }) {
            return Err(ApiError::new(
                502,
                "FIELD_INVALID_RESPONSE",
                "Fieldのテナント境界が一致しません",
            ));
        }
        Ok(tenants)
    }
    async fn authorized_tenant(&self, token: &str, tenant: &str) -> Result<FieldTenant> {
        self.tenants(token)
            .await?
            .into_iter()
            .find(|t| t.id == tenant)
            .ok_or_else(ApiError::missing)
    }
    pub async fn tasks(&self, token: &str, tenant: &str, offset: u32) -> Result<Vec<FieldTask>> {
        self.authorized_tenant(token, tenant).await?;
        let v = self
            .get(
                &format!("/v1/erp/sales-tasks?limit=50&offset={offset}"),
                token,
                tenant,
                &self.platform_id,
            )
            .await?;
        let tasks: Vec<FieldTask> = serde_json::from_value(v["items"].clone()).map_err(|_| {
            ApiError::new(502, "FIELD_INVALID_RESPONSE", "Fieldタスクの形式が不正です")
        })?;
        if tasks.iter().any(|t| t.tenant_id != tenant) {
            return Err(ApiError::new(
                502,
                "FIELD_INVALID_RESPONSE",
                "Fieldタスクのテナントが一致しません",
            ));
        }
        Ok(tasks)
    }
    pub async fn task(&self, token: &str, tenant: &str, id: &str) -> Result<FieldTask> {
        if !valid_id(id) {
            return Err(ApiError::missing());
        }
        self.authorized_tenant(token, tenant).await?;
        let v = self
            .get(
                &format!("/v1/erp/sales-tasks/{id}"),
                token,
                tenant,
                &self.platform_id,
            )
            .await?;
        let task: FieldTask = serde_json::from_value(v).map_err(|_| {
            ApiError::new(502, "FIELD_INVALID_RESPONSE", "Fieldタスクの形式が不正です")
        })?;
        if task.tenant_id != tenant || task.id != id {
            return Err(ApiError::new(
                502,
                "FIELD_INVALID_RESPONSE",
                "Fieldタスクの参照が一致しません",
            ));
        }
        Ok(task)
    }
    pub async fn metrics(&self, token: &str, tenant: &str) -> Result<Value> {
        self.authorized_tenant(token, tenant).await?;
        let data = self
            .get(
                "/v1/erp/sales-contracts/metrics",
                token,
                tenant,
                &self.platform_id,
            )
            .await?;
        if !data.is_object() {
            return Err(ApiError::new(
                502,
                "FIELD_INVALID_RESPONSE",
                "Field指標の形式が不正です",
            ));
        }
        Ok(data)
    }
}
