use crate::db::Tx;
use crate::model::{ApiError, Result};
use crate::params;
use crate::service::{new_id, now, Actor};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ConversationLink {
    pub id: String,
    pub conversation_id: String,
    pub workspace_id: String,
    pub item_id: Option<String>,
    pub screen: Option<String>,
    pub status: String,
    pub source: String,
    pub source_version: String,
    pub idempotency_key: String,
    pub created_at: String,
    pub updated_at: String,
}

pub struct LinkInput<'a> {
    pub connection_id: &'a str,
    pub conversation_id: &'a str,
    pub workspace_id: &'a str,
    pub item_id: Option<&'a str>,
    pub screen: Option<&'a str>,
    pub idempotency_key: &'a str,
}

/// MySQL stores conversation ids in a VARCHAR(191) column. Keep the same
/// boundary in SQLite and at the MCP contract so a link cannot succeed in one
/// deployment and fail after a storage migration in another.
pub const MAX_CONVERSATION_ID_LENGTH: usize = 191;
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 200;

const SELECT: &str = "SELECT id,conversation_id,workspace_id,item_id,screen,status,source,source_version,idempotency_key,created_at,updated_at FROM conversation_links";
const SELECT_FOR_KEY: &str = "SELECT l.id,l.conversation_id,l.workspace_id,l.item_id,l.screen,l.status,l.source,l.source_version,l.idempotency_key,l.created_at,l.updated_at FROM conversation_links l JOIN conversation_link_idempotency k ON k.link_id=l.id";

fn row(row: &crate::db::Row) -> Result<ConversationLink> {
    Ok(ConversationLink {
        id: row.text(0)?,
        conversation_id: row.text(1)?,
        workspace_id: row.text(2)?,
        item_id: row.opt_text(3)?,
        screen: row.opt_text(4)?,
        status: row.text(5)?,
        source: row.text(6)?,
        source_version: row.text(7)?,
        idempotency_key: row.binary_text(8)?,
        created_at: row.text(9)?,
        updated_at: row.text(10)?,
    })
}

async fn claim_key(tx: &mut Tx, actor: &Actor, input: &LinkInput<'_>, link_id: &str) -> Result<()> {
    let existing = tx
        .fetch_optional(
            &format!(
                "{SELECT_FOR_KEY} WHERE k.actor=? AND k.tenant=? AND k.connection_id=? AND k.idempotency_key=?{}",
                tx.lock_reads()
            ),
            &params![
                &actor.id,
                &actor.tenant,
                input.connection_id,
                input.idempotency_key
            ],
        )
        .await?;
    if let Some(existing) = existing {
        let link = row(&existing)?;
        if link.conversation_id != input.conversation_id {
            return Err(ApiError::new(
                409,
                "CONTEXT_LINK_CONFLICT",
                "idempotency key is already used for another context link",
            ));
        }
        return Ok(());
    }
    let result = tx
        .execute(
            "INSERT INTO conversation_link_idempotency(id,actor,tenant,connection_id,idempotency_key,conversation_id,link_id,created_at) VALUES(?,?,?,?,?,?,?,?)",
            &params![
                new_id("clinkkey"),
                &actor.id,
                &actor.tenant,
                input.connection_id,
                input.idempotency_key,
                input.conversation_id,
                link_id,
                now()
            ],
        )
        .await;
    if result.is_err() {
        let winner = tx
            .fetch_optional(
                &format!(
                    "{SELECT_FOR_KEY} WHERE k.actor=? AND k.tenant=? AND k.connection_id=? AND k.idempotency_key=?{}",
                    tx.lock_reads()
                ),
                &params![
                    &actor.id,
                    &actor.tenant,
                    input.connection_id,
                    input.idempotency_key
                ],
            )
            .await?;
        if winner
            .as_ref()
            .map(row)
            .transpose()?
            .is_some_and(|link| link.conversation_id == input.conversation_id)
        {
            return Ok(());
        }
        return Err(ApiError::new(
            409,
            "CONTEXT_LINK_CONFLICT",
            "idempotency key is already used for another context link",
        ));
    }
    Ok(())
}

pub async fn upsert(tx: &mut Tx, actor: &Actor, input: LinkInput<'_>) -> Result<ConversationLink> {
    if input.conversation_id.is_empty()
        || input.conversation_id.chars().count() > MAX_CONVERSATION_ID_LENGTH
    {
        return Err(ApiError::invalid(
            "conversation_id must be 1-191 characters",
        ));
    }
    if input.idempotency_key.is_empty()
        || input.idempotency_key.len() > MAX_IDEMPOTENCY_KEY_BYTES
        || !input.idempotency_key.is_ascii()
    {
        return Err(ApiError::invalid(
            "idempotency_key must be 1-200 ASCII bytes",
        ));
    }
    // Check ownership before the conversation lookup. Otherwise a retry of an
    // existing conversation could silently return it while reusing a key that
    // belongs to another conversation.
    if let Some(existing) = tx
        .fetch_optional(
            &format!(
                "{SELECT_FOR_KEY} WHERE k.actor=? AND k.tenant=? AND k.connection_id=? AND k.idempotency_key=?{}",
                tx.lock_reads()
            ),
            &params![
                &actor.id,
                &actor.tenant,
                input.connection_id,
                input.idempotency_key
            ],
        )
        .await?
    {
        let link = row(&existing)?;
        if link.conversation_id != input.conversation_id {
            return Err(ApiError::new(
                409,
                "CONTEXT_LINK_CONFLICT",
                "idempotency key is already used for another context link",
            ));
        }
    }
    let existing = tx
        .fetch_optional(
            &format!(
                "{SELECT} WHERE actor=? AND tenant=? AND connection_id=? AND conversation_id=?{}",
                tx.lock_reads()
            ),
            &params![
                &actor.id,
                &actor.tenant,
                input.connection_id,
                input.conversation_id
            ],
        )
        .await?;
    if let Some(existing) = existing {
        let link = row(&existing)?;
        if link.workspace_id != input.workspace_id
            || link.item_id.as_deref() != input.item_id
            || link.screen.as_deref() != input.screen
        {
            return Err(ApiError::new(
                409,
                "CONTEXT_LINK_CONFLICT",
                "この会話は別の業務コンテキストにリンク済みです",
            ));
        }
        if link.status == "stopped" {
            claim_key(tx, actor, &input, &link.id).await?;
            tx.execute(
                "UPDATE conversation_links SET connection_id=?,screen=?,status='active',idempotency_key=?,updated_at=? WHERE id=?",
                &params![input.connection_id, input.screen, input.idempotency_key, now(), &link.id],
            )
            .await?;
            let refreshed = tx
                .fetch_one(
                    &format!("{SELECT} WHERE id=?{}", tx.lock_reads()),
                    &params![&link.id],
                )
                .await?;
            return row(&refreshed);
        }
        claim_key(tx, actor, &input, &link.id).await?;
        return Ok(link);
    }
    // The key was checked above; the ownership table also preserves every
    // later key used to retry this same active conversation.
    let timestamp = now();
    let link_id = new_id("clink");
    if let Err(insert_error) = tx.execute(
        "INSERT INTO conversation_links(id,actor,tenant,connection_id,conversation_id,workspace_id,item_id,screen,status,source,source_version,idempotency_key,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        &params![&link_id, &actor.id, &actor.tenant, input.connection_id, input.conversation_id, input.workspace_id, input.item_id, input.screen, "active", "mcp", "1", input.idempotency_key, &timestamp, &timestamp],
    ).await {
        // A concurrent Lambda may have won the unique insert. Read the
        // winner and make a retry idempotent instead of leaking STORAGE_CONFLICT.
        let conversation_winner = tx.fetch_optional(
            &format!(
                "{SELECT} WHERE actor=? AND tenant=? AND connection_id=? AND conversation_id=?{}",
                tx.lock_reads()
            ),
            &params![&actor.id, &actor.tenant, input.connection_id, input.conversation_id],
        ).await?;
        let idempotency_winner = tx.fetch_optional(
            &format!(
                "{SELECT_FOR_KEY} WHERE k.actor=? AND k.tenant=? AND k.connection_id=? AND k.idempotency_key=?{}",
                tx.lock_reads()
            ),
            &params![&actor.id, &actor.tenant, input.connection_id, input.idempotency_key],
        ).await?;
        if let Some(winner) = conversation_winner {
            let link = row(&winner)?;
            let same_target = link.workspace_id == input.workspace_id
                && link.item_id.as_deref() == input.item_id
                && link.screen.as_deref() == input.screen;
            if same_target {
                if let Some(key_winner) = idempotency_winner {
                    let key_link = row(&key_winner)?;
                    if key_link.conversation_id != input.conversation_id {
                        return Err(ApiError::new(
                            409,
                            "CONTEXT_LINK_CONFLICT",
                            "idempotency key is already used for another context link",
                        ));
                    }
                }
                // The losing request may have used a distinct, previously
                // unused key. Claim it for the winning link before returning
                // so a later conversation cannot take that key.
                claim_key(tx, actor, &input, &link.id).await?;
                return Ok(link);
            }
            return Err(ApiError::new(
                409,
                "CONTEXT_LINK_CONFLICT",
                "この会話は別の業務コンテキストにリンク済みです",
            ));
        }
        if let Some(winner) = idempotency_winner {
            let link = row(&winner)?;
            if link.conversation_id != input.conversation_id
                || link.workspace_id != input.workspace_id
                || link.item_id.as_deref() != input.item_id
                || link.screen.as_deref() != input.screen
            {
                return Err(ApiError::new(
                    409,
                    "CONTEXT_LINK_CONFLICT",
                    "idempotency key is already used for another context link",
                ));
            }
            return Ok(link);
        }
        return Err(insert_error);
    }
    claim_key(tx, actor, &input, &link_id).await?;
    let saved = tx
        .fetch_one(
            &format!(
                "{SELECT} WHERE actor=? AND tenant=? AND connection_id=? AND conversation_id=?{}",
                tx.lock_reads()
            ),
            &params![
                &actor.id,
                &actor.tenant,
                input.connection_id,
                input.conversation_id
            ],
        )
        .await?;
    row(&saved)
}

pub async fn get(
    tx: &mut Tx,
    actor: &Actor,
    connection_id: &str,
    conversation_id: &str,
) -> Result<Option<ConversationLink>> {
    let found = tx
        .fetch_optional(
            &format!(
                "{SELECT} WHERE actor=? AND tenant=? AND connection_id=? AND conversation_id=?{}",
                tx.lock_reads()
            ),
            &params![&actor.id, &actor.tenant, connection_id, conversation_id],
        )
        .await?;
    found.map(|value| row(&value)).transpose()
}

pub async fn stop_for_connection(tx: &mut Tx, connection_id: &str) -> Result<()> {
    tx.execute("UPDATE conversation_links SET status='stopped',updated_at=? WHERE connection_id=? AND status='active'", &params![now(), connection_id])
        .await
        .map(|_| ())
}
