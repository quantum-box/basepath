use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Json, Router,
};
use pathbase_api::{
    auth::{AuthConfig, TachyonAuth},
    field::FieldClient,
    preflight,
    service::Service,
    HttpState,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
#[derive(Clone)]
struct MockState {
    base: String,
    nonce: Arc<Mutex<String>>,
    audience: Arc<Mutex<String>>,
    calls: Arc<Mutex<Vec<String>>>,
    field_status: Arc<Mutex<u16>>,
    expires: Arc<Mutex<i64>>,
    token_lifetime: Arc<Mutex<i64>>,
}
async fn mock(
    State(s): State<MockState>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    body: String,
) -> Response {
    let path = uri.path();
    s.calls.lock().unwrap().push(path.into());
    match path {
        "/.well-known/openid-configuration"=>Json(json!({"issuer":s.base,"authorization_endpoint":format!("{}/authorize",s.base),"token_endpoint":format!("{}/token",s.base),"jwks_uri":format!("{}/jwks",s.base)})).into_response(),
        "/jwks"=>Json(serde_json::from_str::<Value>(include_str!("fixtures/oidc-jwks.json")).unwrap()).into_response(),
        "/oauth2/login"=> {
            assert_eq!(method, Method::POST);
            let credentials=serde_json::from_str::<Value>(&body).unwrap();
            assert_eq!(credentials["username"],"test-user");
            assert_eq!(credentials["password"],"test-password");
            assert_eq!(credentials["client_id"],"pathbase-test");
            Json(json!({"status":"authenticated","session_token":"test-session-token","user_id":"us_verified"})).into_response()
        },
        "/authorize"=> {
            assert_eq!(method, Method::POST);
            assert_eq!(headers.get("authorization").unwrap(),"Bearer test-session-token");
            let request=serde_json::from_str::<Value>(&body).unwrap();
            assert_eq!(request["code_challenge_method"],"S256");
            assert!(request["code_challenge"].as_str().unwrap().len()>32);
            *s.nonce.lock().unwrap()=request["nonce"].as_str().unwrap().into();
            Json(json!({"authorization_code":"code","redirect_uri":request["redirect_uri"],"state":request["state"]})).into_response()
        },
        "/token"=> {
            assert!(body.contains("code_verifier=") || body.contains("refresh_token="));
            let mut header=jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);header.kid=Some("test-key".into());
            let claims=json!({"sub":"upstream-subject","aud":s.audience.lock().unwrap().clone(),"iss":s.base,"exp":*s.expires.lock().unwrap(),"nonce":s.nonce.lock().unwrap().clone()});
            let key=jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!("fixtures/oidc-test-key.pem")).unwrap();let id_token=jsonwebtoken::encode(&header,&claims,&key).unwrap();
            Json(json!({"access_token":"test-access-token","refresh_token":"test-refresh-token","id_token":id_token,"expires_in":*s.token_lifetime.lock().unwrap()})).into_response()
        },
        "/auth/v1beta/verify"=> {
            if headers.get("authorization").and_then(|value| value.to_str().ok()) != Some("Bearer test-access-token") {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            assert!(headers.get("x-user-id").is_none());
            assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["token"],"test-access-token");
            Json(json!({"user":{"id":"us_verified","name":"Verified user","email":null,"tenants":["do-not-trust-callback-memberships"]}})).into_response()
        },
        "/v1/me"=> {
            assert_eq!(method, Method::GET);
            if headers.get("authorization").and_then(|value| value.to_str().ok()) != Some("Bearer test-access-token") {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            assert!(headers.get("x-user-id").is_none());
            Json(json!({
                "user":{"id":"us_verified","username":"verified-user","name":"Verified user","email":null},
                "tenants":[
                    {"id":"tn_allowed","name":"Allowed company"},
                    {"id":"tn_other","name":"Other company"}
                ]
            })).into_response()
        },
        "/get_tenants"=> {
            assert_eq!(method, Method::POST);
            assert_eq!(headers.get("x-platform-id").unwrap(),"tn_platform");assert_eq!(headers.get("x-operator-id").unwrap(),"tn_root");assert!(uri.query().unwrap().contains("field%3AViewSalesAnalytics"));
            if headers.get("authorization").is_none() {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            assert_eq!(headers.get("authorization").unwrap(),"Bearer test-access-token");
            Json(json!([{"id":"tn_allowed","name":"Allowed company","environment":"sandbox","platformId":"tn_platform"}])).into_response()
        },
        "/v1/erp/sales-tasks"=> {
            assert_eq!(headers.get("x-operator-id").unwrap(),"tn_allowed");assert!(headers.get("x-user-id").is_none());
            let status=StatusCode::from_u16(*s.field_status.lock().unwrap()).unwrap();
            (status,Json(json!({"items":[{"id":"task_1","tenantId":"tn_allowed","title":"Follow up","status":"open","dueAt":null,"updatedAt":"2026-09-12T00:00:00Z"}]}))).into_response()
        },
        "/v1/erp/sales-tasks/task_1"=> {
            assert_eq!(headers.get("x-operator-id").unwrap(),"tn_allowed");
            Json(json!({"id":"task_1","tenantId":"tn_allowed","title":"Follow up","status":"open","dueAt":null,"updatedAt":"2026-09-12T00:00:00Z"})).into_response()
        },
        "/v1/erp/sales-contracts/metrics"=> {
            assert_eq!(headers.get("authorization").unwrap(),"Bearer test-access-token");
            assert_eq!(headers.get("x-operator-id").unwrap(),"tn_allowed");
            Json(json!({"mrr":50000,"arr":600000,"backlogAmount":100000,"receivableOutstanding":0,"daysSalesOutstanding":null})).into_response()
        },
        _=>StatusCode::NOT_FOUND.into_response()
    }
}
async fn upstream() -> (MockState, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let s = MockState {
        base,
        nonce: Default::default(),
        audience: Arc::new(Mutex::new("pathbase-test".into())),
        calls: Default::default(),
        field_status: Arc::new(Mutex::new(200)),
        expires: Arc::new(Mutex::new(chrono::Utc::now().timestamp() + 3600)),
        token_lifetime: Arc::new(Mutex::new(3600)),
    };
    let router = Router::new().fallback(mock).with_state(s.clone());
    let handle = tokio::spawn(async {
        axum::serve(listener, router).await.unwrap();
    });
    (s, handle)
}
async fn auth(s: &MockState) -> TachyonAuth {
    TachyonAuth::new(AuthConfig {
        issuer: s.base.clone(),
        client_id: "pathbase-test".into(),
        client_secret: None,
        redirect_uri: "http://localhost:1420/api/auth/callback".into(),
        public_url: "http://localhost:1420".into(),
        tachyon_api_url: s.base.clone(),
    })
    .await
    .unwrap()
}

#[test]
fn runtime_auth_uses_tachyon_oauth_routes_without_discovery() {
    let auth = TachyonAuth::for_runtime(AuthConfig {
        issuer: "https://api.example.com".into(),
        client_id: "pathbase-test".into(),
        client_secret: None,
        redirect_uri: "https://pathbase.example.com/api/auth/callback".into(),
        public_url: "https://pathbase.example.com".into(),
        tachyon_api_url: "https://api.example.com".into(),
    })
    .unwrap();

    let (login_url, _) = auth.begin().unwrap();
    let login_url = url::Url::parse(&login_url).unwrap();
    assert_eq!(
        login_url.origin().ascii_serialization(),
        "https://api.example.com"
    );
    assert_eq!(login_url.path(), "/oauth2/authorize");
}

#[tokio::test]
async fn auth_status_uses_the_path_below_the_cloudapp_api_mount() {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("db")).unwrap();
    let auth = TachyonAuth::for_runtime(AuthConfig {
        issuer: "https://api.example.com".into(),
        client_id: "pathbase-test".into(),
        client_secret: None,
        redirect_uri: "https://pathbase.example.com/api/auth/callback".into(),
        public_url: "https://pathbase.example.com".into(),
        tachyon_api_url: "https://api.example.com".into(),
    })
    .unwrap();
    let app = Router::new().nest(
        "/api",
        pathbase_api::router(HttpState {
            service,
            token: String::new(),
            auth: Some(Arc::new(auth)),
            field: None,
        }),
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/auth/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["mode"], "tachyon");
    assert_eq!(body["configured"], true);
}

#[tokio::test]
async fn preflight_checks_configuration_and_unauthenticated_boundaries() {
    let (s, server) = upstream().await;
    let report = preflight::run(
        "tachyon".into(),
        Ok(AuthConfig {
            issuer: s.base.clone(),
            client_id: "pathbase-test".into(),
            client_secret: None,
            redirect_uri: "http://localhost:1420/api/auth/callback".into(),
            public_url: "http://localhost:1420".into(),
            tachyon_api_url: s.base.clone(),
        }),
        false,
        Ok(Some(
            FieldClient::new(s.base.clone(), "tn_platform".into(), "tn_root".into()).unwrap(),
        )),
    )
    .await;
    assert!(report.configuration_ready);
    assert!(report.requires_authenticated_check);
    assert!(report.checks.iter().all(|check| check.status != "error"));
    let calls = s.calls.lock().unwrap();
    assert!(calls
        .iter()
        .any(|path| path == "/.well-known/openid-configuration"));
    assert!(calls.iter().any(|path| path == "/auth/v1beta/verify"));
    assert!(calls.iter().any(|path| path == "/get_tenants"));
    server.abort();
}
fn login(a: &TachyonAuth, s: &MockState) -> (HeaderMap, String) {
    let (url, cookie) = a.begin().unwrap();
    let u = url::Url::parse(&url).unwrap();
    let q: HashMap<_, _> = u.query_pairs().into_owned().collect();
    assert_eq!(q["code_challenge_method"], "S256");
    assert!(q["code_challenge"].len() > 32);
    *s.nonce.lock().unwrap() = q["nonce"].clone();
    let mut headers = HeaderMap::new();
    headers.insert("cookie", cookie.split(';').next().unwrap().parse().unwrap());
    (headers, q["state"].clone())
}
#[tokio::test]
async fn encrypted_session_survives_auth_instance_replacement() {
    let (s, server) = upstream().await;
    let config = AuthConfig {
        issuer: s.base.clone(),
        client_id: "pathbase-test".into(),
        client_secret: None,
        redirect_uri: "http://localhost:1420/api/auth/callback".into(),
        public_url: "http://localhost:1420".into(),
        tachyon_api_url: s.base.clone(),
    };
    let key = [7_u8; 32];
    let a = TachyonAuth::new_with_session_keys(config.clone(), vec![key])
        .await
        .unwrap();
    let cookie = a.direct_login("test-user", "test-password").await.unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("cookie", cookie.split(';').next().unwrap().parse().unwrap());
    drop(a);
    let replacement = TachyonAuth::new_with_session_keys(config, vec![key])
        .await
        .unwrap();
    assert_eq!(
        replacement.session(&headers).await.unwrap().identity.id,
        "us_verified"
    );
    let wrong_key = TachyonAuth::new_with_session_keys(
        AuthConfig {
            issuer: s.base.clone(),
            client_id: "pathbase-test".into(),
            client_secret: None,
            redirect_uri: "http://localhost:1420/api/auth/callback".into(),
            public_url: "http://localhost:1420".into(),
            tachyon_api_url: s.base.clone(),
        },
        vec![[8_u8; 32]],
    )
    .await
    .unwrap();
    assert_eq!(wrong_key.session(&headers).await.err().unwrap().status, 401);
    server.abort();
}
#[tokio::test]
async fn field_references_and_observations_are_idempotent_and_preserve_missing_values() {
    let (s, server) = upstream().await;
    let a = auth(&s).await;
    let cookie = a.direct_login("test-user", "test-password").await.unwrap();
    let mut session_headers = HeaderMap::new();
    session_headers.insert("cookie", cookie.split(';').next().unwrap().parse().unwrap());
    let (_, selected_cookie) = a
        .select_tenant(&session_headers, "tn_allowed")
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("db")).unwrap();
    let actor = pathbase_api::service::Actor {
        id: "us_verified".into(),
        agent: false,
    };
    service.provision_personal(&actor).unwrap();
    let ws = service
        .handle(
            &actor,
            "GET",
            "/v1/workspaces",
            &HashMap::new(),
            json!({}),
            None,
        )
        .unwrap();
    let w = ws[0]["id"].as_str().unwrap();
    let outcome = service
        .handle(
            &actor,
            "POST",
            &format!("/v1/workspaces/{w}/items"),
            &HashMap::new(),
            json!({"title":"Sales outcome","kind":"outcome"}),
            Some("item"),
        )
        .unwrap();
    let mut metric_ids = vec![];
    for (n, unit) in ["円", "日"].iter().enumerate() {
        let metric=service.handle(&actor,"POST",&format!("/v1/workspaces/{w}/metrics"),&HashMap::new(),json!({"item_id":outcome["id"],"name":"Metric","unit":unit,"baseline":0,"target":100000,"direction":"increase"}),Some(&format!("metric-{n}"))).unwrap();
        metric_ids.push(metric["id"].clone());
    }
    let app = pathbase_api::router(HttpState {
        service: service.clone(),
        token: "".into(),
        auth: Some(Arc::new(a)),
        field: Some(
            FieldClient::new(s.base.clone(), "tn_platform".into(), "tn_root".into()).unwrap(),
        ),
    });
    let request = |suffix: &str, key: &str, body: Value| {
        Request::builder()
            .method("POST")
            .uri(format!("/v1/workspaces/{w}/field/{suffix}"))
            .header("cookie", selected_cookie.split(';').next().unwrap())
            .header("x-pathbase-request", "1")
            .header("idempotency-key", key)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let mut attached = vec![];
    for key in ["attach1", "attach2"] {
        let response = app
            .clone()
            .oneshot(request(
                "attach-task",
                key,
                json!({"tenant_id":"tn_allowed","task_id":"task_1"}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let bytes = axum::body::to_bytes(response.into_body(), 10000)
            .await
            .unwrap();
        attached.push(serde_json::from_slice::<Value>(&bytes).unwrap());
    }
    assert_eq!(attached[0]["id"], attached[1]["id"]);
    let mut observed = vec![];
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(request(
                "record-metric",
                "same-observation",
                json!({"tenant_id":"tn_allowed","metric_id":metric_ids[0],"field_key":"mrr"}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let bytes = axum::body::to_bytes(response.into_body(), 10000)
            .await
            .unwrap();
        observed.push(serde_json::from_slice::<Value>(&bytes).unwrap());
    }
    assert_eq!(observed[0], observed[1]);
    assert_eq!(observed[0]["value"].as_f64(), Some(50000.0));
    let response=app.oneshot(request("record-metric","missing",json!({"tenant_id":"tn_allowed","metric_id":metric_ids[1],"field_key":"daysSalesOutstanding"}))).await.unwrap();
    assert_eq!(response.status(), 422);
    let snapshot = service
        .handle(
            &actor,
            "GET",
            &format!("/v1/workspaces/{w}/snapshot"),
            &HashMap::new(),
            json!({}),
            None,
        )
        .unwrap();
    assert_eq!(snapshot["observations"].as_array().unwrap().len(), 1);
    assert!(!s
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.ends_with("/complete")));
    server.abort();
}
#[tokio::test]
async fn direct_login_uses_tachyon_pkce_without_hosted_ui() {
    let (s, server) = upstream().await;
    let a = auth(&s).await;
    let cookie = a.direct_login("test-user", "test-password").await.unwrap();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(!s.calls.lock().unwrap().iter().any(|p| p == "/get_tenants"));
    assert!(s.calls.lock().unwrap().iter().any(|p| p == "/oauth2/login"));
    assert!(s.calls.lock().unwrap().iter().any(|p| p == "/authorize"));
    let mut h = HeaderMap::new();
    h.insert("cookie", cookie.split(';').next().unwrap().parse().unwrap());
    let session = a.session(&h).await.unwrap();
    assert_eq!(session.identity.id, "us_verified");
    assert!(a.logout(&h).unwrap().contains("Max-Age=0"));
    server.abort();
}
#[tokio::test]
async fn oidc_rejects_state_nonce_audience_and_expiry() {
    let (s, server) = upstream().await;
    let a = auth(&s).await;
    let (_, state) = login(&a, &s);
    assert_eq!(
        a.callback(&HeaderMap::new(), &state, "code")
            .await
            .unwrap_err()
            .status,
        401
    );
    let (h, state) = login(&a, &s);
    *s.nonce.lock().unwrap() = "wrong".into();
    assert_eq!(
        a.callback(&h, &state, "code").await.unwrap_err().status,
        401
    );
    let (h, state) = login(&a, &s);
    *s.audience.lock().unwrap() = "other-client".into();
    assert_eq!(
        a.callback(&h, &state, "code").await.unwrap_err().status,
        401
    );
    *s.audience.lock().unwrap() = "pathbase-test".into();
    let (h, state) = login(&a, &s);
    *s.expires.lock().unwrap() = chrono::Utc::now().timestamp() - 3600;
    assert_eq!(
        a.callback(&h, &state, "code").await.unwrap_err().status,
        401
    );
    server.abort();
}
#[tokio::test]
async fn field_delegates_tenant_and_denies_cross_tenant_without_fetching_content() {
    let (s, server) = upstream().await;
    let field = FieldClient::new(s.base.clone(), "tn_platform".into(), "tn_root".into()).unwrap();
    let tasks = field
        .tasks("test-access-token", "tn_allowed", 0)
        .await
        .unwrap();
    assert_eq!(tasks[0].title, "Follow up");
    let n = s
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|p| *p == "/v1/erp/sales-tasks")
        .count();
    assert_eq!(
        field
            .tasks("test-access-token", "tn_other", 0)
            .await
            .unwrap_err()
            .status,
        404
    );
    assert_eq!(
        s.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == "/v1/erp/sales-tasks")
            .count(),
        n
    );
    *s.field_status.lock().unwrap() = 403;
    assert_eq!(
        field
            .tasks("test-access-token", "tn_allowed", 0)
            .await
            .unwrap_err()
            .status,
        403
    );
    *s.field_status.lock().unwrap() = 401;
    assert_eq!(
        field
            .tasks("test-access-token", "tn_allowed", 0)
            .await
            .unwrap_err()
            .status,
        401
    );
    server.abort();
}
#[tokio::test]
async fn authenticated_api_uses_verified_identity_and_never_local_owner() {
    let (s, server) = upstream().await;
    let a = auth(&s).await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("db")).unwrap();
    service.initialize(true).unwrap();
    let app = pathbase_api::router(HttpState {
        service: service.clone(),
        token: "preview-key".into(),
        auth: Some(Arc::new(a)),
        field: None,
    });
    let hosted_ui = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/callback?code=obsolete&state=obsolete")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hosted_ui.status(), 410);
    let wrong_origin = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .header("x-pathbase-request", "1")
                .header("origin", "https://example.invalid")
                .body(Body::from(
                    json!({"username":"test-user","password":"test-password"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_origin.status(), 403);
    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .header("x-pathbase-request", "1")
                .header("origin", "http://localhost:1420")
                .body(Body::from(
                    json!({"username":"test-user","password":"test-password"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let cookie = login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/workspaces")
                .header("authorization", "Bearer preview-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), 401);
    let selection_required = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/workspaces")
                .header("cookie", cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(selection_required.status(), 428);
    let available = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/tenants")
                .header("cookie", cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(available.status(), 200);
    let body = axum::body::to_bytes(available.into_body(), 10000)
        .await
        .unwrap();
    let available: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(available["tenants"].as_array().unwrap().len(), 2);
    assert!(available["selected_tenant_id"].is_null());
    let forbidden_tenant = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/tenant-selection")
                .header("cookie", cookie.split(';').next().unwrap())
                .header("content-type", "application/json")
                .header("x-pathbase-request", "1")
                .header("origin", "http://localhost:1420")
                .body(Body::from(json!({"tenant_id":"tn_unknown"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_tenant.status(), 403);
    let selected = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/tenant-selection")
                .header("cookie", cookie.split(';').next().unwrap())
                .header("content-type", "application/json")
                .header("x-pathbase-request", "1")
                .header("origin", "http://localhost:1420")
                .body(Body::from(json!({"tenant_id":"tn_allowed"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(selected.status(), 200);
    let selected_cookie = selected
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/workspaces")
                .header("cookie", selected_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), 10000)
        .await
        .unwrap();
    let ws: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ws.as_array().unwrap().len(), 1);
    assert!(ws[0]["id"].as_str().unwrap().starts_with("personal-"));
    assert_ne!(ws[0]["id"], "personal");
    let denied = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/workspaces/personal/items")
                .header("cookie", cookie.split(';').next().unwrap())
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    server.abort();
}
#[tokio::test]
async fn local_http_contract_auth_json_paging_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("db")).unwrap();
    service.initialize(false).unwrap();
    let app = pathbase_api::router(HttpState {
        service,
        token: "test-only-token".into(),
        auth: None,
        field: None,
    });
    let request = |method: &str, path: &str, body: &str| {
        Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", "Bearer test-only-token")
            .header("idempotency-key", "unique")
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let bad = app
        .clone()
        .oneshot(request("POST", "/v1/workspaces/personal/items", "{"))
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
    let ok = app
        .clone()
        .oneshot(request(
            "POST",
            "/v1/workspaces/personal/items",
            r#"{"title":"A"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    let response = app
        .clone()
        .oneshot(request("GET", "/v1/workspaces/personal/items?limit=1", ""))
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), 10000)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["items"][0]["title"],
        "A"
    );
    let openapi = app
        .oneshot(request("GET", "/v1/openapi.json", ""))
        .await
        .unwrap();
    assert_eq!(openapi.status(), 200);
}
