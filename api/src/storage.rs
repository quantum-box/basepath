//! Document-level persistence shared by every entry point (HTTP, MCP, Tauri).
//!
//! All statements here are portable between the SQLite preview database and
//! TiDB; anything dialect-specific comes from [`crate::db::Dialect`].
use crate::db::Tx;
use crate::model::*;
use crate::params;
use crate::service::Actor;
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

/// Reads a bounded page from a document collection in insertion order.
///
/// The cursor is the last document ID returned on the previous page. Using
/// the collection sequence rather than an offset keeps the amount read per
/// request bounded as the collection grows.
pub async fn list_page<T: DeserializeOwned>(
    tx: &mut Tx,
    w: &str,
    col: &str,
    cursor: Option<&str>,
    requested_limit: usize,
) -> Result<(Vec<(String, T)>, Option<String>)> {
    let limit = requested_limit.clamp(1, 200);
    let fetch_limit = i64::try_from(limit + 1).unwrap_or(201);
    let after = if let Some(cursor) = cursor {
        let row = tx
            .fetch_optional(
                &format!(
                    "SELECT seq FROM documents WHERE workspace_id=? AND collection=? AND id=?{}",
                    tx.lock_reads()
                ),
                &params![w, col, cursor],
            )
            .await?
            .ok_or_else(|| ApiError::invalid("カーソルが無効です"))?;
        Some(row.text(0)?)
    } else {
        None
    };
    let sql = format!(
        "SELECT id, body FROM documents WHERE workspace_id=? AND collection=?{} ORDER BY seq LIMIT ?{}",
        if after.is_some() { " AND seq>?" } else { "" },
        tx.lock_reads()
    );
    let rows = if let Some(after) = after {
        tx.fetch_all(&sql, &params![w, col, after, fetch_limit])
            .await?
    } else {
        tx.fetch_all(&sql, &params![w, col, fetch_limit]).await?
    };
    let has_more = rows.len() > limit;
    let next_cursor = if has_more {
        Some(rows[limit - 1].text(0)?)
    } else {
        None
    };
    let items = rows
        .iter()
        .take(limit)
        .map(|row| Ok((row.text(0)?, serde_json::from_str(&row.text(1)?)?)))
        .collect::<Result<Vec<_>>>()?;
    Ok((items, next_cursor))
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

/// The workspaces this actor may see *in the tenant they are acting in*.
///
/// A membership alone is not enough. The same person is a member of
/// workspaces across several tenants, and showing them one list would be the
/// tenant boundary leaking into the first screen they land on.
pub async fn memberships(tx: &mut Tx, actor: &Actor) -> Result<Vec<Workspace>> {
    if actor.tenant.is_empty() {
        return Ok(vec![]);
    }
    let sql = format!(
        "SELECT w.body,m.role,w.tenant_id FROM workspaces w JOIN memberships m ON w.id=m.workspace_id \
         WHERE m.actor=? AND w.tenant_id=? ORDER BY w.seq{}",
        tx.lock_reads()
    );
    let rows = tx
        .fetch_all(&sql, &params![&actor.id, &actor.tenant])
        .await?;
    rows.iter()
        .map(|row| {
            let mut workspace: Workspace = serde_json::from_str(&row.text(0)?)?;
            workspace.role = row.text(1)?;
            workspace.tenant_id = row.text(2)?;
            Ok(workspace)
        })
        .collect()
}

/// The tenant a workspace belongs to, or `None` if there is no such workspace.
pub async fn workspace_tenant(tx: &mut Tx, w: &str) -> Result<Option<String>> {
    let sql = format!(
        "SELECT tenant_id FROM workspaces WHERE id=?{}",
        tx.lock_reads()
    );
    tx.fetch_optional(&sql, &params![w])
        .await?
        .map(|row| row.text(0))
        .transpose()
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

/// The one gate every workspace-scoped request passes through.
///
/// Order matters. The tenant is checked *before* the role, and a workspace in
/// another tenant is reported as missing rather than forbidden: "you may not
/// open this" would confirm that it exists, which is exactly the fact the
/// boundary is there to withhold. A membership carried across a tenant switch
/// stops meaning anything here, which is what makes selecting a tenant a
/// change of what the person can reach rather than a change of label.
pub async fn authorize(tx: &mut Tx, actor: &Actor, w: &str, write: bool) -> Result<()> {
    match workspace_tenant(tx, w).await? {
        Some(tenant) if !tenant.is_empty() && tenant == actor.tenant => {}
        _ => return Err(ApiError::missing()),
    }
    match role(tx, w, &actor.id).await?.as_deref() {
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
