//! The flow a person actually performs, on the storage that ships.
//!
//! Every other suite checks one contract in isolation. This one is the
//! acceptance run: a person signs in, an AI client is authorized and reads
//! their plan over real HTTP MCP, proposes a change, the person approves the
//! exact content they were shown, it is applied atomically to TiDB, and the
//! same state comes back through a *different* execution environment. Then
//! they disconnect, and the still-unexpired token stops working.
//!
//! What runs where is deliberate:
//!
//! - **The AI's side is real HTTP** against a spawned API process, over the
//!   MCP protocol, with a token obtained through the real OAuth flow.
//! - **The person's side is in process**, through the same `Service` the HTTP
//!   layer calls. Their browser surface is covered by the browser suite; what
//!   is being checked here is that both surfaces see one database.
//! - **The database is a real TiDB**, because the concurrency and atomicity
//!   claims are about the engine that ships. Without
//!   `PATHBASE_TEST_DATABASE_URL` every test here skips rather than quietly
//!   proving something about SQLite instead.
use pathbase_api::{
    mcp_auth::ResourceConfig,
    oauth,
    service::{Actor, Service},
};
use reqwest::{Client as HttpClient, StatusCode};
use serde_json::{json, Value};
use sha2::Digest;
use std::{
    collections::HashMap,
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::Duration,
};

/// A throwaway TiDB database, and the processes that share it.
struct Fixture {
    url: String,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let base = std::env::var("PATHBASE_TEST_DATABASE_URL")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())?;
        let admin = sqlx::MySqlPool::connect(&base)
            .await
            .expect("PATHBASE_TEST_DATABASE_URL must point at a reachable server");
        let version: String = sqlx::query_scalar("SELECT tidb_version()")
            .fetch_one(&admin)
            .await
            .expect("PATHBASE_TEST_DATABASE_URL must point at TiDB, not MySQL");
        assert!(version.contains("TiDB") || version.contains("Release Version"));
        let name = format!("pathbase_acceptance_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE `{name}`"))
            .execute(&admin)
            .await
            .expect("the test account must be allowed to create a database");
        admin.close().await;
        let url = match base.rsplit_once('/') {
            Some((prefix, _)) => format!("{prefix}/{name}"),
            None => format!("{base}/{name}"),
        };
        Some(Self { url })
    }

    /// One execution environment: its own pool, its own process-local state.
    async fn service(&self) -> Service {
        Service::open(&self.url).await.expect("open TiDB service")
    }
}

macro_rules! tidb {
    () => {
        match Fixture::new().await {
            Some(fixture) => fixture,
            None => {
                eprintln!("skipped: set PATHBASE_TEST_DATABASE_URL to run the acceptance suite");
                return;
            }
        }
    };
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawns an API process against the shared database.
///
/// `canonical` is the MCP resource every process of this deployment serves;
/// two processes sharing it is what makes them one resource to a client.
async fn start(fixture: &Fixture, canonical: Option<&str>) -> (Server, String, String) {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let public = format!("http://127.0.0.1:{port}");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_pathbase-api"))
            .env("PATHBASE_MODE", "local-preview")
            .env("DATABASE_URL", &fixture.url)
            .env(
                "PATHBASE_API_TOKEN",
                "acceptance-token-with-32-characters!!",
            )
            .env("PATHBASE_API_PORT", port.to_string())
            .env("PATHBASE_PUBLIC_URL", &public)
            .env("PATHBASE_API_BASE_URL", &public)
            .env("PATHBASE_MCP_ENABLED", "1")
            .env(
                "PATHBASE_MCP_RESOURCE",
                canonical
                    .map(String::from)
                    .unwrap_or(format!("{public}/mcp")),
            )
            .env("PATHBASE_MCP_ALLOWED_HOSTS", "127.0.0.1")
            .env("PATHBASE_SEED_DEMO", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    // A fresh TiDB database has to be migrated before the process serves
    // anything, and several of these run at once, so the wait is generous.
    let client = HttpClient::new();
    for _ in 0..600 {
        if client
            .get(format!("{public}/health"))
            .send()
            .await
            .is_ok_and(|response| response.status() == StatusCode::OK)
        {
            return (server, format!("{public}/mcp"), public);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut server = server;
    let mut output = String::new();
    if let Some(mut err) = server.0.stderr.take() {
        use std::io::Read;
        let _ = err.read_to_string(&mut output);
    }
    panic!("the acceptance API process did not become ready at {public}: {output}");
}

fn person(id: &str) -> Actor {
    Actor {
        id: id.into(),
        tenant: pathbase_api::service::LOCAL_TENANT.into(),
        agent: false,
        connection: None,
    }
}

/// The version a membership change has to be made against.
async fn workspace_version(service: &Service, who: &Actor, workspace: &str) -> i64 {
    as_person(
        service,
        who,
        "GET",
        &format!("/v1/workspaces/{workspace}/members"),
        json!({}),
    )
    .await
    .unwrap()["workspace"]["version"]
        .as_i64()
        .expect("the member list carries the workspace version")
}

async fn as_person(
    service: &Service,
    who: &Actor,
    method: &str,
    path: &str,
    body: Value,
) -> pathbase_api::model::Result<Value> {
    service
        .handle(
            who,
            method,
            path,
            &HashMap::new(),
            body,
            Some(&uuid::Uuid::new_v4().to_string()),
        )
        .await
}

/// Authorizes an AI client the way a host does, and returns its access token.
///
/// Registration and the token exchange are real HTTP. The consent between them
/// is in process because it is a signed-in person clicking in Basepath, and
/// there is deliberately no request an AI client can make that performs it.
async fn connect(
    http: &HttpClient,
    service: &Service,
    public: &str,
    actor: &str,
    scopes: &[&str],
) -> String {
    let redirect = "http://127.0.0.1:9/callback";
    let registered: Value = http
        .post(format!("{public}/oauth/register"))
        .json(&json!({"client_name":"Acceptance host","redirect_uris":[redirect]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let client_id = registered["client_id"].as_str().unwrap().to_owned();

    use base64::Engine;
    let verifier = format!("{actor}-verifier-0123456789abcdefghijklmnopqrstuvwxyz");
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
        ("state", "acceptance"),
    ] {
        query.insert(key.to_string(), value.to_string());
    }
    let oauth::Authorization::Ask(_, handle) =
        oauth::begin_authorization(&service.db, &resource, public, &query)
            .await
            .unwrap()
    else {
        panic!("the authorization request should have reached the person");
    };
    let decided = oauth::decide(
        service,
        &person(actor),
        public,
        &handle,
        &scopes.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
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
    let issued: Value = http
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
        .unwrap()
        .json()
        .await
        .unwrap();
    issued["access_token"].as_str().unwrap().to_owned()
}

/// One MCP request over HTTP. Returns the JSON-RPC result.
async fn mcp(
    http: &HttpClient,
    url: &str,
    token: &str,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let response = http
        .post(url)
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{method}");
    let body: Value = response.json().await.unwrap();
    assert!(body.get("error").is_none(), "{method}: {body}");
    body["result"].clone()
}

/// An MCP tool call, returning the structured content and whether it failed.
async fn tool(
    http: &HttpClient,
    url: &str,
    token: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> (bool, Value) {
    let result = mcp(
        http,
        url,
        token,
        id,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
    .await;
    (
        result["isError"] == json!(true),
        result["structuredContent"].clone(),
    )
}

#[tokio::test]
async fn a_person_plans_with_an_ai_approves_the_change_and_finds_it_everywhere() {
    let fixture = tidb!();
    let http = HttpClient::new();
    let alice = person("us_alice");

    // --- 1. the person's own workspace ----------------------------------
    let service = fixture.service().await;
    service.provision_personal(&alice).await.unwrap();
    let workspaces = as_person(&service, &alice, "GET", "/v1/workspaces", json!({}))
        .await
        .unwrap();
    let workspace = workspaces[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(workspaces[0]["scope"], "個人");

    let (_server, mcp_url, public) = start(&fixture, None).await;

    // --- 2. the AI is authorized and reads the plan ----------------------
    let token = connect(
        &http,
        &service,
        &public,
        "us_alice",
        &["pathbase.read", "pathbase.propose", "pathbase.apply"],
    )
    .await;
    mcp(
        &http,
        &mcp_url,
        &token,
        1,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acceptance","version":"1"}}),
    )
    .await;
    let (failed, context) = tool(
        &http,
        &mcp_url,
        &token,
        2,
        "pathbase_get_context",
        json!({}),
    )
    .await;
    assert!(!failed, "{context}");
    assert_eq!(context["me"]["id"], "us_alice");
    let seen: Vec<&str> = context["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    assert_eq!(seen, vec![workspace.as_str()], "only her own workspace");
    // The surfaces the conversation renders from.
    for (id, name, arguments) in [
        (3, "pathbase_get_graph", json!({"workspace_id": workspace})),
        (
            4,
            "pathbase_get_today",
            json!({"workspace_id": workspace, "local_date": "2026-09-14"}),
        ),
        (
            5,
            "pathbase_get_week",
            json!({"workspace_id": workspace, "start": "2026-09-14", "end": "2026-09-20"}),
        ),
        (
            6,
            "pathbase_get_weekly_review",
            json!({"workspace_id": workspace, "week_start": "2026-09-14"}),
        ),
    ] {
        let (failed, value) = tool(&http, &mcp_url, &token, id, name, arguments).await;
        assert!(!failed, "{name}: {value}");
    }

    // --- 3. the AI proposes; nothing is applied --------------------------
    let (failed, change) = tool(
        &http,
        &mcp_url,
        &token,
        7,
        "pathbase_propose_plan",
        json!({
            "workspace_id": workspace,
            "idempotency_key": "acceptance-proposal",
            "title": "受入テストの計画案",
            "operations": [
                {"method":"POST","path":format!("/v1/workspaces/{workspace}/items"),
                 "body":{"kind":"outcome","title":"受入: 体力をつける"}},
                {"method":"POST","path":format!("/v1/workspaces/{workspace}/items"),
                 "body":{"kind":"action","title":"受入: 朝の散歩"}}
            ]
        }),
    )
    .await;
    assert!(!failed, "{change}");
    let change_id = change["id"].as_str().unwrap().to_owned();
    assert_eq!(change["status"], "pending");
    let hash = change["hash"].as_str().unwrap().to_owned();

    // The plan is untouched until a person acts.
    let items = as_person(
        &service,
        &alice,
        "GET",
        &format!("/v1/workspaces/{workspace}/items"),
        json!({}),
    )
    .await
    .unwrap();
    assert!(
        items["items"].as_array().unwrap().is_empty(),
        "previewing is not doing"
    );

    // Applying without approval is refused, whatever the client says.
    let (failed, refused) = tool(
        &http,
        &mcp_url,
        &token,
        8,
        "pathbase_apply_changes",
        json!({"workspace_id":workspace,"preview_id":change_id,"idempotency_key":"early-apply"}),
    )
    .await;
    assert!(failed);
    assert_eq!(refused["code"], "APPROVAL_REQUIRED");

    // --- 4. the person approves the exact content they were shown --------
    let approve = format!("/v1/workspaces/{workspace}/changesets/{change_id}/approve");
    // A digest that is not what was rendered is refused rather than approved.
    let wrong = as_person(
        &service,
        &alice,
        "POST",
        &approve,
        json!({"hash":"not-the-digest"}),
    )
    .await
    .unwrap_err();
    assert_eq!(wrong.code, "CHANGESET_SUPERSEDED");
    let approved = as_person(&service, &alice, "POST", &approve, json!({"hash": hash}))
        .await
        .unwrap();
    // Approving is the write. Nothing is left for the person to come back to.
    assert_eq!(approved["status"], "applied");
    assert_eq!(approved["applied_by"], alice.id);

    // The model asking afterwards is told it is already done, not refused.
    let (failed, applied) = tool(
        &http,
        &mcp_url,
        &token,
        9,
        "pathbase_apply_changes",
        json!({"workspace_id":workspace,"preview_id":change_id,"idempotency_key":"apply-1"}),
    )
    .await;
    assert!(!failed, "{applied}");
    assert_eq!(applied["changeset"]["status"], "applied");

    // Double click: the stored result comes back, applied once.
    let (failed, again) = tool(
        &http,
        &mcp_url,
        &token,
        10,
        "pathbase_apply_changes",
        json!({"workspace_id":workspace,"preview_id":change_id,"idempotency_key":"apply-1"}),
    )
    .await;
    assert!(!failed, "{again}");
    assert_eq!(again["changeset"]["id"], change_id);

    // --- 5. a second execution environment sees the same database --------
    let elsewhere = fixture.service().await;
    let stored = as_person(
        &elsewhere,
        &alice,
        "GET",
        &format!("/v1/workspaces/{workspace}/items"),
        json!({}),
    )
    .await
    .unwrap();
    let titles: Vec<&str> = stored["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles.len(), 2, "both operations, or neither: {titles:?}");
    assert!(titles.contains(&"受入: 体力をつける"));
    let action = stored["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "action")
        .unwrap()
        .clone();
    assert_eq!(action["version"], 1);

    // The audit says who did it and through which delegation.
    let audit = as_person(
        &elsewhere,
        &alice,
        "GET",
        &format!("/v1/workspaces/{workspace}/audit"),
        json!({}),
    )
    .await
    .unwrap();
    let entries = audit.as_array().unwrap();
    assert!(
        entries.iter().any(|entry| entry["actor"] == "us_alice"),
        "the applied change is attributed to the person who approved it"
    );

    // --- 6. recording progress, through the AI, still as proposals -------
    let (failed, completion) = tool(
        &http,
        &mcp_url,
        &token,
        11,
        "pathbase_complete_action",
        json!({
            "workspace_id": workspace,
            "item_id": action["id"],
            "expected_version": 1,
            "local_date": "2026-09-16",
            "idempotency_key": "complete-1"
        }),
    )
    .await;
    assert!(!failed, "{completion}");
    assert_eq!(
        completion["status"], "pending",
        "completion is a proposal too"
    );

    // The person writes their own week, on their own surface.
    let review = as_person(
        &service,
        &alice,
        "POST",
        &format!("/v1/workspaces/{workspace}/weekly-reviews/draft"),
        json!({"week_start":"2026-09-14","learnings":"観測: 2件作成","challenges":"","next_focus":""}),
    )
    .await
    .unwrap();
    as_person(
        &service,
        &alice,
        "POST",
        &format!(
            "/v1/workspaces/{workspace}/weekly-reviews/{}/finalize",
            review["id"].as_str().unwrap()
        ),
        json!({"expected_version": review["version"]}),
    )
    .await
    .unwrap();
    let (failed, weekly) = tool(
        &http,
        &mcp_url,
        &token,
        12,
        "pathbase_get_weekly_review",
        json!({"workspace_id": workspace, "week_start": "2026-09-14"}),
    )
    .await;
    assert!(!failed, "{weekly}");
    assert_eq!(weekly["review"]["status"], "finalized");
    assert_eq!(weekly["review"]["learnings"], "観測: 2件作成");

    // --- 7. disconnecting ends it, and the data stays --------------------
    let connections = as_person(&service, &alice, "GET", "/v1/mcp/connections", json!({}))
        .await
        .unwrap();
    let connection_id = connections[0]["id"].as_str().unwrap().to_owned();
    as_person(
        &service,
        &alice,
        "POST",
        &format!("/v1/mcp/connections/{connection_id}/revoke"),
        json!({}),
    )
    .await
    .unwrap();

    let after = http
        .post(&mcp_url)
        .bearer_auth(&token)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .json(&json!({"jsonrpc":"2.0","id":13,"method":"tools/list","params":{}}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        after.status(),
        StatusCode::UNAUTHORIZED,
        "a disconnect must take effect on the next request"
    );

    // Her plan is still hers.
    let survived = as_person(
        &elsewhere,
        &alice,
        "GET",
        &format!("/v1/workspaces/{workspace}/items"),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(survived["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn two_people_and_a_shared_workspace_never_mix() {
    let fixture = tidb!();
    let http = HttpClient::new();
    let alice = person("us_alice");
    let bob = person("us_bob");

    let service = fixture.service().await;
    service.provision_personal(&alice).await.unwrap();
    service.provision_personal(&bob).await.unwrap();
    let alice_personal = as_person(&service, &alice, "GET", "/v1/workspaces", json!({}))
        .await
        .unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let (_server, mcp_url, public) = start(&fixture, None).await;

    // Each person authorizes separately; nobody inherits anyone's grant.
    let alice_token = connect(&http, &service, &public, "us_alice", &["pathbase.read"]).await;
    let bob_token = connect(
        &http,
        &service,
        &public,
        "us_bob",
        &["pathbase.read", "pathbase.propose"],
    )
    .await;
    assert_ne!(alice_token, bob_token);

    // Bob's client cannot read Alice's personal workspace by naming it.
    let (failed, refused) = tool(
        &http,
        &mcp_url,
        &bob_token,
        1,
        "pathbase_search_items",
        json!({"workspace_id": alice_personal}),
    )
    .await;
    assert!(failed, "{refused}");

    // Alice granted only reading, so proposing is refused for her client even
    // though Bob's may.
    let (failed, denied) = tool(
        &http,
        &mcp_url,
        &alice_token,
        2,
        "pathbase_propose_plan",
        json!({"workspace_id": alice_personal, "idempotency_key":"k", "operations":[]}),
    )
    .await;
    assert!(failed);
    assert_eq!(denied["code"], "INSUFFICIENT_SCOPE");

    // --- a shared workspace, with the three roles ------------------------
    let shared = as_person(
        &service,
        &alice,
        "POST",
        "/v1/workspaces",
        json!({"name":"受入チーム","scope":"チーム"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for (target, role) in [("us_bob", "editor"), ("us_carol", "viewer")] {
        // Inviting is a membership change, so it carries the workspace version
        // it was decided against.
        let version = workspace_version(&service, &alice, &shared).await;
        let invitation = as_person(
            &service,
            &alice,
            "POST",
            &format!("/v1/workspaces/{shared}/invitations"),
            json!({"target_actor": target, "role": role, "expected_version": version}),
        )
        .await
        .unwrap();
        as_person(
            &service,
            &person(target),
            "POST",
            &format!(
                "/v1/invitations/{}/accept",
                invitation["id"].as_str().unwrap()
            ),
            json!({"expected_version": 1}),
        )
        .await
        .unwrap();
    }

    // The editor can write; the viewer cannot.
    as_person(
        &service,
        &bob,
        "POST",
        &format!("/v1/workspaces/{shared}/items"),
        json!({"kind":"action","title":"編集者が作った行動"}),
    )
    .await
    .unwrap();
    let refused = as_person(
        &service,
        &person("us_carol"),
        "POST",
        &format!("/v1/workspaces/{shared}/items"),
        json!({"kind":"action","title":"閲覧者は書けない"}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.status, 403, "{refused:?}");
    // The viewer can still read it.
    let visible = as_person(
        &service,
        &person("us_carol"),
        "GET",
        &format!("/v1/workspaces/{shared}/items"),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(visible["items"].as_array().unwrap().len(), 1);

    // Removing the membership removes the access, immediately and everywhere.
    // The change is versioned against the workspace, so two owners editing
    // membership at once cannot both win.
    let membership_version = workspace_version(&service, &alice, &shared).await;
    as_person(
        &service,
        &alice,
        "DELETE",
        &format!("/v1/workspaces/{shared}/members/us_carol"),
        json!({"expected_version": membership_version}),
    )
    .await
    .unwrap();
    let elsewhere = fixture.service().await;
    let gone = as_person(
        &elsewhere,
        &person("us_carol"),
        "GET",
        &format!("/v1/workspaces/{shared}/items"),
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(gone.status, 404, "{gone:?}");
}

#[tokio::test]
async fn state_survives_a_redeploy_and_reaches_every_execution_environment() {
    let fixture = tidb!();
    let http = HttpClient::new();
    let alice = person("us_alice");
    let service = fixture.service().await;
    service.provision_personal(&alice).await.unwrap();
    let workspace = as_person(&service, &alice, "GET", "/v1/workspaces", json!({}))
        .await
        .unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // One deployment, two processes — which is what Lambda produces.
    let (first, first_url, public) = start(&fixture, None).await;
    let token = connect(
        &http,
        &service,
        &public,
        "us_alice",
        &["pathbase.read", "pathbase.propose"],
    )
    .await;
    let (_second, second_url, _) = start(&fixture, Some(&format!("{public}/mcp"))).await;

    // A proposal made through one process is visible through the other, with
    // no session and no initialize in between.
    let (failed, change) = tool(
        &http,
        &first_url,
        &token,
        1,
        "pathbase_propose_plan",
        json!({
            "workspace_id": workspace,
            "idempotency_key": "cross-instance",
            "operations": [{"method":"POST","path":format!("/v1/workspaces/{workspace}/items"),
                            "body":{"kind":"action","title":"別プロセス経由"}}]
        }),
    )
    .await;
    assert!(!failed, "{change}");
    let (failed, seen) = tool(
        &http,
        &second_url,
        &token,
        2,
        "pathbase_get_change",
        json!({"workspace_id": workspace, "preview_id": change["id"]}),
    )
    .await;
    assert!(!failed, "{seen}");
    assert_eq!(seen["status"], "pending");

    // A redeploy: the first process goes away entirely.
    drop(first);
    let (failed, still_there) = tool(
        &http,
        &second_url,
        &token,
        3,
        "pathbase_get_change",
        json!({"workspace_id": workspace, "preview_id": change["id"]}),
    )
    .await;
    assert!(!failed, "{still_there}");
    assert_eq!(still_there["id"], change["id"]);

    // And a process started after the fact serves the same delegation without
    // the person authorizing again.
    let (_third, third_url, _) = start(&fixture, Some(&format!("{public}/mcp"))).await;
    let (failed, context) = tool(
        &http,
        &third_url,
        &token,
        4,
        "pathbase_get_context",
        json!({}),
    )
    .await;
    assert!(!failed, "{context}");
    assert_eq!(context["me"]["id"], "us_alice");
}
