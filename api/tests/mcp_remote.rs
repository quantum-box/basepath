use pathbase_api::service::{Actor, Service};
use reqwest::{header::HeaderMap, Client as HttpClient, StatusCode};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

const MCP_TOKEN: &str = "remote-mcp-test-token-with-32-characters";
const API_TOKEN: &str = "local-api-test-token-with-32-characters";

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start_server(db: &Path) -> (Server, String) {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_pathbase-api"))
            .env("PATHBASE_MODE", "local-preview")
            .env("PATHBASE_DB", db)
            .env("PATHBASE_SEED_DEMO", "0")
            .env("PATHBASE_API_TOKEN", API_TOKEN)
            .env("PATHBASE_MCP_TOKEN", MCP_TOKEN)
            .env("PATHBASE_MCP_ACTOR_ID", "local-owner")
            .env("PATHBASE_MCP_ALLOWED_HOSTS", "127.0.0.1")
            .env("PATHBASE_API_PORT", port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://127.0.0.1:{port}");
    let client = HttpClient::new();
    for _ in 0..100 {
        if client
            .get(format!("{base}/health"))
            .send()
            .await
            .is_ok_and(|response| response.status() == StatusCode::OK)
        {
            return (server, format!("{base}/mcp"));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("remote MCP child process did not start");
}

async fn post(
    client: &HttpClient,
    url: &str,
    session: Option<&str>,
    body: Value,
) -> reqwest::Response {
    let mut request = client
        .post(url)
        .bearer_auth(MCP_TOKEN)
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
    session: Option<&str>,
    id: u64,
    method: &str,
    params: Value,
) -> (Value, HeaderMap) {
    let response = post(
        client,
        url,
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

#[tokio::test]
async fn remote_mcp_auth_session_permissions_and_approval_are_stable() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("remote-mcp.sqlite3");
    let (_server, url) = start_server(&db).await;
    let client = HttpClient::new();

    let unauthorized = client
        .post(&url)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let (initialized, headers) = request(
        &client,
        &url,
        None,
        2,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"pathbase-remote-test","version":"1"}}),
    )
    .await;
    assert!(initialized["capabilities"]["tools"].is_object());
    let session = headers
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let notification = post(
        &client,
        &url,
        Some(&session),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    assert_eq!(notification.status(), StatusCode::ACCEPTED);

    for id in 3..=5 {
        let (tools, _) = request(&client, &url, Some(&session), id, "tools/list", json!({})).await;
        assert_eq!(tools["tools"].as_array().unwrap().len(), 13);
    }
    let (resources, _) = request(
        &client,
        &url,
        Some(&session),
        20,
        "resources/templates/list",
        json!({}),
    )
    .await;
    assert_eq!(resources["resourceTemplates"].as_array().unwrap().len(), 1);
    let (prompts, _) = request(&client, &url, Some(&session), 21, "prompts/list", json!({})).await;
    assert_eq!(prompts["prompts"].as_array().unwrap().len(), 3);

    let (invalid, _) = request(
        &client,
        &url,
        Some(&session),
        6,
        "tools/call",
        json!({"name":"pathbase_get_item","arguments":{"workspace_id":"personal"}}),
    )
    .await;
    assert_eq!(invalid["isError"], true);
    assert_eq!(invalid["structuredContent"]["code"], "VALIDATION_ERROR");

    let (forbidden, _) = request(
        &client,
        &url,
        Some(&session),
        7,
        "tools/call",
        json!({"name":"pathbase_search_items","arguments":{"workspace_id":"someone-elses-workspace"}}),
    )
    .await;
    assert_eq!(forbidden["isError"], true);

    let (proposal, _) = request(
        &client,
        &url,
        Some(&session),
        8,
        "tools/call",
        json!({"name":"pathbase_propose_plan","arguments":{"workspace_id":"personal","idempotency_key":"remote-proposal","operations":[{"method":"POST","path":"/v1/workspaces/personal/items","body":{"kind":"action","title":"Remote proposal"}}]}}),
    )
    .await;
    assert_eq!(proposal["isError"], false);
    let preview_id = proposal["structuredContent"]["id"].as_str().unwrap();
    let apply = json!({"name":"pathbase_apply_changes","arguments":{"workspace_id":"personal","preview_id":preview_id,"idempotency_key":"remote-apply"}});
    let (denied, _) = request(
        &client,
        &url,
        Some(&session),
        9,
        "tools/call",
        apply.clone(),
    )
    .await;
    assert_eq!(denied["isError"], true);
    assert_eq!(denied["structuredContent"]["code"], "APPROVAL_REQUIRED");

    let service = Service::open(&db.to_string_lossy()).await.unwrap();
    service
        .handle(
            &Actor::local(),
            "POST",
            &format!("/v1/workspaces/personal/changesets/{preview_id}/approve"),
            &HashMap::new(),
            json!({}),
            Some("human-approval"),
        )
        .await
        .unwrap();
    let (applied, _) = request(&client, &url, Some(&session), 10, "tools/call", apply).await;
    assert_eq!(applied["isError"], false);
}
