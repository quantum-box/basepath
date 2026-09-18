//! Document-level persistence shared by every entry point (HTTP, MCP, Tauri).
//!
//! All statements here are portable between the SQLite preview database and
//! TiDB; anything dialect-specific comes from [`crate::db::Dialect`].
use crate::db::Tx;
use crate::model::*;
use crate::params;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};

/// Lexicographically sortable creation stamp, replacing SQLite's `rowid`
/// ordering. Millisecond time keeps rows from different execution
/// environments in the order they were written; the per-process counter keeps
/// rows written inside the same millisecond distinct and stable.
pub fn sequence() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let tick = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{millis:013}-{tick:010}-{:08x}", rand_suffix())
}

fn rand_suffix() -> u32 {
    // uuid v4 already pulls from the OS entropy source; reuse it rather than
    // adding a second RNG dependency.
    u32::from_be_bytes(uuid::Uuid::new_v4().as_bytes()[..4].try_into().unwrap())
}

pub async fn list<T: DeserializeOwned>(tx: &mut Tx, w: &str, col: &str) -> Result<Vec<T>> {
    let sql = format!(
        "SELECT body FROM documents WHERE workspace_id=? AND collection=? ORDER BY seq{}",
        tx.lock_reads()
    );
    let rows = tx.fetch_all(&sql, &params![w, col]).await?;
    rows.iter()
        .map(|row| Ok(serde_json::from_str(&row.text(0)?)?))
        .collect()
}

pub async fn get<T: DeserializeOwned>(tx: &mut Tx, w: &str, col: &str, id: &str) -> Result<T> {
    let sql = format!(
        "SELECT body FROM documents WHERE workspace_id=? AND collection=? AND id=?{}",
        tx.lock_reads()
    );
    let row = tx
        .fetch_optional(&sql, &params![w, col, id])
        .await?
        .ok_or_else(ApiError::missing)?;
    Ok(serde_json::from_str(&row.text(0)?)?)
}

pub async fn exists(tx: &mut Tx, w: &str, col: &str, id: &str) -> Result<bool> {
    let sql = format!(
        "SELECT 1 FROM documents WHERE workspace_id=? AND collection=? AND id=?{}",
        tx.lock_reads()
    );
    Ok(tx
        .fetch_optional(&sql, &params![w, col, id])
        .await?
        .is_some())
}

pub async fn put<T: Serialize>(tx: &mut Tx, w: &str, col: &str, id: &str, value: &T) -> Result<()> {
    let sql = tx.dialect().upsert(
        "documents",
        &["workspace_id", "collection", "id", "body", "seq"],
        &["workspace_id", "collection", "id"],
        &["body"],
    );
    tx.execute(
        &sql,
        &params![w, col, id, serde_json::to_string(value)?, sequence()],
    )
    .await?;
    Ok(())
}

/// Insert that leaves an existing document untouched. Used where the same
/// logical event may be derived more than once (notifications).
pub async fn put_new<T: Serialize>(
    tx: &mut Tx,
    w: &str,
    col: &str,
    id: &str,
    value: &T,
) -> Result<()> {
    let sql = tx.dialect().insert_ignore(
        "documents",
        &["workspace_id", "collection", "id", "body", "seq"],
    );
    tx.execute(
        &sql,
        &params![w, col, id, serde_json::to_string(value)?, sequence()],
    )
    .await?;
    Ok(())
}

pub async fn remove(tx: &mut Tx, w: &str, col: &str, id: &str) -> Result<()> {
    if tx
        .execute(
            "DELETE FROM documents WHERE workspace_id=? AND collection=? AND id=?",
            &params![w, col, id],
        )
        .await?
        == 0
    {
        return Err(ApiError::missing());
    }
    Ok(())
}

pub async fn memberships(tx: &mut Tx, actor: &str) -> Result<Vec<Workspace>> {
    let sql = format!(
        "SELECT w.body,m.role FROM workspaces w JOIN memberships m ON w.id=m.workspace_id \
         WHERE m.actor=? ORDER BY w.seq{}",
        tx.lock_reads()
    );
    let rows = tx.fetch_all(&sql, &params![actor]).await?;
    rows.iter()
        .map(|row| {
            let mut workspace: Workspace = serde_json::from_str(&row.text(0)?)?;
            workspace.role = row.text(1)?;
            Ok(workspace)
        })
        .collect()
}

pub async fn role(tx: &mut Tx, w: &str, actor: &str) -> Result<Option<String>> {
    let sql = format!(
        "SELECT role FROM memberships WHERE workspace_id=? AND actor=?{}",
        tx.lock_reads()
    );
    tx.fetch_optional(&sql, &params![w, actor])
        .await?
        .map(|row| row.text(0))
        .transpose()
}

pub async fn authorize(tx: &mut Tx, actor: &str, w: &str, write: bool) -> Result<()> {
    match role(tx, w, actor).await?.as_deref() {
        Some("owner" | "editor") => Ok(()),
        Some("viewer") if !write => Ok(()),
        Some("viewer") => Err(ApiError::new(
            403,
            "FORBIDDEN",
            "このワークスペースは閲覧のみです",
        )),
        _ => Err(ApiError::missing()),
    }
}

pub fn value<T: Serialize>(v: T) -> Result<Value> {
    Ok(serde_json::to_value(v)?)
}
