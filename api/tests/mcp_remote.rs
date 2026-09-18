//! The hosted MCP endpoint, driven over real HTTP by a real child process.
//!
//! Identity comes from an OAuth access token Basepath issued after the person
//! consented on Basepath's own origin. There is no shared-secret mode, so this
//! test also covers what an attacker would try: no token, a token minted by
//! the identity provider for the browser, a token for another user, a
//! connection that was revoked, and a scope the person did not grant.
//!
//! The consent step runs in process, against the same database file, because
//! it is a signed-in person clicking in Basepath — there is no HTTP request an
//! AI client could make that performs it. Everything a client actually does —
//! registration, the token exchange, and every MCP call — goes over HTTP.
use axum::{extract::State, http::HeaderMap, response::IntoResponse, routing::any, Json, Router};
use pathbase_api::{
    mcp_auth::ResourceConfig,
    oauth,
    service::{Actor, Service},
};
use reqwest::{header::HeaderMap as ClientHeaders, Client as HttpClient, StatusCode};
use serde_json::{json, Value};
use sha2::Digest;
use std::{
    collections::HashMap,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

const API_TOKEN: &str = "local-api-test-token-with-32-characters";
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

/// Starts one API process.
///
/// `canonical` names the MCP resource this process serves. Two processes of
/// one deployment share it — that is what makes them the same resource to a
/// client — while a test that starts a single process lets it derive its own.
async fn start_server(
    db: &Path,
    upstream: &str,
    canonical: Option<&str>,
) -> (Server, String, String) {
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
            // Basepath is the authorization server for its own MCP resource.
            .env("PATHBASE_MCP_ENABLED", "1")
            // Run directly, with no Worker in front, so the API's public base
            // is the origin rather than `{origin}/api`.
            .env("PATHBASE_API_BASE_URL", &public)
            .env(
                "PATHBASE_MCP_RESOURCE",
                canonical
                    .map(String::from)
                    .unwrap_or(format!("{public}/mcp")),
            )
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

/// The endpoint answers with plain JSON, not SSE framing: there is no session
/// to stream into.
fn json_rpc(body: &str) -> Value {
    serde_json::from_str(body)
        .unwrap_or_else(|_| panic!("expected a JSON-RPC response body: {body:?}"))
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
    let value = json_rpc(&response.text().await.unwrap());
    assert!(value.get("error").is_none(), "{value}");
    (value["result"].clone(), headers)
}

/// Runs `initialize` and asserts the endpoint advertises no session.
///
/// Consecutive requests reach different Lambda execution environments, so
/// there is nothing to pin a session to. The client simply sends every request
/// on its own.
async fn initialize(client: &HttpClient, url: &str, token: &str) {
    let (result, headers) = request(
        client,
        url,
        token,
        None,
        1,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"pathbase-remote-test","version":"1"}}),
    )
    .await;
    assert!(result["capabilities"]["tools"].is_object());
    assert!(
        headers.get("mcp-session-id").is_none(),
        "a stateless endpoint must not hand out a session id"
    );
}

/// Completes the OAuth flow the way a host does, and returns the access token.
///
/// Registration and the token exchange are real HTTP calls, because that is
/// what a host performs. The consent in between is in process against the same
/// database: it is a signed-in person clicking in Basepath, and there is
/// deliberately no request an AI client can make that stands in for it.
async fn connect(
    client: &HttpClient,
    service: &Service,
    public: &str,
    actor: &str,
    scopes: &[&str],
) -> String {
    let redirect = "http://127.0.0.1:8123/callback";
    let registered: Value = client
        .post(format!("{public}/oauth/register"))
        .json(&json!({"client_name":"Remote test host","redirect_uris":[redirect]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let client_id = registered["client_id"].as_str().unwrap().to_owned();

    use base64::Engine;
    let verifier = format!("{actor}-verifier-{actor}-verifier-0123456789abcdef");
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    let resource = ResourceConfig {
        resource: format!("{public}/mcp"),
        issuer: public.to_owned(),
        api_base: public.to_owned(),
        consent_url: format!("{public}/settings/connections"),
    };
    let mut query = HashMap::new();
    for (key, value) in [
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("scope", &scopes.join(" ")),
        ("state", "remote-test"),
    ] {
        query.insert(key.to_string(), value.to_string());
    }
    let asked = oauth::begin_authorization(&service.db, &resource, public, &query)
        .await
        .unwrap();
    let oauth::Authorization::Ask(_, handle) = asked else {
        panic!("the authorization request should have reached the person");
    };
    let decided = oauth::decide(
        service,
        &Actor {
            id: actor.into(),
            agent: false,
            connection: None,
        },
        public,
        &handle,
        &scopes
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>(),
        true,
    )
    .await
    .unwrap();
    let code = url::Url::parse(decided["redirect_to"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .unwrap();

    let issued = client
        .post(format!("{public}/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("redirect_uri", redirect),
            ("client_id", client_id.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(issued.status(), StatusCode::OK);
    // A token response must never be cached by anything in between.
    assert_eq!(issued.headers().get("cache-control").unwrap(), "no-store");
    let issued: Value = issued.json().await.unwrap();
    assert_eq!(issued["scope"], scopes.join(" "));
    issued["access_token"].as_str().unwrap().to_owned()
}

/// The connection id of the one delegation this person holds.
async fn connection_of(service: &Service, actor: &str) -> Value {
    let who = Actor {
        id: actor.into(),
        agent: false,
        connection: None,
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
    connections.as_array().unwrap().first().unwrap().clone()
}

#[tokio::test]
async fn hosted_mcp_delegates_to_the_person_and_honours_scope_and_disconnect() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("remote-mcp.sqlite3");
    let upstream = start_upstream().await;
    let issuer = format!("{upstream}/pool");
    let (_server, url, public) = start_server(&db, &upstream, None).await;
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
    // The authorization server is Basepath itself, not the identity provider:
    // the pool advertises no PKCE method and offers no registration endpoint,
    // so a host that follows the specification could not use it.
    assert_eq!(metadata["authorization_servers"][0], public);
    assert!(metadata["scopes_supported"]
        .as_array()
        .unwrap()
        .contains(&json!("pathbase.read")));

    let server_metadata: Value = client
        .get(format!("{public}/.well-known/oauth-authorization-server"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(server_metadata["issuer"], public);
    assert_eq!(
        server_metadata["code_challenge_methods_supported"][0],
        "S256"
    );
    assert_eq!(
        server_metadata["registration_endpoint"],
        format!("{public}/oauth/register")
    );

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

    // --- an identity-provider token is not a delegation ------------------
    //
    // A browser access token from the user pool proves who someone is. It says
    // nothing about which AI client they allowed, so it is not accepted here
    // however valid it is.
    let browser_token = mint(&issuer, "us_alice", WEB_CLIENT, 3600);
    let wrong_credential = post(
        &client,
        &url,
        &browser_token,
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )
    .await;
    assert_eq!(wrong_credential.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        wrong_credential.json::<Value>().await.unwrap()["error"],
        "INVALID_TOKEN"
    );

    // --- a made-up bearer is refused -------------------------------------
    let forged = post(
        &client,
        &url,
        "pbmcp_at_not-a-token-this-server-ever-issued",
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )
    .await;
    assert_eq!(forged.status(), StatusCode::UNAUTHORIZED);

    // --- connected for reading only --------------------------------------
    let alice = connect(&client, &service, &public, "us_alice", &["pathbase.read"]).await;
    initialize(&client, &url, &alice).await;
    let (tools, _) = request(&client, &url, &alice, None, 2, "tools/list", json!({})).await;
    let listed = tools["tools"].as_array().unwrap();
    assert_eq!(listed.len(), 21);
    // Annotations describe the real effect: a change set can contain DELETE
    // operations, so proposing and applying one are not "non-destructive".
    let shape = |name: &str| {
        listed
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing tool {name}"))["annotations"]
            .clone()
    };
    assert_eq!(shape("pathbase_get_graph")["readOnlyHint"], true);
    assert_eq!(shape("pathbase_get_graph")["destructiveHint"], false);
    assert_eq!(shape("pathbase_propose_plan")["readOnlyHint"], false);
    assert_eq!(shape("pathbase_propose_plan")["destructiveHint"], true);
    assert_eq!(shape("pathbase_apply_changes")["destructiveHint"], true);
    assert_eq!(shape("pathbase_record_checkin")["destructiveHint"], false);

    // Alice has a personal workspace of her own, provisioned on sign-in. She
    // cannot read one she is not a member of.
    let (forbidden, _) = request(
        &client,
        &url,
        &alice,
        None,
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
        None,
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
        connection: None,
    };
    let connection_id = connection_of(&service, "us_alice").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
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
    // The same, still-valid token stops working on the very next request:
    // the delegation is read from the shared database, not cached.
    let after_revoke = post(
        &client,
        &url,
        &alice,
        None,
        json!({"jsonrpc":"2.0","id":5,"method":"tools/list","params":{}}),
    )
    .await;
    // The token itself is dead, so the endpoint no longer even recognises it.
    assert_eq!(after_revoke.status(), StatusCode::UNAUTHORIZED);

    // --- another user is a separate delegation ---------------------------
    let bob = connect(
        &client,
        &service,
        &public,
        "us_bob",
        &["pathbase.read", "pathbase.propose"],
    )
    .await;
    initialize(&client, &url, &bob).await;
    let (bob_context, _) = request(
        &client,
        &url,
        &bob,
        None,
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
        connection: None,
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
}

/// Two independent server processes over one database, alternating requests.
///
/// This is the shape Lambda produces: the client keeps one logical connection,
/// but consecutive requests land on different execution environments, and one
/// of them may have started after the other.
#[tokio::test]
async fn consecutive_requests_may_reach_different_instances() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("stateless.sqlite3");
    let upstream = start_upstream().await;
    let (_first, first_url, first_public) = start_server(&db, &upstream, None).await;
    let client = HttpClient::new();
    let service = Service::open(&db.to_string_lossy()).await.unwrap();

    let token = connect(
        &client,
        &service,
        &first_public,
        "us_carol",
        &["pathbase.read", "pathbase.propose"],
    )
    .await;

    // A second process, told it serves the same resource — which is what a
    // second Lambda execution environment is.
    let (_second, second_url, _) =
        start_server(&db, &upstream, Some(&format!("{first_public}/mcp"))).await;
    assert_ne!(first_url, second_url);

    // No initialize against the second instance, and no session id anywhere:
    // the request simply works.
    let (tools, headers) = request(
        &client,
        &second_url,
        &token,
        None,
        2,
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(tools["tools"].as_array().unwrap().len(), 21);
    assert!(headers.get("mcp-session-id").is_none());
    // Nothing the endpoint returns may be cached by a proxy in between.
    assert_eq!(headers.get("cache-control").unwrap(), "no-store");
    assert!(headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("application/json"));

    // A proposal made through one instance is visible through the other.
    let (context, _) = request(
        &client,
        &first_url,
        &token,
        None,
        3,
        "tools/call",
        json!({"name":"pathbase_get_context","arguments":{}}),
    )
    .await;
    let workspace = context["structuredContent"]["workspaces"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (proposed, _) = request(
        &client,
        &second_url,
        &token,
        None,
        4,
        "tools/call",
        json!({"name":"pathbase_propose_plan","arguments":{"workspace_id":workspace,"idempotency_key":"cross-instance","operations":[{"method":"POST","path":format!("/v1/workspaces/{workspace}/items"),"body":{"kind":"action","title":"別インスタンス経由の案"}}]}}),
    )
    .await;
    assert_eq!(proposed["isError"], false);
    let change_id = proposed["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Reading it back through the first instance shows the same change set.
    let (seen, _) = request(
        &client,
        &first_url,
        &token,
        None,
        5,
        "tools/call",
        json!({"name":"pathbase_get_change","arguments":{"workspace_id":workspace,"preview_id":change_id}}),
    )
    .await;
    assert_eq!(seen["isError"], false);
    assert_eq!(seen["structuredContent"]["status"], "pending");

    // Replaying the same idempotency key returns the same change set rather
    // than creating a second one.
    let (replayed, _) = request(
        &client,
        &first_url,
        &token,
        None,
        6,
        "tools/call",
        json!({"name":"pathbase_propose_plan","arguments":{"workspace_id":workspace,"idempotency_key":"cross-instance","operations":[{"method":"POST","path":format!("/v1/workspaces/{workspace}/items"),"body":{"kind":"action","title":"別インスタンス経由の案"}}]}}),
    )
    .await;
    assert_eq!(replayed["structuredContent"]["id"], change_id);

    // The transport advertises only what it implements: there is no SSE stream
    // to open, so GET is refused rather than left hanging.
    let stream = client
        .get(&second_url)
        .bearer_auth(&token)
        .header("accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert!(
        stream.status().is_client_error(),
        "GET must be refused, got {}",
        stream.status()
    );

    // A request for a host this deployment does not serve is refused, so a
    // DNS-rebinding attempt cannot reach the tools.
    let rebind = client
        .post(&second_url)
        .bearer_auth(&token)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .header("host", "evil.example")
        .json(&json!({"jsonrpc":"2.0","id":7,"method":"tools/list","params":{}}))
        .send()
        .await
        .unwrap();
    assert!(rebind.status().is_client_error(), "{}", rebind.status());
}
