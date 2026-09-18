//! The hosted MCP endpoint, driven over real HTTP by a real child process.
//!
//! Identity comes from an OAuth access token issued to this deployment's MCP
//! client. There is no shared-secret mode, so this test also covers what an
//! attacker would try: no token, a token for the web sign-in client, a token
//! for another user, a connection that was never approved, one that was
//! revoked, and a scope the person did not grant.
use axum::{extract::State, http::HeaderMap, response::IntoResponse, routing::any, Json, Router};
use pathbase_api::service::{Actor, Service};
use reqwest::{header::HeaderMap as ClientHeaders, Client as HttpClient, StatusCode};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

const API_TOKEN: &str = "local-api-test-token-with-32-characters";
const MCP_CLIENT: &str = "pathbase-mcp-test-client";
const WEB_CLIENT: &str = "pathbase-web-test-client";
const SESSION_KEYS: &str = "dGVzdC1zZXNzaW9uLWtleS0zMi1ieXRlcy1sb25nISE";

/// Mock Cognito pool plus Tachyon `/v1/me`, hosted by the test process.
#[derive(Clone)]
struct Upstream {
    base: String,
}

async fn upstream_handler(
    State(state): State<Upstream>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
) -> axum::response::Response {
    match uri.path() {
        "/.well-known/openid-configuration" => Json(json!({
            "issuer": state.base,
            "authorization_endpoint": format!("{}/authorize", state.base),
            "token_endpoint": format!("{}/token", state.base),
            "jwks_uri": format!("{}/jwks", state.base),
        }))
        .into_response(),
        "/jwks" | "/pool/.well-known/jwks.json" => {
            Json(serde_json::from_str::<Value>(include_str!("fixtures/oidc-jwks.json")).unwrap())
                .into_response()
        }
        // Canonical identity. The mock trusts the token's subject because the
        // API has already verified its signature, issuer and expiry.
        "/v1/me" => {
            let Some(subject) = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .and_then(subject_of)
            else {
                return StatusCode::UNAUTHORIZED.into_response();
            };
            Json(json!({
                "user": {"id": subject, "username": "tester", "name": "Tester", "email": null},
                "tenants": [{"id": "tn_allowed", "name": "Allowed company"}],
            }))
            .into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn subject_of(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    claims["sub"].as_str().map(str::to_owned)
}

fn mint(issuer: &str, subject: &str, client_id: &str, expires_in: i64) -> String {
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("test-key".into());
    let claims = json!({
        "sub": subject,
        "iss": issuer,
        "exp": chrono::Utc::now().timestamp() + expires_in,
        "token_use": "access",
        "client_id": client_id,
    });
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!("fixtures/oidc-test-key.pem"))
        .unwrap();
    jsonwebtoken::encode(&header, &claims, &key).unwrap()
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start_upstream() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = Upstream { base: base.clone() };
    let router = Router::new()
        .fallback(any(upstream_handler))
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    base
}

async fn start_server(db: &Path, upstream: &str) -> (Server, String, String) {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let public = format!("http://127.0.0.1:{port}");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_pathbase-api"))
            // A real deployment mode: the hosted MCP endpoint only exists
            // where Tachyon authentication is configured.
            .env("PATHBASE_MODE", "tachyon")
            .env("DATABASE_URL", db)
            .env("PATHBASE_DB_ENVIRONMENT", "test")
            .env("PATHBASE_API_TOKEN", API_TOKEN)
            .env("PATHBASE_API_PORT", port.to_string())
            .env("PATHBASE_PUBLIC_URL", &public)
            .env("PATHBASE_SESSION_KEYS", SESSION_KEYS)
            .env("TACHYON_OIDC_ISSUER", upstream)
            .env("TACHYON_OIDC_CLIENT_ID", WEB_CLIENT)
            .env(
                "TACHYON_OIDC_REDIRECT_URI",
                format!("{public}/api/auth/callback"),
            )
            .env("TACHYON_API_URL", upstream)
            .env("PATHBASE_COGNITO_ISSUER", format!("{upstream}/pool"))
            .env("PATHBASE_COGNITO_CLIENT_ID", WEB_CLIENT)
            // The MCP endpoint has its own OAuth client, so a browser token is
            // not a token for this resource.
            .env("PATHBASE_MCP_CLIENT_ID", MCP_CLIENT)
            .env("PATHBASE_MCP_RESOURCE", format!("{public}/mcp"))
            .env("PATHBASE_MCP_ALLOWED_HOSTS", "127.0.0.1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let client = HttpClient::new();
    for _ in 0..200 {
        if client
            .get(format!("{public}/health"))
            .send()
            .await
            .is_ok_and(|response| response.status() == StatusCode::OK)
        {
            return (server, format!("{public}/mcp"), public);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("remote MCP child process did not start");
}

async fn post(
    client: &HttpClient,
    url: &str,
    token: &str,
    session: Option<&str>,
    body: Value,
) -> reqwest::Response {
    let mut request = client
        .post(url)
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .json(&body);
    if let Some(session) = session {
        request = request
            .header("mcp-session-id", session)
            .header("mcp-protocol-version", "2025-11-25");
    }
    request.send().await.unwrap()
}

fn json_rpc_from_sse(body: &str) -> Value {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
        .find_map(|data| serde_json::from_str(data).ok())
        .unwrap_or_else(|| panic!("SSE response must contain a JSON-RPC data event: {body:?}"))
}

async fn request(
    client: &HttpClient,
    url: &str,
    token: &str,
    session: Option<&str>,
    id: u64,
    method: &str,
    params: Value,
) -> (Value, ClientHeaders) {
    let response = post(
        client,
        url,
        token,
        session,
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let value = json_rpc_from_sse(&response.text().await.unwrap());
    assert!(value.get("error").is_none(), "{value}");
    (value["result"].clone(), headers)
}

/// Opens an MCP session for a token and returns its session id.
async fn open_session(client: &HttpClient, url: &str, token: &str) -> String {
    let (_, headers) = request(
        client,
        url,
        token,
        None,
        1,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"pathbase-remote-test","version":"1"}}),
    )
    .await;
    let session = headers
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let accepted = post(
        client,
        url,
        token,
        Some(&session),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    session
}

/// Approves a connection the way the PathBase screen does, as the person.
async fn approve(service: &Service, actor: &str, scopes: Value) -> Value {
    let who = Actor {
        id: actor.into(),
        agent: false,
    };
    let connections = service
        .handle(
            &who,
            "GET",
            "/v1/mcp/connections",
            &HashMap::new(),
            json!({}),
            None,
        )
        .await
        .unwrap();
    let connection = connections.as_array().unwrap().first().unwrap().clone();
    service
        .handle(
            &who,
            "POST",
            &format!(
                "/v1/mcp/connections/{}/approve",
                connection["id"].as_str().unwrap()
            ),
            &HashMap::new(),
            json!({"scopes":scopes,"expected_version":connection["version"]}),
            Some(&uuid::Uuid::new_v4().to_string()),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn hosted_mcp_delegates_to_the_person_and_honours_scope_and_disconnect() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("remote-mcp.sqlite3");
    let upstream = start_upstream().await;
    let issuer = format!("{upstream}/pool");
    let (_server, url, public) = start_server(&db, &upstream).await;
    let client = HttpClient::new();
    let service = Service::open(&db.to_string_lossy()).await.unwrap();

    // --- discovery -------------------------------------------------------
    let metadata = client
        .get(format!("{public}/.well-known/oauth-protected-resource/mcp"))
        .send()
        .await
        .unwrap();
    assert_eq!(metadata.status(), StatusCode::OK);
    let metadata: Value = metadata.json().await.unwrap();
    assert_eq!(metadata["resource"], format!("{public}/mcp"));
    assert_eq!(metadata["authorization_servers"][0], issuer);
    assert!(metadata["scopes_supported"]
        .as_array()
        .unwrap()
        .contains(&json!("pathbase.read")));

    // --- no token: 401 pointing at the metadata --------------------------
    let anonymous = client
        .post(&url)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let challenge = anonymous
        .headers()
        .get("www-authenticate")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(challenge.contains("resource_metadata="), "{challenge}");
    assert!(
        challenge.contains("/.well-known/oauth-protected-resource/mcp"),
        "{challenge}"
    );

    // --- a token for the web sign-in client is not a token for this ------
    let browser_token = mint(&issuer, "us_alice", WEB_CLIENT, 3600);
    let wrong_audience = post(
        &client,
        &url,
        &browser_token,
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )
    .await;
    assert_eq!(wrong_audience.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        wrong_audience.json::<Value>().await.unwrap()["error"],
        "INVALID_AUDIENCE"
    );

    // --- an expired token is refused -------------------------------------
    let expired = mint(&issuer, "us_alice", MCP_CLIENT, -3600);
    let refused = post(
        &client,
        &url,
        &expired,
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);

    // --- a valid token still needs the person's approval -----------------
    let alice = mint(&issuer, "us_alice", MCP_CLIENT, 3600);
    let pending = post(
        &client,
        &url,
        &alice,
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )
    .await;
    assert_eq!(pending.status(), StatusCode::FORBIDDEN);
    let body: Value = pending.json().await.unwrap();
    assert_eq!(body["error"], "CONNECTION_APPROVAL_REQUIRED");
    assert!(body["consent_url"].as_str().unwrap().contains("/settings/"));

    // --- approved for reading only ---------------------------------------
    approve(&service, "us_alice", json!(["pathbase.read"])).await;
    let session = open_session(&client, &url, &alice).await;
    let (tools, _) = request(
        &client,
        &url,
        &alice,
        Some(&session),
        2,
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(tools["tools"].as_array().unwrap().len(), 13);

    // Alice has a personal workspace of her own, provisioned on sign-in. She
    // cannot read one she is not a member of.
    let (forbidden, _) = request(
        &client,
        &url,
        &alice,
        Some(&session),
        3,
        "tools/call",
        json!({"name":"pathbase_search_items","arguments":{"workspace_id":"someone-elses-workspace"}}),
    )
    .await;
    assert_eq!(forbidden["isError"], true);

    // Proposing needs a scope she did not grant.
    let (denied, _) = request(
        &client,
        &url,
        &alice,
        Some(&session),
        4,
        "tools/call",
        json!({"name":"pathbase_propose_plan","arguments":{"workspace_id":"personal","idempotency_key":"k1","operations":[]}}),
    )
    .await;
    assert_eq!(denied["isError"], true);
    assert_eq!(denied["structuredContent"]["code"], "INSUFFICIENT_SCOPE");

    // --- disconnecting takes effect immediately --------------------------
    let who = Actor {
        id: "us_alice".into(),
        agent: false,
    };
    let connections = service
        .handle(
            &who,
            "GET",
            "/v1/mcp/connections",
            &HashMap::new(),
            json!({}),
            None,
        )
        .await
        .unwrap();
    let connection_id = connections[0]["id"].as_str().unwrap().to_owned();
    service
        .handle(
            &who,
            "POST",
            &format!("/v1/mcp/connections/{connection_id}/revoke"),
            &HashMap::new(),
            json!({}),
            Some("revoke-1"),
        )
        .await
        .unwrap();
    // The same, still-valid token no longer works, and the existing session
    // does not survive it either.
    let after_revoke = post(
        &client,
        &url,
        &alice,
        Some(&session),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/list","params":{}}),
    )
    .await;
    assert_eq!(after_revoke.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        after_revoke.json::<Value>().await.unwrap()["error"],
        "CONNECTION_REVOKED"
    );

    // --- another user is a separate delegation ---------------------------
    let bob = mint(&issuer, "us_bob", MCP_CLIENT, 3600);
    let bob_pending = post(
        &client,
        &url,
        &bob,
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )
    .await;
    assert_eq!(
        bob_pending.status(),
        StatusCode::FORBIDDEN,
        "Bob does not inherit Alice's approval"
    );
    approve(
        &service,
        "us_bob",
        json!(["pathbase.read", "pathbase.propose"]),
    )
    .await;
    let bob_session = open_session(&client, &url, &bob).await;
    let (bob_context, _) = request(
        &client,
        &url,
        &bob,
        Some(&bob_session),
        6,
        "tools/call",
        json!({"name":"pathbase_get_context","arguments":{}}),
    )
    .await;
    assert_eq!(bob_context["isError"], false);
    assert_eq!(bob_context["structuredContent"]["me"]["id"], "us_bob");
    let bob_workspaces = bob_context["structuredContent"]["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| workspace["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(
        !bob_workspaces.is_empty(),
        "Bob sees his own personal workspace"
    );

    // --- an agent cannot manage its own delegation -----------------------
    let agent = Actor {
        id: "us_bob".into(),
        agent: true,
    };
    let refused = service
        .handle(
            &agent,
            "GET",
            "/v1/mcp/connections",
            &HashMap::new(),
            json!({}),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(refused.status, 404);

    // --- nothing leaked the token ----------------------------------------
    let serialized = serde_json::to_string(&bob_context).unwrap();
    assert!(!serialized.contains(&bob), "a token must never be echoed");
    assert!(!serialized.contains(MCP_CLIENT.trim_start_matches("pathbase-")));
}
