//! The MCP Apps contract this server publishes.
//!
//! Basepath returns structured data and a text representation from every tool,
//! and the plan-reading tools point at one reusable embedded tree surface.
use serde_json::{json, Value};
use std::{
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

fn start(db: &std::path::Path) -> Client {
    start_with_public_url(db, None)
}

fn start_with_public_url(db: &std::path::Path, public_url: Option<&str>) -> Client {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pathbase-api"));
    if let Some(url) = public_url {
        command.env("PATHBASE_PUBLIC_URL", url);
    }
    let mut child = command
        .arg("--mcp-stdio")
        .env("PATHBASE_MODE", "local-preview")
        .env("PATHBASE_DB", db)
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
    client.request(
        1,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"mcp-apps-contract","version":"1"}}),
    );
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client
}

#[tokio::test]
async fn tools_and_resources_publish_one_tree_surface() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = start(&dir.path().join("mcp-apps.sqlite3"));

    let tools = client.request(2, "tools/list", json!({}));
    let listed = tools["tools"].as_array().unwrap();
    assert!(!listed.is_empty());
    let mut ui_tools = 0;
    for tool in listed {
        if tool["_meta"]["ui"]["resourceUri"] == "ui://basepath/plan-v2.html" {
            ui_tools += 1;
            assert_eq!(
                tool["_meta"]["openai/outputTemplate"],
                "ui://basepath/plan-v2.html"
            );
        }
        assert!(
            tool["inputSchema"].is_object(),
            "missing input schema: {tool}"
        );
        assert!(
            tool["annotations"].is_object(),
            "missing annotations: {tool}"
        );
    }
    assert!(
        ui_tools >= 3,
        "plan-reading tools must open the tree: {ui_tools}"
    );

    let resources = client.request(3, "resources/list", json!({}));
    let listed_resources = resources["resources"].as_array().unwrap();
    let ui = listed_resources
        .iter()
        .find(|resource| resource["uri"] == "ui://basepath/plan-v2.html")
        .unwrap_or_else(|| panic!("tree resource missing: {listed_resources:?}"));
    assert_eq!(ui["mimeType"], "text/html;profile=mcp-app");
    assert!(ui["_meta"]["ui"]["csp"].is_object());
    assert!(listed_resources.iter().any(|resource| {
        resource["uri"]
            .as_str()
            .unwrap_or_default()
            .starts_with("skill://")
    }));

    let resource = client.request(
        4,
        "resources/read",
        json!({"uri":"ui://basepath/plan-v2.html"}),
    );
    assert_eq!(
        resource["contents"][0]["mimeType"],
        "text/html;profile=mcp-app"
    );
    assert!(resource["contents"][0]["_meta"]["ui"]["csp"].is_object());
    assert!(resource["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("id=\"root\""));

    // The model-facing result keeps both forms: structured data for reliable
    // follow-up calls and text for hosts that only forward MCP content.
    let context = client.request(
        5,
        "tools/call",
        json!({"name":"pathbase_get_context","arguments":{}}),
    );
    assert!(context["structuredContent"].is_object(), "{context}");
    assert_eq!(context["content"][0]["type"], "text");
    assert!(!context["content"][0]["text"].as_str().unwrap().is_empty());

    // The item resource template remains available independently of any UI.
    let templates = client.request(6, "resources/templates/list", json!({}));
    assert_eq!(templates["resourceTemplates"].as_array().unwrap().len(), 1);
}

/// A host with no custom UI still gets a usable approval link and explicit
/// instructions in both structured data and the text content.
#[tokio::test]
async fn every_change_set_carries_somewhere_to_go() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = start_with_public_url(
        &dir.path().join("links.sqlite3"),
        Some("https://basepath.example/"),
    );

    let call = |client: &mut Client, id: u64, name: &str, arguments: Value| {
        client.request(
            id,
            "tools/call",
            json!({"name": name, "arguments": arguments}),
        )
    };

    let proposed = call(
        &mut client,
        10,
        "pathbase_preview_changes",
        json!({
            "workspace_id": "personal",
            "title": "会話からの提案",
            "operations": [{
                "method": "POST",
                "path": "/v1/workspaces/personal/items",
                "body": {"kind": "action", "title": "朝の散歩"},
            }],
            "idempotency_key": "links-1",
        }),
    );
    let change = &proposed["structuredContent"];
    let id = change["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{proposed}"));
    assert_eq!(
        change["approval_url"],
        json!(format!("https://basepath.example/changes/personal/{id}"))
    );
    let instruction = change["where_to_approve"].as_str().unwrap();
    assert!(instruction.contains("URL"), "{instruction}");
    assert!(instruction.contains("Basepath"), "{instruction}");
    assert!(proposed["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains(&format!("https://basepath.example/changes/personal/{id}")));

    let listed = call(
        &mut client,
        11,
        "pathbase_list_changes",
        json!({"workspace_id": "personal"}),
    );
    assert_eq!(
        listed["structuredContent"]["items"][0]["approval_url"],
        change["approval_url"]
    );
    let read = call(
        &mut client,
        12,
        "pathbase_get_change",
        json!({"workspace_id": "personal", "preview_id": id}),
    );
    assert_eq!(
        read["structuredContent"]["approval_url"],
        change["approval_url"]
    );
    assert_eq!(read["structuredContent"]["status"], "pending");
    assert_eq!(
        read["structuredContent"]["auto_apply_eligible"],
        json!(false)
    );
}
