//! The Skills extension, over a real MCP session.
//!
//! The workflows are written once, in `skills/`. This checks that a host
//! connecting to this server actually receives them — with a manifest it can
//! verify, and with the same bytes the distributable packages carry. If those
//! two ever diverge, a person gets different instructions depending on how
//! they connected, and neither copy says which is right.
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
    /// Sends a request and returns the whole response, error included: some of
    /// these assertions are about the error.
    fn raw(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(10))
                .expect("MCP response timeout");
            let response: Value =
                serde_json::from_str(&line).expect("stdout must contain only JSON-RPC");
            if response["id"] == id {
                return response;
            }
        }
    }
    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let response = self.raw(id, method, params);
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
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
    let initialized = client.request(
        1,
        "initialize",
        json!({"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"skills-contract","version":"1"}}),
    );
    // A client only asks for skills after seeing this, so the declaration is
    // part of the contract rather than a detail.
    assert!(
        initialized["capabilities"]["extensions"]["io.modelcontextprotocol/skills"].is_object(),
        "the skills extension must be declared: {initialized}"
    );
    assert!(
        initialized["capabilities"]["resources"].is_object(),
        "skill files are served through resources/read"
    );
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client
}

fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

#[tokio::test]
async fn a_host_can_discover_verify_and_read_every_skill() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = start(&dir.path().join("skills.sqlite3"));

    let listed = client.request(2, "skills/list", json!({}));
    assert_eq!(listed["resultType"], "complete");
    assert!(listed["ttlMs"].as_u64().is_some());
    assert_eq!(listed["cacheScope"], "public");
    let skills = listed["skills"].as_array().unwrap();
    let names: Vec<&str> = skills
        .iter()
        .map(|skill| skill["frontmatter"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "basepath-goal-breakdown",
            "basepath-week-planning",
            "basepath-record-progress",
            "basepath-weekly-review",
        ]
    );

    let mut id = 10;
    for skill in skills {
        let uri = skill["uri"].as_str().unwrap();
        // The directory's last segment must match the declared name, or a host
        // refuses the skill outright.
        assert!(
            uri.starts_with("skill://")
                && uri.ends_with("/SKILL.md")
                && uri.contains(skill["frontmatter"]["name"].as_str().unwrap()),
            "{uri}"
        );
        assert!(
            skill["frontmatter"]["description"]
                .as_str()
                .is_some_and(|value| value.len() > 40),
            "{uri}: the description decides when the skill is used"
        );

        // Every file in the manifest reads back with the size and digest the
        // manifest promised. This is the whole point of the manifest: a host
        // that cannot verify a file must not load it.
        for file in skill["resources"].as_array().unwrap() {
            id += 1;
            let contents = client.request(id, "resources/read", json!({"uri": file["uri"]}));
            let body = &contents["contents"][0];
            assert_eq!(body["uri"], file["uri"]);
            assert_eq!(body["mimeType"], "text/markdown");
            let text = body["text"].as_str().unwrap();
            assert_eq!(file["size"].as_u64().unwrap() as usize, text.len());
            assert_eq!(file["digest"].as_str().unwrap(), digest(text));
        }

        // A known URI loads without a listing, which is how a host that
        // remembered a skill re-reads it.
        id += 1;
        let fetched = client.request(id, "skills/get", json!({"uri": uri}));
        assert_eq!(fetched["skill"], *skill, "get must match the listing");
        assert_eq!(fetched["resultType"], "complete");
    }
}

#[tokio::test]
async fn the_served_bytes_are_the_files_in_the_repository() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = start(&dir.path().join("skills-bytes.sqlite3"));
    let listed = client.request(2, "skills/list", json!({}));

    // The packages under `plugin/<host>/` are built from these same files. If
    // the binary ever embedded a different copy, a person would get different
    // instructions depending on whether their host reads them over MCP or from
    // a package — and nothing would say which was current.
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("skills");
    for skill in listed["skills"].as_array().unwrap() {
        let name = skill["frontmatter"]["name"].as_str().unwrap();
        let on_disk = std::fs::read_to_string(repository.join(name).join("SKILL.md"))
            .unwrap_or_else(|_| panic!("skills/{name}/SKILL.md must exist"));
        let served = client.request(3, "resources/read", json!({"uri": skill["uri"]}));
        assert_eq!(
            served["contents"][0]["text"].as_str().unwrap(),
            on_disk,
            "{name}: the served skill and the repository's must not diverge"
        );
    }
}

#[tokio::test]
async fn an_unknown_skill_stops_the_load_instead_of_returning_something() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = start(&dir.path().join("skills-unknown.sqlite3"));

    // -32602 is what the specification requires: the host stops rather than
    // treating an empty answer as "this skill adds nothing".
    let unknown = client.raw(
        2,
        "skills/get",
        json!({"uri":"skill://not-a-skill/SKILL.md"}),
    );
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");

    // A path that escapes the skill is not a file this server has.
    let escape = client.raw(
        3,
        "resources/read",
        json!({"uri":"skill://basepath-weekly-review/../../../etc/passwd"}),
    );
    assert!(escape.get("error").is_some(), "{escape}");

    // And a method this server does not implement is reported as such rather
    // than answered with an empty success.
    let absent = client.raw(4, "skills/definitely-not-a-method", json!({}));
    assert_eq!(absent["error"]["code"], -32601, "{absent}");
}

#[tokio::test]
async fn a_host_without_the_extension_can_still_read_the_instructions() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = start(&dir.path().join("skills-plain.sqlite3"));

    // Not every host implements the extension. Listing the skills as ordinary
    // resources costs nothing and means the instructions are never completely
    // unavailable. The plan tree is published as one reusable UI resource
    // beside the ordinary skill resources.
    let resources = client.request(2, "resources/list", json!({}));
    let uris: Vec<&str> = resources["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|resource| resource["uri"].as_str().unwrap())
        .collect();
    assert!(
        uris.contains(&"skill://basepath-weekly-review/SKILL.md"),
        "{uris:?}"
    );
    assert!(
        uris.contains(&"ui://basepath/plan.html"),
        "the plan tree resource must be published: {uris:?}"
    );

    let read = client.request(
        3,
        "resources/read",
        json!({"uri":"skill://basepath-weekly-review/SKILL.md"}),
    );
    assert!(read["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Finalizing is not proposable"));
}
