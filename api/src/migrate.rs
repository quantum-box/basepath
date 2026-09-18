//! Moving business data between two PathBase databases, and proving the copy.
//!
//! This is deliberately not "swap the connection string". A migration has to
//! show that what arrived is what left: the same rows, the same content, and
//! the same business invariants (owners, references, versions, audit,
//! idempotency). Every step reports what it found rather than asserting
//! success, so an operator can read the report instead of trusting the tool.
use crate::db::{Db, Tx};
use crate::model::{ApiError, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Every table holding business state, in an order that satisfies references.
///
/// `schema_migrations` and `database_identity` describe the database itself,
/// not its contents, and are therefore never copied.
const TABLES: &[(&str, &[&str])] = &[
    ("workspaces", &["id", "body", "seq"]),
    ("memberships", &["workspace_id", "actor", "role"]),
    (
        "documents",
        &["workspace_id", "collection", "id", "body", "seq"],
    ),
    (
        "invitations",
        &[
            "id",
            "workspace_id",
            "target_actor",
            "role",
            "status",
            "created_by",
            "created_at",
            "expires_at",
            "version",
        ],
    ),
    ("settings", &["actor", "body"]),
    (
        "audit",
        &[
            "id",
            "workspace_id",
            "actor",
            "origin",
            "command",
            "created_at",
            "seq",
        ],
    ),
    (
        "idempotency",
        &[
            "actor",
            "workspace_id",
            "`key`",
            "fingerprint",
            "response",
            "created_at",
        ],
    ),
];

/// Columns read as integers rather than text.
fn is_integer(table: &str, column: &str) -> bool {
    table == "invitations" && column == "version"
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TableInventory {
    pub rows: u64,
    /// SHA-256 over the table's rows, ordered independently of how the
    /// database returned them, so two databases can be compared directly.
    pub digest: String,
}

/// Row counts and content digests for every business table.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Inventory {
    pub tables: BTreeMap<String, TableInventory>,
    pub total_rows: u64,
    /// Digest over the per-table digests: one value that identifies the
    /// whole dataset.
    pub digest: String,
}

impl Inventory {
    pub fn is_empty(&self) -> bool {
        self.total_rows == 0
    }
}

/// One named check with its outcome, so a failing report still says what did
/// pass.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

impl Check {
    fn pass(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: true,
            detail: detail.into(),
        }
    }
    fn fail(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: false,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationReport {
    pub dry_run: bool,
    pub source: Inventory,
    pub target_before: Inventory,
    pub target_after: Option<Inventory>,
    pub checks: Vec<Check>,
}

impl MigrationReport {
    pub fn succeeded(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }
}

fn digest_rows(mut rows: Vec<String>) -> String {
    rows.sort();
    let mut hasher = Sha256::new();
    for row in &rows {
        hasher.update(row.as_bytes());
        hasher.update([0x1e]);
    }
    format!("{:x}", hasher.finalize())
}

fn select(table: &str, columns: &[&str]) -> String {
    format!("SELECT {} FROM {table}", columns.join(","))
}

/// Reads counts and content digests without changing anything.
pub async fn inventory(db: &Db) -> Result<Inventory> {
    let mut tx = db.begin_read().await?;
    let mut tables = BTreeMap::new();
    let mut total_rows = 0;
    for (table, columns) in TABLES {
        let rows = tx
            .fetch_all(&select(table, columns), &[])
            .await
            .map_err(|error| {
                ApiError::new(
                    500,
                    "MIGRATION_READ_FAILED",
                    &format!("{table} を読み取れません: {}", error.message),
                )
            })?;
        let mut serialized = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut parts = Vec::with_capacity(columns.len());
            for (index, column) in columns.iter().enumerate() {
                let value = if is_integer(table, column.trim_matches('`')) {
                    row.int(index)?.to_string()
                } else {
                    row.opt_text(index)?.unwrap_or_default()
                };
                parts.push(format!("{column}={value}"));
            }
            serialized.push(parts.join("\u{1f}"));
        }
        total_rows += serialized.len() as u64;
        tables.insert(
            (*table).to_owned(),
            TableInventory {
                rows: serialized.len() as u64,
                digest: digest_rows(serialized),
            },
        );
    }
    tx.commit().await?;
    let digest = digest_rows(
        tables
            .iter()
            .map(|(table, inventory)| format!("{table}={}:{}", inventory.rows, inventory.digest))
            .collect(),
    );
    Ok(Inventory {
        tables,
        total_rows,
        digest,
    })
}

/// Business invariants that must hold in a database people are about to use.
///
/// These are checked against the data, not against the code that wrote it, so
/// a copy that lost or reordered rows is caught here rather than by a user.
pub async fn validate_integrity(db: &Db) -> Result<Vec<Check>> {
    let mut tx = db.begin_read().await?;
    let mut checks = Vec::new();

    let workspaces: Vec<String> = tx
        .fetch_all("SELECT id FROM workspaces", &[])
        .await?
        .iter()
        .map(|row| row.text(0))
        .collect::<Result<_>>()?;

    // Every workspace keeps at least one owner.
    let owners = tx
        .fetch_all(
            "SELECT workspace_id FROM memberships WHERE role='owner'",
            &[],
        )
        .await?
        .iter()
        .map(|row| row.text(0))
        .collect::<Result<std::collections::HashSet<_>>>()?;
    let ownerless: Vec<_> = workspaces
        .iter()
        .filter(|id| !owners.contains(*id))
        .cloned()
        .collect();
    checks.push(if ownerless.is_empty() {
        Check::pass(
            "workspace_has_owner",
            format!("{} workspace(s) each keep an owner", workspaces.len()),
        )
    } else {
        Check::fail(
            "workspace_has_owner",
            format!("workspaces without an owner: {ownerless:?}"),
        )
    });

    // Memberships, documents, invitations and audit rows all reference a
    // workspace that exists.
    let known: std::collections::HashSet<_> = workspaces.iter().cloned().collect();
    for (table, column) in [
        ("memberships", "workspace_id"),
        ("documents", "workspace_id"),
        ("invitations", "workspace_id"),
        ("audit", "workspace_id"),
    ] {
        let referenced = tx
            .fetch_all(&format!("SELECT DISTINCT {column} FROM {table}"), &[])
            .await?
            .iter()
            .map(|row| row.text(0))
            .collect::<Result<Vec<_>>>()?;
        let dangling: Vec<_> = referenced
            .into_iter()
            // Global rows (settings, cross-workspace audit) carry an empty id.
            .filter(|id| !id.is_empty() && !known.contains(id))
            .collect();
        checks.push(if dangling.is_empty() {
            Check::pass(
                &format!("{table}_workspace_reference"),
                "every referenced workspace exists",
            )
        } else {
            Check::fail(
                &format!("{table}_workspace_reference"),
                format!("unknown workspace ids: {dangling:?}"),
            )
        });
    }

    // Documents parse as the JSON the service expects, and versioned records
    // carry a version.
    let mut unparsable = Vec::new();
    let mut unversioned = Vec::new();
    for row in tx
        .fetch_all("SELECT collection,id,body FROM documents", &[])
        .await?
    {
        let (collection, id, body) = (row.text(0)?, row.text(1)?, row.text(2)?);
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(value) => {
                if ["items", "relations", "metrics", "weekly_reviews"]
                    .contains(&collection.as_str())
                    && value["version"].as_i64().is_none_or(|version| version < 1)
                {
                    unversioned.push(format!("{collection}/{id}"));
                }
            }
            Err(_) => unparsable.push(format!("{collection}/{id}")),
        }
    }
    checks.push(if unparsable.is_empty() {
        Check::pass("document_json", "every document body parses")
    } else {
        Check::fail("document_json", format!("unparsable: {unparsable:?}"))
    });
    checks.push(if unversioned.is_empty() {
        Check::pass("document_version", "every versioned document has a version")
    } else {
        Check::fail(
            "document_version",
            format!("missing version: {unversioned:?}"),
        )
    });

    // Relations, metrics and observations point at rows that exist.
    let mut items = std::collections::HashSet::new();
    let mut metrics = std::collections::HashSet::new();
    for row in tx
        .fetch_all(
            "SELECT workspace_id,collection,id FROM documents WHERE collection IN ('items','metrics')",
            &[],
        )
        .await?
    {
        let key = format!("{}/{}", row.text(0)?, row.text(2)?);
        if row.text(1)? == "items" {
            items.insert(key);
        } else {
            metrics.insert(key);
        }
    }
    let mut dangling = Vec::new();
    for row in tx
        .fetch_all(
            "SELECT workspace_id,collection,id,body FROM documents \
             WHERE collection IN ('relations','metrics','observations')",
            &[],
        )
        .await?
    {
        let (workspace, collection, id, body) =
            (row.text(0)?, row.text(1)?, row.text(2)?, row.text(3)?);
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) else {
            continue;
        };
        let references: Vec<(&str, &std::collections::HashSet<String>)> = match collection.as_str()
        {
            "relations" => vec![("source_id", &items), ("target_id", &items)],
            "metrics" => vec![("item_id", &items)],
            _ => vec![("metric_id", &metrics)],
        };
        for (field, known) in references {
            if let Some(target) = value[field].as_str() {
                if !known.contains(&format!("{workspace}/{target}")) {
                    dangling.push(format!("{collection}/{id}.{field} -> {target}"));
                }
            }
        }
    }
    checks.push(if dangling.is_empty() {
        Check::pass(
            "document_reference",
            "relations, metrics and observations resolve",
        )
    } else {
        Check::fail("document_reference", format!("dangling: {dangling:?}"))
    });

    // Stored idempotency responses are still replayable JSON.
    let broken = tx
        .fetch_all("SELECT actor,`key`,response FROM idempotency", &[])
        .await?
        .iter()
        .map(|row| Ok((row.text(0)?, row.text(1)?, row.text(2)?)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|(_, _, response)| serde_json::from_str::<serde_json::Value>(response).is_err())
        .map(|(actor, key, _)| format!("{actor}/{key}"))
        .collect::<Vec<_>>();
    checks.push(if broken.is_empty() {
        Check::pass("idempotency_replayable", "stored responses parse")
    } else {
        Check::fail("idempotency_replayable", format!("unparsable: {broken:?}"))
    });

    tx.commit().await?;
    Ok(checks)
}

async fn copy_table(
    source: &mut Tx,
    target: &mut Tx,
    table: &str,
    columns: &[&str],
) -> Result<u64> {
    let rows = source.fetch_all(&select(table, columns), &[]).await?;
    let placeholders = vec!["?"; columns.len()].join(",");
    let insert = format!(
        "INSERT INTO {table}({}) VALUES({placeholders})",
        columns.join(",")
    );
    let mut written = 0;
    for row in &rows {
        let mut binds = Vec::with_capacity(columns.len());
        for (index, column) in columns.iter().enumerate() {
            if is_integer(table, column.trim_matches('`')) {
                binds.push(crate::db::Param::Int(row.int(index)?));
            } else {
                binds.push(match row.opt_text(index)? {
                    Some(value) => crate::db::Param::Text(value),
                    None => crate::db::Param::Null,
                });
            }
        }
        target.execute(&insert, &binds).await?;
        written += 1;
    }
    Ok(written)
}

/// Copies every business table from `source` into `target`.
///
/// The target must already have its schema and must be empty: this tool
/// fills a new database, it does not merge into a live one. `dry_run` reads
/// and validates everything but writes nothing.
pub async fn migrate_data(source: &Db, target: &Db, dry_run: bool) -> Result<MigrationReport> {
    let source_inventory = inventory(source).await?;
    let target_before = inventory(target).await?;
    let mut checks = Vec::new();

    // The target has to be at the schema this build expects.
    let status = target.schema_status().await?;
    checks.push(if status.schema_version == status.expected_schema_version {
        Check::pass(
            "target_schema",
            format!("schema version {}", status.schema_version),
        )
    } else {
        Check::fail(
            "target_schema",
            format!(
                "target is at version {} but this build expects {}",
                status.schema_version, status.expected_schema_version
            ),
        )
    });

    checks.push(if target_before.is_empty() {
        Check::pass("target_empty", "the target holds no business rows")
    } else {
        Check::fail(
            "target_empty",
            format!(
                "the target already holds {} row(s); migrate into a new database",
                target_before.total_rows
            ),
        )
    });

    let mut source_checks = validate_integrity(source).await?;
    for check in &mut source_checks {
        check.name = format!("source_{}", check.name);
    }
    checks.append(&mut source_checks);

    if source_inventory.is_empty() {
        checks.push(Check::pass(
            "migration_needed",
            "the source holds no business rows: nothing to migrate",
        ));
    }

    if dry_run || !checks.iter().all(|check| check.passed) {
        return Ok(MigrationReport {
            dry_run: true,
            source: source_inventory,
            target_before,
            target_after: None,
            checks,
        });
    }
    // One transaction per table keeps a failure from leaving half a table,
    // and keeps a large copy from holding one lock for its whole duration.
    for (table, columns) in TABLES {
        let mut read = source.begin_read().await?;
        let mut write = target.begin_write().await?;
        let written = copy_table(&mut read, &mut write, table, columns).await?;
        write.commit().await?;
        read.commit().await?;
        let expected = source_inventory.tables[*table].rows;
        checks.push(if written == expected {
            Check::pass(&format!("copied_{table}"), format!("{written} row(s)"))
        } else {
            Check::fail(
                &format!("copied_{table}"),
                format!("wrote {written} row(s) but the source holds {expected}"),
            )
        });
    }

    let target_after = inventory(target).await?;
    checks.push(if target_after.digest == source_inventory.digest {
        Check::pass(
            "content_digest",
            format!("source and target agree ({})", &target_after.digest[..16]),
        )
    } else {
        Check::fail(
            "content_digest",
            format!(
                "source {} but target {}",
                &source_inventory.digest[..16],
                &target_after.digest[..16]
            ),
        )
    });

    let mut target_checks = validate_integrity(target).await?;
    for check in &mut target_checks {
        check.name = format!("target_{}", check.name);
    }
    checks.append(&mut target_checks);

    Ok(MigrationReport {
        dry_run: false,
        source: source_inventory,
        target_before,
        target_after: Some(target_after),
        checks,
    })
}
