use crate::model::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

pub fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;
        CREATE TABLE IF NOT EXISTS migrations(version INTEGER PRIMARY KEY);
        INSERT OR IGNORE INTO migrations VALUES(1);
        CREATE TABLE IF NOT EXISTS workspaces(id TEXT PRIMARY KEY, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS memberships(workspace_id TEXT NOT NULL REFERENCES workspaces(id), actor TEXT NOT NULL, role TEXT NOT NULL, PRIMARY KEY(workspace_id, actor));
        CREATE TABLE IF NOT EXISTS documents(workspace_id TEXT NOT NULL REFERENCES workspaces(id), collection TEXT NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL CHECK(json_valid(body)), PRIMARY KEY(workspace_id, collection, id));
        CREATE INDEX IF NOT EXISTS document_items ON documents(workspace_id, collection, json_extract(body,'$.state'), json_extract(body,'$.updated_at'));
        CREATE INDEX IF NOT EXISTS relation_sources ON documents(workspace_id, collection, json_extract(body,'$.source_id'));
        CREATE INDEX IF NOT EXISTS relation_targets ON documents(workspace_id, collection, json_extract(body,'$.target_id'));
        CREATE TABLE IF NOT EXISTS idempotency(actor TEXT NOT NULL, workspace_id TEXT NOT NULL, key TEXT NOT NULL, fingerprint TEXT NOT NULL, response TEXT NOT NULL, created_at TEXT NOT NULL, PRIMARY KEY(actor, workspace_id, key));
        CREATE TABLE IF NOT EXISTS settings(actor TEXT PRIMARY KEY, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS audit(id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, actor TEXT NOT NULL, origin TEXT NOT NULL, command TEXT NOT NULL, created_at TEXT NOT NULL);")?;
    db.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS invitations(id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL REFERENCES workspaces(id), target_actor TEXT NOT NULL, role TEXT NOT NULL CHECK(role IN ('editor','viewer')), status TEXT NOT NULL CHECK(status IN ('pending','accepted','declined','revoked')), created_by TEXT NOT NULL, created_at TEXT NOT NULL, expires_at TEXT NOT NULL, version INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS invitations_recipient ON invitations(target_actor,status);
        CREATE INDEX IF NOT EXISTS invitations_workspace ON invitations(workspace_id,status);
        INSERT OR IGNORE INTO migrations VALUES(2);
        COMMIT;")?;
    Ok(())
}
pub fn list<T: DeserializeOwned>(db: &Connection, w: &str, col: &str) -> Result<Vec<T>> {
    let mut q = db.prepare(
        "SELECT body FROM documents WHERE workspace_id=?1 AND collection=?2 ORDER BY rowid",
    )?;
    let rows = q.query_map(params![w, col], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
pub fn get<T: DeserializeOwned>(db: &Connection, w: &str, col: &str, id: &str) -> Result<T> {
    let raw: Option<String> = db
        .query_row(
            "SELECT body FROM documents WHERE workspace_id=?1 AND collection=?2 AND id=?3",
            params![w, col, id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(serde_json::from_str(&raw.ok_or_else(ApiError::missing)?)?)
}
pub fn put<T: Serialize>(db: &Connection, w: &str, col: &str, id: &str, value: &T) -> Result<()> {
    db.execute("INSERT INTO documents VALUES(?1,?2,?3,?4) ON CONFLICT(workspace_id,collection,id) DO UPDATE SET body=excluded.body",params![w,col,id,serde_json::to_string(value)?])?;
    Ok(())
}
pub fn remove(db: &Connection, w: &str, col: &str, id: &str) -> Result<()> {
    if db.execute(
        "DELETE FROM documents WHERE workspace_id=?1 AND collection=?2 AND id=?3",
        params![w, col, id],
    )? == 0
    {
        return Err(ApiError::missing());
    }
    Ok(())
}
pub fn memberships(db: &Connection, actor: &str) -> Result<Vec<Workspace>> {
    let mut q = db.prepare("SELECT w.body,m.role FROM workspaces w JOIN memberships m ON w.id=m.workspace_id WHERE m.actor=?1 ORDER BY w.rowid")?;
    let rows = q.query_map([actor], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    rows.map(|r| {
        let (raw, role) = r?;
        let mut w: Workspace = serde_json::from_str(&raw)?;
        w.role = role;
        Ok(w)
    })
    .collect()
}
pub fn authorize(db: &Connection, actor: &str, w: &str, write: bool) -> Result<()> {
    let role: Option<String> = db
        .query_row(
            "SELECT role FROM memberships WHERE workspace_id=?1 AND actor=?2",
            params![w, actor],
            |r| r.get(0),
        )
        .optional()?;
    match role.as_deref() {
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
