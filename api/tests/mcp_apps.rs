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
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"apps-contract","version":"1"}}),
    );
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client
}

const UI_RESOURCE_PERSONAL: &str = "ui://basepath/personal/plan.html";
const UI_RESOURCE_ORGANIZATION: &str = "ui://basepath/organization/plan.html";
const UI_RESOURCE_URI: &str = "ui://basepath/plan.html";
const UI_RESOURCE_MIME: &str = "text/html;profile=mcp-app";
const UI_RESOURCE_PERSONAL_OPENAI: &str = "ui://basepath/personal/plan.skybridge.html";
const UI_RESOURCE_ORGANIZATION_OPENAI: &str = "ui://basepath/organization/plan.skybridge.html";
const UI_RESOURCE_MIME_OPENAI: &str = "text/html+skybridge";

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
            "pathbase_list_changes",
            "pathbase_get_change",
            "pathbase_preview_changes",
            "pathbase_propose_plan",
            "pathbase_apply_changes",
            "pathbase_reject_change",
            "pathbase_complete_action",
            "pathbase_record_checkin",
            "pathbase_record_observation",
        ],
        "the plan, the person's own memory, and every change set open the personal view"
    );
    // The regression this list exists for: a tool that makes a proposal and
    // cannot show it asks the person to agree to something they never saw.
    for proposing in [
        "pathbase_preview_changes",
        "pathbase_propose_plan",
        "pathbase_complete_action",
        "pathbase_record_checkin",
        "pathbase_record_observation",
    ] {
        assert!(
            personal.contains(&proposing),
            "{proposing} creates a change set and must be able to render it"
        );
    }
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
    // --- and the same links in the spelling ChatGPT reads ------------------
    //
    // ChatGPT never looks at `ui.resourceUri`. Publishing only that is why a
    // real Developer Mode connection rendered nothing at all, which was then
    // indistinguishable from the view itself being broken. Every tool that
    // names one convention names the other, for the same document.
    for tool in listed {
        let name = tool["name"].as_str().unwrap();
        let mcp_apps = tool["_meta"]["ui"]["resourceUri"].as_str();
        let apps_sdk = tool["_meta"]["openai/outputTemplate"].as_str();
        match mcp_apps {
            Some(UI_RESOURCE_PERSONAL) => {
                assert_eq!(apps_sdk, Some(UI_RESOURCE_PERSONAL_OPENAI), "{name}");
            }
            Some(UI_RESOURCE_ORGANIZATION) => {
                assert_eq!(apps_sdk, Some(UI_RESOURCE_ORGANIZATION_OPENAI), "{name}");
            }
            _ => assert_eq!(apps_sdk, None, "{name} claims a view it does not have"),
        }
        // A view that cannot call back is a picture. The person pressing
        // "reflect this" in the conversation needs the call to reach here.
        assert_eq!(
            tool["_meta"]["openai/widgetAccessible"].as_bool(),
            mcp_apps.map(|_| true),
            "{name}"
        );
    }
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
    // The Apps SDK twins are listed too — a host that only knows that
    // convention has to be able to find the document by `resources/list`.
    for (index, uri) in [UI_RESOURCE_PERSONAL_OPENAI, UI_RESOURCE_ORGANIZATION_OPENAI]
        .into_iter()
        .enumerate()
    {
        let resource = &listed_resources[2 + index];
        assert_eq!(resource["uri"], uri);
        assert_eq!(resource["mimeType"], UI_RESOURCE_MIME_OPENAI);
        assert_eq!(
            resource["_meta"]["openai/widgetCSP"]["connect_domains"],
            json!([])
        );
        assert_eq!(
            resource["_meta"]["openai/widgetCSP"]["resource_domains"],
            json!([])
        );
    }

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
    // Same bytes under the other convention, with the media type that
    // convention recognises. One document, two names — not two documents.
    for uri in [UI_RESOURCE_PERSONAL_OPENAI, UI_RESOURCE_ORGANIZATION_OPENAI] {
        let read = client.request(4, "resources/read", json!({ "uri": uri }));
        let content = &read["contents"].as_array().unwrap()[0];
        assert_eq!(content["uri"], uri);
        assert_eq!(content["mimeType"], UI_RESOURCE_MIME_OPENAI);
        assert_eq!(
            content["text"].as_str().unwrap().len(),
            client.request(4, "resources/read", json!({"uri": UI_RESOURCE_PERSONAL}))["contents"]
                .as_array()
                .unwrap()[0]["text"]
                .as_str()
                .unwrap()
                .len()
        );
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

/// A host that renders nothing must still not be a dead end.
///
/// On 2026-09-19 a proposal reached the server from a real conversation, the
/// host drew no view, and the model answered that approving happens in
/// Basepath — without saying where. The person had nothing to click and the
/// proposal expired (PLT-4943). The view is the fix for the good case; this is
/// the fix for the case where there is no view, and it is the one that has to
/// hold whatever any host does.
#[tokio::test]
async fn every_change_set_carries_somewhere_to_go() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = start_with_public_url(
        &dir.path().join("links.sqlite3"),
        Some("https://basepath.example/"),
    );

    let call = |client: &mut Client, id: u64, name: &str, arguments: Value| {
        client.request(id, "tools/call", json!({"name": name, "arguments": arguments}))
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
    let id = change["id"].as_str().unwrap_or_else(|| panic!("{proposed}"));
    // Absolute, and built by the server rather than by whatever is reading it:
    // the same string reaches the model, the view and the person.
    assert_eq!(
        change["approval_url"],
        json!(format!("https://basepath.example/changes/personal/{id}"))
    );
    // And a sentence, because in a host with no view this is what the model
    // reads before it answers. It has to tell it to hand the URL over, not
    // merely that approval happens somewhere.
    let instruction = change["where_to_approve"].as_str().unwrap();
    assert!(instruction.contains("URL"), "{instruction}");
    assert!(instruction.contains("Basepath"), "{instruction}");
    // The link is in the text block too, not only in `structuredContent`: a
    // host that passes along only text still hands over something usable.
    assert!(proposed["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains(&format!("https://basepath.example/changes/personal/{id}")));

    // The same on every other shape a change set comes back in.
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
    // Nothing was applied by any of that: with no range set, the proposal is
    // still waiting for the person.
    assert_eq!(read["structuredContent"]["status"], "pending");
    assert_eq!(read["structuredContent"]["auto_apply_eligible"], json!(false));
}
