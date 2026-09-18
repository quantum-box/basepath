pub mod auth;
pub mod collaboration;
pub mod db;
pub mod field;
pub mod mcp;
pub mod mcp_auth;
pub mod migrate;
pub mod model;
pub mod openapi;
pub mod preflight;
mod seed;
pub mod service;
pub mod storage;
mod suggestions;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{header, HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use model::{ApiError, FieldReference, Item};
use serde::Deserialize;
use serde_json::{json, Value};
use service::{Actor, Service};
use std::{collections::HashMap, sync::Arc};
#[derive(Clone)]
pub struct HttpState {
    pub service: Service,
    pub token: String,
    pub auth: Option<Arc<auth::TachyonAuth>>,
    pub field: Option<field::FieldClient>,
}
#[derive(Deserialize)]
struct LoginCredentials {
    username: String,
    password: String,
}
#[derive(Deserialize)]
struct TenantSelection {
    tenant_id: String,
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(self),
        )
            .into_response()
    }
}
/// Builds the hosted MCP router when this deployment exposes one.
///
/// It is enabled by `PATHBASE_MCP_CLIENT_ID`, the OAuth client an access token
/// must have been issued to. There is no shared-secret mode: an MCP client
/// authenticates as the person, or not at all.
pub fn remote_mcp_router(
    service: service::Service,
    auth: Option<&Arc<auth::TachyonAuth>>,
) -> Result<Option<Router<HttpState>>, ApiError> {
    let Some(resource) = mcp_auth::ResourceConfig::from_env()? else {
        return Ok(None);
    };
    let auth = auth.ok_or_else(|| {
        ApiError::new(
            500,
            "AUTH_CONFIGURATION",
            "MCPを公開するにはTachyon認証の設定が必要です",
        )
    })?;
    let allowed_hosts = std::env::var("PATHBASE_MCP_ALLOWED_HOSTS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|host| !host.is_empty())
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            url::Url::parse(&resource.resource)
                .ok()
                .and_then(|url| url.host_str().map(String::from))
                .into_iter()
                .collect()
        });
    if allowed_hosts.is_empty() {
        return Err(ApiError::new(
            500,
            "AUTH_CONFIGURATION",
            "PATHBASE_MCP_ALLOWED_HOSTSを設定してください",
        ));
    }
    // Browser origins that may reach the endpoint. Empty disables the check,
    // which is correct for non-browser MCP clients that send no Origin.
    let allowed_origins = std::env::var("PATHBASE_MCP_ALLOWED_ORIGINS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|origin| !origin.is_empty())
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(Some(mcp::remote_router(
        mcp::RemoteMcp {
            service,
            auth: auth.clone(),
            resource: Arc::new(resource),
        },
        allowed_hosts,
        allowed_origins,
    )))
}

pub fn router(state: HttpState) -> Router {
    router_with_mcp(state, None)
}
pub fn router_with_mcp(state: HttpState, mcp: Option<Router<HttpState>>) -> Router {
    // Report what this process actually persists to, so a deploy that silently
    // came up on the local preview database is visible from /health.
    let (storage, durability) = match state.service.db.dialect() {
        db::Dialect::MySql => ("tidb", "shared-durable"),
        db::Dialect::Sqlite => ("sqlite", "ephemeral-runtime"),
    };
    let app: Router<HttpState> = Router::new()
        .route(
            "/health",
            get(move || async move {
                Json(json!({"status":"ok","service":"pathbase-api","storage":storage,"storage_durability":durability}))
            }),
        )
        // Readiness, unlike /health, actually reaches the database. A
        // candidate whose migration did not run, or that was pointed at
        // another deployment's database, must not be promoted.
        .route("/health/ready", get(readiness));
    let app = if let Some(mcp) = mcp {
        app.nest("/mcp", mcp)
    } else {
        app
    };
    app.fallback(endpoint)
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .layer(axum::middleware::from_fn(no_cache))
        .with_state(state)
}
async fn readiness(State(state): State<HttpState>) -> Response {
    let status = match state.service.db.schema_status().await {
        Ok(status) => status,
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "unavailable",
                    "service": "pathbase-api",
                    "reason": "database_unreachable",
                    "message": error.message,
                })),
            )
                .into_response()
        }
    };
    let ready = status.is_ready();
    let reason = if status.schema_version != status.expected_schema_version {
        "schema_out_of_date"
    } else if status.environment_conflicts() {
        "environment_mismatch"
    } else if status.database_environment.is_none() {
        "unclaimed"
    } else {
        "ok"
    };
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({
            "status": if ready { "ready" } else { "unavailable" },
            "service": "pathbase-api",
            "schema": if status.schema_version == status.expected_schema_version { "current" } else { "out_of_date" },
            "storage": status.storage,
            "storage_durability": status.durability,
            "schema_version": status.schema_version,
            "expected_schema_version": status.expected_schema_version,
            "environment": status.environment,
            "database_environment": status.database_environment,
            "reason": reason,
        })),
    )
        .into_response()
}

async fn no_cache(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
async fn endpoint(
    State(state): State<HttpState>,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, ApiError> {
    let path = uri.path().to_owned();
    // RFC 9728 protected resource metadata. It must be reachable without a
    // token: it is how a client discovers where to get one.
    if method == Method::GET && path.starts_with("/.well-known/oauth-protected-resource") {
        let resource = mcp_auth::ResourceConfig::from_env()?.ok_or_else(ApiError::missing)?;
        if path != resource.metadata_path() && path != "/.well-known/oauth-protected-resource" {
            return Err(ApiError::missing());
        }
        return Ok(Json(resource.metadata()).into_response());
    }
    if path == "/auth/status" && method == Method::GET {
        return Ok(Json(json!({"mode":if state.auth.is_some(){"tachyon"}else{"local-preview"},"configured":state.auth.is_some(),"field_configured":state.field.is_some()})).into_response());
    }
    if let Some(auth) = &state.auth {
        if path == "/auth/login" {
            if method != Method::POST {
                return Err(ApiError::new(
                    405,
                    "METHOD_NOT_ALLOWED",
                    "PathBaseのログイン画面を利用してください",
                ));
            }
            require_login_origin(auth, &headers)?;
            let credentials: LoginCredentials = serde_json::from_slice(&bytes).map_err(|_| {
                ApiError::new(400, "INVALID_JSON", "ログイン情報の形式を確認してください")
            })?;
            let cookie = auth
                .direct_login(&credentials.username, &credentials.password)
                .await?;
            return Ok((
                [(header::SET_COOKIE, cookie)],
                Json(json!({"signed_in":true})),
            )
                .into_response());
        }
        if path == "/auth/callback" {
            return Err(ApiError::new(
                410,
                "HOSTED_LOGIN_DISABLED",
                "PathBaseではHosted UIログインを使用しません",
            ));
        }
    }
    let (actor, session) = if let Some(auth) = &state.auth {
        if method != Method::GET
            && headers
                .get("x-pathbase-request")
                .and_then(|v| v.to_str().ok())
                != Some("1")
        {
            return Err(ApiError::new(
                403,
                "CSRF_REJECTED",
                "同一オリジンのアプリから操作してください",
            ));
        }
        if let Some(origin) = headers.get("origin") {
            if origin.to_str().ok()
                != Some(
                    url::Url::parse(&auth.config.public_url)
                        .unwrap()
                        .origin()
                        .ascii_serialization()
                        .as_str(),
                )
            {
                return Err(ApiError::new(
                    403,
                    "ORIGIN_NOT_ALLOWED",
                    "Origin is not allowed",
                ));
            }
        }
        if path == "/auth/logout" && method == Method::POST {
            return Ok((
                [(header::SET_COOKIE, auth.logout(&headers)?)],
                Json(json!({"signed_out":true})),
            )
                .into_response());
        }
        let session = auth.session(&headers).await?;
        let actor = auth::TachyonAuth::actor(&session);
        (actor, Some(session))
    } else {
        let supplied = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if supplied != format!("Bearer {}", state.token) {
            return Err(ApiError::new(401, "UNAUTHENTICATED", "認証が必要です"));
        }
        if headers.contains_key("origin") {
            return Err(ApiError::new(
                403,
                "ORIGIN_NOT_ALLOWED",
                "同一オリジンのアプリから操作してください",
            ));
        }
        (Actor::local(), None)
    };
    if path == "/v1/me" && method == Method::GET {
        return Ok(Json(if let Some(s)=&session{json!({"id":s.identity.id,"name":s.identity.name.as_deref().unwrap_or("あなた"),"email":s.identity.email,"mode":"tachyon"})}else{json!({"id":actor.id,"name":"やまだ はるか","mode":"local-preview"})}).into_response());
    }
    if path == "/v1/tenants" && method == Method::GET {
        let session = session.as_ref().ok_or_else(|| {
            ApiError::new(404, "NOT_FOUND", "Tachyonテナントは設定されていません")
        })?;
        return Ok(Json(json!({
            "tenants": session.identity.tenants,
            "selected_tenant_id": session.selected_tenant,
        }))
        .into_response());
    }
    if path == "/v1/tenant-selection" && method == Method::POST {
        let selection: TenantSelection = serde_json::from_slice(&bytes).map_err(|_| {
            ApiError::new(400, "INVALID_JSON", "テナント選択の形式を確認してください")
        })?;
        let (tenant, cookie) = state
            .auth
            .as_ref()
            .ok_or_else(ApiError::missing)?
            .select_tenant(&headers, &selection.tenant_id)
            .await?;
        return Ok((
            [(header::SET_COOKIE, cookie)],
            Json(json!({"selected_tenant":tenant})),
        )
            .into_response());
    }
    if session
        .as_ref()
        .is_some_and(|session| session.selected_tenant.is_none())
    {
        return Err(ApiError::new(
            428,
            "TENANT_SELECTION_REQUIRED",
            "利用するTachyonテナントを選択してください",
        ));
    }
    if session.is_some() {
        state.service.provision_personal(&actor).await?;
    }
    if path == "/v1/openapi.json" && method == Method::GET {
        return Ok(Json(openapi::document()).into_response());
    }
    let body: Value = if bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::new(400, "INVALID_JSON", "JSONの形式を確認してください"))?
    };
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    if path.starts_with("/v1/integrations/field/") || path.contains("/field/") {
        return field_endpoint(
            &state,
            &actor,
            session.as_ref(),
            (method.as_str(), &path),
            &query,
            body,
            key.as_deref(),
        )
        .await
        .map(|v| Json(v).into_response());
    }
    let method = method.to_string();
    let v = state
        .service
        .handle(&actor, &method, &path, &query, body, key.as_deref())
        .await?;
    Ok(Json(v).into_response())
}
fn require_login_origin(auth: &auth::TachyonAuth, headers: &HeaderMap) -> Result<(), ApiError> {
    if headers
        .get("x-pathbase-request")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        return Err(ApiError::new(
            403,
            "CSRF_REJECTED",
            "同一オリジンのPathBaseからログインしてください",
        ));
    }
    let expected = url::Url::parse(&auth.config.public_url)
        .unwrap()
        .origin()
        .ascii_serialization();
    if headers.get("origin").and_then(|value| value.to_str().ok()) != Some(expected.as_str()) {
        return Err(ApiError::new(
            403,
            "ORIGIN_NOT_ALLOWED",
            "Origin is not allowed",
        ));
    }
    Ok(())
}
async fn field_endpoint(
    state: &HttpState,
    actor: &Actor,
    session: Option<&auth::Session>,
    route: (&str, &str),
    q: &HashMap<String, String>,
    body: Value,
    key: Option<&str>,
) -> model::Result<Value> {
    let (method, path) = route;
    let field = state.field.as_ref().ok_or_else(|| {
        ApiError::new(503, "FIELD_NOT_CONFIGURED", "Field APIの接続設定が必要です")
    })?;
    let session = session.ok_or_else(|| {
        ApiError::new(
            401,
            "UNAUTHENTICATED",
            "Field APIを利用するにはTachyonへのログインが必要です",
        )
    })?;
    let tenant = q
        .get("tenant_id")
        .map(String::as_str)
        .or(body["tenant_id"].as_str())
        .unwrap_or("");
    let selected_tenant = session.selected_tenant.as_deref().ok_or_else(|| {
        ApiError::new(
            428,
            "TENANT_SELECTION_REQUIRED",
            "利用するTachyonテナントを選択してください",
        )
    })?;
    if method == "GET" {
        return match path {
            "/v1/integrations/field/tenants" => storage::value(
                field
                    .tenants(&session.access_token)
                    .await?
                    .into_iter()
                    .filter(|candidate| candidate.id == selected_tenant)
                    .collect::<Vec<_>>(),
            ),
            "/v1/integrations/field/tasks" => {
                require_selected_field_tenant(selected_tenant, tenant)?;
                let offset = q.get("offset").and_then(|s| s.parse().ok()).unwrap_or(0);
                Ok(
                    json!({"items":field.tasks(&session.access_token,tenant,offset).await?,"offset":offset,"limit":50}),
                )
            }
            "/v1/integrations/field/metrics" => {
                require_selected_field_tenant(selected_tenant, tenant)?;
                Ok(
                    json!({"values":field.metrics(&session.access_token,tenant).await?,"observed_at":service::now(),"source":"Field API /v1/erp/sales-contracts/metrics","tenant_id":tenant}),
                )
            }
            _ => Err(ApiError::missing()),
        };
    }
    let p: Vec<_> = path.trim_matches('/').split('/').collect();
    if method != "POST" || p.len() != 5 || p[1] != "workspaces" || p[3] != "field" {
        return Err(ApiError::missing());
    }
    let w = p[2];
    require_selected_field_tenant(selected_tenant, tenant)?;
    {
        let mut tx = state.service.db.begin_read().await?;
        storage::authorize(&mut tx, &actor.id, w, true).await?;
    }
    match p[4] {
        "attach-task" => {
            service::only(&body, &["tenant_id", "task_id"])?;
            let id = body["task_id"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("task_idが必要です"))?;
            let task = field.task(&session.access_token, tenant, id).await?;
            let reference = FieldReference {
                tenant_id: tenant.into(),
                platform_id: field.platform_id.clone(),
                external_id: id.into(),
                source_updated_at: task.updated_at,
                source_status: task.status,
            };
            state.service.handle(actor,"POST",&format!("/v1/workspaces/{w}/items"),&HashMap::new(),json!({"title":task.title,"kind":"action","description":"Fieldの営業タスクを参照する行動。元のタスクの状態はFieldで管理します。","fields":{"field_reference":reference}}),key).await
        }
        "record-metric" => {
            service::only(&body, &["tenant_id", "metric_id", "field_key"])?;
            let metric_id = body["metric_id"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("metric_idが必要です"))?;
            let metric: model::Metric = {
                let mut tx = state.service.db.begin_read().await?;
                storage::get(&mut tx, w, "metrics", metric_id).await?
            };
            let field_key = body["field_key"].as_str().unwrap_or("");
            let unit = match field_key {
                "mrr" | "arr" | "backlogAmount" | "receivableOutstanding" => "円",
                "daysSalesOutstanding" => "日",
                _ => return Err(ApiError::invalid("対応していないField指標です")),
            };
            if metric.unit != unit {
                return Err(ApiError::new(
                    422,
                    "INVALID_UNIT",
                    "Field指標とPathBase指標の単位が一致しません",
                ));
            }
            let values = field.metrics(&session.access_token, tenant).await?;
            let value = values[field_key].as_f64().ok_or_else(|| {
                ApiError::new(
                    422,
                    "UNOBSERVED",
                    "Fieldに観測値がありません。0として記録しません",
                )
            })?;
            state.service.handle_derived(actor,path,&body,model::Operation{method:"POST".into(),path:format!("/v1/workspaces/{w}/observations"),body:json!({"metric_id":metric_id,"value":value,"unit":unit,"source":format!("Field API /v1/erp/sales-contracts/metrics · {tenant} · {field_key}"),"observed_at":service::now()})},key).await
        }
        "refresh-task" => {
            service::only(&body, &["tenant_id", "item_id"])?;
            let item_id = body["item_id"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("item_idが必要です"))?;
            let item: Item = {
                let mut tx = state.service.db.begin_read().await?;
                storage::get(&mut tx, w, "items", item_id).await?
            };
            let reference = item.fields.field_reference.as_ref().ok_or_else(|| {
                ApiError::invalid("Fieldの営業タスクを参照している行動を指定してください")
            })?;
            if reference.tenant_id != tenant || reference.platform_id != field.platform_id {
                return Err(ApiError::new(
                    422,
                    "FIELD_REFERENCE_MISMATCH",
                    "Field参照のテナント境界が一致しません",
                ));
            }
            let task = field
                .task(&session.access_token, tenant, &reference.external_id)
                .await?;
            let refreshed_reference = FieldReference {
                tenant_id: reference.tenant_id.clone(),
                platform_id: reference.platform_id.clone(),
                external_id: reference.external_id.clone(),
                source_updated_at: task.updated_at,
                source_status: task.status,
            };
            state
                .service
                .handle_derived(
                    actor,
                    path,
                    &body,
                    model::Operation {
                        method: "PATCH".into(),
                        path: format!("/v1/workspaces/{w}/items/{item_id}"),
                        body: json!({
                            "expected_version": item.version,
                            "title": task.title,
                            "fields": {"field_reference": refreshed_reference}
                        }),
                    },
                    key,
                )
                .await
        }
        _ => Err(ApiError::missing()),
    }
}

fn require_selected_field_tenant(selected: &str, requested: &str) -> model::Result<()> {
    if requested.is_empty() {
        return Err(ApiError::invalid("tenant_idが必要です"));
    }
    if requested != selected {
        return Err(ApiError::new(
            403,
            "FIELD_TENANT_MISMATCH",
            "選択中のTachyonテナント以外のFieldデータにはアクセスできません",
        ));
    }
    Ok(())
}
