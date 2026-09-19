//! The MCP Apps protocol contract this server publishes.
//!
//! A host discovers the UI through `tools/list` and `resources/list`, then
//! reads the `ui://` resource. These assertions are the shape it depends on,
//! taken from a real MCP session; the browser harness
//! (`tests/e2e/mcp-app.spec.mjs`) then drives the document those bytes contain
//! through a real host bridge.
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_pathbase-api"))
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
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"apps-contract","version":"1"}}),
    );
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client
}

const UI_RESOURCE_PERSONAL: &str = "ui://basepath/personal/plan.html";
const UI_RESOURCE_ORGANIZATION: &str = "ui://basepath/organization/plan.html";
const UI_RESOURCE_URI: &str = "ui://basepath/plan.html";
const UI_RESOURCE_MIME: &str = "text/html;profile=mcp-app";

#[tokio::test]
async fn tools_point_at_the_ui_resource_and_the_resource_is_self_contained() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = start(&dir.path().join("apps.sqlite3"));

    // --- tools/list carries the standard link -----------------------------
    let tools = client.request(2, "tools/list", json!({}));
    let listed = tools["tools"].as_array().unwrap();
    let opening = |uri: &str| {
        listed
            .iter()
            .filter(|tool| tool["_meta"]["ui"]["resourceUri"] == uri)
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>()
    };
    let personal = opening(UI_RESOURCE_PERSONAL);
    let organization = opening(UI_RESOURCE_ORGANIZATION);
    assert_eq!(
        personal,
        vec![
            "pathbase_get_graph",
            "pathbase_get_today",
            "pathbase_get_week",
            "pathbase_get_weekly_review",
            "pathbase_list_memory",
            "pathbase_memory_search",
            "pathbase_memory_context",
        ],
        "the plan and the person's own memory open the personal view"
    );
    assert_eq!(
        organization,
        vec![
            "pathbase_get_alignment",
            "pathbase_get_dashboard",
            "pathbase_get_review_queue",
        ],
        "questions about an organization open the organization view"
    );
    // Two resources, and nothing points at both: a host that cached one is
    // not showing the other's data under it.
    assert!(
        personal.iter().all(|tool| !organization.contains(tool)),
        "a tool opened two different views"
    );
    // Nothing still points at the single pre-split URI.
    assert!(opening(UI_RESOURCE_URI).is_empty());
    let ui_tools: Vec<&str> = personal
        .iter()
        .chain(organization.iter())
        .copied()
        .collect();
    // A tool without a view must not claim one.
    assert!(listed
        .iter()
        .filter(|tool| !ui_tools.contains(&tool["name"].as_str().unwrap()))
        .all(|tool| tool["_meta"]["ui"]["resourceUri"].is_null()));
    // Visibility is never used as an authorization device: every tool stays
    // reachable by the model, and the server authorizes each call regardless.
    assert!(listed
        .iter()
        .all(|tool| tool["_meta"]["ui"]["visibility"].is_null()));

    // --- resources/list advertises the resource with a strict policy -------
    let resources = client.request(3, "resources/list", json!({}));
    let listed_resources = resources["resources"].as_array().unwrap();
    for (index, uri) in [UI_RESOURCE_PERSONAL, UI_RESOURCE_ORGANIZATION]
        .into_iter()
        .enumerate()
    {
        let resource = &listed_resources[index];
        assert_eq!(resource["uri"], uri);
        assert_eq!(resource["mimeType"], UI_RESOURCE_MIME);
        // No origin is requested, because the document loads nothing.
        assert_eq!(resource["_meta"]["ui"]["csp"]["connectDomains"], json!([]));
        assert_eq!(resource["_meta"]["ui"]["csp"]["resourceDomains"], json!([]));
    }
    // Each one says in its own description what is not in it, because a host
    // showing them side by side is where the two are most easily confused.
    assert!(listed_resources[0]["description"]
        .as_str()
        .unwrap()
        .contains("shared workspace"));
    assert!(listed_resources[1]["description"]
        .as_str()
        .unwrap()
        .contains("personal"));

    // --- the document itself ----------------------------------------------
    // The same shell answers either URI: it holds no data, so what differs is
    // the identity the host caches and frames it under.
    for uri in [
        UI_RESOURCE_PERSONAL,
        UI_RESOURCE_ORGANIZATION,
        UI_RESOURCE_URI,
    ] {
        let read = client.request(4, "resources/read", json!({ "uri": uri }));
        let content = &read["contents"].as_array().unwrap()[0];
        assert_eq!(content["uri"], uri);
        assert_eq!(content["mimeType"], UI_RESOURCE_MIME);
    }
    let read = client.request(4, "resources/read", json!({"uri": UI_RESOURCE_PERSONAL}));
    let content = &read["contents"].as_array().unwrap()[0];
    assert_eq!(content["mimeType"], UI_RESOURCE_MIME);
    let document = content["text"].as_str().unwrap();
    assert!(document.starts_with("<!doctype html>"));
    // Self-contained: nothing is fetched from anywhere, so the host's
    // deny-by-default policy needs no exception.
    assert!(!document.contains("src=\"http"), "external script");
    assert!(!document.contains("href=\"http"), "external stylesheet");
    assert!(!document.contains("@import"), "external stylesheet import");
    // The shell carries no data and no credential: it asks the host for both.
    assert!(!document.contains("Bearer "), "no token may be embedded");
    assert!(
        !document.contains("\"workspace_id\":\""),
        "no workspace data may be embedded"
    );

    // The item resource template still works alongside the UI resource.
    let templates = client.request(5, "resources/templates/list", json!({}));
    assert_eq!(templates["resourceTemplates"].as_array().unwrap().len(), 1);
}
