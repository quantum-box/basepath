//! The shared workflows, served over MCP.
//!
//! A skill says how to work with a person's plan: read before proposing, never
//! invent a date or a measurement, and let the person approve the change. None
//! of that depends on which product the conversation is happening in, so it is
//! written once — in `skills/` at the repository root — and served from here
//! under the `io.modelcontextprotocol/skills` extension.
//!
//! Serving them keeps the instructions with the service they describe. A host
//! that supports the extension gets them by connecting; nothing has to be
//! installed, and nothing has to be kept in step with a package. Hosts that do
//! not support it lose only the instructions: every tool works exactly as
//! before, and `plugin/<host>/` still ships the same files for the hosts that
//! read them from disk.
//!
//! The same files are compiled into the binary and copied into the
//! distributable packages, so a host reading them over MCP and a host reading
//! them from a package see identical bytes — which the manifest's digests make
//! checkable rather than merely claimed.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// One skill, with the raw bytes of every file it contains.
struct Skill {
    name: &'static str,
    /// `SKILL.md` first; supporting files, if any, after it.
    files: &'static [(&'static str, &'static str)],
}

/// The skills, embedded from the one place they are written.
///
/// Listed explicitly rather than globbed: a file that ships to every AI host a
/// person connects should appear in a diff when it is added.
const SKILLS: &[Skill] = &[
    Skill {
        name: "basepath-goal-breakdown",
        files: &[(
            "SKILL.md",
            include_str!("../../skills/basepath-goal-breakdown/SKILL.md"),
        )],
    },
    Skill {
        name: "basepath-week-planning",
        files: &[(
            "SKILL.md",
            include_str!("../../skills/basepath-week-planning/SKILL.md"),
        )],
    },
    Skill {
        name: "basepath-record-progress",
        files: &[(
            "SKILL.md",
            include_str!("../../skills/basepath-record-progress/SKILL.md"),
        )],
    },
    Skill {
        name: "basepath-weekly-review",
        files: &[(
            "SKILL.md",
            include_str!("../../skills/basepath-weekly-review/SKILL.md"),
        )],
    },
];

/// How long a host may reuse a listing before asking again.
///
/// The skills change only when this binary is redeployed, so this is about
/// how quickly a correction reaches a running conversation, not about load.
const TTL_MS: u64 = 300_000;

pub const URI_SCHEME: &str = "skill://";

/// The capability declaration, which is what makes a host ask at all.
///
/// `directoryRead` is absent, which the specification reads as `false`: every
/// skill here is a single file, so there is no directory to walk.
pub fn extension_capability() -> (String, serde_json::Map<String, Value>) {
    (
        "io.modelcontextprotocol/skills".to_owned(),
        serde_json::Map::new(),
    )
}

fn digest(contents: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(contents.as_bytes()))
}

/// The YAML frontmatter, preserved field by field.
///
/// The host compares what it parses from the served `SKILL.md` against this,
/// so anything dropped here would make the skill fail verification rather than
/// load with a field missing.
fn frontmatter(source: &str) -> Value {
    let mut fields = serde_json::Map::new();
    let Some(rest) = source.strip_prefix("---\n") else {
        return Value::Object(fields);
    };
    let Some(end) = rest.find("\n---\n") else {
        return Value::Object(fields);
    };
    for line in rest[..end].lines() {
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim().to_owned(), json!(value.trim()));
        }
    }
    Value::Object(fields)
}

fn uri(skill: &Skill, file: &str) -> String {
    format!("{URI_SCHEME}{}/{file}", skill.name)
}

/// One `skills/list` entry: the frontmatter plus the complete file manifest.
fn entry(skill: &Skill) -> Value {
    let manifest: Vec<Value> = skill
        .files
        .iter()
        .map(|(path, contents)| {
            json!({
                "uri": uri(skill, path),
                "digest": digest(contents),
                // Raw bytes, which is what the host verifies against.
                "size": contents.len(),
            })
        })
        .collect();
    json!({
        "uri": uri(skill, "SKILL.md"),
        "frontmatter": frontmatter(skill.files[0].1),
        "resources": manifest,
    })
}

fn find(uri_value: &str) -> Option<(&'static Skill, &'static str)> {
    let path = uri_value.strip_prefix(URI_SCHEME)?;
    let (name, file) = path.split_once('/')?;
    let skill = SKILLS.iter().find(|skill| skill.name == name)?;
    let (_, contents) = skill.files.iter().find(|(path, _)| *path == file)?;
    Some((skill, contents))
}

/// True when this URI names something this module serves.
pub fn owns(uri_value: &str) -> bool {
    uri_value.starts_with(URI_SCHEME)
}

pub fn list() -> Value {
    json!({
        "resultType": "complete",
        "skills": SKILLS.iter().map(entry).collect::<Vec<_>>(),
        "ttlMs": TTL_MS,
        // The instructions are the same for everyone; nothing here is derived
        // from who is asking, so a shared cache is correct.
        "cacheScope": "public",
    })
}

/// `skills/get`. Returns `None` for a URI this server does not serve, which
/// the caller turns into `-32602`.
pub fn get(uri_value: &str) -> Option<Value> {
    let path = uri_value.strip_prefix(URI_SCHEME)?;
    let name = path.split('/').next()?;
    let skill = SKILLS.iter().find(|skill| skill.name == name)?;
    Some(json!({
        "resultType": "complete",
        "skill": entry(skill),
        "ttlMs": TTL_MS,
        "cacheScope": "public",
    }))
}

/// The contents of one skill file, for `resources/read`.
pub fn read(uri_value: &str) -> Option<Value> {
    let (_, contents) = find(uri_value)?;
    Some(json!({
        "uri": uri_value,
        "mimeType": "text/markdown",
        "text": contents,
    }))
}

/// The skills as ordinary resources.
///
/// A host that does not implement the extension still sees them in
/// `resources/list` and can read them, which costs nothing and means the
/// instructions are never completely unavailable.
pub fn resources() -> Vec<Value> {
    SKILLS
        .iter()
        .map(|skill| {
            json!({
                "uri": uri(skill, "SKILL.md"),
                "name": skill.name,
                "description": frontmatter(skill.files[0].1)["description"],
                "mimeType": "text/markdown",
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_skill_is_a_valid_agent_skill() {
        for skill in SKILLS {
            let (path, source) = skill.files[0];
            assert_eq!(path, "SKILL.md", "{}: SKILL.md must come first", skill.name);
            let fields = frontmatter(source);
            // The specification requires the directory name and the declared
            // name to match; a host that finds otherwise refuses the skill.
            assert_eq!(fields["name"], skill.name);
            assert!(
                fields["description"].as_str().is_some_and(|d| d.len() > 40),
                "{}: the description decides when the skill is used",
                skill.name
            );
        }
    }

    #[test]
    fn the_manifest_describes_the_bytes_that_are_served() {
        for skill in SKILLS {
            let listed = entry(skill);
            for (index, (path, contents)) in skill.files.iter().enumerate() {
                let file = &listed["resources"][index];
                assert_eq!(file["uri"], uri(skill, path));
                assert_eq!(file["size"], contents.len());
                assert_eq!(file["digest"], digest(contents));
                // What the host verifies must be what the host receives.
                let served = read(file["uri"].as_str().unwrap()).unwrap();
                assert_eq!(served["text"], *contents);
            }
        }
    }

    #[test]
    fn an_unknown_uri_is_not_answered_with_something_else() {
        assert!(get("skill://not-a-skill/SKILL.md").is_none());
        assert!(read("skill://basepath-weekly-review/../../etc/passwd").is_none());
        assert!(read("skill://basepath-weekly-review/nope.md").is_none());
        assert!(!owns("ui://basepath/plan.html"));
    }
}
