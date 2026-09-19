//! Ranges a person decided in advance.
//!
//! # Why this is not a hole in the approval rule
//!
//! The rule everything else in this server is built on is that a click inside
//! an AI host proves nothing: it arrives on the same connection, with the same
//! token, in the same shape as a call the model made, and there is no field in
//! it the server can check that the model could not also have set. That does
//! not change here and is not being worked around.
//!
//! What changes is *when* the person decides. A row in `auto_apply_rules` is
//! written on Basepath's own origin, with their own session and the
//! same-origin CSRF header — the identical evidence an approval carries — and
//! it says: proposals from this AI connection, in this workspace, of this
//! shape, I have already decided about. Applying one afterwards is not the
//! server trusting the host. It is the server reading a decision the person
//! made earlier, in the one place it can attribute decisions.
//!
//! So this is not approval-per-change traded away for convenience. It is the
//! same approval at a different granularity: one件ずつ becomes 範囲ごと, and
//! the boundary — only a person, only on Basepath's origin — does not move.
//!
//! # What a range cannot contain
//!
//! * **Deletion.** There is no column for it and no argument that would add
//!   one. A proposal that deletes anything is outside every range that can be
//!   expressed, permanently.
//! * **A value that will be read as a commitment.** `due_date`, `start_date`,
//!   `scheduled_date`, `assignee_id`, `self_assessment`, `target`, `baseline`
//!   — the [`crate::copilot::GUARDED`] set. Off unless the person turned it on
//!   for that one range, explicitly, in a second decision.
//! * **Another workspace, or another AI client.** Both are part of the key.
//!   The personal/organization boundary is not crossed by a convenience
//!   setting, and a range granted to one connection says nothing about
//!   another.
//! * **Forever.** Every range expires.
//!
//! # What an AI connection may do with this
//!
//! Neither read it nor write it. [`crate::service`] refuses an agent actor on
//! the management routes before it reads anything, so an agent cannot learn
//! that a range exists, what it covers, or when it ends.
//!
//! It can learn one derived bit — whether *this* proposal is inside a range —
//! and that is deliberate: it is a bit the caller could establish anyway by
//! calling apply and seeing what happened, and publishing it is what stops the
//! conversation from showing a button that cannot work. Knowing that one
//! proposal is covered does not reveal the range's edges.
use crate::copilot::guarded_values;
use crate::db::Tx;
use crate::model::{ApiError, Operation, Result};
use crate::params;
use crate::service::{new_id, now, Actor};
use serde::Serialize;
use serde_json::Value;

/// The longest a range may run before the person has to decide again.
///
/// Not a safety limit so much as a prompt: a permission nobody revisits is one
/// nobody remembers granting.
pub const MAX_DAYS: i64 = 90;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub actor: String,
    pub workspace_id: String,
    pub connection_id: String,
    pub allow_create: bool,
    pub allow_update: bool,
    pub allow_guarded: bool,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub revoked_at: Option<String>,
    pub version: i64,
}

impl Rule {
    /// Whether this range is in force right now.
    ///
    /// Revocation is read here rather than cached anywhere, so turning a range
    /// off takes effect on the next call in every execution environment at
    /// once.
    pub fn active(&self) -> bool {
        self.revoked_at.is_none() && self.expires_at.as_str() > now().as_str()
    }

    /// Whether this range covers every operation in a change set.
    ///
    /// Every operation, not most of them: a proposal is approved or it is not,
    /// and applying the part of it that happens to be in range would leave the
    /// person with half a plan nobody described to them.
    pub fn covers(&self, changeset: &Value) -> bool {
        if !self.active() {
            return false;
        }
        let Ok(operations) = serde_json::from_value::<Vec<Operation>>(changeset["operations"].clone())
        else {
            return false;
        };
        if operations.is_empty() {
            return false;
        }
        operations.iter().all(|op| self.covers_operation(op))
    }

    fn covers_operation(&self, op: &Operation) -> bool {
        // A value that reads afterwards as something the person decided needs
        // the person, unless they said otherwise about this exact range.
        if !self.allow_guarded && !guarded_values(&op.body).is_empty() {
            return false;
        }
        match op.method.as_str() {
            "POST" => self.allow_create,
            "PATCH" => self.allow_update,
            // Deleting is never in range. Not "not by default" — there is no
            // setting that would make this arm return true.
            _ => false,
        }
    }
}

fn row_to_rule(row: &crate::db::Row) -> Result<Rule> {
    Ok(Rule {
        id: row.text(0)?,
        actor: row.text(1)?,
        workspace_id: row.text(2)?,
        connection_id: row.text(3)?,
        allow_create: row.int(4)? != 0,
        allow_update: row.int(5)? != 0,
        allow_guarded: row.int(6)? != 0,
        expires_at: row.text(7)?,
        created_at: row.text(8)?,
        updated_at: row.text(9)?,
        revoked_at: row.opt_text(10)?.filter(|value| !value.is_empty()),
        version: row.int(11)?,
    })
}

const SELECT: &str = "SELECT id,actor,workspace_id,connection_id,allow_create,allow_update,\
                      allow_guarded,expires_at,created_at,updated_at,revoked_at,version \
                      FROM auto_apply_rules";

pub async fn list(tx: &mut Tx, actor: &str) -> Result<Vec<Rule>> {
    let rows = tx
        .fetch_all(
            &format!(
                "{SELECT} WHERE actor=? ORDER BY created_at DESC{}",
                tx.lock_reads()
            ),
            &params![actor],
        )
        .await?;
    rows.iter().map(row_to_rule).collect()
}

async fn find(
    tx: &mut Tx,
    actor: &str,
    workspace_id: &str,
    connection_id: &str,
) -> Result<Option<Rule>> {
    let row = tx
        .fetch_optional(
            &format!(
                "{SELECT} WHERE actor=? AND workspace_id=? AND connection_id=?{}",
                tx.lock_reads()
            ),
            &params![actor, workspace_id, connection_id],
        )
        .await?;
    row.as_ref().map(row_to_rule).transpose()
}

/// The range in force for the connection this request arrived on, if any.
///
/// A request with no connection — the browser — never matches. That is not a
/// special case to remember: there is nothing for a range to do on Basepath's
/// own origin, where the person approves directly.
pub async fn in_force(tx: &mut Tx, actor: &Actor, workspace_id: &str) -> Result<Option<Rule>> {
    let Some(connection) = actor.connection.as_deref().filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    Ok(find(tx, &actor.id, workspace_id, connection)
        .await?
        .filter(Rule::active))
}

/// Whether a change set may be applied on this request under a standing range.
///
/// The proposal must have *arrived* on the same connection that is now
/// applying it. A range is a statement about one AI client's proposals, so a
/// change set proposed elsewhere — in the browser, or through another
/// delegation — is not covered by it even when its contents would be.
pub async fn covering(
    tx: &mut Tx,
    actor: &Actor,
    workspace_id: &str,
    changeset: &Value,
) -> Result<Option<Rule>> {
    let Some(rule) = in_force(tx, actor, workspace_id).await? else {
        return Ok(None);
    };
    if changeset["proposed_by_connection"].as_str() != Some(rule.connection_id.as_str()) {
        return Ok(None);
    }
    Ok(rule.covers(changeset).then_some(rule))
}

/// Records the range a person chose, replacing whatever they chose before.
///
/// Narrowing and widening are the same call because they are the same act: the
/// person is looking at the screen and saying what they want now. What they
/// said before is not a floor.
#[allow(clippy::too_many_arguments)]
pub async fn save(
    tx: &mut Tx,
    actor: &str,
    workspace_id: &str,
    connection_id: &str,
    allow_create: bool,
    allow_update: bool,
    allow_guarded: bool,
    days: i64,
) -> Result<Rule> {
    if !(1..=MAX_DAYS).contains(&days) {
        return Err(ApiError::invalid(&format!(
            "有効期限は1〜{MAX_DAYS}日で指定してください"
        )));
    }
    // A range that permits nothing is not a range, and saving one would leave
    // a row that looks like a permission and grants none.
    if !allow_create && !allow_update {
        return Err(ApiError::invalid(
            "追加か更新のどちらかは許可してください。どちらも許可しない場合は設定を解除してください",
        ));
    }
    // The connection has to be one this person actually granted, and it has to
    // still be granted. A range over a revoked delegation would come back to
    // life the moment they reconnected.
    let connection = crate::mcp_auth::get_connection(tx, actor, connection_id).await?;
    if connection.status != "active" {
        return Err(ApiError::invalid(
            "この接続は有効ではありません。先に接続を許可してください",
        ));
    }
    crate::storage::authorize(tx, actor, workspace_id, true).await?;
    let stamp = now();
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(days)).to_rfc3339();
    let existing = find(tx, actor, workspace_id, connection_id).await?;
    let rule = Rule {
        id: existing
            .as_ref()
            .map(|rule| rule.id.clone())
            .unwrap_or_else(|| new_id("autoapply")),
        actor: actor.to_owned(),
        workspace_id: workspace_id.to_owned(),
        connection_id: connection_id.to_owned(),
        allow_create,
        allow_update,
        allow_guarded,
        expires_at,
        created_at: existing
            .as_ref()
            .map(|rule| rule.created_at.clone())
            .unwrap_or_else(|| stamp.clone()),
        updated_at: stamp,
        // Saving again re-arms a range the person had turned off. That is what
        // they asked for by pressing save on this screen.
        revoked_at: None,
        version: existing.as_ref().map(|rule| rule.version + 1).unwrap_or(1),
    };
    let sql = tx.dialect().upsert(
        "auto_apply_rules",
        &[
            "id",
            "actor",
            "workspace_id",
            "connection_id",
            "allow_create",
            "allow_update",
            "allow_guarded",
            "expires_at",
            "created_at",
            "updated_at",
            "revoked_at",
            "version",
        ],
        &["actor", "workspace_id", "connection_id"],
        &[
            "allow_create",
            "allow_update",
            "allow_guarded",
            "expires_at",
            "updated_at",
            "revoked_at",
            "version",
        ],
    );
    tx.execute(
        &sql,
        &params![
            &rule.id,
            &rule.actor,
            &rule.workspace_id,
            &rule.connection_id,
            i64::from(rule.allow_create),
            i64::from(rule.allow_update),
            i64::from(rule.allow_guarded),
            &rule.expires_at,
            &rule.created_at,
            &rule.updated_at,
            "",
            rule.version
        ],
    )
    .await?;
    Ok(rule)
}

/// Turns a range off. Effective on the next call, everywhere.
///
/// The row stays so that a change set applied under it can still say which
/// range it was applied under. An audit trail that loses its reason when
/// somebody changes their mind is not an audit trail.
pub async fn revoke(tx: &mut Tx, actor: &str, id: &str) -> Result<Rule> {
    let row = tx
        .fetch_optional(
            &format!("{SELECT} WHERE id=? AND actor=?{}", tx.lock_reads()),
            &params![id, actor],
        )
        .await?
        .ok_or_else(ApiError::missing)?;
    let mut rule = row_to_rule(&row)?;
    rule.revoked_at = Some(now());
    rule.updated_at = now();
    rule.version += 1;
    tx.execute(
        "UPDATE auto_apply_rules SET revoked_at=?,updated_at=?,version=? WHERE id=? AND actor=?",
        &params![
            rule.revoked_at.clone().unwrap_or_default(),
            &rule.updated_at,
            rule.version,
            id,
            actor
        ],
    )
    .await?;
    Ok(rule)
}
