pub mod auth;
pub mod collaboration;
pub mod field;
pub mod mcp;
pub mod model;
pub mod openapi;
pub mod preflight;
mod seed;
pub mod service;
pub mod storage;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, OriginalUri, Query, State},
    http::{header, HeaderMap, Method, StatusCode},
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
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(self),
        )
            .into_response()
    }
}
pub fn router(state: HttpState) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async {
                Json(json!({"status":"ok","service":"pathbase-api","storage":"sqlite"}))
            }),
        )
        .fallback(endpoint)
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .layer(axum::middleware::from_fn(no_cache))
        .with_state(state)
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
    OriginalUri(uri): OriginalUri,
    Query(query): Query<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, ApiError> {
    let path = uri.path().to_owned();
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
        state.service.provision_personal(&actor)?;
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
    let v = tokio::task::spawn_blocking(move || {
        state
            .service
            .handle(&actor, &method, &path, &query, body, key.as_deref())
    })
    .await
    .map_err(|_| ApiError::new(500, "INTERNAL_ERROR", "処理に失敗しました"))??;
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
    if method == "GET" {
        return match path {
            "/v1/integrations/field/tenants" => {
                storage::value(field.tenants(&session.access_token).await?)
            }
            "/v1/integrations/field/tasks" => {
                let offset = q.get("offset").and_then(|s| s.parse().ok()).unwrap_or(0);
                Ok(
                    json!({"items":field.tasks(&session.access_token,tenant,offset).await?,"offset":offset,"limit":50}),
                )
            }
            "/v1/integrations/field/metrics" => Ok(
                json!({"values":field.metrics(&session.access_token,tenant).await?,"observed_at":service::now(),"source":"Field API /v1/erp/sales-contracts/metrics","tenant_id":tenant}),
            ),
            _ => Err(ApiError::missing()),
        };
    }
    let p: Vec<_> = path.trim_matches('/').split('/').collect();
    if method != "POST" || p.len() != 5 || p[1] != "workspaces" || p[3] != "field" {
        return Err(ApiError::missing());
    }
    let w = p[2];
    {
        let db = state
            .service
            .db
            .lock()
            .map_err(|_| ApiError::new(500, "STORAGE_ERROR", "Storage unavailable"))?;
        storage::authorize(&db, &actor.id, w, true)?;
    }
    match p[4] {
        "attach-task" => {
            let id = body["task_id"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("task_idが必要です"))?;
            let task = field.task(&session.access_token, tenant, id).await?;
            {
                let db = state
                    .service
                    .db
                    .lock()
                    .map_err(|_| ApiError::new(500, "STORAGE_ERROR", "Storage unavailable"))?;
                if let Some(existing) =
                    storage::list::<Item>(&db, w, "items")?
                        .into_iter()
                        .find(|i| {
                            i.fields.field_reference.as_ref().is_some_and(|r| {
                                r.tenant_id == tenant
                                    && r.external_id == id
                                    && r.platform_id == field.platform_id
                            })
                        })
                {
                    return storage::value(existing);
                }
            }
            let reference = FieldReference {
                tenant_id: tenant.into(),
                platform_id: field.platform_id.clone(),
                external_id: id.into(),
                source_updated_at: task.updated_at,
                source_status: task.status,
            };
            state.service.handle(actor,"POST",&format!("/v1/workspaces/{w}/items"),&HashMap::new(),json!({"title":task.title,"kind":"action","description":"Fieldの営業タスクを参照する行動。元のタスクの状態はFieldで管理します。","fields":{"field_reference":reference}}),key)
        }
        "record-metric" => {
            let metric_id = body["metric_id"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("metric_idが必要です"))?;
            let metric: model::Metric = {
                let db = state
                    .service
                    .db
                    .lock()
                    .map_err(|_| ApiError::new(500, "STORAGE_ERROR", "Storage unavailable"))?;
                storage::get(&db, w, "metrics", metric_id)?
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
            state.service.handle_derived(actor,path,&body,model::Operation{method:"POST".into(),path:format!("/v1/workspaces/{w}/observations"),body:json!({"metric_id":metric_id,"value":value,"unit":unit,"source":format!("Field API /v1/erp/sales-contracts/metrics · {tenant} · {field_key}"),"observed_at":service::now()})},key)
        }
        _ => Err(ApiError::missing()),
    }
}
