use pathbase_api::service::{Actor, Service};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

struct Client {
    child: Child,
    input: ChildStdin,
    lines: Receiver<String>,
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Client {
    fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").unwrap();
        self.input.flush().unwrap();
    }
    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(10))
                .expect("MCP response timeout");
            let response: Value =
                serde_json::from_str(&line).expect("stdout must contain only JSON-RPC");
            if response["id"] == id {
                assert!(response.get("error").is_none(), "{response}");
                return response["result"].clone();
            }
        }
    }
}

#[tokio::test]
async fn stdio_negotiates_and_requires_human_approval_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mcp.sqlite3");
    let mut child = Command::new(env!("CARGO_BIN_EXE_pathbase-api"))
        .arg("--mcp-stdio")
        .env("PATHBASE_MODE", "local-preview")
        .env("PATHBASE_DB", &db)
        .env("PATHBASE_SEED_DEMO", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let output = child.stdout.take().unwrap();
    let input = child.stdin.take().unwrap();
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut client = Client {
        child,
        input,
        lines,
    };
    let initialized=client.request(1,"initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"pathbase-contract-test","version":"1"}}));
    assert!(initialized["protocolVersion"].is_string());
    assert!(initialized["capabilities"]["tools"].is_object());
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let tools = client.request(2, "tools/list", json!({}));
    assert_eq!(tools["tools"].as_array().unwrap().len(), 20);
    let resources = client.request(3, "resources/templates/list", json!({}));
    assert_eq!(resources["resourceTemplates"].as_array().unwrap().len(), 1);
    let prompts = client.request(4, "prompts/list", json!({}));
    assert_eq!(prompts["prompts"].as_array().unwrap().len(), 3);
    let context = client.request(
        5,
        "tools/call",
        json!({"name":"pathbase_get_context","arguments":{}}),
    );
    assert_eq!(context["isError"], false);
    let plan=client.request(6,"tools/call",json!({"name":"pathbase_propose_plan","arguments":{"workspace_id":"personal","title":"Test proposal","idempotency_key":"proposal","operations":[{"method":"POST","path":"/v1/workspaces/personal/items","body":{"title":"Requires approval"}}]}}));
    assert_eq!(plan["isError"], false);
    let id = plan["structuredContent"]["id"].as_str().unwrap();
    let args = json!({"name":"pathbase_apply_changes","arguments":{"workspace_id":"personal","preview_id":id,"idempotency_key":"apply"}});
    let denied = client.request(7, "tools/call", args.clone());
    assert_eq!(denied["isError"], true);
    let service = Service::open(&db.to_string_lossy()).await.unwrap();
    let snapshot = service
        .handle(
            &Actor::local(),
            "GET",
            "/v1/workspaces/personal/snapshot",
            &HashMap::new(),
            json!({}),
            None,
        )
        .await
        .unwrap();
    assert!(snapshot["items"].as_array().unwrap().is_empty());
    service
        .handle(
            &Actor::local(),
            "POST",
            &format!("/v1/workspaces/personal/changesets/{id}/approve"),
            &HashMap::new(),
            json!({}),
            Some("owner-approval"),
        )
        .await
        .unwrap();
    let applied = client.request(8, "tools/call", args);
    assert_eq!(applied["isError"], false);
    let snapshot = service
        .handle(
            &Actor::local(),
            "GET",
            "/v1/workspaces/personal/snapshot",
            &HashMap::new(),
            json!({}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(snapshot["items"].as_array().unwrap().len(), 1);
}
